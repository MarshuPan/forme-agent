//! Installed offline data facade. It never builds a runtime or calls a model.
use crate::{managed_config::check_managed_path, ManagedBrainConfig};
use forme_protocol as p;
pub use forme_store::{OfflineBrainIdentity, OfflineRestorePlan, OfflineRestoreReceipt};
use std::path::Path;

pub struct OfflineBrainMaintenance {
    source: forme_store::OfflineBrainSource,
}
impl OfflineBrainMaintenance {
    /// The gateway/operator authenticates the owner. Config and filesystem
    /// scopes are checked again here before any database or key is opened.
    pub fn open(
        config: &ManagedBrainConfig,
        owner: &p::VerifiedPrincipal,
        backup: Option<&Path>,
    ) -> p::Result<Self> {
        if config.schema_version != p::SchemaVersion(1) || owner.0 != config.owner.0 {
            return Err(p::Error(
                "offline maintenance owner binding is invalid".into(),
            ));
        }
        let keys = config.content_key_directory()?;
        Self::open_storage(
            owner,
            p::Scope(config.workspace.0.clone()),
            &config.store_path,
            &config.artifact_root,
            &keys,
            backup,
        )
    }

    /// Only storage/owner bindings are required for disaster recovery. A broken
    /// model or scheduler configuration must not disable offline inspection.
    /// Caller still authenticates the owner and protects the source config.
    pub fn open_storage(
        owner: &p::VerifiedPrincipal,
        scope: p::Scope,
        database: &Path,
        artifacts: &Path,
        keys: &Path,
        backup: Option<&Path>,
    ) -> p::Result<Self> {
        p::OwnerPrincipalRef(owner.0.clone()).validate()?;
        if scope.0.trim().is_empty() {
            return Err(p::Error("offline workspace binding is invalid".into()));
        }
        for path in [database, artifacts, keys] {
            check_managed_path(path)?;
        }
        if database.starts_with(keys) || keys.starts_with(artifacts) || artifacts.starts_with(keys)
        {
            return Err(p::Error("offline storage bindings overlap".into()));
        }
        let source = backup.unwrap_or(database);
        check_managed_path(source)?;
        let source = forme_store::OfflineBrainSource::open(source, owner, scope, keys)?;
        if backup.is_some() {
            source.verify_backup()?;
        }
        Ok(Self { source })
    }
    pub fn identity(&self) -> &OfflineBrainIdentity {
        self.source.identity()
    }
    pub fn backup(&self, target: &Path) -> p::Result<forme_store::BackupVerification> {
        self.source.create_backup(target)
    }
    pub fn verify(&self) -> p::Result<forme_store::BackupVerification> {
        self.source.verify_backup()
    }
    pub fn plan_restore(&self, target: &Path) -> p::Result<OfflineRestorePlan> {
        self.source.plan_restore(target)
    }
    pub fn restore(
        &self,
        target: &Path,
        plan: &OfflineRestorePlan,
        approved: &p::SchemaDigest,
    ) -> p::Result<OfflineRestoreReceipt> {
        self.source.restore_to(target, plan, approved)
    }
}
