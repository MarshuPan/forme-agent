//! A fixed read-only offline worker. The configuration lock spans the check.
use super::{HostAuth, NativeServiceManager, ProductHostBroker};
use crate::installed::maintenance_pipe::{MaintenancePipe, ReleaseDataProgram};
use forme_protocol as p;
use std::time::{Duration, Instant};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn verify_installed_backup(
        &self,
        auth: &HostAuth,
        request: p::ProductBackupVerificationRequest,
    ) -> p::Result<p::ProductBackupVerificationResponse> {
        self.verify_web_configuration_auth(auth)?;
        request.validate()?;
        let mut store = self
            .product_config
            .as_ref()
            .ok_or_else(unavailable)?
            .lock()
            .map_err(|_| unavailable())?;
        let current = store.snapshot()?;
        if self.install_id != request.install_ref
            || current.owner.as_ref() != Some(&request.owner)
            || current.active_revision != request.expected_config_revision
            || self.journal.operations(0)?.iter().any(|operation| {
                operation.stage != p::MaintenanceStage::Committed
                    && self
                        .journal
                        .command_kind(&operation.operation_id)
                        .map_or(true, |kind| kind.is_lifecycle_transaction())
            })
        {
            return Err(unavailable());
        }
        let installation = self
            .restore_worker
            .as_ref()
            .ok_or_else(unavailable)?
            .installation()
            .clone();
        if installation.install_id != request.install_ref {
            return Err(unavailable());
        }
        let program = ReleaseDataProgram::new(installation)?;
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut pipe: MaintenancePipe<p::ProductBackupVerificationResponse> =
            program.spawn_backup_verification(deadline)?;
        let response = pipe.exchange(&request)?;
        response.validate_for(&request)?;
        pipe.finish()?;
        Ok(response)
    }
}

fn unavailable() -> p::Error {
    p::Error("installed backup verification is unavailable, conflicted or stale".into())
}
