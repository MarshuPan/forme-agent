//! Explicit, offline D4 legacy-plaintext migration into a NEW database.
//!
//! This is a sqlite child module, like lifecycle. It never opens the source
//! through the writable store initializer, claims authority, or deletes it.
//! A failed destination is retained for diagnosis and is NOT a completed
//! migration. The source remains an unprotected backup outside the new DB.
#![forbid(unsafe_code)]

use super::{checksum, PersistedEnvelope, SqliteEventStore, StoredEvent};
use crate::offline_access::{immutable_source, require_cold_source, OfflineSource};
use crate::{at_rest, EventStore, SchemaSnapshot, StoreContentProtection, StoreOptions};
use forme_protocol as p;
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

pub const LEGACY_MIGRATION_SCHEMA_VERSION: p::SchemaVersion = p::SchemaVersion(1);
const PLAN_TTL_MS: i64 = 15 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyMigrationPlan {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: p::SchemaDigest,
    pub store_identity: p::SchemaDigest,
    pub source_path_digest: p::SchemaDigest,
    pub target_path_digest: p::SchemaDigest,
    pub source_database_digest: p::SchemaDigest,
    pub schema_digest: p::SchemaDigest,
    pub target_scope: p::Scope,
    pub event_count: u64,
    pub content_rows: u64,
    pub expires_at: p::Timestamp,
    pub digest: p::SchemaDigest,
}

impl LegacyMigrationPlan {
    fn finish(mut self) -> p::Result<Self> {
        self.digest = p::SchemaDigest(String::new());
        self.digest = p::canonical_digest(&self)?;
        Ok(self)
    }

    pub fn validate(&self) -> p::Result<()> {
        for digest in [
            &self.owner_digest,
            &self.store_identity,
            &self.source_path_digest,
            &self.target_path_digest,
            &self.source_database_digest,
            &self.schema_digest,
        ] {
            p::sha256_digest_bytes(digest)?;
        }
        if self.schema_version != LEGACY_MIGRATION_SCHEMA_VERSION
            || self.target_scope.0.trim().is_empty()
            || self.expires_at <= super::current_time_ms()?
            || self.clone().finish()?.digest != self.digest
        {
            return Err(failure("plan is invalid or expired"));
        }
        Ok(())
    }
}

/// This is deliberately not a deletion or whole-owner protection receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceBackupDisposition {
    UnprotectedBackupOutsideProtectedDatabase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyMigrationReceipt {
    pub schema_version: p::SchemaVersion,
    pub plan_digest: p::SchemaDigest,
    pub owner_digest: p::SchemaDigest,
    pub store_identity: p::SchemaDigest,
    pub source_path_digest: p::SchemaDigest,
    pub source_database_digest: p::SchemaDigest,
    pub target_path_digest: p::SchemaDigest,
    pub target_database_digest: p::SchemaDigest,
    pub event_count: u64,
    pub protected_rows: u64,
    pub integrity_verified: bool,
    pub replay_verified: bool,
    pub source_unchanged: bool,
    pub source_backup: SourceBackupDisposition,
    pub source_deleted: bool,
    pub authority_switched: bool,
}

#[derive(Clone)]
pub struct LegacyPlaintextMigration {
    protection: StoreContentProtection,
}

impl LegacyPlaintextMigration {
    pub fn new(protection: StoreContentProtection) -> Self {
        Self { protection }
    }

    /// `owner` must be supplied by an authenticated owner control surface.
    /// Paths are bound by digest but never exposed in the portable plan.
    pub fn plan(
        &self,
        owner: &p::VerifiedPrincipal,
        source: &Path,
        target: &Path,
    ) -> p::Result<LegacyMigrationPlan> {
        self.require_owner(owner)?;
        let source = source_path(source)?;
        let target = new_target_path(target)?;
        let lock = OfflineSource::acquire_legacy(&source)?;
        let connection = immutable_source(&source)?;
        let inspection = self.inspect_source(&connection)?;
        let digest = file_digest(&source)?;
        let plan = LegacyMigrationPlan {
            schema_version: LEGACY_MIGRATION_SCHEMA_VERSION,
            owner_digest: self.protection.owner_digest().clone(),
            store_identity: inspection.store_identity,
            source_path_digest: path_digest(&source)?,
            target_path_digest: path_digest(&target)?,
            source_database_digest: digest.clone(),
            schema_digest: inspection.schema_digest,
            target_scope: self.protection.default_scope().clone(),
            event_count: inspection.events.len() as u64,
            content_rows: inspection.content.len() as u64,
            expires_at: super::current_time_ms()?
                .checked_add(PLAN_TTL_MS)
                .ok_or_else(|| failure("plan expiry is exhausted"))?,
            digest: p::SchemaDigest(String::new()),
        }
        .finish()?;
        drop(connection);
        lock.release()?;
        require_source_digest(&source, &digest)?;
        require_cold_source(&source)?;
        Ok(plan)
    }

