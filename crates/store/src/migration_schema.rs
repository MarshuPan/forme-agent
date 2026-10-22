//! Explicit schema and content-surface allowlist for offline recovery.
use super::{failure, table_exists};
use forme_protocol as p;
use rusqlite::{params, Connection};
pub(crate) fn supported_schema(c: &Connection) -> p::Result<p::SchemaDigest> {
    let version: i64 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| failure("source schema version is unavailable"))?;
    if version != 9 && version != crate::sqlite::DATABASE_SCHEMA_VERSION {
        return Err(failure("unsupported legacy database schema version"));
    }
    require_explicit_surface_coverage(c)?;
    let reference =
        Connection::open_in_memory().map_err(|_| failure("schema verifier unavailable"))?;
    reference
        .execute_batch(crate::sqlite::SCHEMA)
        .and_then(|()| reference.execute_batch(crate::charter::CHARTER_SCHEMA))
        .and_then(|()| reference.execute_batch(crate::orchestration::SCHEMA))
        .map_err(|_| failure("schema verifier initialization failed"))?;
    if version == crate::sqlite::DATABASE_SCHEMA_VERSION {
        reference
            .execute_batch(crate::restoration_fence::SCHEMA)
            .map_err(|_| failure("restore schema verifier initialization failed"))?;
    }
    if table_exists(c, "events_fts")? {
        reference
            .execute_batch(crate::sqlite::FTS_SCHEMA)
            .map_err(|_| failure("FTS schema verifier unavailable"))?;
    }
    let actual = schema_rows(c)?;
    if actual != schema_rows(&reference)? {
        return Err(failure(
            "unsupported table, column, index, trigger or schema",
        ));
    }
    p::canonical_digest(&actual)
}

