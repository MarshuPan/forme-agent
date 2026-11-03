//! Pure recovery projection and append-only decisions. Never touches a driver.
use super::*;
#[cfg(test)]
mod tests;

impl ProductJournal {
    pub(crate) fn release_recoveries(
        &self,
        owner: &p::OwnerPrincipalRef,
    ) -> p::Result<p::ProductReleaseRecoveryList> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let mut operations: Vec<_> = writer.projection.recoveries.values().cloned().collect();
        operations.sort_by_key(|view| std::cmp::Reverse(view.revision));
        operations.truncate(25);
        let list = p::ProductReleaseRecoveryList {
            schema_version: p::SchemaVersion(1),
            product_revision: writer.projection.sequence,
            owner: owner.clone(),
            operations,
        };
        list.validate()?;
        Ok(list)
    }
    pub(crate) fn prepare_release_recovery_baseline(
        &self,
        expected: u64,
        baseline: p::ProductReleaseRecoveryBaseline,
    ) -> p::Result<()> {
        self.append(
            expected,
            JournalFact::ReleaseRecoveryBaselinePrepared {
                baseline: Box::new(baseline),
            },
        )
    }
    pub(crate) fn release_recovery_baseline(
        &self,
        id: &str,
    ) -> p::Result<p::ProductReleaseRecoveryBaseline> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        writer
            .projection
            .operations
            .get(id)
            .and_then(|s| s.release_recovery_baseline.clone())
            .ok_or_else(invalid_journal)
    }
    pub(crate) fn release_recovery_candidate(
        &self,
        id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::ProductReleaseRecoveryCandidate> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        candidate(&writer.projection, id, now)
    }
    pub fn release_recovery_operation(&self, id: &str) -> p::Result<p::ProductReleaseRecoveryView> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        writer
            .projection
            .recoveries
            .get(id)
            .cloned()
            .ok_or_else(invalid_journal)
    }
    pub(crate) fn find_release_recovery(
        &self,
        request: &p::ProductReleaseRecoveryRequest,
    ) -> p::Result<Option<p::ProductReleaseRecoveryView>> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let projection = &writer.projection;
        if let Some(id) = projection.recovery_keys.get(&request.idempotency_key) {
            let view = projection.recoveries.get(id).ok_or_else(invalid_journal)?;
            if view.request_digest != request.digest()? {
                return Err(invalid_journal());
            }
            return Ok(Some(view.clone()));
        }
        if projection.recoveries.contains_key(&request.operation_id)
            || projection.operations.contains_key(&request.operation_id)
            || projection.commands.contains_key(&request.idempotency_key)
        {
            return Err(invalid_journal());
        }
        Ok(None)
    }
    pub(crate) fn begin_release_recovery(
        &self,
        request: p::ProductReleaseRecoveryRequest,
        at: p::Timestamp,
    ) -> p::Result<()> {
        self.append(
            request.expected_revision,
            JournalFact::ReleaseRecoveryIntent {
                request: Box::new(request),
                actor: p::MaintenanceActorClass::OwnerReauthenticated,
                at,
            },
        )
    }
    pub(crate) fn commit_release_recovery(
        &self,
        id: &str,
        expected: u64,
        request_digest: p::SchemaDigest,
        evidence: p::SchemaDigest,
        deadline: p::Timestamp,
        at: p::Timestamp,
    ) -> p::Result<()> {
        self.append(
            expected,
            JournalFact::ReleaseRecoveryCommitted {
                operation_id: id.into(),
                request_digest,
                expected_evidence: evidence,
                authorization_deadline: deadline,
                actor: p::MaintenanceActorClass::OwnerReauthenticated,
                at,
            },
        )
    }
}

fn candidate(
    projection: &Projection,
    id: &str,
    now: p::Timestamp,
) -> p::Result<p::ProductReleaseRecoveryCandidate> {
    let state = projection.operations.get(id).ok_or_else(invalid_journal)?;
    let takeover = state
        .release_takeover
        .as_ref()
        .ok_or_else(invalid_journal)?;
    let baseline = state
        .release_recovery_baseline
        .as_ref()
        .ok_or_else(invalid_journal)?;
    baseline.validate_for(takeover)?;
    let release = &takeover.release;
    if now < takeover.deadline
        || state.view.pending_stage != Some(p::MaintenanceStage::Quiescing)
        || !matches!(
            state.view.stage,
            p::MaintenanceStage::Validated | p::MaintenanceStage::RecoveryRequired
        )
        || state.release_images_claimed
        || state.release_checking.is_some()
        || projection.active_release.as_ref().is_none_or(|active| {
            active.release_id != release.original_release_ref
                || active.release_digest != release.original_release_digest
        })
    {
        return Err(invalid_journal());
    }
    Ok(p::ProductReleaseRecoveryCandidate {
        schema_version: p::SchemaVersion(1),
        product_revision: projection.sequence,
        owner: takeover.owner.clone(),
        original_operation_id: id.into(),
        original_command_digest: release.command_digest.clone(),
        original_release_ref: release.original_release_ref.clone(),
        original_release_digest: release.original_release_digest.clone(),
        config_revision: takeover.expected_config_revision,
        config_digest: takeover.expected_config_digest.clone(),
        evidence_digest: p::canonical_digest(&(
            "forme-release-recovery-evidence-v1",
            &state.view,
            baseline,
        ))?,
    })
}