    /// Approval binds the exact plan, source bytes, owner, scope and NEW target.
    /// No success receipt exists before encryption, compaction, authentication,
    /// event checksum/continuity and real projection replay all succeed.
    pub fn execute(
        &self,
        owner: &p::VerifiedPrincipal,
        source: &Path,
        target: &Path,
        plan: &LegacyMigrationPlan,
        approved_digest: &p::SchemaDigest,
    ) -> p::Result<LegacyMigrationReceipt> {
        self.require_owner(owner)?;
        plan.validate()?;
        let source = source_path(source)?;
        let target = new_target_path(target)?;
        if approved_digest != &plan.digest
            || plan.owner_digest != *self.protection.owner_digest()
            || plan.target_scope != *self.protection.default_scope()
            || plan.source_path_digest != path_digest(&source)?
            || plan.target_path_digest != path_digest(&target)?
        {
            return Err(failure("approved plan binding mismatch"));
        }
        let lock = OfflineSource::acquire_legacy(&source)?;
        require_source_digest(&source, &plan.source_database_digest)?;
        let source_connection = immutable_source(&source)?;
        let inspection = self.inspect_source(&source_connection)?;
        if inspection.store_identity != plan.store_identity
            || inspection.schema_digest != plan.schema_digest
            || inspection.events.len() as u64 != plan.event_count
            || inspection.content.len() as u64 != plan.content_rows
        {
            return Err(failure("source no longer matches approved plan"));
        }
        // create_new is the one-shot gate. Keep the handle open, denying path
        // replacement on Windows while SQLite operates on this reservation.
        let reservation = reserve_target(&target)?;
        let result: p::Result<p::SchemaDigest> = (|| {
            let mut destination =
                Connection::open_with_flags(&target, OpenFlags::SQLITE_OPEN_READ_WRITE)
                    .map_err(|_| failure("new target cannot be opened"))?;
            {
                let backup = rusqlite::backup::Backup::new(&source_connection, &mut destination)
                    .map_err(|_| failure("SQLite backup could not be started"))?;
                backup
                    .run_to_completion(128, std::time::Duration::from_millis(5), None)
                    .map_err(|_| failure("SQLite backup failed"))?;
            }
            transform(&mut destination, &inspection, &self.protection)?;
            verify_destination(destination, &inspection, &self.protection)?;
            compact_target(&target)?;
            // Reopen after VACUUM/checkpoint. Validation uses the real store
            // cursor and replay, with the same resolver used for encryption.
            let destination =
                Connection::open_with_flags(&target, OpenFlags::SQLITE_OPEN_READ_WRITE)
                    .map_err(|_| failure("compacted target cannot be reopened"))?;
            verify_destination(destination, &inspection, &self.protection)?;
            require_cold_source(&target)?;
            reservation
                .sync_all()
                .map_err(|_| failure("new target could not be synced"))?;
            require_source_digest(&source, &plan.source_database_digest)?;
            let target_digest = file_digest(&target)?;
            Ok(target_digest)
        })();
        drop(source_connection);
        let release = lock.release();
        // A failure leaves only the new target in scope for recovery. Never
        // delete, overwrite, protect, checkpoint or switch the source here.
        let target_digest = result.map_err(|error| {
            p::Error(format!(
                "{error}; migration incomplete, new target retained and may contain plaintext; source remains an unprotected backup; authority unchanged"
            ))
        })?;
        release?;
        require_source_digest(&source, &plan.source_database_digest)?;
        require_cold_source(&source)?;
        Ok(LegacyMigrationReceipt {
            schema_version: LEGACY_MIGRATION_SCHEMA_VERSION,
            plan_digest: plan.digest.clone(),
            owner_digest: plan.owner_digest.clone(),
            store_identity: plan.store_identity.clone(),
            source_path_digest: plan.source_path_digest.clone(),
            source_database_digest: plan.source_database_digest.clone(),
            target_path_digest: plan.target_path_digest.clone(),
            target_database_digest: target_digest,
            event_count: plan.event_count,
            protected_rows: plan.content_rows,
            integrity_verified: true,
            replay_verified: true,
            source_unchanged: true,
            source_backup: SourceBackupDisposition::UnprotectedBackupOutsideProtectedDatabase,
            source_deleted: false,
            authority_switched: false,
        })
    }

