//! Cold SQLite backup and restore verification (prd/23 D3).
//!
//! The service never copies a live database file.  SQLite's online backup API
//! takes a consistent snapshot while the authority connection remains open;
//! the manifest and verification pass then bind that snapshot to its event
//! continuity and projection replay.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_protocol as p;
use rusqlite::{Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::{BrainAuthorityStore, EventStore, SchemaSnapshot, SqliteEventStore, StoreOptions};

mod restore;

const BACKUP_SCHEMA_VERSION: p::SchemaVersion = p::SchemaVersion(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: Option<p::SchemaDigest>,
    pub schema_digest: p::SchemaDigest,
    pub event_count: u64,
    pub last_stream_seq: u64,
    pub database_digest: p::SchemaDigest,
    pub created_at: p::Timestamp,
}

impl BackupManifest {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != BACKUP_SCHEMA_VERSION
            || self.schema_digest.0.trim().is_empty()
            || self.database_digest.0.trim().is_empty()
            || self.created_at < 0
        {
            return Err(p::Error("backup manifest is incomplete".into()));
        }
        p::sha256_digest_bytes(&self.schema_digest)
            .map_err(|_| p::Error("backup schema digest is invalid".into()))?;
        p::sha256_digest_bytes(&self.database_digest)
            .map_err(|_| p::Error("backup database digest is invalid".into()))?;
        if let Some(owner) = &self.owner_digest {
            p::sha256_digest_bytes(owner)
                .map_err(|_| p::Error("backup owner digest is invalid".into()))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupVerification {
    pub schema_version: p::SchemaVersion,
    pub manifest: BackupManifest,
    pub digest_ok: bool,
    pub integrity_ok: bool,
    pub event_continuity_ok: bool,
    pub projection_replay_ok: bool,
}

impl BackupVerification {
    pub fn is_valid(&self) -> bool {
        self.digest_ok && self.integrity_ok && self.event_continuity_ok && self.projection_replay_ok
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreReport {
    pub schema_version: p::SchemaVersion,
    pub manifest: BackupManifest,
    pub verification: BackupVerification,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store_identity: Option<p::SchemaDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_fence_digest: Option<p::SchemaDigest>,
}

#[derive(Clone)]
pub struct BackupService {
    store: SqliteEventStore,
}

impl BackupService {
    pub(crate) fn new(store: SqliteEventStore) -> Self {
        Self { store }
    }

    pub fn create_cold_backup(&self, target: &Path) -> p::Result<BackupManifest> {
        let source = self.source_path()?;
        reject_same_path(&source, target)?;
        prepare_new_target(target)?;

        let connection = self
            .store
            .core_connection()
            .map_err(|error| p::Error(error.to_string()))?;
        connection
            .backup(rusqlite::MAIN_DB, target, None)
            .map_err(|error| p::Error(format!("SQLite online backup failed: {error}")))?;
        drop(connection);

        let manifest = self.manifest_for(target)?;
        write_manifest(target, &manifest)?;
        Ok(manifest)
    }

    pub fn verify_backup(&self, source: &Path) -> p::Result<BackupVerification> {
        let manifest = read_manifest(source)?;
        manifest.validate()?;
        let actual_digest = file_digest(source)?;
        if actual_digest != manifest.database_digest {
            return Err(p::Error(format!(
                "backup database digest mismatch expected={} actual={}",
                manifest.database_digest.0, actual_digest.0
            )));
        }
        let inspection = inspect_database(source)?;
        if inspection.schema_digest != manifest.schema_digest
            || inspection.owner_digest != manifest.owner_digest
            || inspection.event_count != manifest.event_count
            || inspection.last_stream_seq != manifest.last_stream_seq
        {
            return Err(p::Error("backup manifest does not match database".into()));
        }
        if !inspection.integrity_ok || !inspection.event_continuity_ok {
            return Err(p::Error(
                "backup integrity or event continuity failed".into(),
            ));
        }
        let replay_ok = replay_backup(source, self.store.content_protection()?)?;
        if !replay_ok {
            return Err(p::Error(
                "backup projection replay did not reconcile".into(),
            ));
        }
        Ok(BackupVerification {
            schema_version: BACKUP_SCHEMA_VERSION,
            manifest,
            digest_ok: true,
            integrity_ok: inspection.integrity_ok,
            event_continuity_ok: inspection.event_continuity_ok,
            projection_replay_ok: replay_ok,
        })
    }

    fn source_path(&self) -> p::Result<PathBuf> {
        let connection = self
            .store
            .core_connection()
            .map_err(|error| p::Error(error.to_string()))?;
        connection
            .path()
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| p::Error("cold backup requires a file-backed store".into()))
    }

    fn manifest_for(&self, path: &Path) -> p::Result<BackupManifest> {
        let inspection = inspect_database(path)?;
        if !inspection.integrity_ok || !inspection.event_continuity_ok {
            return Err(p::Error("database inspection failed".into()));
        }
        Ok(BackupManifest {
            schema_version: BACKUP_SCHEMA_VERSION,
            owner_digest: inspection.owner_digest,
            schema_digest: inspection.schema_digest,
            event_count: inspection.event_count,
            last_stream_seq: inspection.last_stream_seq,
            database_digest: file_digest(path)?,
            created_at: now_ms(),
        })
    }
}

impl SqliteEventStore {
    pub fn backup_service(&self) -> BackupService {
        BackupService::new(self.clone())
    }
}

struct Inspection {
    owner_digest: Option<p::SchemaDigest>,
    schema_digest: p::SchemaDigest,
    event_count: u64,
    last_stream_seq: u64,
    integrity_ok: bool,
    event_continuity_ok: bool,
}

fn inspect_database(path: &Path) -> p::Result<Inspection> {
    let connection = read_only_connection(path)?;
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|error| p::Error(format!("backup integrity check failed: {error}")))?;
    let (event_count, last_stream_seq): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(MAX(stream_seq), 0) FROM events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| p::Error(format!("backup event catalog failed: {error}")))?;
    let owner_digest = connection
        .query_row(
            "SELECT owner_digest FROM brain_authority ORDER BY owner_digest LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| p::Error(format!("backup owner metadata failed: {error}")))?
        .map(p::SchemaDigest);
    let continuity = event_continuity(&connection)?;
    Ok(Inspection {
        owner_digest,
        schema_digest: schema_digest(&connection)?,
        event_count: u64::try_from(event_count)
            .map_err(|_| p::Error("backup event count is invalid".into()))?,
        last_stream_seq: u64::try_from(last_stream_seq)
            .map_err(|_| p::Error("backup stream sequence is invalid".into()))?,
        integrity_ok: integrity.eq_ignore_ascii_case("ok"),
        event_continuity_ok: continuity,
    })
}

pub(crate) fn event_continuity(connection: &Connection) -> p::Result<bool> {
    let mut statement = connection
        .prepare("SELECT run_id, stream_seq FROM events ORDER BY run_id, stream_seq")
        .map_err(|error| p::Error(format!("backup continuity query failed: {error}")))?;
    let mut current = None::<String>;
    let mut expected = 1_u64;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| p::Error(format!("backup continuity scan failed: {error}")))?;
    for row in rows {
        let (run, sequence) =
            row.map_err(|error| p::Error(format!("backup continuity row failed: {error}")))?;
        let sequence =
            u64::try_from(sequence).map_err(|_| p::Error("backup sequence is negative".into()))?;
        if current.as_deref() != Some(run.as_str()) {
            current = Some(run);
            expected = 1;
        }
        if sequence != expected {
            return Ok(false);
        }
        expected = expected.saturating_add(1);
    }
    Ok(true)
}

pub(crate) fn schema_digest(connection: &Connection) -> p::Result<p::SchemaDigest> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, COALESCE(sql, '') FROM sqlite_master
             WHERE type IN ('table', 'index', 'trigger', 'view')
             ORDER BY type, name",
        )
        .map_err(|error| p::Error(format!("backup schema query failed: {error}")))?;
    let rows = statement
        .query_map([], |row: &Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| p::Error(format!("backup schema scan failed: {error}")))?;
    let mut schema = Vec::new();
    for row in rows {
        schema.push(row.map_err(|error| p::Error(format!("backup schema row failed: {error}")))?);
    }
    p::canonical_digest(&schema)
}

