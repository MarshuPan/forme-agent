//! Explicit offline recovery. No source initialization, authority claim, key
//! allocation, in-place restore or automatic switch is performed here.
use crate::{
    backup,
    offline_access::{immutable_source, require_cold_source, OfflineSource},
    offline_validation::{inspect, invalid},
    store_access::normalized,
    BackupManifest, BackupVerification, FileContentKeyResolver, StoreAccessGuard,
    StoreContentProtection,
};
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineBrainIdentity {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: p::SchemaDigest,
    pub store_identity: p::SchemaDigest,
    pub schema_digest: p::SchemaDigest,
    pub database_digest: p::SchemaDigest,
    pub event_count: u64,
    pub last_stream_seq: u64,
    pub authenticated_content_rows: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineRestorePlan {
    pub schema_version: p::SchemaVersion,
    pub source: OfflineBrainIdentity,
    pub source_path_digest: p::SchemaDigest,
    pub target_path_digest: p::SchemaDigest,
    pub backup_manifest_digest: p::SchemaDigest,
    pub expires_at: p::Timestamp,
    pub digest: p::SchemaDigest,
}
impl OfflineRestorePlan {
    fn seal(mut self) -> p::Result<Self> {
        self.digest = p::SchemaDigest(String::new());
        self.digest = p::canonical_digest(&self)?;
        Ok(self)
    }
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || self.expires_at <= now()
            || self.clone().seal()?.digest != self.digest
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineRestoreReceipt {
    pub schema_version: p::SchemaVersion,
    pub plan_digest: p::SchemaDigest,
    pub execution_fence_digest: p::SchemaDigest,
    pub source: OfflineBrainIdentity,
    pub restored: OfflineBrainIdentity,
    pub verification: BackupVerification,
    pub source_unchanged: bool,
    pub authority_switched: bool,
}

/// Holds the exclusive source fence for its whole lifetime. The authenticated
/// owner must come from the operator/control surface, never an untrusted body.
pub struct OfflineBrainSource {
    path: PathBuf,
    key_directory: PathBuf,
    protection: StoreContentProtection,
    identity: OfflineBrainIdentity,
    _access: OfflineSource,
}
impl OfflineBrainSource {
    pub fn open(
        source: &Path,
        owner: &p::VerifiedPrincipal,
        scope: p::Scope,
        key_directory: &Path,
    ) -> p::Result<Self> {
        let path = normalized(source)?;
        let access = OfflineSource::acquire(&path)?;
        Self::open_guarded(path, owner, scope, key_directory, access)
    }

    /// Candidate discovery must not create even an access-lock sidecar in a
    /// historical source. Retain an existing fence or the actual cold-file lock.
    pub fn open_existing(
        source: &Path,
        owner: &p::VerifiedPrincipal,
        scope: p::Scope,
        key_directory: &Path,
    ) -> p::Result<Self> {
        let path = normalized(source)?;
        let access = OfflineSource::acquire_legacy(&path)?;
        Self::open_guarded(path, owner, scope, key_directory, access)
    }

    fn open_guarded(
        path: PathBuf,
        owner: &p::VerifiedPrincipal,
        scope: p::Scope,
        key_directory: &Path,
        access: OfflineSource,
    ) -> p::Result<Self> {
        let key_directory = normalized(key_directory)?;
        if path.starts_with(&key_directory) {
            return Err(invalid());
        }
        let keys = Arc::new(FileContentKeyResolver::open_existing(&key_directory)?);
        let protection = StoreContentProtection::new(owner, scope, keys)?;
        let identity = inspect(&path, &protection)?;
        if !backup::replay_backup(&path, Some(protection.clone()))? {
            return Err(invalid());
        }
        Ok(Self {
            path,
            key_directory,
            protection,
            identity,
            _access: access,
        })
    }
    pub fn identity(&self) -> &OfflineBrainIdentity {
        &self.identity
    }

    /// Observe the database schema while retaining the existing offline fence.
    /// This is the actual SQLite version, not a manifest/DTO schema version.
    pub fn database_schema_version(&self) -> p::Result<u32> {
        self.require_unchanged()?;
        let version = immutable_source(&self.path)?
            .pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .map_err(|_| invalid())?;
        if version == 0 {
            return Err(invalid());
        }
        Ok(version)
    }

    pub fn execution_fence_digest(&self) -> p::Result<Option<p::SchemaDigest>> {
        self.require_unchanged()?;
        crate::restoration_fence::read(&immutable_source(&self.path)?)?
            .map(|fence| fence.digest())
            .transpose()
    }

    /// A switch candidate cannot contain activity performed after its seal.
    pub fn restored_candidate_fence_digest(&self) -> p::Result<p::SchemaDigest> {
        self.require_unchanged()?;
        let c = immutable_source(&self.path)?;
        let fence = crate::restoration_fence::read(&c)?.ok_or_else(invalid)?;
        if crate::restoration_fence::stream_heads(&c)? != fence.streams {
            return Err(invalid());
        }
        fence.digest()
    }

    pub fn verify_backup(&self) -> p::Result<BackupVerification> {
        self.require_unchanged()?;
        let manifest_path = backup::manifest_path(&self.path);
        normalized(&manifest_path)?;
        let manifest = backup::read_manifest(&self.path)?;
        manifest.validate()?;
        if !matches_manifest(&self.identity, &manifest) {
            return Err(invalid());
        }
        Ok(BackupVerification {
            schema_version: p::SchemaVersion(1),
            manifest,
            digest_ok: true,
            integrity_ok: true,
            event_continuity_ok: true,
            projection_replay_ok: true,
        })
    }

    /// Makes a new consistent backup, preserving external keys separately.
    /// A partial target is retained and cannot be overwritten or auto-retried.
    pub fn create_backup(&self, target: &Path) -> p::Result<BackupVerification> {
        self.require_unchanged()?;
        let target = self.new_target(target)?;
        let _access = StoreAccessGuard::new_offline_target(&target)?;
        copy_database(&self.path, &target)?;
        let identity = inspect(&target, &self.protection)?;
        require_same_brain(&self.identity, &identity)?;
        if !backup::replay_backup(&target, Some(self.protection.clone()))? {
            return Err(invalid());
        }
        let manifest = identity.manifest();
        backup::write_manifest(&target, &manifest)?;
        self.require_unchanged()?;
        Ok(BackupVerification {
            schema_version: p::SchemaVersion(1),
            manifest,
            digest_ok: true,
            integrity_ok: true,
            event_continuity_ok: true,
            projection_replay_ok: true,
        })
    }

    pub fn plan_restore(&self, target: &Path) -> p::Result<OfflineRestorePlan> {
        let verification = self.verify_backup()?;
        let target = self.new_target(target)?;
        OfflineRestorePlan {
            schema_version: p::SchemaVersion(1),
            source: self.identity.clone(),
            source_path_digest: path_digest(&self.path)?,
            target_path_digest: path_digest(&target)?,
            backup_manifest_digest: p::canonical_digest(&verification.manifest)?,
            expires_at: now().checked_add(15 * 60 * 1000).ok_or_else(invalid)?,
            digest: p::SchemaDigest(String::new()),
        }
        .seal()
    }

    /// The plan is an explicit one-shot target reservation, not permission to
    /// switch the active brain. Repeated calls fail on the existing target.
    pub fn restore_to(
        &self,
        target: &Path,
        plan: &OfflineRestorePlan,
        approved: &p::SchemaDigest,
    ) -> p::Result<OfflineRestoreReceipt> {
        plan.validate()?;
        let verification = self.verify_backup()?;
        let target = self.new_target(target)?;
        if approved != &plan.digest
            || self.identity != plan.source
            || plan.source_path_digest != path_digest(&self.path)?
            || plan.target_path_digest != path_digest(&target)?
            || plan.backup_manifest_digest != p::canonical_digest(&verification.manifest)?
        {
            return Err(invalid());
        }
        let _access = StoreAccessGuard::new_offline_target(&target)?;
        // Hashing and re-authentication can outlive the prepared permission.
        // Check its timebox again at the last boundary before creating data.
        plan.validate()?;
        copy_database(&self.path, &target)?;
        let restored = inspect(&target, &self.protection)?;
        require_same_brain(&self.identity, &restored)?;
        if !backup::replay_backup(&target, Some(self.protection.clone()))? {
            return Err(invalid());
        }
        // Seal only the verified new copy. Historical content and source bytes
        // stay untouched; the new authority boundary is part of its manifest.
        let execution_fence_digest =
            crate::restoration_fence::seal_target(&target, &self.identity, &plan.digest)?;
        let restored = inspect(&target, &self.protection)?;
        if !backup::replay_backup(&target, Some(self.protection.clone()))? {
            return Err(invalid());
        }
        let manifest = restored.manifest();
        backup::write_manifest(&target, &manifest)?;
        self.require_unchanged()?;
        Ok(OfflineRestoreReceipt {
            schema_version: p::SchemaVersion(2),
            plan_digest: plan.digest.clone(),
            execution_fence_digest,
            source: self.identity.clone(),
            restored,
            verification: BackupVerification {
                schema_version: p::SchemaVersion(1),
                manifest,
                digest_ok: true,
                integrity_ok: true,
                event_continuity_ok: true,
                projection_replay_ok: true,
            },
            source_unchanged: true,
            authority_switched: false,
        })
    }

    fn new_target(&self, path: &Path) -> p::Result<PathBuf> {
        let path = new_target(path)?;
        if path.starts_with(&self.key_directory) {
            return Err(invalid());
        }
        Ok(path)
    }

    fn require_unchanged(&self) -> p::Result<()> {
        require_cold_source(&self.path)?;
        if backup::file_digest(&self.path)? != self.identity.database_digest {
            return Err(invalid());
        }
        // Re-authenticate as well: recovery keys may have been revoked since
        // initial inspection, including by an owner operation in another DB.
        if inspect(&self.path, &self.protection)? != self.identity {
            return Err(invalid());
        }
        Ok(())
    }
}

impl OfflineBrainIdentity {
    fn manifest(&self) -> BackupManifest {
        BackupManifest {
            schema_version: p::SchemaVersion(1),
            owner_digest: Some(self.owner_digest.clone()),
            schema_digest: self.schema_digest.clone(),
            event_count: self.event_count,
            last_stream_seq: self.last_stream_seq,
            database_digest: self.database_digest.clone(),
            created_at: now(),
        }
    }
}
fn matches_manifest(i: &OfflineBrainIdentity, m: &BackupManifest) -> bool {
    m.owner_digest.as_ref() == Some(&i.owner_digest)
        && m.schema_digest == i.schema_digest
        && m.event_count == i.event_count
        && m.last_stream_seq == i.last_stream_seq
        && m.database_digest == i.database_digest
}
fn require_same_brain(a: &OfflineBrainIdentity, b: &OfflineBrainIdentity) -> p::Result<()> {
    if a.owner_digest != b.owner_digest
        || a.store_identity != b.store_identity
        || a.schema_digest != b.schema_digest
        || a.event_count != b.event_count
        || a.last_stream_seq != b.last_stream_seq
        || a.authenticated_content_rows != b.authenticated_content_rows
    {
        return Err(invalid());
    }
    Ok(())
}
fn new_target(path: &Path) -> p::Result<PathBuf> {
    let path = normalized(path)?;
    if fs::symlink_metadata(&path).is_ok()
        || fs::symlink_metadata(backup::manifest_path(&path)).is_ok()
    {
        return Err(invalid());
    }
    require_cold_source(&path)?;
    Ok(path)
}
fn copy_database(source: &Path, target: &Path) -> p::Result<()> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3);
    }
    let file = options.open(target).map_err(|_| invalid())?;
    immutable_source(source)?
        .backup(rusqlite::MAIN_DB, target, rusqlite::backup::NO_PROGRESS)
        .map_err(|_| invalid())?;
    file.sync_all().map_err(|_| invalid())
}
fn path_digest(path: &Path) -> p::Result<p::SchemaDigest> {
    path.to_str()
        .map(|s| p::sha256_content_digest(s.as_bytes()))
        .ok_or_else(invalid)
}
fn now() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|t| i64::try_from(t.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