    fn require_owner(&self, owner: &p::VerifiedPrincipal) -> p::Result<()> {
        if owner.0.trim().is_empty()
            || p::sha256_content_digest(owner.0.as_bytes()) != *self.protection.owner_digest()
        {
            return Err(failure("authenticated owner is missing or mismatched"));
        }
        Ok(())
    }

    fn inspect_source(&self, connection: &Connection) -> p::Result<Inspection> {
        let schema_digest = supported_schema(connection)?;
        integrity(connection)?;
        let authority = super::read_brain_authority_row(connection)
            .map_err(|_| failure("source owner authority metadata is invalid"))?
            .ok_or_else(|| failure("source owner authority is missing"))?;
        if authority.0 != *self.protection.owner_digest()
            || authority.2 != crate::BRAIN_AUTHORITY_SCHEMA_VERSION
            || count(connection, "SELECT COUNT(*) FROM brain_authority")? != 1
        {
            return Err(failure("source owner authority mismatch"));
        }
        p::sha256_digest_bytes(&authority.1)?;
        let active: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM authority_writer_lease WHERE expires_at > ?1",
                params![super::current_time_ms()?],
                |row| row.get(0),
            )
            .map_err(|_| failure("source authority lease cannot be verified"))?;
        if active != 0 {
            return Err(failure("source authority writer lease has not expired"));
        }
        for table in [
            "schema_migrations",
            "protected_content",
            "protected_rows",
            "protected_key_history",
            "protected_dependencies",
            "data_lifecycle_subjects",
            "data_lifecycle_jobs",
            "data_lifecycle_deleted_objects",
            "data_lifecycle_history",
            "data_lifecycle_state",
        ] {
            if count(connection, &format!("SELECT COUNT(*) FROM {table}"))? != 0 {
                return Err(failure(
                    "mixed protection, lifecycle or custom schema history is unsupported",
                ));
            }
        }
        at_rest::install(connection, Arc::new(RwLock::new(None)))?;
        let events = decode_events(connection)?;
        verify_builtin_replay(connection, &events)?;
        let content = read_content(connection)?;
        if content
            .iter()
            .any(|row| row.bytes.starts_with(at_rest::PREFIX))
        {
            return Err(failure(
                "source contains mixed protected and legacy content",
            ));
        }
        verify_idempotency(connection, &events, false)?;
        Ok(Inspection {
            store_identity: authority.1,
            schema_digest,
            events,
            content,
        })
    }
}

struct Inspection {
    store_identity: p::SchemaDigest,
    schema_digest: p::SchemaDigest,
    events: Vec<p::Event>,
    content: Vec<ContentRow>,
}

struct ContentRow {
    table: &'static str,
    key_column: &'static str,
    column: &'static str,
    rowid: i64,
    key: String,
    subject: String,
    scope: Option<p::Scope>,
    text: bool,
    bytes: Vec<u8>,
}

