//! Defers owner binding until first claim while sharing one Host journal writer.
use super::*;
use crate::installed::ProductConfigStore;
use std::path::PathBuf;

pub struct InstalledDesktopAuthority {
    path: PathBuf,
    installation_ref: String,
    epoch: p::AuthorityEpoch,
    configuration: Arc<Mutex<ProductConfigStore>>,
    provider: Arc<dyn DesktopSessionCatalogProvider>,
    initialized: Mutex<Option<Arc<DesktopAuthority>>>,
}
impl InstalledDesktopAuthority {
    /// Constructing the control surface does not enumerate sessions, start a
    /// helper, or create an owner before the product claim is committed.
    pub fn new(
        path: PathBuf,
        installation_ref: String,
        epoch: p::AuthorityEpoch,
        configuration: Arc<Mutex<ProductConfigStore>>,
        provider: Arc<dyn DesktopSessionCatalogProvider>,
    ) -> p::Result<Self> {
        crate::installed::reject_linked_ancestors(&path)?;
        p::validate_product_id(&installation_ref)?;
        if !path.is_absolute() || epoch.0 == 0 {
            return Err(invalid());
        }
        Ok(Self {
            path,
            installation_ref,
            epoch,
            configuration,
            provider,
            initialized: Mutex::new(None),
        })
    }
    pub(crate) fn with_pending<T>(
        &self,
        pending: &p::DesktopPendingAction,
        action: impl FnOnce() -> p::Result<T>,
    ) -> p::Result<T> {
        let config = self.configuration.lock().map_err(|_| invalid())?;
        config.validate_desktop_pending(pending)?;
        action()
    }
    pub(crate) fn with_execution<T>(
        &self,
        job: &p::WorkerJob,
        execution: &p::WorkerExecutionSpec,
        action: impl FnOnce() -> p::Result<T>,
    ) -> p::Result<T> {
        let config = self.configuration.lock().map_err(|_| invalid())?;
        config.validate_desktop_execution(job, execution)?;
        action()
    }
    pub(crate) fn installation_ref(&self) -> &str {
        &self.installation_ref
    }
    pub(crate) fn authority(&self) -> p::Result<Arc<DesktopAuthority>> {
        let mut initialized = self.initialized.lock().map_err(|_| invalid())?;
        let view = self
            .configuration
            .lock()
            .map_err(|_| invalid())?
            .snapshot()?;
        let owner = p::VerifiedPrincipal(view.owner.ok_or_else(invalid)?.0);
        if let Some(authority) = initialized.as_ref() {
            authority.identity_matches(&owner, self.epoch)?;
            return Ok(authority.clone());
        }
        let authority = Arc::new(DesktopAuthority::open(
            &self.path,
            self.installation_ref.clone(),
            owner,
            self.epoch,
            self.provider.clone(),
        )?);
        *initialized = Some(authority.clone());
        Ok(authority)
    }
}
