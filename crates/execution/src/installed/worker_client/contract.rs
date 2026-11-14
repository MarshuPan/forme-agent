use forme_protocol as p;

/// The only operations a core driver may ask the AgentExecution broker to do.
pub trait WorkerBrokerTransport: Send + Sync {
    fn dispatch_channel(
        &self,
        _input: p::ManagedChannelDeliveryInputV1,
    ) -> p::Result<p::WorkerObservation> {
        Err(p::Error("channel delivery is unavailable".into()))
    }
    fn desktop_output(&self, _job_id: &str) -> p::Result<p::DesktopOutputDescriptor> {
        Err(p::Error("desktop output descriptor is unavailable".into()))
    }
    fn register_desktop_pending(
        &self,
        _request: p::DesktopPendingAction,
    ) -> p::Result<p::DesktopPendingAction> {
        Err(p::Error("desktop authority is unavailable".into()))
    }
    fn desktop_pending_consent(&self, _pending_ref: &str) -> p::Result<p::DesktopConsentView> {
        Err(p::Error("desktop authority is unavailable".into()))
    }
    fn acquire_desktop_grant(
        &self,
        _request: p::DesktopGrantRequest,
    ) -> p::Result<p::DesktopGrantView> {
        Err(p::Error("desktop authority is unavailable".into()))
    }
    fn desktop_grant(&self, _operation_id: &str) -> p::Result<p::DesktopGrantView> {
        Err(p::Error("desktop authority is unavailable".into()))
    }

    fn dispatch(&self, request: p::WorkerDispatchRequest) -> p::Result<p::WorkerObservation>;
    fn probe(&self, job_id: &str) -> p::Result<p::WorkerObservation>;
    fn cancel(&self, job_id: &str) -> p::Result<p::WorkerCancelReceipt>;
    fn result_manifest(&self, job_id: &str) -> p::Result<p::WorkerResultManifest>;
    fn read_result(&self, request: p::WorkerResultRead) -> p::Result<p::WorkerResultChunk>;
    fn read_input(&self, request: p::WorkerInputReadRequest) -> p::Result<p::WorkerInputChunk> {
        let _ = request;
        Err(p::Error("worker input channel is unavailable".into()))
    }
}