// The key and subject expressions mirror the existing live write paths.
// In particular active/history use activation event identity, not aggregate.
type ContentSpec = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    bool,
);
const CORE_CONTENT: &[ContentSpec] = &[
    ("events", "event_id", "payload", "t.run_id", None, false),
    ("session_state", "run_id", "state", "t.run_id", None, false),
    ("transcript", "run_id", "state", "t.run_id", None, false),
    (
        "strategy_candidates",
        "candidate_id",
        "candidate",
        "(SELECT run_id FROM events WHERE event_id=t.created_event_id)",
        Some("json_extract(t.candidate,'$.scope')"),
        false,
    ),
    (
        "stable_strategies",
        "candidate_id",
        "candidate",
        "(SELECT run_id FROM events WHERE event_id=t.promotion_event_id)",
        Some("json_extract(t.candidate,'$.scope')"),
        false,
    ),
    (
        "evolution_evaluations",
        "evaluation_ref",
        "evaluation",
        "(SELECT run_id FROM events WHERE event_id=t.event_id)",
        None,
        false,
    ),
    (
        "active_strategies",
        "activation_event",
        "active",
        "(SELECT run_id FROM events WHERE event_id=t.activation_event)",
        Some("t.scope"),
        false,
    ),
    (
        "evolution_history",
        "event_id",
        "active",
        "(SELECT run_id FROM events WHERE event_id=t.event_id)",
        Some("json_extract(t.active,'$.scope')"),
        false,
    ),
    (
        "workspace_charter_state",
        "workspace",
        "payload_json",
        "t.workspace",
        Some("t.workspace"),
        true,
    ),
    (
        "orchestration_state",
        "stream",
        "payload_json",
        "t.stream",
        None,
        true,
    ),
];

fn read_content(c: &Connection) -> p::Result<Vec<ContentRow>> {
    let mut result = Vec::new();
    for &(table, key, column, subject, scope, text) in CORE_CONTENT {
        read_content_table(c, table, key, column, subject, scope, text, &mut result)?;
    }
    for &(table, key, column, subject, scope) in at_rest::AUXILIARY_CONTENT {
        read_content_table(
            c,
            table,
            key,
            column,
            &subject.replace("NEW.", "t."),
            scope.map(|s| s.replace("NEW.", "t.")).as_deref(),
            false,
            &mut result,
        )?;
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn read_content_table(
    c: &Connection,
    table: &'static str,
    key: &'static str,
    column: &'static str,
    subject: &str,
    scope: Option<&str>,
    text: bool,
    result: &mut Vec<ContentRow>,
) -> p::Result<()> {
    let key_column = key;
    let order = if table == "events" {
        "t.run_id,t.stream_seq"
    } else {
        "t.rowid"
    };
    let mut query = c.prepare(&format!(
        "SELECT t.rowid,CAST(t.{key} AS TEXT),{subject},{},CAST(t.{column} AS BLOB) FROM {table} t ORDER BY {order}",
        scope.unwrap_or("NULL")
    )).map_err(|_| failure("content coverage query is unsupported"))?;
    let rows = query
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Vec<u8>>(4)?,
            ))
        })
        .map_err(|_| failure("content coverage scan failed"))?;
    for row in rows {
        let (rowid, key, subject, row_scope, bytes) =
            row.map_err(|_| failure("content row binding is incomplete"))?;
        if key.trim().is_empty()
            || subject.trim().is_empty()
            || (scope.is_some() && row_scope.as_deref().is_none_or(|s| s.trim().is_empty()))
        {
            return Err(failure("content row binding is incomplete"));
        }
        result.push(ContentRow {
            table,
            key_column,
            column,
            rowid,
            key,
            subject,
            scope: row_scope.map(p::Scope),
            text,
            bytes,
        });
    }
    Ok(())
}

