//! Select the same managed program authority for admission and actual egress.
use super::*;
impl<I: WorkerIsolation, D: DesktopHelperAdapter> WorkerExecutorHost<I, D> {
    #[cfg(windows)]
    pub fn with_managed_mcp_programs(
        mut self,
        programs: std::sync::Arc<super::super::InstalledManagedMcpPrograms>,
        runtime_root: std::path::PathBuf,
    ) -> p::Result<Self> {
        self.managed_mcp = Some(super::super::managed_mcp_stdio::ManagedMcpIsolation::open(
            runtime_root,
            programs,
        )?);
        Ok(self)
    }
    pub(super) fn isolation_for(&self, job: &p::WorkerJob) -> p::Result<&dyn WorkerIsolation> {
        if job.sandbox_profile_ref == p::MANAGED_MCP_PROGRAM_PROFILE {
            #[cfg(windows)]
            if let Some(isolation) = &self.managed_mcp {
                return Ok(isolation);
            }
            return Err(unavailable());
        }
        Ok(&self.isolation)
    }
    pub(super) fn with_mcp_quiescent(
        &self,
        action: &mut dyn FnMut() -> p::Result<()>,
    ) -> p::Result<()> {
        #[cfg(windows)]
        if let Some(isolation) = &self.managed_mcp {
            return isolation.with_quiescent_resources(action);
        }
        action()
    }
    pub(super) fn dispatch_mcp(
        &self,
        auth: &HostAuth,
        request: p::WorkerDispatchRequest,
        now: p::Timestamp,
        entered: std::time::Instant,
    ) -> p::Result<p::WorkerObservation> {
        #[cfg(windows)]
        {
            let isolation = self.managed_mcp.as_ref().ok_or_else(unavailable)?;
            isolation
                .programs
                .with_discovery(&request, now, entered, |admission| {
                    self.dispatch_authorized(
                        auth,
                        request.clone(),
                        now,
                        None,
                        Some(&|| isolation.admit_authorized(admission.clone(), now)),
                    )
                })
        }
        #[cfg(not(windows))]
        {
            let _ = (auth, request, now, entered);
            Err(unavailable())
        }
    }
}
fn unavailable() -> p::Error {
    p::Error("managed MCP execution authority is unavailable".into())
}
