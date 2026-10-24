use super::*;

pub struct SystemReleaseUpdaterPlatform;
impl ReleaseUpdaterPlatform for SystemReleaseUpdaterPlatform {
    fn verify_updater(
        &self,
        installation: &p::InstallationManifest,
        config: &Path,
    ) -> p::Result<()> {
        #[cfg(windows)]
        {
            super::super::registration::updater::verify_installed(installation, config)
        }
        #[cfg(not(windows))]
        {
            let _ = (installation, config);
            Err(invalid())
        }
    }
    fn launch(&self, installation: &p::InstallationManifest, config: &Path) -> p::Result<()> {
        #[cfg(windows)]
        {
            super::super::registration::updater::launch(installation, config)
        }
        #[cfg(not(windows))]
        {
            let _ = (installation, config);
            Err(invalid())
        }
    }
    fn observe(
        &self,
        installation: &p::InstallationManifest,
        root: &Path,
        service: p::BuiltinServiceId,
        state: Option<p::NativeServiceState>,
    ) -> p::Result<p::SchemaDigest> {
        #[cfg(windows)]
        {
            super::super::registration::release_lifecycle::observe(
                installation,
                root,
                service,
                state,
            )
        }
        #[cfg(not(windows))]
        {
            let _ = (installation, root, service, state);
            Err(invalid())
        }
    }
    fn change(
        &self,
        installation: &p::InstallationManifest,
        root: &Path,
        service: p::BuiltinServiceId,
        action: p::NativeServiceAction,
        deadline: Instant,
    ) -> p::Result<p::SchemaDigest> {
        #[cfg(windows)]
        {
            super::super::registration::release_lifecycle::change(
                installation,
                root,
                service,
                action,
                deadline,
            )
        }
        #[cfg(not(windows))]
        {
            let _ = (installation, root, service, action, deadline);
            Err(invalid())
        }
    }
}