fn transform(
    c: &mut Connection,
    inspection: &Inspection,
    protection: &StoreContentProtection,
) -> p::Result<()> {
    c.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON; PRAGMA temp_store=MEMORY; PRAGMA foreign_keys=ON;")
        .map_err(|_| failure("target protection pragmas failed"))?;
    at_rest::install(c, Arc::new(RwLock::new(Some(protection.clone()))))?;
    let tx = c
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| failure("target migration transaction failed"))?;
    tx.execute_batch("DROP TRIGGER events_immutable_update;")
        .map_err(|_| failure("target immutable-write guard cannot be suspended"))?;
    // Each source column is captured before any payload is rewritten.
    // seal_blob installs catalog, key history and the ordinary lineage rules.
    for row in &inspection.content {
        let sealed = at_rest::seal_blob(
            &tx,
            &row.bytes,
            row.scope.as_ref(),
            row.table,
            &row.key,
            &row.subject,
        )?;
        let sql = format!("UPDATE {} SET {}=?1 WHERE rowid=?2", row.table, row.column);
        let changed = if row.text {
            tx.execute(
                &sql,
                params![
                    String::from_utf8(sealed)
                        .map_err(|_| failure("protected text encoding failed"))?,
                    row.rowid
                ],
            )
        } else {
            tx.execute(&sql, params![sealed, row.rowid])
        }
        .map_err(|_| failure("target content rewrite failed"))?;
        if changed != 1 {
            return Err(failure("target content row changed"));
        }
    }
    for row in &inspection.content {
        record_dependencies(&tx, row)?;
    }
    for event in &inspection.events {
        if let Some(cause) = &event.provenance.caused_by {
            tx.execute("INSERT OR IGNORE INTO protected_dependencies(object_ref,source_object) SELECT t.object_ref,s.object_ref FROM protected_rows t,protected_rows s WHERE t.table_name='events' AND t.row_key=?1 AND s.table_name='events' AND s.row_key=?2",params![event.event_id.0,cause.0])
                .map_err(|_| failure("causal lineage migration failed"))?;
        }
    }
    rewrite_checksums(&tx)?;
    // The live protected idempotency ledger stores byte digests, rather than
    // ciphertext; preserve exactly that format from verified original facts.
    for event in &inspection.events {
        if let Some((namespace, key)) = super::domain_idempotency_key(event) {
            let payload =
                serde_json::to_vec(&event.payload).map_err(|_| failure("event encoding failed"))?;
            let provenance = serde_json::to_vec(&event.provenance)
                .map_err(|_| failure("event encoding failed"))?;
            let digest = p::sha256_content_digest(&super::domain_semantic_bytes(
                event,
                &payload,
                &provenance,
            ));
            tx.execute("UPDATE idempotency_keys SET semantic_bytes=?1 WHERE namespace=?2 AND \"key\"=?3 AND event_id=?4",params![digest.0.as_bytes(),namespace,key,event.event_id.0])
                .map_err(|_| failure("idempotency digest migration failed"))?;
        }
    }
    if table_exists(&tx, "events_fts")? {
        tx.execute(
            "INSERT INTO events_fts(events_fts) VALUES('delete-all')",
            [],
        )
        .map_err(|_| failure("plaintext FTS cleanup failed"))?;
    }
    tx.execute("DELETE FROM fts_coverage", [])
        .map_err(|_| failure("FTS coverage cleanup failed"))?;
    tx.execute_batch(super::SCHEMA)
        .map_err(|_| failure("target immutable guards cannot be restored"))?;
    tx.commit()
        .map_err(|_| failure("migration transaction could not be committed"))
}

fn record_dependencies(c: &Connection, row: &ContentRow) -> p::Result<()> {
    let event_columns: &[&str] = match row.table {
        "strategy_candidates" => &["created_event_id"],
        "stable_strategies" => &["created_event_id", "promotion_event_id"],
        "evolution_evaluations" | "evolution_history" | "workspace_charter_state" => &["event_id"],
        "active_strategies" => &["activation_event"],
        "federated_peers"
        | "remote_leases"
        | "ecosystem_publishers"
        | "ecosystem_admissions"
        | "ecosystem_package_states"
        | "ecosystem_distributions" => &["last_event_id"],
        _ => &[],
    };
    for column in event_columns {
        c.execute(&format!("INSERT OR IGNORE INTO protected_dependencies(object_ref,source_object) SELECT t.object_ref,s.object_ref FROM protected_rows t,protected_rows s WHERE t.table_name=?1 AND t.row_key=?2 AND s.table_name='events' AND s.row_key=(SELECT {column} FROM {} WHERE rowid=?3)", row.table),
            params![row.table,row.key,row.rowid])
            .map_err(|_| failure("derived event lineage migration failed"))?;
    }
    // All source events now have catalog entries, including forward refs.
    if serde_json::from_slice::<serde_json::Value>(&row.bytes).is_err() {
        return Ok(());
    }
    c.execute("INSERT OR IGNORE INTO protected_dependencies(object_ref,source_object) SELECT t.object_ref,s.object_ref FROM protected_rows t,protected_rows s WHERE t.table_name=?1 AND t.row_key=?2 AND s.table_name='events' AND s.object_ref!=t.object_ref AND (s.subject_digest=?3 OR s.row_key IN (SELECT value FROM json_each(forme_content_references(?4))))",
        params![row.table,row.key, if row.table == "events" { String::new() } else { p::sha256_content_digest(row.subject.as_bytes()).0 }, row.bytes])
        .map_err(|_| failure("content dependency migration failed"))?;
    Ok(())
}