pub(super) fn fold(projection: &mut Projection, fact: &JournalFact, seq: u64) -> p::Result<()> {
    match fact {
        JournalFact::ReleaseRecoveryBaselinePrepared { baseline } => {
            let state = projection
                .operations
                .get_mut(&baseline.operation_id)
                .ok_or_else(invalid_journal)?;
            let takeover = state
                .release_takeover
                .as_ref()
                .ok_or_else(invalid_journal)?;
            baseline.validate_for(takeover)?;
            if state.view.stage != p::MaintenanceStage::Validated
                || state.view.pending_stage.is_some()
                || state.release_recovery_baseline.is_some()
                || state.release_images_claimed
            {
                return Err(invalid_journal());
            }
            state.release_recovery_baseline = Some(*baseline.clone());
            state.view.revision = seq;
        }
        JournalFact::ReleaseRecoveryIntent { request, actor, at } => {
            request.validate()?;
            if *actor != p::MaintenanceActorClass::OwnerReauthenticated
                || *at < 0
                || *at >= request.requested_deadline
                || request.requested_deadline - *at > 120_000
                || request.expected_revision != projection.sequence
                || request.candidate
                    != candidate(projection, &request.candidate.original_operation_id, *at)?
                || projection
                    .recoveries
                    .values()
                    .any(|r| r.stage == p::ProductReleaseRecoveryStage::Intent)
                || projection.operations.contains_key(&request.operation_id)
                || projection.recoveries.contains_key(&request.operation_id)
                || projection.commands.contains_key(&request.idempotency_key)
                || projection
                    .recovery_keys
                    .contains_key(&request.idempotency_key)
            {
                return Err(invalid_journal());
            }
            let view = p::ProductReleaseRecoveryView {
                schema_version: p::SchemaVersion(1),
                request: *request.clone(),
                request_digest: request.digest()?,
                revision: seq,
                stage: p::ProductReleaseRecoveryStage::Intent,
                receipt_digest: None,
            };
            view.validate()?;
            projection.recovery_keys.insert(
                request.idempotency_key.clone(),
                request.operation_id.clone(),
            );
            projection
                .recoveries
                .insert(request.operation_id.clone(), view);
        }
        JournalFact::ReleaseRecoveryCommitted {
            operation_id,
            request_digest,
            expected_evidence,
            authorization_deadline,
            actor,
            at,
        } => {
            let view = projection
                .recoveries
                .get(operation_id)
                .ok_or_else(invalid_journal)?;
            let observed = candidate(
                projection,
                &view.request.candidate.original_operation_id,
                *at,
            )?;
            // Revision advances for the recovery intent, not the original operation/evidence.
            let mut original_candidate = observed;
            original_candidate.product_revision = view.request.candidate.product_revision;
            if view.stage != p::ProductReleaseRecoveryStage::Intent
                || view.request_digest != *request_digest
                || view.request.candidate != original_candidate
                || *expected_evidence != original_candidate.evidence_digest
                || *actor != p::MaintenanceActorClass::OwnerReauthenticated
                || *at < 0
                || *at >= *authorization_deadline
                || authorization_deadline.saturating_sub(*at) > 120_000
            {
                return Err(invalid_journal());
            }
            let receipt = p::canonical_digest(&("forme-release-recovery-receipt-v1", fact))?;
            let original = projection
                .operations
                .get_mut(&original_candidate.original_operation_id)
                .ok_or_else(invalid_journal)?;
            original.view.stage = p::MaintenanceStage::Aborted;
            original.view.pending_stage = None;
            original.view.revision = seq;
            original.view.receipts.push(receipt.0.clone());
            let view = projection
                .recoveries
                .get_mut(operation_id)
                .ok_or_else(invalid_journal)?;
            view.stage = p::ProductReleaseRecoveryStage::Committed;
            view.revision = seq;
            view.receipt_digest = Some(receipt);
            view.validate()?;
        }
        _ => return Err(invalid_journal()),
    }
    Ok(())
}
