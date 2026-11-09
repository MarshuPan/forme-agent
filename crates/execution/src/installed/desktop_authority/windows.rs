//! Read-only WTS observation using the existing audited native boundary.
use super::*;
use crate::installed::windows_desktop_session_native::WindowsDesktopSessionHost;

pub struct WindowsDesktopSessionCatalog {
    session: WindowsDesktopSessionHost,
}
impl WindowsDesktopSessionCatalog {
    pub fn new(helper_image: impl AsRef<Path>) -> p::Result<Self> {
        Ok(Self {
            session: WindowsDesktopSessionHost::new(helper_image.as_ref())?,
        })
    }
}
impl DesktopSessionCatalogProvider for WindowsDesktopSessionCatalog {
    fn sessions(&self) -> p::Result<Vec<ObservedDesktopSession>> {
        self.session.catalog()
    }
    fn recheck(&self, original: &ObservedDesktopSession) -> p::Result<()> {
        self.session.recheck_observed(original)
    }
}