fn rewrite_checksums(c: &Connection) -> p::Result<()> {
    let mut query = c.prepare("SELECT event_id,run_id,stream_seq,turn_id,kind,payload,schema_version,ts,provenance,checksum FROM events")
        .map_err(|_| failure("ciphertext checksum capture failed"))?;
    let rows = query
        .query_map([], StoredEvent::from_row)
        .map_err(|_| failure("ciphertext checksum scan failed"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| failure("ciphertext checksum row failed"))?;
    drop(query);
    for row in rows {
        let checksum = checksum::calculate(PersistedEnvelope {
            event_id: &row.event_id,
            run_id: &row.run_id,
            stream_seq: u64::try_from(row.stream_seq)
                .map_err(|_| failure("event sequence is invalid"))?,
            turn_id: row.turn_id.as_deref(),
            kind: &row.kind,
            payload: &row.payload,
            schema_version: u32::try_from(row.schema_version)
                .map_err(|_| failure("event schema is invalid"))?,
            ts_unix_ms: row.ts_unix_ms,
            provenance: &row.provenance,
        });
        c.execute(
            "UPDATE events SET checksum=?1 WHERE event_id=?2",
            params![checksum, row.event_id],
        )
        .map_err(|_| failure("ciphertext checksum rewrite failed"))?;
    }
    Ok(())
}

fn verify_destination(
    c: Connection,
    inspection: &Inspection,
    protection: &StoreContentProtection,
) -> p::Result<()> {
    supported_schema(&c)?;
    integrity(&c)?;
    at_rest::install(&c, Arc::new(RwLock::new(Some(protection.clone()))))?;
    for row in &inspection.content {
        let sql = format!("SELECT CAST({} AS BLOB),forme_open_bound(CAST({} AS BLOB),?1,?2) FROM {} WHERE CAST({} AS TEXT)=?2",row.column,row.column,row.table,row.key_column);
        let (sealed, plain): (Vec<u8>, Vec<u8>) = c
            .query_row(&sql, params![row.table, row.key], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|_| failure("target row authentication failed"))?;
        let envelope =
            at_rest::envelope(&sealed)?.ok_or_else(|| failure("target has unprotected content"))?;
        if plain != row.bytes
            || envelope.owner_digest != *protection.owner_digest()
            || envelope.scope
                != row
                    .scope
                    .as_ref()
                    .unwrap_or(protection.default_scope())
                    .clone()
        {
            return Err(failure("target content or owner/scope binding mismatch"));
        }
        let catalog: i64 = c.query_row("SELECT COUNT(*) FROM protected_rows r JOIN protected_key_history h ON r.object_ref=h.object_ref AND r.key_digest=h.key_digest WHERE r.object_ref=?1 AND r.owner_digest=?2 AND r.scope=?3 AND r.subject_digest=?4 AND r.key_digest=?5 AND r.table_name=?6 AND r.row_key=?7",
            params![envelope.object.0,envelope.owner_digest.0,envelope.scope.0,p::sha256_content_digest(row.subject.as_bytes()).0,envelope.key_digest.0,row.table,row.key], |r| r.get(0))
            .map_err(|_| failure("protected content catalog verification failed"))?;
        if catalog != 1 {
            return Err(failure("protected content catalog/history mismatch"));
        }
    }
    if count(&c, "SELECT COUNT(*) FROM protected_rows")? as usize != inspection.content.len()
        || count(&c, "SELECT COUNT(*) FROM protected_key_history")? as usize
            != inspection.content.len()
        || (table_exists(&c, "events_fts")? && count(&c, "SELECT COUNT(*) FROM events_fts")? != 0)
        || count(&c, "SELECT COUNT(*) FROM fts_coverage")? != 0
    {
        return Err(failure("target coverage or FTS cleanup is incomplete"));
    }
    verify_idempotency(&c, &inspection.events, true)?;
    let store = SqliteEventStore::from_connection(c, StoreOptions::default())?;
    store.enable_content_protection(protection.clone())?;
    let mut actual = Vec::new();
    for run in store.run_ids()? {
        actual.extend(
            store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()
                .map_err(|_| failure("target event checksum or authentication failed"))?,
        );
        if !store
            .replay(run, replay_snapshot())
            .map_err(|_| failure("target projection replay failed"))?
            .diff_vs_current
            .is_empty()
        {
            return Err(failure("target projection replay does not reconcile"));
        }
    }
    actual.sort_by(|a, b| (&a.run_id.0, a.stream_seq).cmp(&(&b.run_id.0, b.stream_seq)));
    if actual != inspection.events {
        return Err(failure(
            "target event continuity/content differs from source",
        ));
    }
    Ok(())
}

fn decode_events(c: &Connection) -> p::Result<Vec<p::Event>> {
    let mut events = super::read_all_stored_events(c)?
        .into_iter()
        .map(|e| {
            e.decode(
                &StoreOptions::default().current_schema,
                &super::UpcasterGraph::default(),
            )
            .map_err(|_| failure("source event checksum/schema is invalid"))
        })
        .collect::<p::Result<Vec<_>>>()?;
    events.sort_by(|a, b| (&a.run_id.0, a.stream_seq).cmp(&(&b.run_id.0, b.stream_seq)));
    let mut heads = BTreeMap::new();
    for e in &events {
        let head = heads.entry(&e.run_id).or_insert(0);
        if e.stream_seq != *head + 1 {
            return Err(failure("source event sequence is discontinuous"));
        }
        *head = e.stream_seq;
    }
    Ok(events)
}

fn verify_builtin_replay(c: &Connection, events: &[p::Event]) -> p::Result<()> {
    let rebuilt = super::fold_builtin_projections(
        super::read_all_stored_events(c)?,
        &StoreOptions::default().current_schema,
        &super::UpcasterGraph::default(),
        &crate::projection_privacy::metadata_watermarks(c, None)?,
    )
    .map_err(|_| failure("source projection replay failed"))?;
    for state in rebuilt {
        let run = state
            .session
            .run_id
            .as_ref()
            .ok_or_else(|| failure("source projection run is missing"))?;
        let session = super::read_session_state(c, run)
            .map_err(|_| failure("source session projection is invalid"))?;
        let transcript = super::read_transcript(c, run)
            .map_err(|_| failure("source transcript projection is invalid"))?;
        if !super::projection_diff(
            session.as_ref(),
            &state.session,
            transcript.as_ref(),
            &state.transcript,
        )
        .is_empty()
        {
            return Err(failure("source projection replay does not reconcile"));
        }
    }
    let runs = events
        .iter()
        .map(|e| &e.run_id)
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u64;
    if count(c, "SELECT COUNT(*) FROM session_state")? != runs
        || count(c, "SELECT COUNT(*) FROM transcript")? != runs
    {
        return Err(failure(
            "source contains orphaned or missing builtin projections",
        ));
    }
    Ok(())
}

fn verify_idempotency(c: &Connection, events: &[p::Event], protected: bool) -> p::Result<()> {
    let mut expected = BTreeMap::new();
    for event in events {
        if let Some((namespace, key)) = super::domain_idempotency_key(event) {
            let payload =
                serde_json::to_vec(&event.payload).map_err(|_| failure("event encoding failed"))?;
            let provenance = serde_json::to_vec(&event.provenance)
                .map_err(|_| failure("event encoding failed"))?;
            let bytes = super::domain_semantic_bytes(event, &payload, &provenance);
            let bytes = if protected {
                p::sha256_content_digest(&bytes).0.into_bytes()
            } else {
                bytes
            };
            expected.insert(
                (namespace.to_owned(), key),
                (event.event_id.0.clone(), event.run_id.0.clone(), bytes),
            );
        }
    }
    let mut stmt = c
        .prepare("SELECT namespace,\"key\",event_id,run_id,semantic_bytes FROM idempotency_keys")
        .map_err(|_| failure("idempotency verification failed"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                (
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                ),
            ))
        })
        .map_err(|_| failure("idempotency verification failed"))?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        .map_err(|_| failure("idempotency verification failed"))?;
    if rows != expected {
        return Err(failure("idempotency ledger does not match event facts"));
    }
    Ok(())
}

