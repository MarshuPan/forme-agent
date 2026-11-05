use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use forme_protocol as p;

use super::desktop_helper::{
    required_capability, validate_live_observation, worker_receipt, DesktopHelperAdapter,
    UnavailableDesktopHelper,
};
use super::worker::WorkerJournal;
use super::worker_binding::validate_workspace_scope;
use super::worker_results::{
    installation_ref, WorkerResultHostExt, WorkerResultSealRequest, WorkerResultStore,
};
use super::{HostAuth, HostCredentialDomain};

pub use super::worker_isolation::WorkerIsolation;

#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableWorkerIsolation;

impl WorkerIsolation for UnavailableWorkerIsolation {
    fn admit(&self, _job: &p::WorkerJob, _permit: &p::WorkerPermit) -> p::Result<()> {
        Err(p::Error(
            "worker isolation capability is unavailable; execution is blocked".into(),
        ))
    }

    fn cancel(&self, _job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
        Err(p::Error(
            "worker isolation capability is unavailable; cancellation is unverifiable".into(),
        ))
    }
}

pub trait IsolatedExecutorHost: Send + Sync {
    /// Internal Host-only check. Hold worker admission/observation exclusion
    /// through the callback; external implementations fail closed by default.
    fn with_quiescent_workers(
        &self,
        _action: &mut dyn FnMut(p::SchemaDigest) -> p::Result<()>,
    ) -> p::Result<()> {
        Err(p::Error(
            "worker recovery observation is unavailable".into(),
        ))
    }
    fn dispatch(
        &self,
        auth: &HostAuth,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation>;
    fn dispatch_with_execution(
        &self,
        auth: &HostAuth,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        execution: Option<p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        let _ = execution;
        self.dispatch(auth, job, permit, now)
    }
    fn probe(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation>;
    fn cancel(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt>;
    fn record_receipt(
        &self,
        auth: &HostAuth,
        receipt: p::WorkerReceipt,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation>;
    fn result_manifest(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultManifest> {
        let _ = (auth, job_id, now);
        Err(p::Error("worker result channel is unavailable".into()))
    }
    fn read_result(
        &self,
        auth: &HostAuth,
        request: p::WorkerResultRead,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultChunk> {
        let _ = (auth, request, now);
        Err(p::Error("worker result channel is unavailable".into()))
    }
}

pub struct WorkerExecutorHost<I = UnavailableWorkerIsolation, D = UnavailableDesktopHelper> {
    // Keep the durable transition and platform effect in one control-plane
    // critical section. Journal locking alone cannot prevent probe/cancel
    // from observing the gap between Started and process creation.
    pub(super) operations: Mutex<()>,
    pub(super) journal: WorkerJournal,
    agent_credential_digest: p::SchemaDigest,
    authority_epoch: p::AuthorityEpoch,
    allowed_scopes: BTreeSet<p::Scope>,
    isolation: I,
    desktop_helper: D,
    pub(super) result_store: WorkerResultStore,
    pub(super) installation_ref: String,
}

impl WorkerExecutorHost<UnavailableWorkerIsolation, UnavailableDesktopHelper> {
    pub fn open(
        journal_path: &Path,
        agent_credential_digest: p::SchemaDigest,
        authority_epoch: p::AuthorityEpoch,
        allowed_scopes: BTreeSet<p::Scope>,
    ) -> p::Result<Self> {
        Self::open_with_adapters(
            journal_path,
            agent_credential_digest,
            authority_epoch,
            allowed_scopes,
            UnavailableWorkerIsolation,
            UnavailableDesktopHelper,
        )
    }
}

impl<I: WorkerIsolation> WorkerExecutorHost<I, UnavailableDesktopHelper> {
    pub fn open_with_isolation(
        journal_path: &Path,
        agent_credential_digest: p::SchemaDigest,
        authority_epoch: p::AuthorityEpoch,
        allowed_scopes: BTreeSet<p::Scope>,
        isolation: I,
    ) -> p::Result<Self> {
        p::validate_product_digest(&agent_credential_digest)?;
        if authority_epoch.0 == 0 || allowed_scopes.is_empty() {
            return Err(p::Error(
                "worker host requires a non-zero authority epoch and scope allowlist".into(),
            ));
        }
        for scope in &allowed_scopes {
            validate_workspace_scope(scope)?;
        }
        Self::open_with_adapters(
            journal_path,
            agent_credential_digest,
            authority_epoch,
            allowed_scopes,
            isolation,
            UnavailableDesktopHelper,
        )
    }
}

impl<I: WorkerIsolation, D: DesktopHelperAdapter> WorkerExecutorHost<I, D> {
    pub fn open_with_adapters(
        journal_path: &Path,
        agent_credential_digest: p::SchemaDigest,
        authority_epoch: p::AuthorityEpoch,
        allowed_scopes: BTreeSet<p::Scope>,
        isolation: I,
        desktop_helper: D,
    ) -> p::Result<Self> {
        p::validate_product_digest(&agent_credential_digest)?;
        if authority_epoch.0 == 0 || allowed_scopes.is_empty() {
            return Err(p::Error(
                "worker host requires a non-zero authority epoch and scope allowlist".into(),
            ));
        }
        for scope in &allowed_scopes {
            validate_workspace_scope(scope)?;
        }
        let installation_ref = installation_ref(&agent_credential_digest)?;
        let result_root = journal_path.with_extension("results");
        Ok(Self {
            operations: Mutex::new(()),
            journal: WorkerJournal::open(journal_path)?,
            agent_credential_digest,
            authority_epoch,
            allowed_scopes,
            isolation,
            desktop_helper,
            result_store: WorkerResultStore::open(&result_root)?,
            installation_ref,
        })
    }

    pub(super) fn verify(&self, auth: &HostAuth) -> p::Result<()> {
        if auth.domain() != HostCredentialDomain::AgentExecution
            || auth.proof_ref() != &self.agent_credential_digest
        {
            return Err(p::Error("agent execution authorization is invalid".into()));
        }
        Ok(())
    }

    fn validate_admission(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<()> {
        job.validate()?;
        permit.validate_for(job, now)?;
        if job.authority_epoch != self.authority_epoch {
            return Err(p::Error("worker authority epoch is stale".into()));
        }
        if !self.allowed_scopes.contains(&job.workspace_scope) {
            return Err(p::Error("worker workspace scope is not admitted".into()));
        }
        Ok(())
    }

    fn prepare_desktop_action(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: &p::WorkerExecutionSpec,
        now: p::Timestamp,
    ) -> p::Result<Option<(p::DesktopActionRequest, p::DesktopSessionObservation)>> {
        let Some(binding) = &execution.desktop_session else {
            return Ok(None);
        };
        binding.validate_for(job, permit, now)?;
        let capability = required_capability(&execution.intent)?;
        if !binding.capabilities.contains(&capability) {
            return Err(p::Error("desktop capability is outside the grant".into()));
        }
        let observation = self.desktop_helper.observe(binding, now)?;
        validate_live_observation(binding, &observation, now)?;
        let timeout_ms = i64::try_from(execution.timeout.0)
            .map_err(|_| p::Error("desktop action timeout is too large".into()))?;
        let timeout_deadline = now
            .checked_add(timeout_ms)
            .ok_or_else(|| p::Error("desktop action deadline overflows".into()))?;
        let deadline = binding.expires_at.min(job.deadline).min(timeout_deadline);
        let request = p::DesktopActionRequest {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            job_id: job.job_id.clone(),
            permit_ref: permit.permit_ref.clone(),
            binding: binding.clone(),
            normalized_action_digest: execution.normalized_action_digest.clone(),
            capability,
            deadline,
        };
        request.validate_at(&observation, now)?;
        Ok(Some((request, observation)))
    }

    pub(super) fn refresh_isolation(
        &self,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        let current = self.journal.observation(job_id, now)?;
        if matches!(
            current.state,
            p::WorkerState::Pending | p::WorkerState::Running
        ) {
            let job = self.journal.job(job_id)?;
            if let Some(receipt) = self.isolation.observe(&job, now)? {
                receipt.validate_for(&job)?;
                let source = self.isolation.result_path(&job)?;
                self.seal_result(WorkerResultSealRequest {
                    job: &job,
                    receipt_ref: Some(receipt.receipt_ref.clone()),
                    result_ref: receipt.safe_result_ref.clone(),
                    outcome: Some(receipt.observed_outcome),
                    source: source.as_deref(),
                    evidence_refs: receipt.ground_truth_refs.clone(),
                    now,
                })?;
                self.isolation.result_sealed(&job, now)?;
                self.journal.record_receipt(receipt, now)?;
            }
        }
        self.journal.observation(job_id, now)
    }
}

impl<I: WorkerIsolation, D: DesktopHelperAdapter> IsolatedExecutorHost
    for WorkerExecutorHost<I, D>
{
    fn with_quiescent_workers(
        &self,
        action: &mut dyn FnMut(p::SchemaDigest) -> p::Result<()>,
    ) -> p::Result<()> {
        let _operation = self.operations.lock().map_err(|_| storage_error())?;
        action(self.journal.quiescent_digest()?)
    }
    fn dispatch(
        &self,
        auth: &HostAuth,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        self.dispatch_with_execution(auth, job, permit, None, now)
    }

    fn dispatch_with_execution(
        &self,
        auth: &HostAuth,
        job: p::WorkerJob,
        permit: p::WorkerPermit,
        execution: Option<p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        self.verify(auth)?;
        let _operation = self.operations.lock().map_err(|_| storage_error())?;
        self.validate_admission(&job, &permit, now)?;
        if let Some(execution) = &execution {
            execution.validate_for(&job, &permit, now)?;
        }

        // Duplicate dispatches must return the journaled result before asking
        // a desktop helper for a fresh liveness observation. This is what keeps
        // a terminal Unknown job from becoming a second helper attempt.
        if self
            .journal
            .existing_observation(&job.job_id, now)?
            .is_some()
        {
            let (_, observation) =
                self.journal
                    .admit_with_execution(job, permit, execution.as_ref(), now)?;
            return Ok(observation);
        }

        let desktop = match execution.as_ref() {
            Some(execution) => self.prepare_desktop_action(&job, &permit, execution, now)?,
            None => None,
        };
        let (fresh, observation) = self.journal.admit_with_execution(
            job.clone(),
            permit.clone(),
            execution.as_ref(),
            now,
        )?;
        if !fresh {
            return Ok(observation);
        }

        if let Some((request, initial_observation)) = desktop {
            self.journal.record_desktop_observation(
                &job.job_id,
                request.binding.clone(),
                initial_observation,
                now,
            )?;
            let _started = self.journal.start(&job.job_id, now)?;

            // Re-read liveness after the durable start boundary. A session can
            // be revoked between admission and the helper call.
            let live_observation =
                self.desktop_helper
                    .observe(&request.binding, now)
                    .and_then(|observation| {
                        validate_live_observation(&request.binding, &observation, now)?;
                        Ok(observation)
                    });
            let live_observation = match live_observation {
                Ok(observation) => observation,
                Err(error) => {
                    let reason = p::canonical_digest(&("desktop-session-unknown", &job.job_id))?.0;
                    self.journal.mark_unknown(&job.job_id, reason, now)?;
                    return Err(error);
                }
            };
            self.journal.record_desktop_observation(
                &job.job_id,
                request.binding.clone(),
                live_observation,
                now,
            )?;

            let intent = execution
                .as_ref()
                .map(|execution| &execution.intent)
                .ok_or_else(|| p::Error("desktop execution binding is missing".into()))?;
            let receipt = match self.desktop_helper.execute(&request, intent) {
                Ok(receipt) => receipt,
                Err(error) => {
                    let reason = p::canonical_digest(&("desktop-action-unknown", &job.job_id))?.0;
                    self.journal.mark_unknown(&job.job_id, reason, now)?;
                    return Err(error);
                }
            };
            let worker_receipt = worker_receipt(&request, receipt, now)?;
            self.seal_result(WorkerResultSealRequest {
                job: &job,
                receipt_ref: Some(worker_receipt.receipt_ref.clone()),
                result_ref: worker_receipt.safe_result_ref.clone(),
                outcome: Some(worker_receipt.observed_outcome),
                source: None,
                evidence_refs: worker_receipt.ground_truth_refs.clone(),
                now,
            })?;
            return self.journal.record_receipt(worker_receipt, now);
        }

        // Make the Running transition durable before asking the platform to
        // create a process. If process creation succeeds and the journal write
        // were attempted afterwards, a storage failure could leave a side
        // effect without a recoverable state boundary.
        let started = self.journal.start(&job.job_id, now)?;
        if let Err(error) =
            self.isolation
                .admit_with_execution(&job, &permit, execution.as_ref(), now)
        {
            let reason = p::canonical_digest(&("worker-admission-unknown", &job.job_id))?.0;
            self.journal.mark_unknown(&job.job_id, reason, now)?;
            return Err(error);
        }
        Ok(started)
    }

    fn probe(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        self.verify(auth)?;
        let _operation = self.operations.lock().map_err(|_| storage_error())?;
        match self.refresh_isolation(job_id, now) {
            Ok(observation) => Ok(observation),
            Err(error) => {
                let reason = p::canonical_digest(&("worker-observation-unknown", job_id))?.0;
                self.journal.mark_unknown(job_id, reason, now)?;
                Err(error)
            }
        }
    }

    fn cancel(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt> {
        self.verify(auth)?;
        let _operation = self.operations.lock().map_err(|_| storage_error())?;
        let job = self.journal.job(job_id)?;
        let observation = match self.refresh_isolation(job_id, now) {
            Ok(observation) => observation,
            Err(error) => {
                let reason = p::canonical_digest(&("worker-observation-unknown", job_id))?.0;
                self.journal.mark_unknown(job_id, reason, now)?;
                return Err(error);
            }
        };
        match observation.state {
            p::WorkerState::Unknown => {
                return Ok(p::WorkerCancelReceipt {
                    schema_version: p::SchemaVersion(1),
                    job_id: job.job_id,
                    plan_digest: job.plan_digest,
                    nonce: job.nonce,
                    outcome: p::WorkerOutcome::Unknown,
                    observed_at: now,
                    receipt_ref: observation.receipt_ref,
                    reason_ref: observation.reason_ref,
                })
            }
            p::WorkerState::CancelRequested => {
                return Ok(p::WorkerCancelReceipt {
                    schema_version: p::SchemaVersion(1),
                    job_id: job.job_id,
                    plan_digest: job.plan_digest,
                    nonce: job.nonce,
                    outcome: p::WorkerOutcome::Unknown,
                    observed_at: now,
                    receipt_ref: observation.receipt_ref,
                    reason_ref: observation
                        .reason_ref
                        .or_else(|| Some("worker-cancel-outcome-unknown".into())),
                })
            }
            p::WorkerState::Completed | p::WorkerState::Failed | p::WorkerState::Cancelled => {
                return Err(p::Error("worker job is already terminal".into()))
            }
            p::WorkerState::Pending | p::WorkerState::Running => {}
        }
        self.journal.request_cancel(job_id, now)?;
        let receipt = match self.isolation.cancel_at(&job, now) {
            Ok(receipt) => receipt,
            Err(error) => {
                let reason = p::canonical_digest(&("worker-cancel-unknown", job_id))?.0;
                self.journal.mark_unknown(job_id, reason, now)?;
                return Err(error);
            }
        };
        let finalized = (|| {
            receipt.validate_for(&job)?;
            let source = self.isolation.result_path(&job)?;
            let result_digest =
                p::canonical_digest(&("forme-worker-cancel-result-v1", &job, &receipt))?;
            self.seal_result(WorkerResultSealRequest {
                job: &job,
                receipt_ref: receipt.receipt_ref.clone(),
                result_ref: Some(format!("worker-cancel-result-{}", result_digest.0)),
                outcome: Some(receipt.outcome),
                source: source.as_deref(),
                evidence_refs: Vec::new(),
                now,
            })?;
            self.isolation.result_sealed(&job, now)?;
            self.journal.record_cancel_receipt(receipt.clone(), now)
        })();
        if let Err(error) = finalized {
            let reason = p::canonical_digest(&("worker-cancel-seal-unknown", job_id))?.0;
            self.journal.mark_unknown(job_id, reason, now)?;
            return Err(error);
        }
        Ok(receipt)
    }

    fn record_receipt(
        &self,
        auth: &HostAuth,
        receipt: p::WorkerReceipt,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        self.verify(auth)?;
        let _operation = self.operations.lock().map_err(|_| storage_error())?;
        if receipt.authority_epoch != self.authority_epoch {
            return Err(p::Error("worker receipt authority epoch is stale".into()));
        }
        self.journal.confirm_receipt(&receipt, now)
    }

    fn result_manifest(
        &self,
        auth: &HostAuth,
        job_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultManifest> {
        self.result_manifest_endpoint(auth, job_id, now)
    }

    fn read_result(
        &self,
        auth: &HostAuth,
        request: p::WorkerResultRead,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerResultChunk> {
        self.read_result_endpoint(auth, request, now)
    }
}

fn storage_error() -> p::Error {
    p::Error("worker journal storage is unavailable; outcome must be checked".into())
}
