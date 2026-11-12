//! Dedicated AgentExecution admission; generic dispatch cannot select this grant.
use super::*;
impl<I: WorkerIsolation, D: DesktopHelperAdapter> WorkerExecutorHost<I, D> {
    #[cfg(windows)]
    pub fn with_channel_delivery(
        mut self,
        channels: Arc<super::super::InstalledChannelDelivery>,
        root: std::path::PathBuf,
        image: std::path::PathBuf,
    ) -> p::Result<Self> {
        self.managed_channel = Some(
            super::super::channel_delivery::isolation::ChannelIsolation::open(
                root,
                channels.clone(),
                image,
            )?,
        );
        channels.bind_consumer()?;
        Ok(self)
    }
    pub(super) fn with_channel_quiescent(
        &self,
        action: &mut dyn FnMut() -> p::Result<()>,
    ) -> p::Result<()> {
        #[cfg(windows)]
        if let Some(isolation) = &self.managed_channel {
            return isolation.with_quiescent_resources(action);
        }
        action()
    }
    pub(super) fn dispatch_channel_input(
        &self,
        auth: &HostAuth,
        input: p::ManagedChannelDeliveryInputV1,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        self.verify(auth)?;
        self.validate_admission(&input.execution.job, &input.execution.permit, now)?;
        #[cfg(windows)]
        {
            let isolation = self.managed_channel.as_ref().ok_or_else(storage_error)?;
            let request = input.execution.clone();
            isolation.channels.with_admission(
                input,
                &self.installation_ref,
                now,
                std::time::Instant::now(),
                |admission| {
                    self.dispatch_authorized(
                        auth,
                        request,
                        now,
                        None,
                        Some(&|| isolation.admit_authorized(admission.clone(), now)),
                    )
                },
            )
        }
        #[cfg(not(windows))]
        Err(storage_error())
    }
}