// This is deliberately independent of SCHEMA. A future schema addition must
// get an explicit coverage decision here; merely compiling against newer live
// CREATE statements must not silently make a plaintext surface migratable.
const SURFACE_COLUMNS: &[(&str, &str)] = &[
    ("events", "event_id run_id stream_seq turn_id kind payload schema_version ts provenance checksum"),
    ("idempotency_keys", "namespace key event_id run_id semantic_bytes"),
    ("schema_migrations", "payload_type from_version to_version note implementation_identity"),
    ("session_state", "run_id state last_stream_seq"),
    ("transcript", "run_id state last_stream_seq"),
    ("fts_coverage", "singleton event_set schema_registry upcaster_graph"),
    ("sync_peers", "singleton peer_ref owner_ref semantic_bytes"),
    ("sync_batches", "batch_id peer_ref aggregate expected_version semantic_bytes resulting_version applied_event_ids"),
    ("sync_cursors", "peer_ref aggregate applied_version exported_version"),
    ("strategy_candidates", "candidate_id created_event_id candidate"),
    ("evolution_evaluations", "evaluation_ref event_id evaluation"),
    ("stable_strategies", "candidate_id created_event_id promotion_event_id candidate"),
    ("evolution_versions", "aggregate version"),
    ("active_strategies", "aggregate domain scope aggregate_version activation_event active"),
    ("evolution_history", "aggregate committed_version event_id kind active"),
    ("federation_versions", "aggregate version"),
    ("federation_state", "singleton authority_epoch"),
    ("federated_peers", "peer_ref grant_ref grant_version authority_epoch revoked grant last_event_id"),
    ("federation_history", "aggregate committed_version event_id kind"),
    ("remote_leases", "lease_ref dispatch_id executor_peer grant_ref authority_epoch fence_token state lease last_event_id"),
    ("remote_dispatches", "dispatch_id lease_ref plan_digest attempted"),
    ("replication_exports", "batch_ref peer_ref aggregate from_seq to_seq authority_epoch content_digest semantic_bytes acknowledged"),
    ("replication_checkpoints", "peer_ref aggregate stream_seq authority_epoch batch_ref batch_digest projection_digest"),
    ("federation_control_replay", "peer_ref nonce command_digest"),
    ("federation_device_replay", "signal_ref peer_ref digest"),
    ("federated_checkpoint_artifacts", "checkpoint_ref source_run artifact"),
    ("federated_handoffs", "next_run plan"),
    ("federated_retention", "request_ref state"),
    ("remote_action_recovery", "run_id lease_ref record"),
    ("ecosystem_versions", "aggregate version"),
    ("ecosystem_publishers", "publisher_ref grant_ref grant_version scope status expires_at grant last_event_id"),
    ("ecosystem_admissions", "release_ref admission_ref package_ref package_digest scope admission last_event_id"),
    ("ecosystem_package_states", "package_ref release_ref package_digest scope lifecycle active_generation state last_event_id"),
    ("ecosystem_distributions", "receipt_ref package_ref release_ref scope peer_ref receipt last_event_id"),
    ("ecosystem_history", "aggregate committed_version event_id kind"),
    ("ecosystem_nonces", "nonce plan_digest"),
    ("ecosystem_distribution_attempts", "receipt_ref semantic"),
    ("ecosystem_package_bundles", "release_ref package_ref package_digest bundle"),
    ("brain_authority", "owner_digest store_identity schema_version created_at"),
    ("brain_restore_fences", "generation payload digest"),
    ("brain_aggregates", "owner_digest kind aggregate_run_id"),
    ("authority_writer_lease", "store_identity process_nonce expires_at"),
    ("cognitive_tick_claims", "owner_digest window_start lease_nonce attempt claimed_at expires_at state range_json consumed_json report_digest"),
    ("cognitive_tick_cursor", "owner_digest run_id position"),
    ("approval_consumptions", "nonce owner_digest store_identity approval_id plan_digest semantic_digest claimed_at"),
    ("protected_content", "content_ref owner_digest scope subject_digest key_digest record digest"),
    ("protected_rows", "object_ref owner_digest scope subject_digest key_digest table_name row_key"),
    ("protected_dependencies", "object_ref source_object"),
    ("protected_key_history", "object_ref key_digest"),
    ("data_lifecycle_subjects", "scope subject_digest state"),
    ("data_lifecycle_jobs", "aggregate event_id plan state"),
    ("data_lifecycle_deleted_objects", "object_ref"),
    ("data_lifecycle_history", "aggregate version event_id tombstone digest"),
    ("data_lifecycle_state", "aggregate version event_id tombstone digest"),
    ("workspace_charter_state", "workspace version event_id payload_json digest"),
    ("orchestration_state", "stream revision payload_json digest"),
    ("events_fts", "text"),
    ("events_fts_data", "id block"),
    ("events_fts_idx", "segid term pgno"),
    ("events_fts_docsize", "id sz"),
    ("events_fts_config", "k v"),
];

fn require_explicit_surface_coverage(c: &Connection) -> p::Result<()> {
    let mut tables = c
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .map_err(|_| failure("content surface inventory failed"))?;
    let names = tables
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|_| failure("content surface inventory failed"))?;
    for name in names {
        let name = name.map_err(|_| failure("content surface inventory failed"))?;
        let expected = SURFACE_COLUMNS
            .iter()
            .find(|(table, _)| *table == name)
            .ok_or_else(|| failure("unsupported table has no explicit content coverage"))?
            .1;
        let mut columns = c
            .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
            .map_err(|_| failure("content column inventory failed"))?;
        let actual = columns
            .query_map(params![name], |r| r.get::<_, String>(0))
            .map_err(|_| failure("content column inventory failed"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| failure("content column inventory failed"))?;
        if actual
            .iter()
            .map(String::as_str)
            .ne(expected.split_whitespace())
        {
            return Err(failure(
                "unsupported column has no explicit content coverage",
            ));
        }
    }
    Ok(())
}

type SchemaRow = (String, String, String, Option<String>);
fn schema_rows(c: &Connection) -> p::Result<Vec<SchemaRow>> {
    let mut stmt = c
        .prepare("SELECT type,name,tbl_name,sql FROM sqlite_master ORDER BY type,name")
        .map_err(|_| failure("schema inspection failed"))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(|_| failure("schema inspection failed"))?;
    rows.collect::<rusqlite::Result<_>>()
        .map_err(|_| failure("schema inspection failed"))
}
