//! Independent adjudication for legacy upgrades without a recovery baseline.
//!
//! This lane only records an owner decision in the product journal.  It never
//! performs installation, service, data, worker, or brain operations.
use super::*;
#[cfg(test)]
mod tests;

// The trusted Host evidence collector and its routes are intentionally outside
// this protocol-only increment; keep the journal seam ready without exposing a
// Web/Apply path that could accept browser-supplied evidence.
#[allow(dead_code)]
impl ProductJournal {
    pub(crate) fn legacy_recovery_reviews(
        &self,
        owner: &p::OwnerPrincipalRef,
    ) -> p::Result<p::LegacyReleaseRecoveryReviewList> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let mut reviews: Vec<_> = writer
            .projection
            .legacy_reviews
            .values()
            .filter(|review| review.request.owner == *owner)
            .cloned()
            .collect();
        reviews.sort_by_key(|review| std::cmp::Reverse(review.revision));
        reviews.truncate(25);
        let list = p::LegacyReleaseRecoveryReviewList {
            schema_version: p::SchemaVersion(1),
            product_revision: writer.projection.sequence,
            owner: owner.clone(),
            reviews,
        };
        list.validate()?;
        Ok(list)
    }

    pub(crate) fn legacy_recovery_review_operation(
        &self,
        id: &str,
    ) -> p::Result<p::LegacyReleaseRecoveryReviewView> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        writer
            .projection
            .legacy_reviews
            .get(id)
            .cloned()
            .ok_or_else(invalid_journal)
    }

    /// Same key and identical content is a pure query.  A key or id reused
    /// with any changed binding is a conflict and never appends a fact.
    pub(crate) fn find_legacy_recovery_review(
        &self,
        request: &p::LegacyReleaseRecoveryReviewRequest,
    ) -> p::Result<Option<p::LegacyReleaseRecoveryReviewView>> {
        request.validate()?;
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let projection = &writer.projection;
        if let Some(id) = projection.legacy_review_keys.get(&request.idempotency_key) {
            let view = projection
                .legacy_reviews
                .get(id)
                .ok_or_else(invalid_journal)?;
            if view.request_digest != request.digest()? || id != &request.review_id {
                return Err(invalid_journal());
            }
            return Ok(Some(view.clone()));
        }
        if projection.legacy_reviews.contains_key(&request.review_id)
            || projection.operations.contains_key(&request.review_id)
            || projection.recoveries.contains_key(&request.review_id)
            || projection.commands.contains_key(&request.idempotency_key)
            || projection
                .recovery_keys
                .contains_key(&request.idempotency_key)
        {
            return Err(invalid_journal());
        }
        Ok(None)
    }

    pub(crate) fn begin_legacy_recovery_review(
        &self,
        request: p::LegacyReleaseRecoveryReviewRequest,
        at: p::Timestamp,
    ) -> p::Result<()> {
        request.validate()?;
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
            let view = writer
                .projection
                .legacy_reviews
                .get(id)
                .ok_or_else(invalid_journal)?;
            if id == &request.review_id && view.request_digest == request_digest {
                return Ok(());
            }
            return Err(invalid_journal());
        }
        if writer
            .projection
            .legacy_reviews
            .contains_key(&request.review_id)
            || writer
                .projection
                .operations
                .contains_key(&request.review_id)
            || writer
                .projection
                .recoveries
                .contains_key(&request.review_id)
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
            JournalFact::LegacyRecoveryReviewIntent {
                request: Box::new(request),
                actor: p::MaintenanceActorClass::OwnerReauthenticated,
                at,
            },
        )
    }

    pub(crate) fn commit_legacy_recovery_review(
        &self,
        verification: p::LegacyReleaseRecoveryReviewVerification,
        expected_evidence: p::SchemaDigest,
        at: p::Timestamp,
    ) -> p::Result<()> {
        verification.validate()?;
        p::validate_product_digest(&expected_evidence)?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let view = writer
            .projection
            .legacy_reviews
            .get(&verification.review_id)
            .ok_or_else(invalid_journal)?;
        if view.request_digest != verification.request_digest
            || view.request.owner != verification.owner
        {
            return Err(invalid_journal());
        }
        if view.stage == p::LegacyReleaseRecoveryReviewStage::Committed {
            if view.request.evidence_digest != expected_evidence
                || verification.expected_revision
                    != view.request.expected_revision.saturating_add(1)
            {
                return Err(invalid_journal());
            }
            return Ok(());
        }
        write_fact(
            &mut writer,
            verification.expected_revision,
            JournalFact::LegacyRecoveryReviewCommitted {
                review_id: verification.review_id,
                request_digest: verification.request_digest,
                expected_evidence,
                authorization_deadline: verification.requested_deadline,
                actor: p::MaintenanceActorClass::OwnerReauthenticated,
                at,
            },
        )
    }
}