fn replay_snapshot() -> SchemaSnapshot {
    SchemaSnapshot {
        schema: StoreOptions::default().current_schema,
        policy_version: p::Version(1),
        loop_version: p::Version(1),
        model_profile: p::ModelProfileRef("legacy-migration-replay".into()),
        tool_schema: p::Version(1),
    }
}

#[path = "migration_schema.rs"]
mod schema_inspection;
pub(crate) use schema_inspection::supported_schema;

fn integrity(c: &Connection) -> p::Result<()> {
    let mut stmt = c
        .prepare("PRAGMA integrity_check")
        .map_err(|_| failure("SQLite integrity check failed"))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|_| failure("SQLite integrity check failed"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| failure("SQLite integrity check failed"))?;
    if rows != ["ok"] || count(c, "SELECT COUNT(*) FROM pragma_foreign_key_check")? != 0 {
        return Err(failure(
            "SQLite integrity or foreign-key verification failed",
        ));
    }
    Ok(())
}

fn compact_target(path: &Path) -> p::Result<()> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| failure("target compaction unavailable"))?;
    let (busy, _, _): (i64, i64, i64) = c
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(|_| failure("target checkpoint failed"))?;
    if busy != 0 {
        return Err(failure("target checkpoint is busy"));
    }
    c.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON; VACUUM;")
        .map_err(|_| failure("target plaintext free-page compaction failed"))?;
    let free: i64 = c
        .pragma_query_value(None, "freelist_count", |r| r.get(0))
        .map_err(|_| failure("target freelist verification failed"))?;
    if free != 0 {
        return Err(failure("target free pages remain"));
    }
    c.close()
        .map_err(|_| failure("target compaction could not close"))
}