pub(crate) fn replay_backup(
    path: &Path,
    protection: Option<crate::StoreContentProtection>,
) -> p::Result<bool> {
    // Replay a private copy. Opening a backup with the normal writable store
    // may initialize/checkpoint SQLite state, so verification must never alter
    // the artifact whose digest it is validating.
    let scratch = ReplayScratch::new()?;
    let replay_path = scratch.0.join("replay.sqlite");
    let result = (|| {
        let source = read_only_connection(path)?;
        source
            .backup(rusqlite::MAIN_DB, &replay_path, None)
            .map_err(|error| p::Error(format!("SQLite replay copy failed: {error}")))?;
        drop(source);

        let store = SqliteEventStore::open(&replay_path, StoreOptions::default())?;
        if let Some(protection) = protection {
            store.enable_content_protection(protection)?;
        }
        let schema = StoreOptions::default().current_schema;
        let snapshot = SchemaSnapshot {
            schema,
            policy_version: p::Version(1),
            loop_version: p::Version(1),
            model_profile: p::ModelProfileRef("backup-replay".into()),
            tool_schema: p::Version(1),
        };
        for run in store.run_ids()? {
            if !store
                .replay(run, snapshot.clone())?
                .diff_vs_current
                .is_empty()
            {
                return Ok(false);
            }
        }
        Ok(true)
    })();
    remove_sqlite_files(&replay_path);
    result
}

