//! Independent adjudication; production stays unavailable without a native reviewer.
use super::{HostAuth, NativeServiceManager, ProductHostBroker};
use crate::installed::{IsolatedExecutorHost, ProductConfigStore};
use ed25519_dalek::{Signature, VerifyingKey};
use forme_protocol as p;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[path = "host_legacy_release_recovery/native.rs"]
mod native;
#[cfg(test)]
#[path = "host_legacy_release_recovery_tests.rs"]
mod tests;

/// This is a trusted internal seam, never a browser or package-supplied adapter.
/// A native implementation must authenticate all eight artifacts and hold the
/// data inspection lock until release. Managed composition alone may mount the
/// native implementation from independently provisioned, protected material.
pub(super) trait LegacyReviewCollector: Send + Sync {
    fn load(&self, reference: &str) -> p::Result<p::SignedLegacyReleaseReview>;
    fn inspect(
        &self,
        review: &p::LegacyReleaseReview,
        worker_digest: &p::SchemaDigest,
        deadline: Instant,
    ) -> p::Result<Box<dyn LegacyReviewInspection>>;
}

pub(super) trait LegacyReviewInspection {
    /// Re-read actual artifacts/observations under the original exclusive lock.
    /// Artifact authenticity and semantics belong to the trusted collector;
    /// the broker independently checks signature, bindings, services and CAS.
    fn recheck(&self, deadline: Instant) -> p::Result<LegacyReviewObservations>;
    /// Check the original data guard after potentially slow service queries.
    /// No reacquisition, replacement worker, deadline extension or default OK.
    fn ensure_held(&mut self, deadline: Instant) -> p::Result<()>;
    fn release(self: Box<Self>) -> p::Result<()>;
}

pub(super) struct LegacyReviewObservations {
    pub(super) evidence: Vec<p::LegacyReleaseEvidence>,
    pub(super) worker_digest: p::SchemaDigest,
    pub(super) updater_state: p::NativeServiceState,
    pub(super) updater_capabilities: p::ProductReleaseRecoveryCapabilities,
}

pub(super) struct LegacyReleaseReviewer {
    pub(super) key: VerifyingKey,
    pub(super) collector: Arc<dyn LegacyReviewCollector>,
}

impl LegacyReleaseReviewer {
    fn load(&self, reference: &str) -> p::Result<p::SignedLegacyReleaseReview> {
        let signed = self.collector.load(reference)?;
        self.verify(&signed)?;
        Ok(signed)
    }

    fn verify(&self, signed: &p::SignedLegacyReleaseReview) -> p::Result<()> {
        signed.validate()?;
        let signature = Signature::from_slice(&signed.signature).map_err(|_| invalid())?;
        self.key
            .verify_strict(signed.review_digest()?.0.as_bytes(), &signature)
            .map_err(|_| invalid())
    }
}

impl<M: NativeServiceManager> ProductHostBroker<M> {
    /// Read a stored, signed report; never generates or commits a decision.
    pub fn legacy_release_review(
        &self,
        auth: &HostAuth,
        original_id: &str,
        workers: &dyn IsolatedExecutorHost,
        now: p::Timestamp,
    ) -> p::Result<p::LegacyReleaseReview> {
        self.verify_web_configuration_auth(auth)?;
        p::validate_product_id(original_id)?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        let reviewer = self.legacy_release_reviewer.as_ref().ok_or_else(invalid)?;
        if let Some(saved) = self.journal.pending_legacy_review(original_id, now)? {
            reviewer.verify(&saved)?;
            self.check_legacy_configuration(&mut config, &saved.review.binding)?;
            return Ok(saved.review);
        }
        let signed = reviewer.load(original_id)?;
        if signed.review.binding.original_operation_id != original_id {
            return Err(invalid());
        }
        self.check_legacy_configuration(&mut config, &signed.review.binding)?;
        self.journal.check_legacy_review(&signed, now)?;
        if signed.review.status == p::LegacyReleaseReviewStatus::Verified {
            let deadline = window(
                now,
                signed
                    .review
                    .binding
                    .expires_at
                    .min(now.saturating_add(5_000)),
            )?;
            workers.with_quiescent_workers(&mut |worker_digest| {
                self.check_legacy_services()?;
                let mut inspection =
                    reviewer
                        .collector
                        .inspect(&signed.review, &worker_digest, deadline)?;
                let result = self.check_legacy_observation(
                    &signed,
                    inspection.as_mut(),
                    &worker_digest,
                    deadline,
                );
                let release = inspection.release();
                result.and(release)
            })?;
        }
        Ok(signed.review)
    }

