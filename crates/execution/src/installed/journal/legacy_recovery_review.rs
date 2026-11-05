//! Independent historical recovery decisions for journals without a baseline.
//!
//! This module only records and replays product-management facts. Evidence is
//! collected and authenticated by the Host; replay never opens files, starts
//! services, or calls a worker or model.
use super::*;

#[cfg(test)]
mod tests;

impl ProductJournal {
    pub(crate) fn check_legacy_review(
        &self,
        review: &p::SignedLegacyReleaseReview,
        now: p::Timestamp,
    ) -> p::Result<()> {
        review.validate()?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        validate_review_source(&writer.projection, &review.review.binding, None, now, true)
    }

    pub(crate) fn saved_legacy_review(&self, id: &str) -> p::Result<p::SignedLegacyReleaseReview> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        writer
            .projection
            .legacy_reviews
            .get(id)
            .map(|state| state.review.clone())
            .ok_or_else(invalid_journal)
    }

    /// Read the exact report already bound to an interrupted decision. This
    /// cannot authorize a new decision or advance the existing one.
    pub(crate) fn pending_legacy_review(
        &self,
        original_id: &str,
        now: p::Timestamp,
    ) -> p::Result<Option<p::SignedLegacyReleaseReview>> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let projection = &writer.projection;
        let Some(state) = projection.legacy_reviews.values().find(|state| {
            state.view.stage == p::LegacyReleaseRecoveryStage::Intent
                && state.review.review.binding.original_operation_id == original_id
        }) else {
            return Ok(None);
        };
        if projection.sequence != state.intent_revision || now < state.intent_at {
            return Err(invalid_journal());
        }
        validate_review_binding(projection, &state.view.request, &state.review, now, false)?;
        Ok(Some(state.review.clone()))
    }

    pub(crate) fn legacy_recoveries(
        &self,
        owner: &p::OwnerPrincipalRef,
    ) -> p::Result<p::LegacyReleaseRecoveryList> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let mut operations: Vec<_> = writer
            .projection
            .legacy_reviews
            .values()
            .filter(|state| state.view.request.owner == *owner)
            .map(|state| state.view.clone())
            .collect();
        operations.sort_by_key(|view| std::cmp::Reverse(view.revision));
        operations.truncate(25);
        let list = p::LegacyReleaseRecoveryList {
            schema_version: p::SchemaVersion(1),
            product_revision: writer.projection.sequence,
            owner: owner.clone(),
            operations,
        };
        list.validate()?;
        Ok(list)
    }

    pub(crate) fn legacy_recovery_operation(
        &self,
        id: &str,
    ) -> p::Result<p::LegacyReleaseRecoveryView> {
        p::validate_product_id(id)?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        writer
            .projection
            .legacy_reviews
            .get(id)
            .map(|state| state.view.clone())
            .ok_or_else(invalid_journal)
    }

    /// Same key and request are a read-only idempotent lookup. Any changed
    /// binding, including a cross-domain collision, fails closed.
    pub(crate) fn find_legacy_recovery(
        &self,
        request: &p::LegacyReleaseRecoveryRequest,
    ) -> p::Result<Option<p::LegacyReleaseRecoveryView>> {
        request.validate()?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let projection = &writer.projection;
        if let Some(id) = projection.legacy_review_keys.get(&request.idempotency_key) {
            let state = projection
                .legacy_reviews
                .get(id)
                .ok_or_else(invalid_journal)?;
            if id != &request.operation_id || state.view.request_digest != request.digest()? {
                return Err(invalid_journal());
            }
            return Ok(Some(state.view.clone()));
        }
        if projection
            .legacy_reviews
            .contains_key(&request.operation_id)
            || projection.operations.contains_key(&request.operation_id)
            || projection.recoveries.contains_key(&request.operation_id)
            || projection.commands.contains_key(&request.idempotency_key)
            || projection
                .recovery_keys
                .contains_key(&request.idempotency_key)
        {
            return Err(invalid_journal());
        }
        Ok(None)
    }

    pub(crate) fn begin_legacy_recovery(
        &self,
        request: p::LegacyReleaseRecoveryRequest,
        review: p::SignedLegacyReleaseReview,
        at: p::Timestamp,
    ) -> p::Result<()> {
        request.validate()?;
        review.validate()?;
        let review_digest = review.review_digest()?;
        if review_digest != request.review_digest
            || review.review.status != p::LegacyReleaseReviewStatus::Verified
            || review.review.binding.review_id != request.review_id
            || review.review.binding.owner != request.owner
        {
            return Err(invalid_journal());
        }

        let request_digest = request.digest()?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        if let Some(id) = writer
            .projection
            .legacy_review_keys
            .get(&request.idempotency_key)
        {
            let state = writer
                .projection
                .legacy_reviews
                .get(id)
                .ok_or_else(invalid_journal)?;
            if id == &request.operation_id
                && state.view.request_digest == request_digest
                && state.review.digest()? == review.digest()?
            {
                return Ok(());
            }
            return Err(invalid_journal());
        }
        if writer
            .projection
            .legacy_reviews
            .contains_key(&request.operation_id)
            || writer
                .projection
                .operations
                .contains_key(&request.operation_id)
            || writer
                .projection
                .recoveries
                .contains_key(&request.operation_id)
            || writer
                .projection
                .commands
                .contains_key(&request.idempotency_key)
            || writer
                .projection
                .recovery_keys
                .contains_key(&request.idempotency_key)
        {
            return Err(invalid_journal());
        }
        write_fact(
            &mut writer,
            request.expected_revision,
            JournalFact::LegacyReleaseRecoveryIntent {
                request: Box::new(request),
                review: Box::new(review),
                actor: p::MaintenanceActorClass::OwnerReauthenticated,
                at,
            },
        )
    }

    pub(crate) fn commit_legacy_recovery(
        &self,
        verification: p::LegacyReleaseRecoveryVerification,
        final_observation_digest: p::SchemaDigest,
        at: p::Timestamp,
    ) -> p::Result<()> {
        verification.validate()?;
        p::validate_product_digest(&final_observation_digest)?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let state = writer
            .projection
            .legacy_reviews
            .get(&verification.operation_id)
            .ok_or_else(invalid_journal)?;
        if state.view.request_digest != verification.request_digest
            || state.view.request.owner != verification.owner
            || state.intent_revision != verification.expected_revision
        {
            return Err(invalid_journal());
        }
        if state.view.stage == p::LegacyReleaseRecoveryStage::Committed {
            if state.final_observation_digest.as_ref() == Some(&final_observation_digest)
                && state.intent_revision == verification.expected_revision
            {
                return Ok(());
            }
            return Err(invalid_journal());
        }
        let review_digest = state.view.review_digest.clone();
        write_fact(
            &mut writer,
            verification.expected_revision,
            JournalFact::LegacyReleaseRecoveryCommitted {
                operation_id: verification.operation_id,
                request_digest: verification.request_digest,
                review_digest,
                final_observation_digest,
                authorization_deadline: verification.requested_deadline,
                actor: p::MaintenanceActorClass::OwnerReauthenticated,
                at,
            },
        )
    }
}

