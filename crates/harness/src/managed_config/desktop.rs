//! Computer placement comes only from the independent Host projection.
use super::*;
use forme_execution::installed::InstalledDesktopProvisioning;
use forme_policy::ComputerSurfaceLimit;

impl ManagedBrainConfig {
    pub(super) fn configure_desktop_placement(
        &self,
        capabilities: &mut CapabilityRuntimeConfig,
    ) -> p::Result<()> {
        if let Some(placement) = &self.desktop_placement {
            placement.validate()?;
            if let Some(file) = &self.worker_binding {
                let desktop = placement.binding();
                if file.endpoint != desktop.endpoint
                    || file.authority_epoch != desktop.authority_epoch
                    || file.credential_path != desktop.credential_path
                {
                    return Err(invalid());
                }
            }
        }
        let Some(settings) = self
            .managed_capabilities
            .as_ref()
            .map(|value| &value.computer)
        else {
            return Ok(());
        };
        settings.validate()?;
        if !settings.enabled {
            return Ok(());
        }
        let placement = self.desktop_placement.as_ref().ok_or_else(invalid)?;
        placement.validate_settings(settings)?;
        if self.workspace.0 != settings.scope.0 {
            return Ok(());
        }
        capabilities.computer.enabled = true;
        capabilities.computer.driver = p::ProviderId("computer:native".into());
        capabilities.computer.surfaces = placement
            .surface_refs()
            .iter()
            .map(|surface| ComputerSurfaceLimit {
                schema_version: p::SchemaVersion(1),
                surface: p::SurfaceRef(surface.clone()),
                bounds: InstalledDesktopProvisioning::coordinate_bounds(),
            })
            .collect();
        Ok(())
    }
}