    /// Same key/request is a pure lookup, including after its deadline.
    pub fn owner_legacy_release_recovery(
        &self,
        auth: &HostAuth,
        request: p::LegacyReleaseRecoveryRequest,
        workers: &dyn IsolatedExecutorHost,
        now: p::Timestamp,
    ) -> p::Result<p::LegacyReleaseRecoveryView> {
        self.verify_web_configuration_auth(auth)?;
        request.validate()?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        if config.maintenance_owner()?.as_ref() != Some(&request.owner) {
            return Err(invalid());
        }
        if let Some(view) = self.journal.find_legacy_recovery(&request)? {
            return Ok(view);
        }
        let deadline = window(now, request.requested_deadline)?;
        let reviewer = self.legacy_release_reviewer.as_ref().ok_or_else(invalid)?;
        let signed = reviewer.load(&request.review_id)?;
        if signed.review_digest()? != request.review_digest
            || signed.review.binding.review_id != request.review_id
            || signed.review.status != p::LegacyReleaseReviewStatus::Verified
            || request.requested_deadline > signed.review.binding.expires_at
        {
            return Err(invalid());
        }
        self.check_legacy_configuration(&mut config, &signed.review.binding)?;
        self.journal.check_legacy_review(&signed, now)?;
        if request.expected_revision != self.journal.revision()? {
            return Err(invalid());
        }
        workers.with_quiescent_workers(&mut |worker_digest| {
            self.check_legacy_services()?;
            let mut inspection =
                reviewer
                    .collector
                    .inspect(&signed.review, &worker_digest, deadline)?;
            let result = (|| {
                self.check_legacy_observation(
                    &signed,
                    inspection.as_mut(),
                    &worker_digest,
                    deadline,
                )?;
                self.journal.begin_legacy_recovery(
                    request.clone(),
                    signed.clone(),
                    timestamp()?,
                )?;
                self.finish_legacy_recovery(
                    &signed,
                    p::LegacyReleaseRecoveryVerification {
                        schema_version: p::SchemaVersion(1),
                        operation_id: request.operation_id.clone(),
                        request_digest: request.digest()?,
                        expected_revision: self.journal.revision()?,
                        owner: request.owner.clone(),
                        requested_deadline: request.requested_deadline,
                    },
                    inspection.as_mut(),
                    &worker_digest,
                    deadline,
                )
            })();
            let release = inspection.release();
            result.and(release)
        })?;
        self.journal
            .legacy_recovery_operation(&request.operation_id)
    }

    /// Only an explicit, separately authenticated verification finishes Intent.
    pub fn verify_owner_legacy_release_recovery(
        &self,
        auth: &HostAuth,
        verification: p::LegacyReleaseRecoveryVerification,
        workers: &dyn IsolatedExecutorHost,
        now: p::Timestamp,
    ) -> p::Result<p::LegacyReleaseRecoveryView> {
        self.verify_web_configuration_auth(auth)?;
        verification.validate()?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        let view = self
            .journal
            .legacy_recovery_operation(&verification.operation_id)?;
        if config.maintenance_owner()?.as_ref() != Some(&verification.owner)
            || view.request.owner != verification.owner
            || view.request_digest != verification.request_digest
        {
            return Err(invalid());
        }
        if view.stage == p::LegacyReleaseRecoveryStage::Committed {
            return Ok(view);
        }
        let deadline = window(now, verification.requested_deadline)?;
        if view.revision != verification.expected_revision
            || self.journal.revision()? != verification.expected_revision
        {
            return Err(invalid());
        }
        let reviewer = self.legacy_release_reviewer.as_ref().ok_or_else(invalid)?;
        let signed = self
            .journal
            .saved_legacy_review(&verification.operation_id)?;
        reviewer.verify(&signed)?;
        if verification.requested_deadline > signed.review.binding.expires_at {
            return Err(invalid());
        }
        self.check_legacy_configuration(&mut config, &signed.review.binding)?;
        workers.with_quiescent_workers(&mut |worker_digest| {
            self.check_legacy_services()?;
            let mut inspection =
                reviewer
                    .collector
                    .inspect(&signed.review, &worker_digest, deadline)?;
            let result = self.finish_legacy_recovery(
                &signed,
                verification.clone(),
                inspection.as_mut(),
                &worker_digest,
                deadline,
            );
            let release = inspection.release();
            result.and(release)
        })?;
        self.journal
            .legacy_recovery_operation(&verification.operation_id)
    }

