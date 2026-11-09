//! The current owner configuration is checked again at each desktop boundary.
use super::*;
use crate::installed::{InstalledDesktopProvisioning, InstalledFileProvisioning};

impl ProductConfigStore {
    pub fn open_with_installed_placements(
        path: &Path,
        file: Option<InstalledFileProvisioning>,
        desktop: Option<InstalledDesktopProvisioning>,
    ) -> p::Result<Mutex<Self>> {
        let result = Self::open_with_installed_file(path, file)?;
        {
            let mut store = result.lock().map_err(|_| invalid())?;
            if let Some(value) = &desktop {
                value.validate_template(&store.template)?;
            }
            store.installed_desktop = desktop;
        }
        Ok(result)
    }
    fn enabled_desktop(&self) -> p::Result<&InstalledDesktopProvisioning> {
        let placement = self.state.desktop.as_ref().ok_or_else(invalid)?;
        let settings = &self
            .state
            .managed_capabilities
            .as_ref()
            .ok_or_else(invalid)?
            .computer;
        placement.validate_settings(settings)?;
        if !settings.enabled {
            return Err(invalid());
        }
        Ok(placement)
    }
    pub(crate) fn validate_desktop_pending(
        &self,
        pending: &p::DesktopPendingAction,
    ) -> p::Result<()> {
        if self.state.owner.as_ref().map(|owner| owner.0.as_str())
            != Some(pending.owner_ref.0.as_str())
        {
            return Err(invalid());
        }
        self.enabled_desktop()?.validate_pending(pending)
    }
    pub(crate) fn validate_desktop_execution(
        &self,
        job: &p::WorkerJob,
        execution: &p::WorkerExecutionSpec,
    ) -> p::Result<()> {
        if self.state.owner.as_ref().map(|owner| owner.0.as_str()) != Some(job.owner_ref.0.as_str())
        {
            return Err(invalid());
        }
        self.enabled_desktop()?.validate_execution(job, execution)
    }
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