fn validate_review_binding(
    projection: &Projection,
    request: &p::LegacyReleaseRecoveryRequest,
    review: &p::SignedLegacyReleaseReview,
    at: p::Timestamp,
    require_current_checkpoint: bool,
) -> p::Result<()> {
    request.validate()?;
    review.validate()?;
    if review.review.status != p::LegacyReleaseReviewStatus::Verified
        || review.review.binding.review_id != request.review_id
        || review.review.binding.owner != request.owner
        || review.review.digest()? != request.review_digest
        || review.review.binding.expires_at <= at
        || request.requested_deadline > review.review.binding.expires_at
    {
        return Err(invalid_journal());
    }
    validate_review_source(
        projection,
        &review.review.binding,
        Some(&request.operation_id),
        at,
        require_current_checkpoint,
    )
}

fn validate_review_source(
    projection: &Projection,
    binding: &p::LegacyReleaseReviewBinding,
    recovery_id: Option<&str>,
    at: p::Timestamp,
    require_current_checkpoint: bool,
) -> p::Result<()> {
    if at < 0 || at >= binding.expires_at {
        return Err(invalid_journal());
    }
    if require_current_checkpoint
        && (binding.journal_seq != projection.sequence
            || binding.journal_head != projection.head()?)
    {
        return Err(invalid_journal());
    }
    if projection.install_id.as_deref() != Some(&binding.install_ref) {
        return Err(invalid_journal());
    }
    let state = projection
        .operations
        .get(&binding.original_operation_id)
        .ok_or_else(invalid_journal)?;
    let takeover = state
        .release_takeover
        .as_ref()
        .ok_or_else(invalid_journal)?;
    let release = &takeover.release;
    if at < takeover.deadline
        || takeover.owner != binding.owner
        || release.install_ref != binding.install_ref
        || release.operation_id != binding.original_operation_id
        || release.command_digest != binding.original_command_digest
        || release.original_release_ref != binding.original_release_ref
        || release.original_release_digest != binding.original_release_digest
        || state.view.command_digest != binding.original_command_digest
        || state.view.original_release_ref != binding.original_release_ref
        || state.release_images_claimed
        || state.release_checking.is_some()
        || state.release_recovery_baseline.is_some()
        || state.view.pending_stage != Some(p::MaintenanceStage::Quiescing)
        || !matches!(
            state.view.stage,
            p::MaintenanceStage::Validated | p::MaintenanceStage::RecoveryRequired
        )
        || projection.active_release.as_ref().is_none_or(|active| {
            active.release_id != binding.original_release_ref
                || active.release_digest != binding.original_release_digest
        })
        || binding.config_revision != takeover.expected_config_revision
        || binding.config_digest != takeover.expected_config_digest
        || projection
            .installation_verification
            .as_ref()
            .is_none_or(|(installation_digest, _)| {
                installation_digest != &takeover.installation_digest
            })
    {
        return Err(invalid_journal());
    }
    if projection
        .operations
        .iter()
        .any(|(id, other)| id != &binding.original_operation_id && !other.view.stage.is_terminal())
        || projection
            .recoveries
            .values()
            .any(|recovery| recovery.stage == p::ProductReleaseRecoveryStage::Intent)
        || projection.legacy_reviews.values().any(|existing| {
            existing.view.stage == p::LegacyReleaseRecoveryStage::Intent
                && Some(existing.view.request.operation_id.as_str()) != recovery_id
        })
    {
        return Err(invalid_journal());
    }
    Ok(())
}

