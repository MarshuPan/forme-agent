//! Owner controls share the exact authority consumed by the worker host.
use super::*;

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn with_desktop_authority(
        mut self,
        authority: Arc<super::super::InstalledDesktopAuthority>,
    ) -> p::Result<Self> {
        if authority.installation_ref() != self.install_id {
            return Err(p::Error(
                "desktop authority belongs to another installation".into(),
            ));
        }
        self.desktop_authority = Some(authority);
        Ok(self)
    }
    pub(super) fn desktop_owner_authority(
        &self,
        auth: &HostAuth,
    ) -> p::Result<Arc<super::super::DesktopAuthority>> {
        self.verify_web_configuration_auth(auth)?;
        self.desktop_authority
            .as_ref()
            .ok_or_else(|| p::Error("desktop authority is unavailable".into()))?
            .authority()
    }
}
