use super::*;

pub struct SystemReleaseUpdaterPlatform;
impl ReleaseUpdaterPlatform for SystemReleaseUpdaterPlatform {
    fn observe_recovery_boundary(
        &self,
        installation: &p::InstallationManifest,
        root: &Path,
    ) -> p::Result<p::SchemaDigest> {
        #[cfg(windows)]
        {
            let updater = super::super::registration::updater::observe_stopped(
                installation,
                &installation.paths.config.join("host.json"),
            )?;
            super::capabilities::verify(installation)?;
            let mut configurations = Vec::new();
            for id in services() {
                configurations.push(
                    super::super::registration::release_lifecycle::recovery_configuration(
                        installation,
                        root,
                        id,
                    )?,
                );
            }
            p::canonical_digest(&(
                "forme-release-recovery-services-v1",
                updater,
                configurations,
            ))
        }
        #[cfg(not(windows))]
        {
            let _ = (installation, root);
            Err(invalid())
        }
    }
    fn verify_updater(
        &self,
        installation: &p::InstallationManifest,
        config: &Path,
    ) -> p::Result<()> {
        #[cfg(windows)]
        {
            super::super::registration::updater::verify_installed(installation, config)?;
            // Preflight must bind the fixed updater's actual version even when
            // the Brain is running and no offline recovery baseline is taken.
            super::capabilities::verify(installation)
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
