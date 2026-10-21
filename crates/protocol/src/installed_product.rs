//! Installed-product contracts. These are host facts, never brain events.
#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{canonical_digest, Error, Result, SchemaDigest, SchemaVersion, Timestamp};
mod worker;
pub use worker::*;

fn version(version: SchemaVersion) -> Result<()> {
    if version != SchemaVersion(1) {
        return Err(Error("unsupported installed-product schema".into()));
    }
    Ok(())
}

pub fn validate_product_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
        || value == "."
        || value == ".."
    {
        return Err(Error("invalid installed-product reference".into()));
    }
    Ok(())
}

pub fn validate_product_digest(digest: &SchemaDigest) -> Result<()> {
    let raw = digest
        .0
        .strip_prefix("sha256:")
        .ok_or_else(|| Error("installed-product digest must identify SHA-256".into()))?;
    if raw.len() != 64
        || !raw
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(Error("invalid installed-product digest".into()));
    }
    Ok(())
}

/// Reject traversal on every OS, including Windows syntax on Unix.
pub fn validate_release_relative_path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 512
        || value.contains(['\\', ':', '\0', '<', '>', '"', '|', '?', '*'])
        || value.chars().any(char::is_control)
        || value.starts_with('/')
        || value.ends_with('/')
        || value
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with([' ', '.']))
        || Path::new(value)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err(Error(
            "release path must be a bounded relative file path".into(),
        ));
    }
    for part in value.split('/') {
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                stem.strip_prefix(prefix)
                    .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0")
            })
        {
            return Err(Error("release path uses a reserved device name".into()));
        }
    }
    Ok(())
}