fn count(c: &Connection, sql: &str) -> p::Result<u64> {
    let value: i64 = c
        .query_row(sql, [], |r| r.get(0))
        .map_err(|_| failure("coverage query failed"))?;
    u64::try_from(value).map_err(|_| failure("coverage count is invalid"))
}
fn table_exists(c: &Connection, name: &str) -> p::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        params![name],
        |r| r.get(0),
    )
    .map_err(|_| failure("table inspection failed"))
}

fn source_path(path: &Path) -> p::Result<PathBuf> {
    let path = crate::store_access::normalized(path)?;
    if !path.is_file() {
        return Err(failure("source must be an existing regular database"));
    }
    Ok(path)
}
fn new_target_path(path: &Path) -> p::Result<PathBuf> {
    let name = path
        .file_name()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| failure("target path is missing"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let target = parent
        .canonicalize()
        .map_err(|_| failure("target parent must already exist"))?
        .join(name);
    if fs::symlink_metadata(&target).is_ok() {
        return Err(failure("target must be a NEW path"));
    }
    require_cold_source(&target)?;
    Ok(target)
}
fn path_digest(path: &Path) -> p::Result<p::SchemaDigest> {
    path.to_str()
        .map(|s| p::sha256_content_digest(s.as_bytes()))
        .ok_or_else(|| failure("path encoding is unsupported"))
}
fn file_digest(path: &Path) -> p::Result<p::SchemaDigest> {
    fs::read(path)
        .map(|b| p::sha256_content_digest(&b))
        .map_err(|_| failure("database digest unavailable"))
}
fn require_source_digest(path: &Path, expected: &p::SchemaDigest) -> p::Result<()> {
    if file_digest(path)? != *expected {
        return Err(failure("source changed since the approved plan"));
    }
    Ok(())
}
fn reserve_target(path: &Path) -> p::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3); // FILE_SHARE_READ | FILE_SHARE_WRITE, no deletion.
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| failure("target must be a NEW regular file"))
}

fn failure(reason: &str) -> p::Error {
    p::Error(format!("legacy plaintext migration: {reason}"))
}
