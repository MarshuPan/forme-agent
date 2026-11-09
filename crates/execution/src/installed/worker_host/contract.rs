//! Authenticated executor surface; additions fail closed for older hosts.
use crate::installed::HostAuth;
use forme_protocol as p;

pub trait IsolatedExecutorHost: Send + Sync {
    fn desktop_output(
        &self,
        _auth: &HostAuth,
        _job_id: &str,
        _now: p::Timestamp,
    ) -> p::Result<p::DesktopOutputDescriptor> {
        Err(p::Error("desktop output descriptor is unavailable".into()))
    }
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
    fn register_desktop_pending(
        &self,
        _auth: &HostAuth,
        _request: p::DesktopPendingAction,
        _now: p::Timestamp,
    ) -> p::Result<p::DesktopPendingAction> {
        Err(p::Error("desktop authority is unavailable".into()))
    }
    fn desktop_pending_consent(
        &self,
        _auth: &HostAuth,
        _pending_ref: &str,
        _now: p::Timestamp,
    ) -> p::Result<p::DesktopConsentView> {
        Err(p::Error("desktop authority is unavailable".into()))
    }
    fn acquire_desktop_grant(
        &self,
        _auth: &HostAuth,
        _request: p::DesktopGrantRequest,
        _now: p::Timestamp,
    ) -> p::Result<p::DesktopGrantView> {
        Err(p::Error("desktop authority is unavailable".into()))
    }
    fn desktop_grant(
        &self,
        _auth: &HostAuth,
        _operation_id: &str,
        _now: p::Timestamp,
    ) -> p::Result<p::DesktopGrantView> {
        Err(p::Error("desktop authority is unavailable".into()))
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
