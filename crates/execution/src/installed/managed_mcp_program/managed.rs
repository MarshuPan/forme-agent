//! Lazy owner binding; configuration exclusion precedes registry exclusion.
use super::*;
use crate::installed::ProductConfigStore;
use std::sync::Arc;

#[path = "admission.rs"]
pub(crate) mod admission;

pub struct InstalledManagedMcpPrograms {
    journal: PathBuf,
    installation: String,
    epoch: p::AuthorityEpoch,
    configuration: Arc<Mutex<ProductConfigStore>>,
    root: PathBuf,
    scopes: Vec<p::Scope>,
    protected: Vec<PathBuf>,
    registry: Mutex<Option<Arc<ManagedMcpProgramRegistry>>>,
}
impl InstalledManagedMcpPrograms {
    pub fn new(
        journal: PathBuf,
        installation: String,
        epoch: p::AuthorityEpoch,
        configuration: Arc<Mutex<ProductConfigStore>>,
        root: PathBuf,
        scopes: Vec<p::Scope>,
        protected: Vec<PathBuf>,
    ) -> p::Result<Self> {
        p::validate_product_id(&installation)?;
        crate::installed::reject_linked_ancestors(&journal)?;
        crate::installed::reject_linked_ancestors(&root)?;
        if epoch.0 == 0 || scopes.is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            journal,
            installation,
            epoch,
            configuration,
            root,
            scopes,
            protected,
            registry: Mutex::new(None),
        })
    }
    pub(crate) fn matches_configuration(
        &self,
        installation: &str,
        configuration: &Arc<Mutex<ProductConfigStore>>,
    ) -> bool {
        self.installation == installation && Arc::ptr_eq(&self.configuration, configuration)
    }

    /// No owner is created by a read; source files/journal are opened only after
    /// the existing product claim has supplied the authenticated owner binding.
    pub(crate) fn with_registry<T>(
        &self,
        action: impl FnOnce(&ManagedMcpProgramRegistry) -> p::Result<T>,
    ) -> p::Result<T> {
        let mut config = self.configuration.lock().map_err(|_| invalid())?;
        let snapshot = config.snapshot()?;
        let registry = self.registry_for(&snapshot)?;
        action(&registry)
    }

    fn registry_for(
        &self,
        snapshot: &p::ProductConfigView,
    ) -> p::Result<Arc<ManagedMcpProgramRegistry>> {
        if snapshot.install_ref != self.installation {
            return Err(invalid());
        }
        let owner = snapshot.owner.as_ref().ok_or_else(invalid)?;
        let mut initialized = self.registry.lock().map_err(|_| invalid())?;
        if initialized.is_none() {
            *initialized = Some(Arc::new(ManagedMcpProgramRegistry::open(
                &self.journal,
                self.installation.clone(),
                owner.clone(),
                self.epoch,
                &self.root,
                &self.scopes,
                &self.protected,
            )?));
        }
        let registry = initialized.as_ref().ok_or_else(invalid)?;
        registry.identity_matches(&self.installation, owner, self.epoch)?;
        Ok(registry.clone())
    }
}