pub(super) fn fold(projection: &mut Projection, fact: &JournalFact, seq: u64) -> p::Result<()> {
    match fact {
        JournalFact::LegacyReleaseRecoveryIntent {
            request,
            review,
            actor,
            at,
        } => {
            validate_review_binding(projection, request, review, *at, true)?;
            if *actor != p::MaintenanceActorClass::OwnerReauthenticated
                || *at < 0
                || request.expected_revision != projection.sequence
                || *at >= request.requested_deadline
                || request.requested_deadline - *at > 120_000
                || projection
                    .legacy_reviews
                    .contains_key(&request.operation_id)
                || projection.operations.contains_key(&request.operation_id)
                || projection.recoveries.contains_key(&request.operation_id)
                || projection.commands.contains_key(&request.idempotency_key)
                || projection
                    .recovery_keys
                    .contains_key(&request.idempotency_key)
                || projection
                    .legacy_review_keys
                    .contains_key(&request.idempotency_key)
            {
                return Err(invalid_journal());
            }
            let view = p::LegacyReleaseRecoveryView {
                schema_version: p::SchemaVersion(1),
                request: *request.clone(),
                request_digest: request.digest()?,
                revision: seq,
                stage: p::LegacyReleaseRecoveryStage::Intent,
                review_digest: review.review_digest()?,
                receipt_digest: None,
            };
            view.validate()?;
            projection.legacy_review_keys.insert(
                request.idempotency_key.clone(),
                request.operation_id.clone(),
            );
            projection.legacy_reviews.insert(
                request.operation_id.clone(),
                LegacyReviewState {
                    view,
                    review: *review.clone(),
                    intent_revision: seq,
                    intent_at: *at,
                    final_observation_digest: None,
                },
            );
        }
        JournalFact::LegacyReleaseRecoveryCommitted {
            operation_id,
            request_digest,
            review_digest,
            final_observation_digest,
            authorization_deadline,
            actor,
            at,
        } => {
            p::validate_product_digest(final_observation_digest)?;
            let state = projection
                .legacy_reviews
                .get(operation_id)
                .ok_or_else(invalid_journal)?;
            if state.view.stage != p::LegacyReleaseRecoveryStage::Intent
                || state.view.request_digest != *request_digest
                || state.view.review_digest != *review_digest
                || state.review.review_digest()? != *review_digest
                || *actor != p::MaintenanceActorClass::OwnerReauthenticated
                || *at < 0
                || *at < state.intent_at
                || *at >= *authorization_deadline
                || *authorization_deadline > state.review.review.binding.expires_at
                || authorization_deadline.saturating_sub(*at) > 120_000
                || projection.sequence != state.view.revision
            {
                return Err(invalid_journal());
            }
            validate_review_binding(projection, &state.view.request, &state.review, *at, false)?;
            let original_id = state.review.review.binding.original_operation_id.clone();
            let receipt = p::canonical_digest(&("forme-legacy-release-recovery-receipt-v1", fact))?;
            let original = projection
                .operations
                .get_mut(&original_id)
                .ok_or_else(invalid_journal)?;
            original.view.stage = p::MaintenanceStage::Aborted;
            original.view.pending_stage = None;
            original.view.revision = seq;
            original.view.receipts.push(receipt.0.clone());
            let state = projection
                .legacy_reviews
                .get_mut(operation_id)
                .ok_or_else(invalid_journal)?;
            state.view.stage = p::LegacyReleaseRecoveryStage::Committed;
            state.view.revision = seq;
            state.view.receipt_digest = Some(receipt);
            state.final_observation_digest = Some(final_observation_digest.clone());
            state.view.validate()?;
        }
        _ => return Err(invalid_journal()),
    }
    Ok(())
}