fn validate_review_binding(
    projection: &Projection,
    request: &p::LegacyReleaseRecoveryReviewRequest,
    require_current_checkpoint: bool,
) -> p::Result<()> {
    request.validate()?;
    let evidence = &request.evidence;
    if require_current_checkpoint
        && (evidence.current_journal_sequence != projection.sequence
            || evidence.current_journal_head != projection.head()?
            || evidence.current_journal_history_digest != projection.history_digest()?)
    {
        return Err(invalid_journal());
    }
    if projection.install_id.as_deref() != Some(evidence.installation_id.as_str()) {
        return Err(invalid_journal());
    }
    let state = projection
        .operations
        .get(&evidence.original_operation_id)
        .ok_or_else(invalid_journal)?;
    let takeover = state
        .release_takeover
        .as_ref()
        .ok_or_else(invalid_journal)?;
    let release = &takeover.release;
    if takeover.owner != evidence.owner
        || release.install_ref != evidence.installation_id
        || release.operation_id != evidence.original_operation_id
        || release.command_digest != evidence.original_command_digest
        || release.original_release_ref != evidence.original_release_ref
        || release.original_release_digest != evidence.original_release_digest
        || state.view.command_digest != evidence.original_command_digest
        || state.view.original_release_ref != evidence.original_release_ref
        || state.release_images_claimed
        || state.release_checking.is_some()
        || state.release_recovery_baseline.is_some()
        || state.view.pending_stage != Some(p::MaintenanceStage::Quiescing)
        || !matches!(
            state.view.stage,
            p::MaintenanceStage::Validated | p::MaintenanceStage::RecoveryRequired
        )
        || projection.active_release.as_ref().is_none_or(|active| {
            active.release_id != evidence.original_release_ref
                || active.release_digest != evidence.original_release_digest
        })
    {
        return Err(invalid_journal());
    }
    if projection
        .operations
        .iter()
        .any(|(id, other)| id != &evidence.original_operation_id && !other.view.stage.is_terminal())
    {
        return Err(invalid_journal());
    }
    if evidence.historical_data_identity != evidence.current_data_identity
        || evidence.historical_worker_digest != evidence.current_worker_digest
        || evidence.historical_service_digest != evidence.current_service_digest
        || evidence.historical_config_digest != evidence.current_config_digest
        || evidence.historical_program_digest != evidence.current_program_digest
    {
        return Err(invalid_journal());
    }
    Ok(())
}

pub(super) fn fold(projection: &mut Projection, fact: &JournalFact, seq: u64) -> p::Result<()> {
    match fact {
        JournalFact::LegacyRecoveryReviewIntent { request, actor, at } => {
            validate_review_binding(projection, request, true)?;
            if *actor != p::MaintenanceActorClass::OwnerReauthenticated
                || *at < 0
                || *at >= request.requested_deadline
                || request.requested_deadline.saturating_sub(*at) > 120_000
                || request.expected_revision != projection.sequence
                || projection
                    .legacy_reviews
                    .values()
                    .any(|review| review.stage == p::LegacyReleaseRecoveryReviewStage::Intent)
                || projection
                    .recoveries
                    .values()
                    .any(|review| review.stage == p::ProductReleaseRecoveryStage::Intent)
                || projection.legacy_reviews.contains_key(&request.review_id)
                || projection.operations.contains_key(&request.review_id)
                || projection.recoveries.contains_key(&request.review_id)
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
            let view = p::LegacyReleaseRecoveryReviewView {
                schema_version: p::SchemaVersion(1),
                request: *request.clone(),
                request_digest: request.digest()?,
                revision: seq,
                stage: p::LegacyReleaseRecoveryReviewStage::Intent,
                receipt_digest: None,
            };
            view.validate()?;
            projection
                .legacy_review_keys
                .insert(request.idempotency_key.clone(), request.review_id.clone());
            projection
                .legacy_reviews
                .insert(request.review_id.clone(), view);
        }
        JournalFact::LegacyRecoveryReviewCommitted {
            review_id,
            request_digest,
            expected_evidence,
            authorization_deadline,
            actor,
            at,
        } => {
            let view = projection
                .legacy_reviews
                .get(review_id)
                .cloned()
                .ok_or_else(invalid_journal)?;
            if view.stage != p::LegacyReleaseRecoveryReviewStage::Intent
                || view.request_digest != *request_digest
                || view.request.evidence_digest != *expected_evidence
                || *actor != p::MaintenanceActorClass::OwnerReauthenticated
                || *at < 0
                || *at >= *authorization_deadline
                || authorization_deadline.saturating_sub(*at) > 120_000
                || projection.sequence != view.revision
            {
                return Err(invalid_journal());
            }
            validate_review_binding(projection, &view.request, false)?;
            let original_id = view.request.evidence.original_operation_id.clone();
            let receipt = p::canonical_digest(&("forme-legacy-release-recovery-receipt-v1", fact))?;
            let original = projection
                .operations
                .get_mut(&original_id)
                .ok_or_else(invalid_journal)?;
            original.view.stage = p::MaintenanceStage::Aborted;
            original.view.pending_stage = None;
            original.view.revision = seq;
            original.view.receipts.push(receipt.0.clone());
            let view = projection
                .legacy_reviews
                .get_mut(review_id)
                .ok_or_else(invalid_journal)?;
            view.stage = p::LegacyReleaseRecoveryReviewStage::Committed;
            view.revision = seq;
            view.receipt_digest = Some(receipt);
            view.validate()?;
        }
        _ => return Err(invalid_journal()),
    }
    Ok(())
}
