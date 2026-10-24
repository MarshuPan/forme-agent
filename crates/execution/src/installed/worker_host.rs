use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use forme_protocol as p;

use super::worker::WorkerJournal;
use super::{HostAuth, HostCredentialDomain};

/// An isolation implementation must represent real OS-level admission. The
/// default is intentionally unavailable, so an unconfigured product cannot
/// silently execute a worker as the brain service account.
pub trait WorkerIsolation: Send + Sync {
    fn admit(&self, job: &p::WorkerJob, permit: &p::WorkerPermit) -> p::Result<()>;

    fn admit_with_execution(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        _execution: Option<&p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<()> {
        self.admit_at(job, permit, now)
    }

    fn admit_at(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        _now: p::Timestamp,
    ) -> p::Result<()> {
        self.admit(job, permit)
    }

    fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt>;

    /// Return a durable receipt only when the underlying worker has a known
    /// terminal result. The default keeps non-process fixtures observation
    /// only; it never invents a completion outcome.
    fn observe(
        &self,
        _job: &p::WorkerJob,
        _now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        Ok(None)
    }

    /// The timestamp-aware form lets a real process adapter bind a cancel
    /// receipt to the actual observation time without changing older test
    /// adapters that implement `cancel`.
    fn cancel_at(
        &self,
        job: &p::WorkerJob,
        _now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt> {
        self.cancel(job)
    }
}

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
}

pub struct WorkerExecutorHost<I = UnavailableWorkerIsolation> {
    // Keep the durable transition and platform effect in one control-plane
    // critical section. Journal locking alone cannot prevent probe/cancel
    // from observing the gap between Started and process creation.
    operations: Mutex<()>,
    journal: WorkerJournal,
    agent_credential_digest: p::SchemaDigest,
    authority_epoch: p::AuthorityEpoch,
    allowed_scopes: BTreeSet<p::Scope>,
    isolation: I,
}

impl WorkerExecutorHost<UnavailableWorkerIsolation> {
    pub fn open(
        journal_path: &Path,
        agent_credential_digest: p::SchemaDigest,
        authority_epoch: p::AuthorityEpoch,
        allowed_scopes: BTreeSet<p::Scope>,
    ) -> p::Result<Self> {
        Self::open_with_isolation(
            journal_path,
            agent_credential_digest,
            authority_epoch,
            allowed_scopes,
            UnavailableWorkerIsolation,
        )
    }
}

impl<I: WorkerIsolation> WorkerExecutorHost<I> {
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
        Ok(Self {
            operations: Mutex::new(()),
            journal: WorkerJournal::open(journal_path)?,
            agent_credential_digest,
            authority_epoch,
            allowed_scopes,
            isolation,
        })
    }

    fn verify(&self, auth: &HostAuth) -> p::Result<()> {
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

    fn refresh_isolation(
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
                self.journal.record_receipt(receipt, now)?;
            }
        }
        self.journal.observation(job_id, now)
    }
}

impl<I: WorkerIsolation> IsolatedExecutorHost for WorkerExecutorHost<I> {
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
        let (fresh, observation) = self.journal.admit_with_execution(
            job.clone(),
            permit.clone(),
            execution.as_ref(),
            now,
        )?;
        if !fresh {
            return Ok(observation);
        }
        // Make the Running transition durable before asking the platform to
        // create a process. If process creation succeeds and the journal
        // write were attempted afterwards, a storage failure could leave a
        // side effect without a recoverable state boundary.
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
        receipt.validate_for(&job)?;
        self.journal.record_cancel_receipt(receipt.clone(), now)?;
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
        self.journal.record_receipt(receipt, now)
    }
}

fn validate_workspace_scope(value: &p::Scope) -> p::Result<()> {
    if value.0.len() > 512
        || !value.0.starts_with("workspace:")
        || value.0.contains(['\\', '\0'])
        || value.0.chars().any(char::is_control)
        || value
            .0
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(p::Error("worker workspace scope is invalid".into()));
    }
    Ok(())
}

fn storage_error() -> p::Error {
    p::Error("worker journal storage is unavailable; outcome must be checked".into())
}
