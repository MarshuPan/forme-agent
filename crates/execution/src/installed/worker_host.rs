use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

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

mod contract;
mod desktop;
mod mcp;
mod output;
pub use contract::IsolatedExecutorHost;

pub struct WorkerExecutorHost<I = UnavailableWorkerIsolation, D = UnavailableDesktopHelper> {
    // Keep the durable transition and platform effect in one control-plane
    // critical section. Journal locking alone cannot prevent probe/cancel
    // from observing the gap between Started and process creation.
    pub(super) operations: Mutex<()>,
    pub(super) journal: Arc<WorkerJournal>,
    agent_credential_digest: p::SchemaDigest,
    authority_epoch: p::AuthorityEpoch,
    allowed_scopes: BTreeSet<p::Scope>,
    isolation: I,
    #[cfg(windows)]
    managed_mcp: Option<super::managed_mcp_stdio::ManagedMcpIsolation>,
    desktop_helper: D,
    desktop_authority: Option<std::sync::Arc<super::DesktopAuthority>>,
    installed_desktop_authority: Option<std::sync::Arc<super::InstalledDesktopAuthority>>,
    pub(super) result_store: Arc<WorkerResultStore>,
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
            journal: Arc::new(WorkerJournal::open(journal_path)?),
            agent_credential_digest,
            authority_epoch,
            allowed_scopes,
            isolation,
            #[cfg(windows)]
            managed_mcp: None,
            desktop_helper,
            desktop_authority: None,
            installed_desktop_authority: None,
            result_store: Arc::new(WorkerResultStore::open(&result_root)?),
            installation_ref,
        })
    }

    /// Installs the same Host-owned authority used by owner consent controls.
    pub fn with_desktop_authority(
        mut self,
        authority: std::sync::Arc<super::DesktopAuthority>,
    ) -> Self {
        self.desktop_authority = Some(authority);
        self
    }

    pub fn with_installed_desktop_authority(
        mut self,
        authority: std::sync::Arc<super::InstalledDesktopAuthority>,
    ) -> Self {
        self.installed_desktop_authority = Some(authority);
        self
    }
    fn desktop_authority(&self) -> p::Result<std::sync::Arc<super::DesktopAuthority>> {
        if let Some(installed) = &self.installed_desktop_authority {
            return installed.authority();
        }
        self.desktop_authority
            .clone()
            .ok_or_else(|| p::Error("desktop authority is unavailable".into()))
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
            if let Some(receipt) = self.isolation_for(&job)?.observe(&job, now)? {
                receipt.validate_for(&job)?;
                let source = self.isolation_for(&job)?.result_path(&job)?;
                self.seal_result(WorkerResultSealRequest {
                    job: &job,
                    receipt_ref: Some(receipt.receipt_ref.clone()),
                    result_ref: receipt.safe_result_ref.clone(),
                    outcome: Some(receipt.observed_outcome),
                    source: source.as_deref(),
                    evidence_refs: receipt.ground_truth_refs.clone(),
                    now,
                })?;
                self.isolation_for(&job)?.result_sealed(&job, now)?;
                self.journal.record_receipt(receipt, now)?;
            }
        }
        self.journal.observation(job_id, now)
    }
}

