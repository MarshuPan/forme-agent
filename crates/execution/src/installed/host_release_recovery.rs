//! Owner-only bounded adjudication; zero service/image/backup/brain mutations.
use super::{HostAuth, NativeServiceManager, ProductHostBroker};
use crate::installed::{IsolatedExecutorHost, ProductConfigStore};
use forme_protocol as p;
use std::time::{Duration, Instant};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn owner_release_recoveries(
        &self,
        auth: &HostAuth,
    ) -> p::Result<p::ProductReleaseRecoveryList> {
        self.verify_web_configuration_auth(auth)?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        let owner = config.maintenance_owner()?.ok_or_else(invalid)?;
        self.journal.release_recoveries(&owner)
    }
    pub fn release_recovery_candidate(
        &self,
        auth: &HostAuth,
        id: &str,
        workers: &dyn IsolatedExecutorHost,
        now: p::Timestamp,
    ) -> p::Result<p::ProductReleaseRecoveryCandidate> {
        self.verify_web_configuration_auth(auth)?;
        p::validate_product_id(id)?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        let mut candidate = None;
        workers.with_quiescent_workers(&mut |worker_digest| {
            let current = self.journal.release_recovery_candidate(id, now)?;
            self.verify_recovery_configuration(&mut config, &current)?;
            let observation = self.observe_recovery(
                &current,
                worker_digest,
                now,
                Instant::now() + Duration::from_secs(5),
            )?;
            observation.release()?;
            if self.journal.revision()? != current.product_revision {
                return Err(invalid());
            }
            candidate = Some(current);
            Ok(())
        })?;
        candidate.ok_or_else(invalid)
    }

    /// Same key is a pure query, including after expiry or an interrupted intent.
    pub fn owner_release_recovery(
        &self,
        auth: &HostAuth,
        request: p::ProductReleaseRecoveryRequest,
        workers: &dyn IsolatedExecutorHost,
        now: p::Timestamp,
    ) -> p::Result<p::ProductReleaseRecoveryView> {
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
        if let Some(view) = self.journal.find_release_recovery(&request)? {
            return Ok(view);
        }
        let deadline = recovery_deadline(now, request.requested_deadline)?;
        if self.journal.revision()? != request.expected_revision {
            return Err(invalid());
        }
        workers.with_quiescent_workers(&mut |worker_digest| {
            let candidate = self
                .journal
                .release_recovery_candidate(&request.candidate.original_operation_id, now)?;
            if candidate != request.candidate {
                return Err(invalid());
            }
            self.verify_recovery_configuration(&mut config, &candidate)?;
            let mut observation =
                self.observe_recovery(&candidate, worker_digest, now, deadline)?;
            self.journal
                .begin_release_recovery(request.clone(), actual_now()?)?;
            // The store remains exclusively held until the durable decision.
            self.commit_recovery(
                &request,
                self.journal.revision()?,
                request.requested_deadline,
                deadline,
                &mut observation,
            )?;
            observation.release()
        })?;
        self.journal
            .release_recovery_operation(&request.operation_id)
    }

    /// Only this explicitly reauthenticated method may finish a durable intent.
    /// A lost response or a plain read never reaches it automatically.
    pub fn verify_owner_release_recovery(
        &self,
        auth: &HostAuth,
        verification: p::ProductReleaseRecoveryVerification,
        workers: &dyn IsolatedExecutorHost,
        now: p::Timestamp,
    ) -> p::Result<p::ProductReleaseRecoveryView> {
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
            .release_recovery_operation(&verification.operation_id)?;
        if config.maintenance_owner()?.as_ref() != Some(&verification.owner)
            || view.request.owner != verification.owner
            || view.request_digest != verification.request_digest
        {
            return Err(invalid());
        }
        if view.stage == p::ProductReleaseRecoveryStage::Committed {
            return Ok(view);
        }
        let deadline = recovery_deadline(now, verification.requested_deadline)?;
        if self.journal.revision()? != verification.expected_revision {
            return Err(invalid());
        }
        workers.with_quiescent_workers(&mut |worker_digest| {
            let candidate = self
                .journal
                .release_recovery_candidate(&view.request.candidate.original_operation_id, now)?;
            let mut bound = candidate.clone();
            bound.product_revision = view.request.candidate.product_revision;
            if bound != view.request.candidate {
                return Err(invalid());
            }
            self.verify_recovery_configuration(&mut config, &candidate)?;
            let mut observation =
                self.observe_recovery(&candidate, worker_digest, now, deadline)?;
            self.commit_recovery(
                &view.request,
                verification.expected_revision,
                verification.requested_deadline,
                deadline,
                &mut observation,
            )?;
            observation.release()
        })?;
        self.journal
            .release_recovery_operation(&verification.operation_id)
    }

    pub fn owner_release_recovery_operation(
        &self,
        auth: &HostAuth,
        id: &str,
    ) -> p::Result<p::ProductReleaseRecoveryView> {
        self.verify_web_configuration_auth(auth)?;
        p::validate_product_id(id)?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        let view = self.journal.release_recovery_operation(id)?;
        if config.maintenance_owner()?.as_ref() != Some(&view.request.owner) {
            return Err(invalid());
        }
        Ok(view)
    }

    fn verify_recovery_configuration(
        &self,
        config: &mut ProductConfigStore,
        candidate: &p::ProductReleaseRecoveryCandidate,
    ) -> p::Result<()> {
        candidate.validate()?;
        if config.release_configuration_identity()?
            != (
                candidate.owner.clone(),
                candidate.config_revision,
                candidate.config_digest.clone(),
            )
            || config
                .release_binding()
                .is_some_and(|b| b.operation_id == candidate.original_operation_id)
        {
            return Err(invalid());
        }
        let active = self.active_release_state()?;
        if active.id != candidate.original_release_ref
            || active.digest.as_ref() != Some(&candidate.original_release_digest)
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn observe_recovery(
        &self,
        candidate: &p::ProductReleaseRecoveryCandidate,
        worker_digest: p::SchemaDigest,
        now: p::Timestamp,
        deadline: Instant,
    ) -> p::Result<super::super::release_updater::RecoveryObservation> {
        let baseline = self
            .journal
            .release_recovery_baseline(&candidate.original_operation_id)?;
        let takeover = self
            .journal
            .release_takeover(&candidate.original_operation_id)?
            .ok_or_else(invalid)?;
        if now < takeover.deadline {
            return Err(invalid());
        }
        let observed = self
            .release_updater
            .as_ref()
            .ok_or_else(invalid)?
            .observe_release_recovery(
                &self.journal,
                &takeover,
                worker_digest,
                baseline.captured_at,
                deadline,
            )?;
        if observed.baseline != baseline {
            return Err(invalid());
        }
        Ok(observed)
    }

    fn commit_recovery(
        &self,
        request: &p::ProductReleaseRecoveryRequest,
        expected: u64,
        authorization_deadline: p::Timestamp,
        deadline: Instant,
        observation: &mut super::super::release_updater::RecoveryObservation,
    ) -> p::Result<()> {
        let takeover = self
            .journal
            .release_takeover(&request.candidate.original_operation_id)?
            .ok_or_else(invalid)?;
        let baseline = self
            .journal
            .release_recovery_baseline(&request.candidate.original_operation_id)?;
        self.release_updater
            .as_ref()
            .ok_or_else(invalid)?
            .recheck_recovery_files(&self.journal, &takeover, &baseline, deadline)?;
        observation.ensure_held(deadline)?;
        self.journal.commit_release_recovery(
            &request.operation_id,
            expected,
            request.digest()?,
            request.candidate.evidence_digest.clone(),
            authorization_deadline,
            actual_now()?,
        )
    }
}

fn recovery_deadline(now: p::Timestamp, deadline: p::Timestamp) -> p::Result<Instant> {
    if now < 0 || deadline <= now || deadline.saturating_sub(now) > 120_000 {
        return Err(invalid());
    }
    Ok(Instant::now() + Duration::from_millis((deadline - now) as u64))
}
fn actual_now() -> p::Result<p::Timestamp> {
    super::host_configuration::current_timestamp()
}
fn invalid() -> p::Error {
    p::Error(
        "release recovery is unavailable, stale or unverifiable; inspect the original operation"
            .into(),
    )
}
