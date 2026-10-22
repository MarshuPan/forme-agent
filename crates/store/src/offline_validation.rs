//! Read-only validation shared by old-brain inspection and offline restore.
use crate::{at_rest, backup, offline_access::immutable_source, StoreContentProtection};
use forme_protocol as p;
use rusqlite::{params, Connection};
use std::{
    path::Path,
    sync::{Arc, RwLock},
};

const CONTENT: &[(&str, &str, &str)] = &[
    ("events", "event_id", "payload"),
    ("session_state", "run_id", "state"),
    ("transcript", "run_id", "state"),
    ("workspace_charter_state", "workspace", "payload_json"),
    ("orchestration_state", "stream", "payload_json"),
    ("strategy_candidates", "candidate_id", "candidate"),
    ("stable_strategies", "candidate_id", "candidate"),
    ("evolution_evaluations", "evaluation_ref", "evaluation"),
    ("active_strategies", "activation_event", "active"),
    ("evolution_history", "event_id", "active"),
];

pub(crate) fn inspect(
    path: &Path,
    protection: &StoreContentProtection,
) -> p::Result<super::OfflineBrainIdentity> {
    let connection = immutable_source(path)?;
    crate::sqlite::migration::supported_schema(&connection)?;
    let schema_digest = backup::schema_digest(&connection)?;
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|_| invalid())?;
    if integrity != "ok" || !backup::event_continuity(&connection)? {
        return Err(invalid());
    }
    let (owner, identity, schema): (String, String, u32) = connection
        .query_row(
            "SELECT owner_digest,store_identity,schema_version FROM brain_authority",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| invalid())?;
    if owner != protection.owner_digest().0
        || schema != crate::BRAIN_AUTHORITY_SCHEMA_VERSION.0
        || count(&connection, "SELECT COUNT(*) FROM brain_authority")? != 1
        || count(
            &connection,
            "SELECT COUNT(*) FROM data_lifecycle_jobs WHERE state='pending'",
        )? != 0
    {
        return Err(invalid());
    }
    let identity = p::SchemaDigest(identity);
    p::sha256_digest_bytes(&identity)?;
    at_rest::install(&connection, Arc::new(RwLock::new(Some(protection.clone()))))?;
    let mut rows = 0;
    for (table, key, column) in CONTENT.iter().copied().chain(
        at_rest::AUXILIARY_CONTENT
            .iter()
            .map(|(t, k, c, _, _)| (*t, *k, *c)),
    ) {
        let mut query = connection
            .prepare(&format!(
                "SELECT CAST({key} AS TEXT), CAST({column} AS BLOB) FROM {table}"
            ))
            .map_err(|_| invalid())?;
        let mut values = query.query([]).map_err(|_| invalid())?;
        while let Some(row) = values.next().map_err(|_| invalid())? {
            let key: String = row.get(0).map_err(|_| invalid())?;
            let bytes: Vec<u8> = row.get(1).map_err(|_| invalid())?;
            let envelope = at_rest::envelope(&bytes)?.ok_or_else(invalid)?;
            let expected = p::ObjectRef(format!(
                "content:{}",
                p::sha256_content_digest(format!("{table}:{key}").as_bytes()).0
            ));
            if envelope.object != expected || envelope.owner_digest != *protection.owner_digest() {
                return Err(invalid());
            }
            verify_catalog(&connection, table, &key, &envelope)?;
            let deleted: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM data_lifecycle_deleted_objects WHERE object_ref=?1)",params![envelope.object.0],|r|r.get(0)).map_err(|_|invalid())?;
            if deleted {
                if protection.open(&bytes).is_ok() {
                    return Err(invalid());
                }
            } else {
                // Both authentication and row binding are checked, never just
                // envelope shape or the availability of some unrelated key.
                at_rest::open_bound_blob(&connection, &bytes, table, &key)?;
            }
            rows += 1;
        }
    }
    let mut query = connection
        .prepare("SELECT content_ref, record, digest FROM protected_content")
        .map_err(|_| invalid())?;
    let mut records = query.query([]).map_err(|_| invalid())?;
    while let Some(row) = records.next().map_err(|_| invalid())? {
        let key: String = row.get(0).map_err(|_| invalid())?;
        let bytes: Vec<u8> = row.get(1).map_err(|_| invalid())?;
        let digest: String = row.get(2).map_err(|_| invalid())?;
        let record: crate::ProtectedContentRecord =
            serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        record.validate()?;
        if record.content_ref.0 != key
            || record.owner_digest != *protection.owner_digest()
            || p::canonical_digest(&record)?.0 != digest
        {
            return Err(invalid());
        }
        verify_catalog(&connection, "protected_content", &key, &record.envelope)?;
        protection
            .content_service()
            .decrypt(&record.envelope.binding(), &record.envelope)?;
        rows += 1;
    }
    let last: i64 = connection
        .query_row("SELECT COALESCE(MAX(stream_seq),0) FROM events", [], |r| {
            r.get(0)
        })
        .map_err(|_| invalid())?;
    Ok(super::OfflineBrainIdentity {
        schema_version: p::SchemaVersion(1),
        owner_digest: protection.owner_digest().clone(),
        store_identity: identity,
        schema_digest,
        database_digest: backup::file_digest(path)?,
        event_count: count(&connection, "SELECT COUNT(*) FROM events")?,
        last_stream_seq: u64::try_from(last).map_err(|_| invalid())?,
        authenticated_content_rows: rows,
    })
}

fn verify_catalog(
    c: &Connection,
    table: &str,
    key: &str,
    e: &p::EncryptedContentEnvelope,
) -> p::Result<()> {
    let matches: i64 = c.query_row("SELECT COUNT(*) FROM protected_rows r JOIN protected_key_history h ON r.object_ref=h.object_ref AND r.key_digest=h.key_digest WHERE r.object_ref=?1 AND r.owner_digest=?2 AND r.scope=?3 AND r.key_digest=?4 AND r.table_name=?5 AND r.row_key=?6",
        params![e.object.0,e.owner_digest.0,e.scope.0,e.key_digest.0,table,key],|r|r.get(0)).map_err(|_|invalid())?;
    if matches != 1 {
        return Err(invalid());
    }
    Ok(())
}

fn count(c: &Connection, sql: &str) -> p::Result<u64> {
    let value: i64 = c.query_row(sql, [], |r| r.get(0)).map_err(|_| invalid())?;
    u64::try_from(value).map_err(|_| invalid())
}
pub(crate) fn invalid() -> p::Error {
    p::Error("offline brain identity, protection, lineage or integrity verification failed".into())
}
