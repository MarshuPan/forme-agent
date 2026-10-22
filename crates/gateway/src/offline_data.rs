//! Local operator facade over Harness offline maintenance. This module has no
//! HTTP route and never opens a database itself or creates ProductionRuntime.
use forme_harness::offline_data::OfflineBrainMaintenance;
pub use forme_harness::offline_data::{
    OfflineBrainIdentity, OfflineRestorePlan, OfflineRestoreReceipt,
};
use forme_protocol as p;
use serde::{de::IgnoredAny, Deserialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
mod restore;
pub use restore::OfflineRestoreWorker;

// A closed projection of the managed config, deliberately independent from
// runtime/model/transport decoding. Ignored fields cannot influence storage.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageConfig {
    schema_version: p::SchemaVersion,
    install_ref: String,
    revision: u64,
    brain: StorageBinding,
    #[serde(rename = "transport")]
    _transport: IgnoredAny,
    #[serde(rename = "drain_timeout_ms")]
    _drain_timeout: IgnoredAny,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageBinding {
    schema_version: p::SchemaVersion,
    owner: p::OwnerPrincipalRef,
    workspace: p::WorkspaceRef,
    store_path: PathBuf,
    #[serde(default)]
    content_key_directory: Option<PathBuf>,
    #[serde(default)]
    expected_restoration_fence: Option<p::SchemaDigest>,
    artifact_root: PathBuf,
    #[serde(rename = "agent_profile")]
    _agent_profile: IgnoredAny,
    #[serde(rename = "models")]
    _models: IgnoredAny,
    #[serde(rename = "scheduler_enabled")]
    _scheduler_enabled: IgnoredAny,
    #[serde(rename = "scheduler")]
    _scheduler: IgnoredAny,
    #[serde(rename = "cognition")]
    _cognition: IgnoredAny,
    #[serde(rename = "authority_lease")]
    _authority_lease: IgnoredAny,
}

impl StorageConfig {
    fn load(path: &Path) -> p::Result<Self> {
        crate::web::config::check_path(path)?;
        let file = std::fs::File::open(path).map_err(|_| invalid())?;
        if !file.metadata().map_err(|_| invalid())?.is_file() {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() > 256 * 1024 {
            return Err(invalid());
        }
        let config: Self = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if config.schema_version != p::SchemaVersion(1)
            || config.brain.schema_version != p::SchemaVersion(1)
            || config.revision == 0
        {
            return Err(invalid());
        }
        p::validate_product_id(&config.install_ref)?;
        config.brain.owner.validate()?;
        Ok(config)
    }
}

fn invalid() -> p::Error {
    p::Error("offline storage configuration is unavailable or invalid".into())
}

pub struct OfflineCoreMaintenance {
    inner: OfflineBrainMaintenance,
}
impl OfflineCoreMaintenance {
    /// Config is an OS-protected installed-service file. The local command
    /// runs under the operator's existing filesystem permissions; it is not
    /// an elevation, arbitrary SQL or anonymous Web maintenance endpoint.
    pub fn open(config_path: &Path, backup: Option<&Path>) -> p::Result<Self> {
        let config = StorageConfig::load(config_path)?;
        if let Some(digest) = &config.brain.expected_restoration_fence {
            p::sha256_digest_bytes(digest)?;
            if config.brain.content_key_directory.is_none() {
                return Err(invalid());
            }
        }
        let owner = p::VerifiedPrincipal(config.brain.owner.0.clone());
        let keys = config
            .brain
            .content_key_directory
            .as_ref()
            .cloned()
            .unwrap_or_else(|| config.brain.store_path.with_extension("content-keys"));
        let inner = OfflineBrainMaintenance::open_storage(
            &owner,
            p::Scope(config.brain.workspace.0),
            &config.brain.store_path,
            &config.brain.artifact_root,
            &keys,
            backup,
        )?;
        if backup.is_none() {
            inner.require_restoration_fence(config.brain.expected_restoration_fence.as_ref())?;
        }
        Ok(Self { inner })
    }
    pub fn identity(&self) -> &OfflineBrainIdentity {
        self.inner.identity()
    }
    pub fn backup(&self, target: &Path) -> p::Result<forme_harness::BackupVerification> {
        self.inner.backup(target)
    }
    pub fn verify(&self) -> p::Result<forme_harness::BackupVerification> {
        self.inner.verify()
    }
    pub fn plan_restore(&self, target: &Path) -> p::Result<OfflineRestorePlan> {
        self.inner.plan_restore(target)
    }
    pub fn restore(
        &self,
        target: &Path,
        plan: &OfflineRestorePlan,
        approved: &p::SchemaDigest,
    ) -> p::Result<OfflineRestoreReceipt> {
        self.inner.restore(target, plan, approved)
    }
}