struct ReplayScratch(PathBuf);
impl ReplayScratch {
    fn new() -> p::Result<Self> {
        let mut random = [0; 32];
        getrandom::fill(&mut random)
            .map_err(|_| p::Error("backup replay workspace is unavailable".into()))?;
        let digest = p::sha256_content_digest(&random);
        let path = std::env::temp_dir().join(format!("forme-replay-{}", &digest.0[7..]));
        let directory = fs::DirBuilder::new();
        #[cfg(unix)]
        let directory = {
            use std::os::unix::fs::DirBuilderExt;
            let mut directory = directory;
            directory.mode(0o700);
            directory
        };
        directory
            .create(&path)
            .map_err(|_| p::Error("backup replay workspace is unavailable".into()))?;
        Ok(Self(path))
    }
}
impl Drop for ReplayScratch {
    fn drop(&mut self) {
        // This random directory was atomically created by this operation.
        // Never recursively follow or remove unexpected children.
        remove_sqlite_files(&self.0.join("replay.sqlite"));
        let _ = fs::remove_dir(&self.0);
    }
}

fn remove_sqlite_files(path: &Path) {
    let _ = fs::remove_file(path);
    let raw_path = path.to_string_lossy();
    let _ = fs::remove_file(format!("{raw_path}-wal"));
    let _ = fs::remove_file(format!("{raw_path}-shm"));
}

fn read_only_connection(path: &Path) -> p::Result<Connection> {
    crate::offline_access::require_cold_source(path)?;
    crate::offline_access::immutable_source(path)
}

pub(crate) fn file_digest(path: &Path) -> p::Result<p::SchemaDigest> {
    let file =
        fs::File::open(path).map_err(|_| p::Error("backup digest source is unavailable".into()))?;
    p::sha256_reader_digest(file)
}

pub(crate) fn manifest_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.manifest.json", path.display()))
}

pub(crate) fn read_manifest(path: &Path) -> p::Result<BackupManifest> {
    let path = crate::store_access::normalized(&manifest_path(path))?;
    let file =
        fs::File::open(path).map_err(|_| p::Error("backup manifest is unavailable".into()))?;
    let meta = file
        .metadata()
        .map_err(|_| p::Error("backup manifest is unavailable".into()))?;
    if !meta.is_file() || meta.len() > 64 * 1024 {
        return Err(p::Error("backup manifest exceeds its bound".into()));
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| p::Error("backup manifest is unavailable".into()))?;
    if bytes.len() > 64 * 1024 {
        return Err(p::Error("backup manifest exceeds its bound".into()));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| p::Error(format!("backup manifest is invalid: {error}")))
}

pub(crate) fn write_manifest(path: &Path, manifest: &BackupManifest) -> p::Result<()> {
    manifest.validate()?;
    let destination = manifest_path(path);
    let temporary = destination.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|error| p::Error(format!("backup manifest encoding failed: {error}")))?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| p::Error("backup manifest temporary path is not new".into()))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| p::Error("backup manifest write failed".into()))?;
    drop(file);
    fs::hard_link(&temporary, destination)
        .map_err(|_| p::Error("backup manifest target is not new".into()))?;
    fs::remove_file(temporary)
        .map_err(|_| p::Error("backup manifest temporary cleanup failed".into()))
}

fn prepare_new_target(path: &Path) -> p::Result<()> {
    if path.as_os_str().is_empty() || path.exists() || manifest_path(path).exists() {
        return Err(p::Error("backup target must be a new path".into()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| p::Error(format!("backup target directory failed: {error}")))?;
    }
    // Reserve exactly this new file atomically. SQLite may open the empty
    // reservation, but must never overwrite a target won by another request.
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| p::Error("backup target must be a new regular file".into()))?;
    Ok(())
}

fn reject_same_path(left: &Path, right: &Path) -> p::Result<()> {
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    if left == right {
        return Err(p::Error("backup source and target must differ".into()));
    }
    Ok(())
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EventStore;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "forme-backup-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn event() -> p::Event {
        p::Event::new(
            p::EventId("event:backup".into()),
            p::RunId("run:backup".into()),
            None,
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::UserTurn,
                session_ref: p::SessionId("session:backup".into()),
                input_ref: p::InputRef("input:backup".into()),
                idempotency_key: None,
            }),
            p::SchemaVersion(1),
            1,
            p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        )
    }

    fn cleanup(root: &Path) {
        let _ = fs::remove_file(root.join("source.db"));
        let _ = fs::remove_file(root.join("backup.db"));
        let _ = fs::remove_file(root.join("backup.db.manifest.json"));
        let _ = fs::remove_file(root.join("restored.db"));
        let _ = fs::remove_file(root.join("restored.db.manifest.json"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cold_backup_restore_and_tamper_verification_are_fail_closed() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.db");
        let backup = root.join("backup.db");
        let restored = root.join("restored.db");
        let store = SqliteEventStore::open(&source, StoreOptions::default()).unwrap();
        store.append(event()).unwrap();
        let service = store.backup_service();
        let manifest = service.create_cold_backup(&backup).unwrap();
        assert_eq!(manifest.event_count, 1);
        assert!(service.verify_backup(&backup).unwrap().is_valid());
        let report = service.restore_to(&backup, &restored).unwrap();
        assert!(report.verification.is_valid());
        let mut bytes = fs::read(&backup).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0x01;
        fs::write(&backup, bytes).unwrap();
        assert!(service.verify_backup(&backup).is_err());
        cleanup(&root);
    }
}