pub fn validate_release_id(value: &str) -> Result<()> {
    validate_product_id(value)?;
    if value.contains(':') || value.contains('/') {
        return Err(Error(
            "release identity must be a single safe directory name".into(),
        ));
    }
    validate_release_relative_path(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductTarget {
    WindowsX86_64,
    MacosArm64,
    MacosX86_64,
    Ubuntu2404X86_64,
    Ubuntu2404Arm64,
    Rocky9X86_64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductState {
    Unclaimed,
    ConfigurationRequired,
    Starting,
    Ready,
    Degraded,
    Maintenance,
    RecoveryRequired,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinServiceId {
    Host,
    Web,
    Brain,
}

impl BuiltinServiceId {
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Host => "forme-hostd",
            Self::Web => "forme-webd",
            Self::Brain => "forme-gatewayd",
        }
    }
    pub fn service_name(self) -> &'static str {
        match self {
            Self::Host => "forme-host",
            Self::Web => "forme-web",
            Self::Brain => "forme-brain",
        }
    }

    pub fn required_account(self, target: ProductTarget) -> &'static str {
        match (target, self) {
            (ProductTarget::WindowsX86_64, Self::Host) => "LocalSystem",
            (ProductTarget::WindowsX86_64, Self::Web) => "NT SERVICE\\forme-web",
            (ProductTarget::WindowsX86_64, Self::Brain) => "NT SERVICE\\forme-brain",
            (ProductTarget::MacosArm64 | ProductTarget::MacosX86_64, Self::Host) => "root",
            (ProductTarget::MacosArm64 | ProductTarget::MacosX86_64, Self::Web) => "_formeweb",
            (ProductTarget::MacosArm64 | ProductTarget::MacosX86_64, Self::Brain) => "_formebrain",
            (_, Self::Host) => "root",
            (_, Self::Web) => "forme-web",
            (_, Self::Brain) => "forme-brain",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPaths {
    pub schema_version: SchemaVersion,
    pub releases: PathBuf,
    pub config: PathBuf,
    pub brain: PathBuf,
    pub web_auth: PathBuf,
    pub host_state: PathBuf,
    pub keys: PathBuf,
    pub logs: PathBuf,
    pub backups: PathBuf,
    pub workspaces: PathBuf,
    pub ipc: PathBuf,
}

impl ProductPaths {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        let paths = [
            &self.releases,
            &self.config,
            &self.brain,
            &self.web_auth,
            &self.host_state,
            &self.keys,
            &self.logs,
            &self.backups,
            &self.workspaces,
            &self.ipc,
        ];
        for (i, path) in paths.iter().enumerate() {
            if !path.is_absolute()
                || path.parent().is_none()
                || path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
            {
                return Err(Error(
                    "installation paths must be absolute, bounded and normalized".into(),
                ));
            }
            for other in paths.iter().skip(i + 1) {
                if path.starts_with(other) || other.starts_with(path) {
                    return Err(Error("installation state domains must not overlap".into()));
                }
                #[cfg(windows)]
                {
                    let left = PathBuf::from(path.to_string_lossy().to_lowercase());
                    let right = PathBuf::from(other.to_string_lossy().to_lowercase());
                    if left.starts_with(&right) || right.starts_with(&left) {
                        return Err(Error("installation paths alias the same domain".into()));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceIdentityBinding {
    pub schema_version: SchemaVersion,
    pub service: BuiltinServiceId,
    pub account: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationManifest {
    pub schema_version: SchemaVersion,
    pub install_id: String,
    pub target: ProductTarget,
    pub release_id: String,
    pub release_digest: SchemaDigest,
    pub paths: ProductPaths,
    pub services: Vec<ServiceIdentityBinding>,
    pub created_at: Timestamp,
}

impl InstallationManifest {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.install_id)?;
        validate_release_id(&self.release_id)?;
        validate_product_digest(&self.release_digest)?;
        self.paths.validate()?;
        if self.services.len() != 3 {
            return Err(Error("installation requires exactly three services".into()));
        }
        let mut ids = BTreeSet::new();
        let mut accounts = BTreeSet::new();
        for service in &self.services {
            version(service.schema_version)?;
            if !ids.insert(service.service)
                || service.account.trim().is_empty()
                || service.account.len() > 128
                || service.account.contains(['\0', '\r', '\n'])
                || !accounts.insert(service.account.to_ascii_lowercase())
                || !service
                    .account
                    .eq_ignore_ascii_case(service.service.required_account(self.target))
            {
                return Err(Error(
                    "service identities must be distinct and bounded".into(),
                ));
            }
        }
        Ok(())
    }

    /// Digest the validated installation identity that a managed host binds
    /// into its product journal at startup.
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaReadWriteRange {
    pub schema_version: SchemaVersion,
    pub read_min: u32,
    pub read_max: u32,
    pub write_version: u32,
}

impl SchemaReadWriteRange {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        if self.read_min == 0
            || self.read_min > self.write_version
            || self.write_version > self.read_max
        {
            return Err(Error("invalid release compatibility range".into()));
        }
        Ok(())
    }
    pub fn can_read(&self, written: u32) -> bool {
        (self.read_min..=self.read_max).contains(&written)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseFile {
    pub schema_version: SchemaVersion,
    pub relative_path: String,
    pub digest: SchemaDigest,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema_version: SchemaVersion,
    pub release_id: String,
    pub target: ProductTarget,
    pub files: Vec<ReleaseFile>,
    pub ui_digest: SchemaDigest,
    pub managed_dependencies: Vec<SchemaDigest>,
    pub config_range: SchemaReadWriteRange,
    pub data_range: SchemaReadWriteRange,
    pub minimum_upgrader: u32,
    pub signature_ref: String,
}

impl ReleaseManifest {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_release_id(&self.release_id)?;
        validate_product_id(&self.signature_ref)?;
        validate_product_digest(&self.ui_digest)?;
        self.config_range.validate()?;
        self.data_range.validate()?;
        if self.minimum_upgrader == 0 || self.files.is_empty() || self.files.len() > 10_000 {
            return Err(Error("release file set or upgrader is invalid".into()));
        }
        let mut paths = BTreeSet::new();
        for file in &self.files {
            version(file.schema_version)?;
            validate_release_relative_path(&file.relative_path)?;
            validate_product_digest(&file.digest)?;
            if !paths.insert(file.relative_path.to_ascii_lowercase()) {
                return Err(Error("release contains colliding file paths".into()));
            }
        }
        for digest in &self.managed_dependencies {
            validate_product_digest(digest)?;
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        let mut normalized = self.clone();
        normalized
            .files
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        normalized
            .managed_dependencies
            .sort_by(|a, b| a.0.cmp(&b.0));
        normalized.managed_dependencies.dedup();
        canonical_digest(&normalized)
    }
    pub fn compatible_with(&self, current: &Self, upgrader: u32) -> bool {
        self.validate().is_ok()
            && current.validate().is_ok()
            && self.target == current.target
            && upgrader >= self.minimum_upgrader
            && self
                .config_range
                .can_read(current.config_range.write_version)
            && self.data_range.can_read(current.data_range.write_version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaintenanceKind {
    Inspect,
    StartCore,
    StopCore,
    RestartCore,
    ValidateConfig {
        config_revision: u64,
    },
    ApplyValidatedConfig {
        config_revision: u64,
        validation_digest: SchemaDigest,
    },
    ConfigureLan {
        config_revision: u64,
        validation_digest: SchemaDigest,
    },
    StageSignedRelease {
        release_id: String,
        release_digest: SchemaDigest,
    },
    Upgrade {
        release_id: String,
        release_digest: SchemaDigest,
        backup_ref: String,
    },
    RollbackCompatibleRelease {
        release_id: String,
        release_digest: SchemaDigest,
        backup_ref: String,
    },
    PrepareRestore {
        backup_ref: String,
    },
    CommitRestore {
        candidate_ref: String,
        candidate_digest: SchemaDigest,
        owner_digest: SchemaDigest,
    },
    UninstallPlan {
        preserve_owner_data: bool,
    },
}

impl MaintenanceKind {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::StageSignedRelease {
                release_id,
                release_digest,
            } => {
                validate_release_id(release_id)?;
                validate_product_digest(release_digest)?;
            }
            Self::Upgrade {
                release_id,
                release_digest,
                backup_ref,
            }
            | Self::RollbackCompatibleRelease {
                release_id,
                release_digest,
                backup_ref,
            } => {
                validate_release_id(release_id)?;
                validate_product_digest(release_digest)?;
                validate_product_id(backup_ref)?;
            }
            Self::PrepareRestore { backup_ref } => validate_product_id(backup_ref)?,
            Self::CommitRestore {
                candidate_ref,
                candidate_digest,
                owner_digest,
            } => {
                validate_product_id(candidate_ref)?;
                validate_product_digest(candidate_digest)?;
                validate_product_digest(owner_digest)?;
            }
            Self::ApplyValidatedConfig {
                validation_digest, ..
            }
            | Self::ConfigureLan {
                validation_digest, ..
            } => validate_product_digest(validation_digest)?,
            Self::UninstallPlan {
                preserve_owner_data: false,
            } => {
                return Err(Error(
                    "owner-data deletion requires the separate data-lifecycle authority".into(),
                ));
            }
            _ => {}
        }
        Ok(())
    }
    pub fn is_lifecycle_transaction(&self) -> bool {
        matches!(
            self,
            Self::Upgrade { .. }
                | Self::RollbackCompatibleRelease { .. }
                | Self::CommitRestore { .. }
        )
    }
    pub fn setup_allowed(&self) -> bool {
        matches!(self, Self::Inspect | Self::PrepareRestore { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceCommand {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub idempotency_key: String,
    pub expected_revision: u64,
    /// Opaque reference resolved by the authenticated broker, never a caller's role.
    /// The proof itself, including its nonce/credential, is not journal data.
    pub actor_proof: MaintenanceProofRef,
    pub kind: MaintenanceKind,
    pub requested_deadline: Timestamp,
}

impl MaintenanceCommand {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.operation_id)?;
        validate_product_id(&self.idempotency_key)?;
        version(self.actor_proof.schema_version)?;
        validate_product_id(&self.actor_proof.proof_ref)?;
        self.kind.validate()
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        // Renewing an authentication proof must not change command semantics.
        canonical_digest(&(
            self.schema_version,
            &self.operation_id,
            &self.idempotency_key,
            self.expected_revision,
            &self.kind,
            self.requested_deadline,
        ))
    }

    /// The broker's final binding includes all server-resolved lifecycle refs.
    pub fn bound_digest(&self, references: &MaintenanceReferences) -> Result<SchemaDigest> {
        references.validate(&self.kind)?;
        canonical_digest(&(self.digest()?, references))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceProofRef {
    pub schema_version: SchemaVersion,
    pub proof_ref: String,
}

/// Server-derived references pinned when planning, not browser-supplied paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceReferences {
    pub schema_version: SchemaVersion,
    pub original_release_ref: String,
    pub target_release_ref: String,
    pub data_candidate_ref: Option<String>,
    pub backup_ref: Option<String>,
}

impl MaintenanceReferences {
    pub fn validate(&self, kind: &MaintenanceKind) -> Result<()> {
        version(self.schema_version)?;
        validate_release_id(&self.original_release_ref)?;
        validate_release_id(&self.target_release_ref)?;
        for reference in [&self.data_candidate_ref, &self.backup_ref]
            .into_iter()
            .flatten()
        {
            validate_product_id(reference)?;
        }
        let target = match kind {
            MaintenanceKind::StageSignedRelease { release_id, .. }
            | MaintenanceKind::Upgrade { release_id, .. }
            | MaintenanceKind::RollbackCompatibleRelease { release_id, .. } => release_id,
            _ => &self.original_release_ref,
        };
        let backup = match kind {
            MaintenanceKind::Upgrade { backup_ref, .. }
            | MaintenanceKind::RollbackCompatibleRelease { backup_ref, .. }
            | MaintenanceKind::PrepareRestore { backup_ref } => Some(backup_ref),
            _ => None,
        };
        let candidate = match kind {
            MaintenanceKind::CommitRestore { candidate_ref, .. } => Some(candidate_ref),
            _ => None,
        };
        if self.target_release_ref != *target
            || self.backup_ref.as_ref() != backup
            || self.data_candidate_ref.as_ref() != candidate
        {
            return Err(Error(
                "maintenance references differ from the bound command".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceStage {
    Planned,
    Validated,
    Quiescing,
    BackedUp,
    Switching,
    Checking,
    Committed,
    RecoveryRequired,
}

impl MaintenanceStage {
    pub fn permits(self, next: Self, lifecycle: bool) -> bool {
        use MaintenanceStage::*;
        matches!(
            (self, next),
            (Planned, Validated)
                | (Validated, Quiescing)
                | (Quiescing, BackedUp)
                | (BackedUp, Switching)
                | (Switching, Checking)
                | (Checking, Committed)
        ) || !lifecycle && matches!((self, next), (Validated, Checking))
            || !matches!(self, Committed | RecoveryRequired) && next == RecoveryRequired
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceActorClass {
    LocalAdministrator,
    OwnerReauthenticated,
    Setup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductManagementRecord {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub product_seq: u64,
    pub expected_revision: u64,
    pub actor_class: MaintenanceActorClass,
    pub command_digest: SchemaDigest,
    pub stage: MaintenanceStage,
    pub safe_refs: Vec<String>,
    pub outcome: ProductOperationOutcome,
    pub timestamp: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductOperationOutcome {
    Intent,
    Receipt,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceOperation {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub revision: u64,
    pub stage: MaintenanceStage,
    pub pending_stage: Option<MaintenanceStage>,
    pub original_release_ref: String,
    pub target_release_ref: String,
    pub data_candidate_ref: Option<String>,
    pub backup_ref: Option<String>,
    pub receipts: Vec<String>,
    pub failure_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostManagementRequest {
    pub schema_version: SchemaVersion,
    pub command: MaintenanceCommand,
    pub references: MaintenanceReferences,
}

impl HostManagementRequest {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        self.command.validate()?;
        self.references.validate(&self.command.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostOperationResponse {
    pub schema_version: SchemaVersion,
    pub operation: MaintenanceOperation,
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostStatus {
    pub schema_version: SchemaVersion,
    pub install_id_safe_ref: String,
    pub product_state: ProductState,
    pub core_health: CoreHealth,
    pub active_release: String,
    pub active_config_revision: u64,
    pub acknowledged_config_revision: Option<u64>,
    pub issue_refs: Vec<String>,
    pub allowed_operations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreHealth {
    Unconfigured,
    Stopped,
    Starting,
    Ready,
    Degraded,
    Unreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeServiceState {
    NotInstalled,
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeServiceStatus {
    pub schema_version: SchemaVersion,
    pub service: BuiltinServiceId,
    pub state: NativeServiceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeServiceAction {
    Start,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatedServiceChange {
    pub schema_version: SchemaVersion,
    pub service: BuiltinServiceId,
    pub action: NativeServiceAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeReceipt {
    pub schema_version: SchemaVersion,
    pub service: BuiltinServiceId,
    pub requested: NativeServiceAction,
    pub observed: NativeServiceState,
}