    pub fn owner_legacy_release_recovery_operation(
        &self,
        auth: &HostAuth,
        id: &str,
    ) -> p::Result<p::LegacyReleaseRecoveryView> {
        self.verify_web_configuration_auth(auth)?;
        p::validate_product_id(id)?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        let view = self.journal.legacy_recovery_operation(id)?;
        if config.maintenance_owner()?.as_ref() != Some(&view.request.owner) {
            return Err(invalid());
        }
        Ok(view)
    }

    pub fn owner_legacy_release_recoveries(
        &self,
        auth: &HostAuth,
    ) -> p::Result<p::LegacyReleaseRecoveryList> {
        self.verify_web_configuration_auth(auth)?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        let owner = config.maintenance_owner()?.ok_or_else(invalid)?;
        self.journal.legacy_recoveries(&owner)
    }

    fn check_legacy_configuration(
        &self,
        config: &mut ProductConfigStore,
        binding: &p::LegacyReleaseReviewBinding,
    ) -> p::Result<()> {
        if binding.install_ref != self.install_id
            || config.release_configuration_identity()?
                != (
                    binding.owner.clone(),
                    binding.config_revision,
                    binding.config_digest.clone(),
                )
            || config
                .release_binding()
                .is_some_and(|b| b.operation_id == binding.original_operation_id)
        {
            return Err(invalid());
        }
        let active = self.active_release_state()?;
        if active.id != binding.original_release_ref
            || active.digest.as_ref() != Some(&binding.original_release_digest)
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn check_legacy_services(&self) -> p::Result<()> {
        use p::{BuiltinServiceId as Id, NativeServiceState as State};
        for id in [Id::Host, Id::Web, Id::Brain] {
            let observed = self.services.inspect(id)?;
            if observed.schema_version != p::SchemaVersion(1)
                || observed.service != id
                || !match id {
                    Id::Host => observed.state == State::Running,
                    Id::Web => matches!(observed.state, State::Running | State::Stopped),
                    _ => observed.state == State::Stopped,
                }
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    fn check_legacy_observation(
        &self,
        signed: &p::SignedLegacyReleaseReview,
        inspection: &mut dyn LegacyReviewInspection,
        worker_digest: &p::SchemaDigest,
        deadline: Instant,
    ) -> p::Result<()> {
        if signed.review.evidence.first()
            != Some(
                &self
                    .journal
                    .legacy_journal_evidence(&signed.review.binding, timestamp()?)?,
            )
        {
            return Err(invalid());
        }
        let observed = inspection.recheck(deadline)?;
        observed.updater_capabilities.validate()?;
        if Instant::now() >= deadline
            || timestamp()? >= signed.review.binding.expires_at
            || observed.evidence != signed.review.evidence
            || observed.worker_digest != *worker_digest
            || observed.updater_state != p::NativeServiceState::Stopped
            || observed.updater_capabilities.journal_read_max != 3
            || observed.updater_capabilities.updater_version != 3
        {
            return Err(invalid());
        }
        self.check_legacy_services()?;
        inspection.ensure_held(deadline)
    }

    fn finish_legacy_recovery(
        &self,
        signed: &p::SignedLegacyReleaseReview,
        verification: p::LegacyReleaseRecoveryVerification,
        inspection: &mut dyn LegacyReviewInspection,
        worker_digest: &p::SchemaDigest,
        deadline: Instant,
    ) -> p::Result<()> {
        self.check_legacy_observation(signed, inspection, worker_digest, deadline)?;
        let final_digest = p::canonical_digest(&(
            "forme-legacy-release-final-observation-v1",
            signed.digest()?,
            &signed.review.evidence,
            worker_digest,
        ))?;
        inspection.ensure_held(deadline)?;
        self.journal
            .commit_legacy_recovery(verification, final_digest, timestamp()?)
    }
}

fn window(now: p::Timestamp, deadline: p::Timestamp) -> p::Result<Instant> {
    if now < 0 || deadline <= now || deadline.saturating_sub(now) > 120_000 {
        return Err(invalid());
    }
    Ok(Instant::now() + Duration::from_millis((deadline - now) as u64))
}
fn timestamp() -> p::Result<p::Timestamp> {
    super::host_configuration::current_timestamp()
}
fn invalid() -> p::Error {
    p::Error("legacy release review is unavailable, stale or unverifiable; inspect the original operation".into())
}
