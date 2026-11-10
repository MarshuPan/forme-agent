//! Configuration exclusion covers native admission and each actual RPC write.
use super::*;
use std::{
    sync::{MutexGuard, TryLockError},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) struct DiscoveryAdmission {
    pub(crate) spec: p::ManagedMcpDiscoverySpec,
    pub(crate) lease: Arc<ManagedMcpProgramLease>,
    pub(crate) deadline: Instant,
    pub(crate) job: p::WorkerJob,
    registry: Arc<ManagedMcpProgramRegistry>,
}
impl InstalledManagedMcpPrograms {
    pub(crate) fn with_discovery<T>(
        &self,
        request: &p::WorkerDispatchRequest,
        now: p::Timestamp,
        entered: Instant,
        action: impl FnOnce(DiscoveryAdmission) -> p::Result<T>,
    ) -> p::Result<T> {
        let execution = request.execution.as_ref().ok_or_else(invalid)?;
        execution.validate_for(&request.job, &request.permit, now)?;
        let p::ActionParameters::McpDiscovery(spec) = &execution.intent.parameters else {
            return Err(invalid());
        };
        spec.validate_intent(&execution.intent)?;
        if request.job.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
            || request.job.authority_epoch != self.epoch
        {
            return Err(invalid());
        }
        let remaining = u64::try_from(request.job.deadline.checked_sub(now).ok_or_else(invalid)?)
            .map_err(|_| invalid())?
            .min(spec.timeout.0);
        let deadline = entered
            .checked_add(Duration::from_millis(remaining))
            .ok_or_else(invalid)?;
        let cancel = crate::CancelToken::default();
        let mut config = self.configuration_until(deadline, &cancel)?;
        let snapshot = config.snapshot()?;
        let registry = self.registry_for(&snapshot)?;
        self.check_discovery(&mut config, &request.job, spec)?;
        let lease = registry.acquire(
            &p::OwnerPrincipalRef(request.job.owner_ref.0.clone()),
            self.epoch,
            &spec.program_ref,
            &spec.registration_digest,
            &request.job.workspace_scope,
        )?;
        live(deadline, &cancel)?;
        // The configuration guard remains held through worker journal start and
        // native launch. The controller uses cancellable try_lock after launch.
        let result = action(DiscoveryAdmission {
            spec: spec.clone(),
            lease: Arc::new(lease),
            deadline,
            job: request.job.clone(),
            registry,
        });
        drop(config);
        result
    }
    pub(crate) fn with_discovery_egress<T>(
        &self,
        admission: &DiscoveryAdmission,
        cancelled: &crate::CancelToken,
        write: impl FnOnce() -> p::Result<T>,
    ) -> p::Result<T> {
        let mut config = self.configuration_until(admission.deadline, cancelled)?;
        self.check_discovery(&mut config, &admission.job, &admission.spec)?;
        admission.registry.with_active(&admission.lease, || {
            live(admission.deadline, cancelled)?;
            let result = write()?;
            live(admission.deadline, cancelled)?;
            Ok(result)
        })
    }
    fn check_discovery(
        &self,
        config: &mut ProductConfigStore,
        job: &p::WorkerJob,
        spec: &p::ManagedMcpDiscoverySpec,
    ) -> p::Result<()> {
        let snapshot = config.snapshot_settings_v3()?;
        if snapshot.install_ref != self.installation
            || snapshot.owner.as_ref().map(|owner| &owner.0) != Some(&job.owner_ref.0)
            || job.authority_epoch != self.epoch
        {
            return Err(invalid());
        }
        let settings = snapshot
            .managed_capabilities
            .mcp_servers
            .iter()
            .find(|server| server.server_ref == spec.server.0)
            .ok_or_else(invalid)?;
        spec.validate_settings(settings, &job.workspace_scope)
    }
    fn configuration_until(
        &self,
        deadline: Instant,
        cancelled: &crate::CancelToken,
    ) -> p::Result<MutexGuard<'_, ProductConfigStore>> {
        loop {
            live(deadline, cancelled)?;
            match self.configuration.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(TryLockError::Poisoned(_)) => return Err(invalid()),
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    }
}
fn live(deadline: Instant, cancelled: &crate::CancelToken) -> p::Result<()> {
    if cancelled.is_cancelled() || Instant::now() >= deadline {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