impl<I: WorkerIsolation, D: DesktopHelperAdapter> IsolatedExecutorHost
    for WorkerExecutorHost<I, D>
{
    fn register_desktop_pending(
        &self,
        auth: &HostAuth,
        request: p::DesktopPendingAction,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopPendingAction> {
        self.verify(auth)?;
        self.desktop_authority()?
            .identity_matches(&request.owner_ref, self.authority_epoch)?;
        let authority = self.desktop_authority()?;
        match &self.installed_desktop_authority {
            Some(installed) => installed.with_pending(&request, || {
                authority.register_pending(request.clone(), now)
            }),
            None => authority.register_pending(request, now),
        }
    }
    fn desktop_pending_consent(
        &self,
        auth: &HostAuth,
        pending_ref: &str,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopConsentView> {
        self.verify(auth)?;
        self.desktop_authority()?.pending_consent(pending_ref, now)
    }
    fn acquire_desktop_grant(
        &self,
        auth: &HostAuth,
        request: p::DesktopGrantRequest,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopGrantView> {
        self.verify(auth)?;
        self.validate_admission(&request.job, &request.permit, now)?;
        self.desktop_authority()?
            .identity_matches(&request.job.owner_ref, self.authority_epoch)?;
        let authority = self.desktop_authority()?;
        match &self.installed_desktop_authority {
            Some(installed) => installed.with_execution(&request.job, &request.execution, || {
                authority.issue(request.clone(), now)
            }),
            None => authority.issue(request, now),
        }
    }
    fn desktop_grant(
        &self,
        auth: &HostAuth,
        operation_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopGrantView> {
        self.verify(auth)?;
        self.desktop_authority()?.grant_operation(operation_id, now)
    }
    fn with_quiescent_workers(
        &self,
        action: &mut dyn FnMut(p::SchemaDigest) -> p::Result<()>,
    ) -> p::Result<()> {
        let _operation = self.operations.lock().map_err(|_| storage_error())?;
        self.isolation.with_quiescent_resources(&mut || {
            self.with_mcp_quiescent(&mut || action(self.journal.quiescent_digest()?))
        })
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
        let entered = std::time::Instant::now();
        self.verify(auth)?;
        self.validate_admission(&job, &permit, now)?;
        if let Some(spec) = &execution {
            spec.validate_for(&job, &permit, now)?;
        }
        // Original observations remain readable after disable/revoke and never
        // need a new config/authority lock or helper observation.
        if self
            .journal
            .existing_observation(&job.job_id, now)?
            .is_some()
        {
            return self
                .journal
                .admit_with_execution(job, permit, execution.as_ref(), now)
                .map(|(_, observation)| observation);
        }
        let desktop = execution.as_ref().is_some_and(|spec| {
            spec.backend == p::BackendKind::Computer || spec.desktop_session.is_some()
        });
        let authority = if desktop {
            Some(self.desktop_authority()?)
        } else {
            None
        };
        let request = p::WorkerDispatchRequest {
            schema_version: p::SchemaVersion(1),
            job,
            permit,
            execution,
        };
        if request.execution.as_ref().is_some_and(|spec| {
            matches!(spec.intent.parameters, p::ActionParameters::McpDiscovery(_))
        }) || request.job.sandbox_profile_ref == p::MANAGED_MCP_PROGRAM_PROFILE
        {
            return self.dispatch_mcp(auth, request, now, entered);
        }
        if desktop {
            if let Some(installed) = &self.installed_desktop_authority {
                // Match maintenance's config -> worker -> authority lock order.
                return installed.with_execution(
                    &request.job,
                    request
                        .execution
                        .as_ref()
                        .ok_or_else(|| p::Error("desktop execution is missing".into()))?,
                    || self.dispatch_authorized(auth, request.clone(), now, authority, None),
                );
            }
        }
        self.dispatch_authorized(auth, request, now, authority, None)
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
        let receipt = match self.isolation_for(&job)?.cancel_at(&job, now) {
            Ok(receipt) => receipt,
            Err(error) => {
                let reason = p::canonical_digest(&("worker-cancel-unknown", job_id))?.0;
                self.journal.mark_unknown(job_id, reason, now)?;
                return Err(error);
            }
        };
        let finalized = (|| {
            receipt.validate_for(&job)?;
            let source = self.isolation_for(&job)?.result_path(&job)?;
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
            self.isolation_for(&job)?.result_sealed(&job, now)?;
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

    fn desktop_output(
        &self,
        auth: &HostAuth,
        job_id: &str,
        _now: p::Timestamp,
    ) -> p::Result<p::DesktopOutputDescriptor> {
        self.verify(auth)?;
        let _operation = self.operations.lock().map_err(|_| storage_error())?;
        self.journal.desktop_output(job_id)
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
