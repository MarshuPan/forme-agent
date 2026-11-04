//! Pure replay of product management facts. No filesystem or platform actions.
use super::*;

impl Projection {
    pub(super) fn head(&self) -> p::Result<p::SchemaDigest> {
        self.head
            .clone()
            .map_or_else(|| p::canonical_digest(&"forme-product-journal-v1"), Ok)
    }
    pub(super) fn fold(&mut self, envelope: &JournalEnvelope) -> p::Result<()> {
        self.fold_with_digest(
            envelope,
            JournalEnvelope::calculate(
                envelope.schema_version,
                envelope.product_seq,
                &envelope.previous,
                &envelope.fact,
            )?,
        )
    }

    pub(super) fn fold_with_digest(
        &mut self,
        envelope: &JournalEnvelope,
        calculated: p::SchemaDigest,
    ) -> p::Result<()> {
        if !matches!(envelope.schema_version.0, 1 | 2)
            || envelope.schema_version.0 < self.schema_floor
            || envelope.product_seq != self.sequence.checked_add(1).ok_or_else(invalid_journal)?
            || envelope.previous != self.head()?
            || envelope.digest != calculated
        {
            return Err(invalid_journal());
        }
        match &envelope.fact {
            JournalFact::InstallationBound { install_id } => {
                p::validate_product_id(install_id)?;
                if self.install_id.is_some() || self.sequence != 0 {
                    return Err(invalid_journal());
                }
                self.install_id = Some(install_id.clone());
            }
            JournalFact::InstallationVerified {
                manifest_digest,
                release_digest,
                active_release,
            } => {
                p::validate_product_digest(manifest_digest)?;
                p::validate_product_digest(release_digest)?;
                if self.install_id.is_none()
                    || self.sequence != 1
                    || self.installation_verification.is_some()
                {
                    return Err(invalid_journal());
                }
                self.installation_verification =
                    Some((manifest_digest.clone(), release_digest.clone()));
                if let Some(active) = active_release {
                    verification::validate_release_ref(active)?;
                    self.active_release = Some(active.clone());
                }
            }
            JournalFact::ActiveReleaseBound { release } => {
                verification::validate_release_ref(release)?;
                if self.installation_verification.is_none()
                    || self.active_release.is_some()
                    || self.has_unfinished_operation()
                {
                    return Err(invalid_journal());
                }
                self.active_release = Some(release.clone());
            }
            JournalFact::ActiveReleaseChanged {
                operation_id,
                from,
                to,
            } => {
                verification::validate_release_ref(from)?;
                verification::validate_release_ref(to)?;
                let operation = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                let (target_release_ref, target_release_digest) = match &operation.command.kind {
                    p::MaintenanceKind::Upgrade {
                        release_id,
                        release_digest,
                        ..
                    }
                    | p::MaintenanceKind::RollbackCompatibleRelease {
                        release_id,
                        release_digest,
                        ..
                    } => (release_id, release_digest),
                    _ => return Err(invalid_journal()),
                };
                if operation.view.pending_stage != Some(p::MaintenanceStage::Switching)
                    || self.active_release.as_ref() != Some(from)
                    || from.release_id != operation.view.original_release_ref
                    || to.release_id != *target_release_ref
                    || to.release_digest != *target_release_digest
                {
                    return Err(invalid_journal());
                }
                self.active_release = Some(to.clone());
                operation.view.revision = envelope.product_seq;
            }
            JournalFact::Planned {
                command,
                references,
                actor,
                at,
                network_contract,
            } => {
                command.validate()?;
                references.validate(&command.kind)?;
                if self.install_id.is_none()
                    || command.expected_revision != self.sequence
                    || *at >= command.requested_deadline
                    || self.operations.contains_key(&command.operation_id)
                    || self.recoveries.contains_key(&command.operation_id)
                    || self.legacy_reviews.contains_key(&command.operation_id)
                    || self.recovery_keys.contains_key(&command.idempotency_key)
                    || self
                        .legacy_review_keys
                        .contains_key(&command.idempotency_key)
                    || self.commands.contains_key(&command.idempotency_key)
                    || self.has_unfinished_operation()
                    || *actor == p::MaintenanceActorClass::Setup && !command.kind.setup_allowed()
                {
                    return Err(invalid_journal());
                }
                let digest = command.bound_digest(references)?;
                self.commands.insert(
                    command.idempotency_key.clone(),
                    (digest.clone(), command.operation_id.clone()),
                );
                self.operations.insert(
                    command.operation_id.clone(),
                    OperationState {
                        command: command.as_ref().clone(),
                        view: p::MaintenanceOperation {
                            schema_version: p::SchemaVersion(1),
                            operation_id: command.operation_id.clone(),
                            command_digest: digest,
                            revision: envelope.product_seq,
                            stage: p::MaintenanceStage::Planned,
                            pending_stage: None,
                            original_release_ref: references.original_release_ref.clone(),
                            target_release_ref: references.target_release_ref.clone(),
                            data_candidate_ref: references.data_candidate_ref.clone(),
                            backup_ref: references.backup_ref.clone(),
                            receipts: Vec::new(),
                            failure_ref: None,
                            network_result: None,
                        },
                        network_contract: *network_contract == Some(1),
                        network_progress: None,
                        release_images_claimed: false,
                        release_takeover: None,
                        release_checking: None,
                        release_recovery_baseline: None,
                    },
                );
            }
            JournalFact::ReleaseImagesClaimed {
                operation_id,
                command_digest,
                prepared_digest,
                at,
            } => {
                p::validate_product_digest(prepared_digest)?;
                let state = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                if state.view.pending_stage != Some(p::MaintenanceStage::Switching)
                    || state.view.stage != p::MaintenanceStage::BackedUp
                    || state.view.command_digest != *command_digest
                    || state.release_images_claimed
                    || *at < 0
                    || *at >= state.command.requested_deadline
                    || !matches!(
                        state.command.kind,
                        p::MaintenanceKind::Upgrade { .. }
                            | p::MaintenanceKind::RollbackCompatibleRelease { .. }
                    )
                {
                    return Err(invalid_journal());
                }
                state.release_images_claimed = true;
                state.view.revision = envelope.product_seq;
            }
            JournalFact::ReleaseTakeoverPrepared { takeover, at } => {
                release_takeover::fold_takeover(self, takeover, *at, envelope.product_seq)?;
            }
            JournalFact::ReleaseCheckingPrepared { checking, at } => {
                release_takeover::fold_checking(self, checking, *at, envelope.product_seq)?;
            }
            JournalFact::ReleaseRecoveryBaselinePrepared { .. }
            | JournalFact::ReleaseRecoveryIntent { .. }
            | JournalFact::ReleaseRecoveryCommitted { .. } => {
                if envelope.schema_version.0 != 2 {
                    return Err(invalid_journal());
                }
                release_recovery::fold(self, &envelope.fact, envelope.product_seq)?;
            }
            JournalFact::LegacyRecoveryReviewIntent { .. }
            | JournalFact::LegacyRecoveryReviewCommitted { .. } => {
                if envelope.schema_version.0 != 2 {
                    return Err(invalid_journal());
                }
                legacy_recovery_review::fold(self, &envelope.fact, envelope.product_seq)?;
            }
            JournalFact::StageIntent {
                operation_id,
                stage,
                actor,
                at,
            } => {
                let state = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                if state.view.pending_stage.is_some()
                    || !state
                        .view
                        .stage
                        .permits(*stage, state.command.kind.is_lifecycle_transaction())
                    || *stage == p::MaintenanceStage::RecoveryRequired
                    || *at >= state.command.requested_deadline
                    || *actor == p::MaintenanceActorClass::Setup
                        && !state.command.kind.setup_allowed()
                {
                    return Err(invalid_journal());
                }
                state.view.pending_stage = Some(*stage);
                state.view.revision = envelope.product_seq;
            }
            JournalFact::StageReceipt {
                operation_id,
                stage,
                receipts,
                network_result,
                ..
            } => {
                let state = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                if matches!(
                    state.view.stage,
                    p::MaintenanceStage::RecoveryRequired
                        | p::MaintenanceStage::Committed
                        | p::MaintenanceStage::Aborted
                ) || state.view.pending_stage != Some(*stage)
                    || receipts.is_empty()
                    || receipts.len() > 32
                {
                    return Err(invalid_journal());
                }
                for receipt in receipts {
                    p::validate_product_digest(receipt)?;
                }
                network_journal::validate_stage_receipt(state, *stage, network_result.as_ref())?;
                state.view.stage = *stage;
                state.view.pending_stage = None;
                state
                    .view
                    .receipts
                    .extend(receipts.iter().map(|r| r.0.clone()));
                if network_result.is_some() {
                    state.view.network_result = network_result.clone();
                }
                state.view.revision = envelope.product_seq;
            }
            JournalFact::NetworkProgress {
                operation_id,
                progress,
                ..
            } => {
                let state = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                network_journal::apply_progress(state, progress, envelope.product_seq)?;
            }
            JournalFact::RecoveryRequired {
                operation_id,
                issue,
                ..
            } => {
                p::validate_product_digest(issue)?;
                let state = self
                    .operations
                    .get_mut(operation_id)
                    .ok_or_else(invalid_journal)?;
                if matches!(
                    state.view.stage,
                    p::MaintenanceStage::Committed
                        | p::MaintenanceStage::RecoveryRequired
                        | p::MaintenanceStage::Aborted
                ) {
                    return Err(invalid_journal());
                }
                state.view.stage = p::MaintenanceStage::RecoveryRequired;
                // Retain pending stage: it may already have changed the host.
                state.view.failure_ref = Some(issue.0.clone());
                state.view.revision = envelope.product_seq;
            }
        }
        let previous_history = self.history_digest()?;
        self.history_digest = Some(p::canonical_digest(&(
            "forme-product-journal-history-v1",
            previous_history,
            envelope.product_seq,
            &envelope.digest,
        ))?);
        self.sequence = envelope.product_seq;
        self.schema_floor = envelope.schema_version.0;
        self.head = Some(envelope.digest.clone());
        Ok(())
    }
    pub(super) fn has_unfinished_operation(&self) -> bool {
        self.operations
            .values()
            .any(|op| !op.view.stage.is_terminal())
            || self
                .recoveries
                .values()
                .any(|r| r.stage == p::ProductReleaseRecoveryStage::Intent)
            || self
                .legacy_reviews
                .values()
                .any(|r| r.stage == p::LegacyReleaseRecoveryReviewStage::Intent)
    }

    pub(super) fn history_digest(&self) -> p::Result<p::SchemaDigest> {
        self.history_digest.clone().map_or_else(
            || p::canonical_digest(&"forme-product-journal-history-v1"),
            Ok,
        )
    }
}
