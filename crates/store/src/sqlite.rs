use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex, RwLock},
};

#[cfg(test)]
use std::cell::RefCell;

use forme_protocol as p;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction, TransactionBehavior};

use crate::{
    checksum::{self, PersistedEnvelope},
    cursor::{CursorPage, EventCursor},
    projection::searchable_text,
    replay::projection_diff,
    schema::UpcasterGraph,
    EcosystemEventStore, EcosystemPackageArchive, EcosystemProjection, EcosystemRuntimeLedger,
    EventStore, EvolutionEventStore, EvolutionHistoryEntry, EvolutionProjection,
    FederationEventStore, FederationProjection, FederationRuntimeLedger, PayloadType, Projection,
    ProjectionScope, RemoteDispatchLedger, ReplayReport, SchemaSnapshot, SearchHit, SessionState,
    SessionStateProjection, StableStrategyRecord, SyncCursorState, Transcript,
    TranscriptProjection, Upcaster, VersionedEventStore,
};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    event_id       TEXT PRIMARY KEY,
    run_id         TEXT NOT NULL,
    stream_seq     INTEGER NOT NULL,
    turn_id        TEXT,
    kind           TEXT NOT NULL,
    payload        BLOB NOT NULL,
    schema_version INTEGER NOT NULL,
    ts             INTEGER NOT NULL,
    provenance     BLOB NOT NULL,
    checksum       TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_events_run ON events(run_id, stream_seq);
CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    namespace      TEXT NOT NULL,
    "key"          TEXT NOT NULL,
    event_id       TEXT NOT NULL,
    run_id         TEXT NOT NULL,
    semantic_bytes BLOB NOT NULL,
    UNIQUE(namespace, "key"),
    FOREIGN KEY(event_id) REFERENCES events(event_id)
        DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE IF NOT EXISTS schema_migrations (
    payload_type           TEXT NOT NULL,
    from_version           INTEGER NOT NULL,
    to_version             INTEGER NOT NULL,
    note                   TEXT NOT NULL,
    implementation_identity TEXT NOT NULL,
    PRIMARY KEY(payload_type, from_version),
    CHECK(length(trim(note)) > 0),
    CHECK(length(trim(implementation_identity)) > 0),
    CHECK(to_version > from_version)
);

CREATE TABLE IF NOT EXISTS session_state (
    run_id          TEXT PRIMARY KEY,
    state           BLOB NOT NULL,
    last_stream_seq INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS transcript (
    run_id          TEXT PRIMARY KEY,
    state           BLOB NOT NULL,
    last_stream_seq INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS fts_coverage (
    singleton        INTEGER PRIMARY KEY CHECK(singleton = 1),
    event_set        BLOB NOT NULL,
    schema_registry  BLOB NOT NULL,
    upcaster_graph   BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_peers (
    singleton      INTEGER PRIMARY KEY CHECK(singleton = 1),
    peer_ref       TEXT NOT NULL UNIQUE,
    owner_ref      TEXT NOT NULL,
    semantic_bytes BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_batches (
    batch_id          TEXT PRIMARY KEY,
    peer_ref          TEXT NOT NULL,
    aggregate         TEXT NOT NULL,
    expected_version  INTEGER NOT NULL,
    semantic_bytes    BLOB NOT NULL,
    resulting_version INTEGER NOT NULL,
    applied_event_ids BLOB NOT NULL,
    FOREIGN KEY(peer_ref) REFERENCES sync_peers(peer_ref)
);

CREATE TABLE IF NOT EXISTS sync_cursors (
    peer_ref        TEXT NOT NULL,
    aggregate       TEXT NOT NULL,
    applied_version INTEGER NOT NULL DEFAULT 0,
    exported_version INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(peer_ref, aggregate),
    FOREIGN KEY(peer_ref) REFERENCES sync_peers(peer_ref)
);

CREATE TABLE IF NOT EXISTS strategy_candidates (
    candidate_id     TEXT PRIMARY KEY,
    created_event_id TEXT NOT NULL UNIQUE,
    candidate        BLOB NOT NULL,
    FOREIGN KEY(created_event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS evolution_evaluations (
    evaluation_ref TEXT PRIMARY KEY,
    event_id       TEXT NOT NULL UNIQUE,
    evaluation     BLOB NOT NULL,
    FOREIGN KEY(event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS stable_strategies (
    candidate_id       TEXT PRIMARY KEY,
    created_event_id   TEXT NOT NULL,
    promotion_event_id TEXT NOT NULL UNIQUE,
    candidate          BLOB NOT NULL,
    FOREIGN KEY(created_event_id) REFERENCES events(event_id),
    FOREIGN KEY(promotion_event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS evolution_versions (
    aggregate TEXT PRIMARY KEY,
    version   INTEGER NOT NULL CHECK(version >= 0)
);

CREATE TABLE IF NOT EXISTS active_strategies (
    aggregate         TEXT NOT NULL,
    domain            TEXT NOT NULL,
    scope             TEXT NOT NULL,
    aggregate_version INTEGER NOT NULL CHECK(aggregate_version > 0),
    activation_event  TEXT NOT NULL,
    active            BLOB NOT NULL,
    PRIMARY KEY(aggregate, domain, scope),
    FOREIGN KEY(activation_event) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS evolution_history (
    aggregate         TEXT NOT NULL,
    committed_version INTEGER NOT NULL CHECK(committed_version > 0),
    event_id          TEXT NOT NULL UNIQUE,
    kind              TEXT NOT NULL,
    active            BLOB NOT NULL,
    PRIMARY KEY(aggregate, committed_version),
    FOREIGN KEY(event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS federation_versions (
    aggregate TEXT PRIMARY KEY,
    version   INTEGER NOT NULL CHECK(version >= 0)
);

CREATE TABLE IF NOT EXISTS federation_state (
    singleton       INTEGER PRIMARY KEY CHECK(singleton = 1),
    authority_epoch INTEGER NOT NULL CHECK(authority_epoch >= 0)
);

CREATE TABLE IF NOT EXISTS federated_peers (
    peer_ref          TEXT PRIMARY KEY,
    grant_ref         TEXT NOT NULL UNIQUE,
    grant_version     INTEGER NOT NULL CHECK(grant_version > 0),
    authority_epoch   INTEGER NOT NULL CHECK(authority_epoch > 0),
    revoked           INTEGER NOT NULL CHECK(revoked IN (0, 1)),
    grant              BLOB NOT NULL,
    last_event_id      TEXT NOT NULL UNIQUE,
    FOREIGN KEY(last_event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS federation_history (
    aggregate         TEXT NOT NULL,
    committed_version INTEGER NOT NULL CHECK(committed_version > 0),
    event_id          TEXT NOT NULL UNIQUE,
    kind              TEXT NOT NULL,
    PRIMARY KEY(aggregate, committed_version),
    FOREIGN KEY(event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS remote_leases (
    lease_ref         TEXT PRIMARY KEY,
    dispatch_id       TEXT NOT NULL UNIQUE,
    executor_peer     TEXT NOT NULL,
    grant_ref         TEXT NOT NULL,
    authority_epoch   INTEGER NOT NULL CHECK(authority_epoch > 0),
    fence_token       INTEGER NOT NULL CHECK(fence_token > 0),
    state             TEXT NOT NULL,
    lease              BLOB NOT NULL,
    last_event_id      TEXT NOT NULL UNIQUE,
    FOREIGN KEY(last_event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS remote_dispatches (
    dispatch_id       TEXT PRIMARY KEY,
    lease_ref         TEXT NOT NULL UNIQUE,
    plan_digest       TEXT NOT NULL,
    attempted         INTEGER NOT NULL CHECK(attempted IN (0, 1)),
    FOREIGN KEY(lease_ref) REFERENCES remote_leases(lease_ref)
);

CREATE TABLE IF NOT EXISTS replication_exports (
    batch_ref         TEXT PRIMARY KEY,
    peer_ref          TEXT NOT NULL,
    aggregate         TEXT NOT NULL,
    from_seq          INTEGER NOT NULL,
    to_seq            INTEGER NOT NULL,
    authority_epoch   INTEGER NOT NULL,
    content_digest    TEXT NOT NULL,
    semantic_bytes    BLOB NOT NULL,
    acknowledged      INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged IN (0, 1)),
    UNIQUE(peer_ref, aggregate, from_seq, to_seq)
);

CREATE TABLE IF NOT EXISTS replication_checkpoints (
    peer_ref          TEXT NOT NULL,
    aggregate         TEXT NOT NULL,
    stream_seq        INTEGER NOT NULL,
    authority_epoch   INTEGER NOT NULL,
    batch_ref         TEXT NOT NULL,
    batch_digest      TEXT NOT NULL,
    projection_digest TEXT NOT NULL,
    PRIMARY KEY(peer_ref, aggregate),
    FOREIGN KEY(batch_ref) REFERENCES replication_exports(batch_ref)
);

CREATE TABLE IF NOT EXISTS federation_control_replay (
    peer_ref       TEXT NOT NULL,
    nonce          TEXT NOT NULL,
    command_digest TEXT NOT NULL,
    PRIMARY KEY(peer_ref, nonce)
);

CREATE TABLE IF NOT EXISTS federation_device_replay (
    signal_ref TEXT PRIMARY KEY,
    peer_ref   TEXT NOT NULL,
    digest     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS federated_checkpoint_artifacts (
    checkpoint_ref TEXT PRIMARY KEY,
    source_run     TEXT NOT NULL,
    artifact       BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS federated_handoffs (
    next_run TEXT PRIMARY KEY,
    plan     BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS federated_retention (
    request_ref TEXT PRIMARY KEY,
    state       BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS remote_action_recovery (
    run_id    TEXT PRIMARY KEY,
    lease_ref TEXT NOT NULL UNIQUE,
    record    BLOB NOT NULL,
    FOREIGN KEY(lease_ref) REFERENCES remote_leases(lease_ref)
);

CREATE TABLE IF NOT EXISTS ecosystem_versions (
    aggregate TEXT PRIMARY KEY,
    version   INTEGER NOT NULL CHECK(version >= 0)
);

CREATE TABLE IF NOT EXISTS ecosystem_publishers (
    publisher_ref  TEXT PRIMARY KEY,
    grant_ref      TEXT NOT NULL UNIQUE,
    grant_version  INTEGER NOT NULL CHECK(grant_version > 0),
    scope          TEXT NOT NULL,
    status         TEXT NOT NULL,
    expires_at     INTEGER NOT NULL,
    grant          BLOB NOT NULL,
    last_event_id  TEXT NOT NULL UNIQUE,
    FOREIGN KEY(last_event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS ecosystem_admissions (
    release_ref    TEXT PRIMARY KEY,
    admission_ref  TEXT NOT NULL UNIQUE,
    package_ref    TEXT NOT NULL,
    package_digest TEXT NOT NULL,
    scope          TEXT NOT NULL,
    admission      BLOB NOT NULL,
    last_event_id  TEXT NOT NULL UNIQUE,
    FOREIGN KEY(last_event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS ecosystem_package_states (
    package_ref       TEXT PRIMARY KEY,
    release_ref       TEXT NOT NULL,
    package_digest    TEXT NOT NULL,
    scope             TEXT NOT NULL,
    lifecycle         TEXT NOT NULL,
    active_generation INTEGER NOT NULL CHECK(active_generation > 0),
    state             BLOB NOT NULL,
    last_event_id     TEXT NOT NULL UNIQUE,
    FOREIGN KEY(last_event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS ecosystem_distributions (
    receipt_ref    TEXT PRIMARY KEY,
    package_ref    TEXT NOT NULL,
    release_ref    TEXT NOT NULL,
    scope          TEXT NOT NULL,
    peer_ref       TEXT NOT NULL,
    receipt        BLOB NOT NULL,
    last_event_id  TEXT NOT NULL UNIQUE,
    FOREIGN KEY(last_event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS ecosystem_history (
    aggregate         TEXT NOT NULL,
    committed_version INTEGER NOT NULL CHECK(committed_version > 0),
    event_id          TEXT NOT NULL UNIQUE,
    kind              TEXT NOT NULL,
    PRIMARY KEY(aggregate, committed_version),
    FOREIGN KEY(event_id) REFERENCES events(event_id)
);

CREATE TABLE IF NOT EXISTS ecosystem_nonces (
    nonce       TEXT PRIMARY KEY,
    plan_digest TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ecosystem_distribution_attempts (
    receipt_ref TEXT PRIMARY KEY,
    semantic    BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS ecosystem_package_bundles (
    release_ref    TEXT PRIMARY KEY,
    package_ref    TEXT NOT NULL,
    package_digest TEXT NOT NULL UNIQUE,
    bundle         BLOB NOT NULL
);

CREATE TRIGGER IF NOT EXISTS events_immutable_update
BEFORE UPDATE ON events
BEGIN
    SELECT RAISE(ABORT, 'events are immutable');
END;

CREATE TRIGGER IF NOT EXISTS events_immutable_delete
BEFORE DELETE ON events
BEGIN
    SELECT RAISE(ABORT, 'events are immutable');
END;
"#;

const FTS_SCHEMA: &str = "CREATE VIRTUAL TABLE IF NOT EXISTS events_fts \
                         USING fts5(text, content='');";
const DATABASE_SCHEMA_VERSION: i64 = 6;

#[cfg(test)]
std::thread_local! {
    static REPLAY_AFTER_EVENTS_MATERIALIZED: RefCell<Option<Box<dyn FnOnce()>>> =
        RefCell::new(None);
    static SEARCH_AFTER_COVERAGE_VALIDATED: RefCell<Option<Box<dyn FnOnce()>>> =
        RefCell::new(None);
}

#[cfg(test)]
fn install_replay_after_events_materialized_hook(hook: impl FnOnce() + 'static) {
    REPLAY_AFTER_EVENTS_MATERIALIZED.with(|slot| {
        let previous = slot.borrow_mut().replace(Box::new(hook));
        assert!(
            previous.is_none(),
            "replay coordination hook already installed"
        );
    });
}

#[cfg(test)]
fn run_replay_after_events_materialized_hook() {
    let hook = REPLAY_AFTER_EVENTS_MATERIALIZED.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
fn install_search_after_coverage_validated_hook(hook: impl FnOnce() + 'static) {
    SEARCH_AFTER_COVERAGE_VALIDATED.with(|slot| {
        let previous = slot.borrow_mut().replace(Box::new(hook));
        assert!(
            previous.is_none(),
            "search coordination hook already installed"
        );
    });
}

#[cfg(test)]
fn run_search_after_coverage_validated_hook() {
    let hook = SEARCH_AFTER_COVERAGE_VALIDATED.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreOptions {
    pub fts_enabled: bool,
    pub current_schema: BTreeMap<PayloadType, p::SchemaVersion>,
    pub cursor_page_size: usize,
    pub sync_peer: Option<p::SyncPeer>,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            fts_enabled: false,
            current_schema: PayloadType::ALL
                .into_iter()
                .map(|payload_type| (payload_type, p::SchemaVersion(1)))
                .collect(),
            cursor_page_size: 128,
            sync_peer: None,
        }
    }
}

#[derive(Clone)]
pub struct SqliteEventStore {
    core: Arc<StoreCore>,
    cursor_page_size: usize,
}

impl SqliteEventStore {
    pub fn open(path: impl AsRef<Path>, options: StoreOptions) -> p::Result<Self> {
        validate_options(&options)?;
        let mut connection = Connection::open(path.as_ref())
            .map_err(|error| store_error("failed to open event database", error))?;
        initialize_connection(
            &mut connection,
            true,
            options.fts_enabled,
            options.sync_peer.as_ref(),
        )?;
        Ok(Self::from_connection(connection, options))
    }

    pub fn open_in_memory(options: StoreOptions) -> p::Result<Self> {
        validate_options(&options)?;
        let mut connection = Connection::open_in_memory()
            .map_err(|error| store_error("failed to open in-memory event database", error))?;
        initialize_connection(
            &mut connection,
            false,
            options.fts_enabled,
            options.sync_peer.as_ref(),
        )?;
        Ok(Self::from_connection(connection, options))
    }

    pub fn run_ids(&self) -> p::Result<Vec<p::RunId>> {
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let mut statement = connection
            .prepare("SELECT DISTINCT run_id FROM events ORDER BY run_id")
            .map_err(|error| store_error("failed to prepare run scan", error))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| store_error("failed to scan event runs", error))?;
        rows.map(|row| {
            row.map(p::RunId)
                .map_err(|error| store_error("failed to decode event run", error))
        })
        .collect()
    }

    pub fn load_session_state(&self, run_id: p::RunId) -> p::Result<Option<SessionState>> {
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_session_state(&connection, &run_id)
    }

    pub fn load_transcript(&self, run_id: p::RunId) -> p::Result<Option<Transcript>> {
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_transcript(&connection, &run_id)
    }

    pub fn sync_cursor(
        &self,
        peer: &p::SyncPeerRef,
        aggregate: &p::RunId,
    ) -> p::Result<Option<SyncCursorState>> {
        if peer.0.trim().is_empty() || aggregate.0.trim().is_empty() {
            return Err(p::Error("sync cursor identity is incomplete".into()));
        }
        self.core.require_sync_peer_ref(peer)?;
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        connection
            .query_row(
                "SELECT applied_version, exported_version
                 FROM sync_cursors WHERE peer_ref = ?1 AND aggregate = ?2",
                params![&peer.0, &aggregate.0],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(|error| store_error("failed to read sync cursor", error))?
            .map(|(applied_version, exported_version)| {
                Ok(SyncCursorState {
                    schema_version: p::SchemaVersion(1),
                    peer: peer.clone(),
                    aggregate: aggregate.clone(),
                    applied_version: u64::try_from(applied_version)
                        .map_err(|_| p::Error("stored applied sync cursor is invalid".into()))?,
                    exported_version: u64::try_from(exported_version)
                        .map_err(|_| p::Error("stored export sync cursor is invalid".into()))?,
                })
            })
            .transpose()
    }

    pub fn register_upcaster(
        &self,
        payload_type: PayloadType,
        from: p::SchemaVersion,
        to: p::SchemaVersion,
        upcaster: Upcaster,
    ) -> p::Result<()> {
        let _registration = self
            .core
            .upcaster_registration
            .lock()
            .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?;

        self.core
            .upcasters
            .read()
            .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
            .validate_registration(payload_type, from, to, &upcaster)?;

        {
            let mut connection = self
                .core
                .connection
                .lock()
                .map_err(|_| p::Error("event database lock is poisoned".into()))?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| {
                    store_error("failed to begin schema migration registration", error)
                })?;
            let existing = transaction
                .query_row(
                    "SELECT to_version, note, implementation_identity FROM schema_migrations
                     WHERE payload_type = ?1 AND from_version = ?2",
                    params![payload_type.as_str(), i64::from(from.0)],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| store_error("failed to inspect schema migration", error))?;
            if let Some((persisted_to, persisted_note, persisted_identity)) = existing {
                let persisted_to = u32::try_from(persisted_to)
                    .map_err(|_| p::Error("stored migration target is invalid".into()))?;
                if persisted_to != to.0
                    || persisted_note != upcaster.migration_note()
                    || persisted_identity != upcaster.implementation_identity()
                {
                    return Err(p::Error(format!(
                        "conflicting persisted upcaster implementation edge for {payload_type} schema {}",
                        from.0,
                    )));
                }
            } else {
                transaction
                    .execute(
                        "INSERT INTO schema_migrations(
                            payload_type, from_version, to_version, note,
                            implementation_identity
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            payload_type.as_str(),
                            i64::from(from.0),
                            i64::from(to.0),
                            upcaster.migration_note(),
                            upcaster.implementation_identity(),
                        ],
                    )
                    .map_err(|error| store_error("failed to persist schema migration", error))?;
            }
            transaction.commit().map_err(|error| {
                store_error("failed to commit schema migration registration", error)
            })?;
        }

        let mut graph = self
            .core
            .upcasters
            .write()
            .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?;
        graph.validate_registration(payload_type, from, to, &upcaster)?;
        graph.insert(payload_type, from, to, upcaster);
        Ok(())
    }

    pub fn rebuild_builtin_projections(&self) -> p::Result<()> {
        let upcasters = self.core.upcaster_snapshot()?;
        let captured_upcaster_token = upcasters.token();
        let captured_upcaster_identity = upcasters.identity_bytes();
        let schema_identity = schema_registry_identity(&self.core.current_schema);
        let (stored_events, captured_token) = {
            let mut connection = self
                .core
                .connection
                .lock()
                .map_err(|_| p::Error("event database lock is poisoned".into()))?;
            capture_rebuild_events(&mut connection)?
        };
        let rebuilt =
            fold_builtin_projections(stored_events, &self.core.current_schema, &upcasters)?;

        let _registration = self
            .core
            .upcaster_registration
            .lock()
            .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?;
        let current_upcaster_token = self
            .core
            .upcasters
            .read()
            .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
            .token();
        if current_upcaster_token != captured_upcaster_token {
            return Err(p::Error(
                "upcaster graph changed during projection rebuild; stale caches were not committed"
                    .into(),
            ));
        }

        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin projection rebuild", error))?;
        if read_event_set_token(&transaction)? != captured_token {
            return Err(p::Error(
                "authoritative event set changed during projection rebuild; stale caches were not committed"
                    .into(),
            ));
        }

        transaction
            .execute("DELETE FROM session_state", [])
            .map_err(|error| store_error("failed to clear session projection cache", error))?;
        transaction
            .execute("DELETE FROM transcript", [])
            .map_err(|error| store_error("failed to clear transcript projection cache", error))?;
        if self.core.fts_enabled {
            transaction
                .execute(
                    "INSERT INTO events_fts(events_fts) VALUES('delete-all')",
                    [],
                )
                .map_err(|error| store_error("failed to clear full-text search cache", error))?;
        }

        for state in rebuilt {
            write_session_state(&transaction, &state.session)?;
            write_transcript(&transaction, &state.transcript)?;
            if self.core.fts_enabled {
                for entry in state.fts_entries {
                    let inserted = transaction
                        .execute(
                            "INSERT INTO events_fts(rowid, text)\n                             SELECT rowid, ?2 FROM events WHERE event_id = ?1",
                            params![entry.event_id, entry.text],
                        )
                        .map_err(|error| {
                            store_error("failed to rebuild full-text search entry", error)
                        })?;
                    if inserted != 1 {
                        return Err(p::Error(
                            "authoritative event disappeared during full-text search rebuild"
                                .into(),
                        ));
                    }
                }
            }
        }

        if self.core.fts_enabled {
            write_fts_coverage(
                &transaction,
                &FtsCoverage::new(&captured_token, schema_identity, captured_upcaster_identity),
            )?;
        }

        transaction
            .commit()
            .map_err(|error| store_error("failed to commit projection rebuild", error))
    }

    pub fn search(&self, query: &str, limit: usize) -> p::Result<Vec<SearchHit>> {
        if !self.core.fts_enabled {
            return Err(p::Error("full-text search is disabled".into()));
        }
        let limit = i64::try_from(limit)
            .map_err(|_| p::Error("full-text search limit is invalid".into()))?;
        let _registration = self
            .core
            .upcaster_registration
            .lock()
            .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?;
        let upcaster_identity = self
            .core
            .upcasters
            .read()
            .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
            .identity_bytes();
        let schema_identity = schema_registry_identity(&self.core.current_schema);
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| store_error("failed to begin full-text search snapshot", error))?;
        let event_set = read_event_set_token(&transaction)?;
        let expected = FtsCoverage::new(&event_set, schema_identity, upcaster_identity);
        let coverage = read_fts_coverage(&transaction)?;
        if coverage.as_ref() != Some(&expected) && !(coverage.is_none() && event_set.is_empty()) {
            return Err(p::Error(
                "full-text search coverage is stale; rebuild derived caches before searching"
                    .into(),
            ));
        }
        #[cfg(test)]
        run_search_after_coverage_validated_hook();
        let hits = {
            let mut statement = transaction
                .prepare(
                    "SELECT events.event_id, events.run_id, events.stream_seq, events.kind
                     FROM events_fts
                     JOIN events ON events.rowid = events_fts.rowid
                     WHERE events_fts MATCH ?1
                     ORDER BY bm25(events_fts)
                     LIMIT ?2",
                )
                .map_err(|error| store_error("failed to prepare full-text search", error))?;
            let rows = statement
                .query_map(params![query, limit], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(|error| store_error("failed to query full-text search", error))?;

            rows.map(|row| {
                let (event_id, run_id, stream_seq, kind) =
                    row.map_err(|error| store_error("failed to read full-text search hit", error))?;
                Ok(SearchHit {
                    event_id: p::EventId(event_id),
                    run_id: p::RunId(run_id),
                    stream_seq: u64::try_from(stream_seq)
                        .map_err(|_| p::Error("stored event sequence is invalid".into()))?,
                    kind: kind.parse()?,
                })
            })
            .collect::<p::Result<Vec<_>>>()?
        };
        transaction
            .commit()
            .map_err(|error| store_error("failed to finish full-text search snapshot", error))?;
        Ok(hits)
    }

    pub fn stable_strategy(
        &self,
        domain: p::StrategyDomain,
        scope: &p::Scope,
        version: &p::StrategyVersionRef,
    ) -> p::Result<Option<StableStrategyRecord>> {
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_stable_strategy(&connection, domain, scope, version)
    }

    pub fn strategy_candidate(
        &self,
        candidate: &p::CandidateId,
    ) -> p::Result<Option<p::StrategyCandidate>> {
        if candidate.0.trim().is_empty() {
            return Err(p::Error("strategy candidate identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        connection
            .query_row(
                "SELECT candidate FROM strategy_candidates WHERE candidate_id = ?1",
                params![&candidate.0],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| store_error("failed to read strategy candidate", error))?
            .map(|encoded| {
                let candidate = serde_json::from_slice::<p::StrategyCandidate>(&encoded)
                    .map_err(|error| store_error("failed to decode strategy candidate", error))?;
                candidate.validate()?;
                Ok(candidate)
            })
            .transpose()
    }

    pub fn active_for(
        &self,
        aggregate: &p::EvolutionAggregateRef,
        domain: p::StrategyDomain,
        scope: &p::Scope,
    ) -> p::Result<Option<p::ActiveStrategyRef>> {
        if aggregate.0.trim().is_empty() || scope.0.trim().is_empty() {
            return Err(p::Error("active strategy lookup is incomplete".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_active_strategy(&connection, aggregate, domain, scope)
    }

    pub fn has_active_strategies(&self, scope: &p::Scope) -> p::Result<bool> {
        if scope.0.trim().is_empty() {
            return Err(p::Error("active strategy scope is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        Ok(!read_active_strategies(&connection, None, scope, None)?.is_empty())
    }

    pub fn evolution_history(
        &self,
        aggregate: &p::EvolutionAggregateRef,
    ) -> p::Result<Vec<EvolutionHistoryEntry>> {
        if aggregate.0.trim().is_empty() {
            return Err(p::Error("evolution aggregate identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_evolution_history(&connection, aggregate)
    }

    pub fn preview_evolution_snapshot(&self, event: &p::Event) -> p::Result<p::EvolutionSnapshot> {
        evolution_control_versions(event)?;
        validate_evolution_control_provenance(event)?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| store_error("failed to begin evolution preview", error))?;
        validate_evolution_transition(&transaction, event)?;
        let snapshot = preview_evolution_snapshot_in_transaction(&transaction, event)?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to finish evolution preview", error))?;
        Ok(snapshot)
    }

    pub fn rebuild_evolution_projection(&self) -> p::Result<()> {
        let upcasters = self.core.upcaster_snapshot()?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin evolution projection rebuild", error))?;
        rebuild_evolution_projection_in_transaction(
            &transaction,
            &self.core.current_schema,
            &upcasters,
        )?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit evolution projection rebuild", error))
    }

    fn prepare_event(&self, event: p::Event) -> p::Result<PreparedEvent> {
        event.validate_payload_kind()?;
        let payload_type = PayloadType::from_event_kind(event.kind);
        let current_schema = self.core.current_schema_for(payload_type)?;
        if event.schema_version != current_schema {
            return Err(p::Error(format!(
                "event schema version {} is not current schema version {} for {payload_type}",
                event.schema_version.0, current_schema.0
            )));
        }
        let payload = serde_json::to_vec(&event.payload)
            .map_err(|error| store_error("failed to serialize event payload", error))?;
        let provenance = serde_json::to_vec(&event.provenance)
            .map_err(|error| store_error("failed to serialize event provenance", error))?;
        let event_semantics = event_semantic_bytes(&event, &payload, &provenance);
        let domain_key = domain_idempotency_key(&event);
        let domain_semantics = domain_key
            .as_ref()
            .map(|_| domain_semantic_bytes(&event, &payload, &provenance));
        Ok(PreparedEvent {
            event,
            payload,
            provenance,
            event_semantics,
            domain_key,
            domain_semantics,
        })
    }

    fn from_connection(connection: Connection, options: StoreOptions) -> Self {
        Self {
            core: Arc::new(StoreCore {
                connection: Mutex::new(connection),
                current_schema: options.current_schema,
                fts_enabled: options.fts_enabled,
                sync_peer: options.sync_peer,
                upcasters: RwLock::new(UpcasterGraph::default()),
                upcaster_registration: Mutex::new(()),
            }),
            cursor_page_size: options.cursor_page_size,
        }
    }
}

impl EventStore for SqliteEventStore {
    fn append(&self, event: p::Event) -> p::Result<p::EventId> {
        reject_non_evolution_write_path(&event)?;
        let prepared = self.prepare_event(event)?;
        let _registration = if self.core.fts_enabled {
            Some(
                self.core
                    .upcaster_registration
                    .lock()
                    .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?,
            )
        } else {
            None
        };
        let fts_upcaster_identity = if self.core.fts_enabled {
            Some(
                self.core
                    .upcasters
                    .read()
                    .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
                    .identity_bytes(),
            )
        } else {
            None
        };
        let fts_schema_identity = self
            .core
            .fts_enabled
            .then(|| schema_registry_identity(&self.core.current_schema));
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin append transaction", error))?;
        let outcome = append_prepared(
            &transaction,
            prepared,
            self.core.fts_enabled,
            fts_schema_identity.as_deref(),
            fts_upcaster_identity.as_deref(),
        )?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit appended event", error))?;
        Ok(outcome.event_id)
    }

    fn read_run(&self, run: p::RunId) -> EventCursor {
        EventCursor::new(Arc::clone(&self.core), run, self.cursor_page_size)
    }

    fn project<P: Projection>(&self, scope: ProjectionScope) -> p::Result<P::State> {
        let ProjectionScope::Run(run_id) = scope else {
            return Err(p::Error(
                "generic projection All is unsupported because cross-run ordering is undefined"
                    .into(),
            ));
        };
        let mut state = P::empty();
        for event in self.read_run(run_id) {
            P::apply(&mut state, &event?);
        }
        Ok(state)
    }

    fn replay(&self, run: p::RunId, at: SchemaSnapshot) -> p::Result<ReplayReport> {
        let (stored_events, current_session, current_transcript) = {
            let mut connection = self
                .core
                .connection
                .lock()
                .map_err(|_| p::Error("event database lock is poisoned".into()))?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Deferred)
                .map_err(|error| store_error("failed to begin replay snapshot", error))?;
            let stored_events = {
                let mut statement = transaction
                    .prepare(
                        "SELECT event_id, run_id, stream_seq, turn_id, kind, payload,
                                schema_version, ts, provenance, checksum
                         FROM events
                         WHERE run_id = ?1
                         ORDER BY stream_seq",
                    )
                    .map_err(|error| store_error("failed to prepare replay stream", error))?;
                let rows = statement
                    .query_map(params![&run.0], StoredEvent::from_row)
                    .map_err(|error| store_error("failed to query replay stream", error))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(|error| store_error("failed to read replay event", error))?
            };
            #[cfg(test)]
            run_replay_after_events_materialized_hook();
            let current_session = read_session_state(&transaction, &run)?;
            let current_transcript = read_transcript(&transaction, &run)?;
            transaction
                .commit()
                .map_err(|error| store_error("failed to finish replay snapshot", error))?;
            (stored_events, current_session, current_transcript)
        };

        let upcasters = self.core.upcaster_snapshot()?;
        let mut replayed_session_state = SessionStateProjection::empty();
        let mut replayed_transcript = TranscriptProjection::empty();
        for stored_event in stored_events {
            let event = stored_event.decode_at(&at, &upcasters)?;
            SessionStateProjection::apply(&mut replayed_session_state, &event);
            TranscriptProjection::apply(&mut replayed_transcript, &event);
        }
        let diff_vs_current = projection_diff(
            current_session.as_ref(),
            &replayed_session_state,
            current_transcript.as_ref(),
            &replayed_transcript,
        );

        Ok(ReplayReport {
            run,
            schema_snapshot: at,
            diff_vs_current,
            replayed_session_state,
            replayed_transcript,
        })
    }
}

impl VersionedEventStore for SqliteEventStore {
    fn aggregate_version(&self, aggregate: p::RunId) -> p::Result<p::AggregateVersion> {
        if aggregate.0.trim().is_empty() {
            return Err(p::Error("aggregate identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let value = connection
            .query_row(
                "SELECT COALESCE(MAX(stream_seq), 0) FROM events WHERE run_id = ?1",
                params![&aggregate.0],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| store_error("failed to read aggregate version", error))?;
        Ok(p::AggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate,
            value: u64::try_from(value)
                .map_err(|_| p::Error("stored aggregate version is invalid".into()))?,
        })
    }

    fn append_expected(
        &self,
        event: p::Event,
        expected: p::AggregateVersion,
    ) -> p::Result<p::ExpectedAppend> {
        reject_non_evolution_write_path(&event)?;
        expected.validate()?;
        if event.run_id != expected.aggregate {
            return Err(p::Error(
                "expected append aggregate does not match the event".into(),
            ));
        }
        let prepared = self.prepare_event(event)?;
        let _registration = if self.core.fts_enabled {
            Some(
                self.core
                    .upcaster_registration
                    .lock()
                    .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?,
            )
        } else {
            None
        };
        let fts_upcaster_identity = if self.core.fts_enabled {
            Some(
                self.core
                    .upcasters
                    .read()
                    .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
                    .identity_bytes(),
            )
        } else {
            None
        };
        let fts_schema_identity = self
            .core
            .fts_enabled
            .then(|| schema_registry_identity(&self.core.current_schema));
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin expected append transaction", error))?;
        let actual = aggregate_version_in_transaction(&transaction, &expected.aggregate)?;
        if actual != expected.value {
            transaction
                .rollback()
                .map_err(|error| store_error("failed to finish expected append conflict", error))?;
            return Ok(p::ExpectedAppend {
                schema_version: p::SchemaVersion(1),
                status: p::ExpectedAppendStatus::Conflict,
                expected_version: expected.value,
                actual_version: actual,
                resulting_version: actual,
                event_id: None,
            });
        }
        let outcome = append_prepared(
            &transaction,
            prepared,
            self.core.fts_enabled,
            fts_schema_identity.as_deref(),
            fts_upcaster_identity.as_deref(),
        )?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit expected append", error))?;
        Ok(p::ExpectedAppend {
            schema_version: p::SchemaVersion(1),
            status: match outcome.disposition {
                AppendDisposition::Applied => p::ExpectedAppendStatus::Applied,
                AppendDisposition::Duplicate => p::ExpectedAppendStatus::Duplicate,
            },
            expected_version: expected.value,
            actual_version: actual,
            resulting_version: outcome.resulting_version,
            event_id: Some(outcome.event_id),
        })
    }

    fn apply_sync_batch(&self, batch: p::SyncWriteBatch) -> p::Result<p::SyncApplyReport> {
        batch.validate()?;
        self.core.require_sync_peer(&batch.peer)?;
        let semantic_bytes = sync_batch_semantic_bytes(&batch)?;
        let expected_version = batch.expected_version.value;
        let batch_id = batch.batch_id.clone();
        let peer = batch.peer.clone();
        let aggregate = batch.aggregate.clone();
        let prepared = batch
            .events
            .into_iter()
            .map(|event| {
                reject_non_evolution_write_path(&event)?;
                if !sync_event_allowed(&peer, &event) {
                    return Err(p::Error(
                        "sync event scope is outside the configured peer grant".into(),
                    ));
                }
                self.prepare_event(event)
            })
            .collect::<p::Result<Vec<_>>>()?;
        let _registration = if self.core.fts_enabled {
            Some(
                self.core
                    .upcaster_registration
                    .lock()
                    .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?,
            )
        } else {
            None
        };
        let fts_upcaster_identity = if self.core.fts_enabled {
            Some(
                self.core
                    .upcasters
                    .read()
                    .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
                    .identity_bytes(),
            )
        } else {
            None
        };
        let fts_schema_identity = self
            .core
            .fts_enabled
            .then(|| schema_registry_identity(&self.core.current_schema));
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin sync batch transaction", error))?;
        verify_sync_peer(&transaction, &peer)?;

        if let Some((stored_semantics, stored_expected, stored_result, stored_events)) = transaction
            .query_row(
                "SELECT semantic_bytes, expected_version, resulting_version, applied_event_ids
                 FROM sync_batches WHERE batch_id = ?1",
                params![&batch_id.0],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| store_error("failed to inspect sync batch ledger", error))?
        {
            if stored_semantics != semantic_bytes {
                return Err(p::Error(format!(
                    "sync batch idempotency collision for {}: existing semantics differ",
                    batch_id.0
                )));
            }
            let actual = aggregate_version_in_transaction(&transaction, &aggregate)?;
            let applied_events: Vec<p::EventId> = serde_json::from_slice(&stored_events)
                .map_err(|error| store_error("failed to decode sync batch ledger", error))?;
            transaction
                .commit()
                .map_err(|error| store_error("failed to finish duplicate sync batch", error))?;
            return Ok(p::SyncApplyReport {
                schema_version: p::SchemaVersion(1),
                batch_id,
                status: p::SyncApplyStatus::Duplicate,
                expected_version: u64::try_from(stored_expected)
                    .map_err(|_| p::Error("stored sync expected version is invalid".into()))?,
                actual_version: actual,
                resulting_version: u64::try_from(stored_result)
                    .map_err(|_| p::Error("stored sync result version is invalid".into()))?,
                applied_events,
            });
        }

        let actual = aggregate_version_in_transaction(&transaction, &aggregate)?;
        if actual != expected_version {
            transaction
                .rollback()
                .map_err(|error| store_error("failed to finish sync CAS conflict", error))?;
            return Ok(p::SyncApplyReport {
                schema_version: p::SchemaVersion(1),
                batch_id,
                status: p::SyncApplyStatus::Conflict,
                expected_version,
                actual_version: actual,
                resulting_version: actual,
                applied_events: Vec::new(),
            });
        }

        let mut applied_events = Vec::with_capacity(prepared.len());
        let mut resulting_version = actual;
        for prepared_event in prepared {
            let outcome = append_prepared(
                &transaction,
                prepared_event,
                self.core.fts_enabled,
                fts_schema_identity.as_deref(),
                fts_upcaster_identity.as_deref(),
            )?;
            if outcome.disposition != AppendDisposition::Applied {
                return Err(p::Error(
                    "sync batch event was already present outside its batch ledger".into(),
                ));
            }
            resulting_version = outcome.resulting_version;
            applied_events.push(outcome.event_id);
        }
        let applied_event_ids = serde_json::to_vec(&applied_events)
            .map_err(|error| store_error("failed to encode sync batch ledger", error))?;
        transaction
            .execute(
                "INSERT INTO sync_batches(
                    batch_id, peer_ref, aggregate, expected_version, semantic_bytes,
                    resulting_version, applied_event_ids
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    &batch_id.0,
                    &peer.reference.0,
                    &aggregate.0,
                    i64::try_from(expected_version)
                        .map_err(|_| p::Error("sync expected version is too large".into()))?,
                    semantic_bytes,
                    i64::try_from(resulting_version)
                        .map_err(|_| p::Error("sync result version is too large".into()))?,
                    applied_event_ids,
                ],
            )
            .map_err(|error| store_error("failed to record sync batch", error))?;
        update_sync_cursor(
            &transaction,
            &peer.reference,
            &aggregate,
            Some(resulting_version),
            None,
        )?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit sync batch", error))?;
        Ok(p::SyncApplyReport {
            schema_version: p::SchemaVersion(1),
            batch_id,
            status: p::SyncApplyStatus::Applied,
            expected_version,
            actual_version: actual,
            resulting_version,
            applied_events,
        })
    }

    fn export_sync_batch(&self, request: p::SyncExportRequest) -> p::Result<p::SyncTransferBatch> {
        request.validate()?;
        self.core.require_sync_peer(&request.peer)?;
        if sync_peer_contains_sensitive_identity(&request.peer, &request.redaction) {
            return Err(p::Error(
                "sync peer identity or scope contains a forbidden sensitive marker".into(),
            ));
        }
        let upcasters = self.core.upcaster_snapshot()?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin sync export transaction", error))?;
        verify_sync_peer(&transaction, &request.peer)?;
        let actual = aggregate_version_in_transaction(&transaction, &request.aggregate)?;
        if request.after_version.value > actual {
            return Err(p::Error(format!(
                "sync export cursor {} is ahead of aggregate version {actual}",
                request.after_version.value
            )));
        }
        let stored_events = {
            let limit = i64::from(request.limit);
            let after = i64::try_from(request.after_version.value)
                .map_err(|_| p::Error("sync export cursor is too large".into()))?;
            let mut statement = transaction
                .prepare(
                    "SELECT event_id, run_id, stream_seq, turn_id, kind, payload,
                            schema_version, ts, provenance, checksum
                     FROM events
                     WHERE run_id = ?1 AND stream_seq > ?2
                     ORDER BY stream_seq
                     LIMIT ?3",
                )
                .map_err(|error| store_error("failed to prepare sync export", error))?;
            let rows = statement
                .query_map(
                    params![&request.aggregate.0, after, limit],
                    StoredEvent::from_row,
                )
                .map_err(|error| store_error("failed to query sync export", error))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| store_error("failed to read sync export event", error))?
        };
        let mut events = Vec::with_capacity(stored_events.len());
        for stored in stored_events {
            let event = stored.decode(&self.core.current_schema, &upcasters)?;
            if sync_event_contains_sensitive_identity(&event, &request.redaction) {
                return Err(p::Error(
                    "sync event identity contains a forbidden sensitive marker".into(),
                ));
            }
            let payload_bytes = serde_json::to_vec(&event.payload)
                .map_err(|error| store_error("failed to inspect sync payload", error))?;
            let redaction_reason = sync_redaction_reason(&event, &request);
            let payload = if let Some(reason) = redaction_reason {
                p::SyncTransferPayload::Redacted {
                    kind: event.kind,
                    reason: p::ReasonRef(reason.into()),
                    digest: p::SchemaDigest(format!(
                        "fnv64:{}",
                        checksum::calculate_bytes(&payload_bytes)
                    )),
                }
            } else {
                p::SyncTransferPayload::Full(Box::new(event.payload))
            };
            events.push(p::SyncTransferEvent {
                schema_version: p::SchemaVersion(1),
                event_id: event.event_id,
                aggregate: event.run_id,
                source_stream_seq: event.stream_seq,
                turn_id: event.turn_id,
                kind: event.kind,
                payload,
                event_schema_version: event.schema_version,
                ts_unix_ms: event.ts_unix_ms,
                provenance: event.provenance,
            });
        }
        let to_version = events
            .last()
            .map(|event| event.source_stream_seq)
            .unwrap_or(request.after_version.value);
        update_sync_cursor(
            &transaction,
            &request.peer.reference,
            &request.aggregate,
            None,
            Some(to_version),
        )?;
        let batch_id = sync_export_batch_id(
            &request.peer.reference,
            &request.aggregate,
            request.after_version.value,
            to_version,
        );
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit sync export cursor", error))?;
        Ok(p::SyncTransferBatch {
            schema_version: p::SchemaVersion(1),
            batch_id,
            peer: request.peer.reference,
            aggregate: request.aggregate,
            from_version: request.after_version.value,
            to_version,
            events,
        })
    }
}

impl EvolutionProjection for SqliteEventStore {
    fn active(
        &self,
        domain: p::StrategyDomain,
        scope: p::Scope,
    ) -> p::Result<Option<p::ActiveStrategyRef>> {
        if scope.0.trim().is_empty() {
            return Err(p::Error("active strategy scope is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let matches = read_active_strategies(&connection, Some(domain), &scope, None)?;
        match matches.as_slice() {
            [] => Ok(None),
            [active] => Ok(Some(active.clone())),
            _ => Err(p::Error(
                "active strategy lookup is ambiguous across evolution aggregates".into(),
            )),
        }
    }

    fn snapshot(&self, scope: p::Scope) -> p::Result<p::EvolutionSnapshot> {
        if scope.0.trim().is_empty() {
            return Err(p::Error("evolution snapshot scope is empty".into()));
        }
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| store_error("failed to begin evolution snapshot", error))?;
        let snapshot = evolution_snapshot_in_transaction(&transaction, &scope, None)?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to finish evolution snapshot", error))?;
        Ok(snapshot)
    }
}

impl EvolutionEventStore for SqliteEventStore {
    fn evolution_version(
        &self,
        aggregate: &p::EvolutionAggregateRef,
    ) -> p::Result<p::EvolutionAggregateVersion> {
        if aggregate.0.trim().is_empty() {
            return Err(p::Error("evolution aggregate identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        Ok(p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: aggregate.clone(),
            value: evolution_version_in_transaction(&connection, aggregate)?,
        })
    }

    fn append_evolution_expected(
        &self,
        event: p::Event,
        aggregate: &p::EvolutionAggregateRef,
        expected: p::EvolutionAggregateVersion,
    ) -> p::Result<p::ExpectedAppend> {
        expected.validate()?;
        if aggregate.0.trim().is_empty() || expected.aggregate != *aggregate {
            return Err(p::Error(
                "expected evolution version does not match the requested aggregate".into(),
            ));
        }
        let (_, payload_expected, payload_committed) = evolution_control_versions(&event)?;
        if payload_expected != &expected
            || payload_committed.aggregate != *aggregate
            || payload_committed.value != expected.value.checked_add(1).unwrap_or(0)
        {
            return Err(p::Error(
                "evolution event version fields do not match the expected append".into(),
            ));
        }
        validate_evolution_control_provenance(&event)?;
        let committed_version = payload_committed.value;
        let prepared = self.prepare_event(event)?;
        let control_event = prepared.event.clone();

        let _registration = if self.core.fts_enabled {
            Some(
                self.core
                    .upcaster_registration
                    .lock()
                    .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?,
            )
        } else {
            None
        };
        let fts_upcaster_identity = if self.core.fts_enabled {
            Some(
                self.core
                    .upcasters
                    .read()
                    .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
                    .identity_bytes(),
            )
        } else {
            None
        };
        let fts_schema_identity = self
            .core
            .fts_enabled
            .then(|| schema_registry_identity(&self.core.current_schema));
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin evolution append transaction", error))?;
        let actual = evolution_version_in_transaction(&transaction, aggregate)?;
        if actual != expected.value {
            if actual == committed_version
                && evolution_history_contains(&transaction, aggregate, &control_event.event_id)?
            {
                let outcome = append_prepared(
                    &transaction,
                    prepared,
                    self.core.fts_enabled,
                    fts_schema_identity.as_deref(),
                    fts_upcaster_identity.as_deref(),
                )?;
                if outcome.disposition != AppendDisposition::Duplicate {
                    return Err(p::Error(
                        "evolution history references a non-duplicate event".into(),
                    ));
                }
                transaction.commit().map_err(|error| {
                    store_error("failed to finish duplicate evolution append", error)
                })?;
                return Ok(p::ExpectedAppend {
                    schema_version: p::SchemaVersion(1),
                    status: p::ExpectedAppendStatus::Duplicate,
                    expected_version: expected.value,
                    actual_version: actual,
                    resulting_version: actual,
                    event_id: Some(outcome.event_id),
                });
            }
            transaction.rollback().map_err(|error| {
                store_error("failed to finish evolution append conflict", error)
            })?;
            return Ok(p::ExpectedAppend {
                schema_version: p::SchemaVersion(1),
                status: p::ExpectedAppendStatus::Conflict,
                expected_version: expected.value,
                actual_version: actual,
                resulting_version: actual,
                event_id: None,
            });
        }

        validate_evolution_transition(&transaction, &control_event)?;
        let outcome = append_prepared(
            &transaction,
            prepared,
            self.core.fts_enabled,
            fts_schema_identity.as_deref(),
            fts_upcaster_identity.as_deref(),
        )?;
        if outcome.disposition != AppendDisposition::Applied {
            return Err(p::Error(
                "evolution event exists without its committed aggregate version".into(),
            ));
        }
        apply_evolution_control_event(&transaction, &control_event, true)?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit evolution append", error))?;
        Ok(p::ExpectedAppend {
            schema_version: p::SchemaVersion(1),
            status: p::ExpectedAppendStatus::Applied,
            expected_version: expected.value,
            actual_version: actual,
            resulting_version: committed_version,
            event_id: Some(outcome.event_id),
        })
    }
}

impl FederationProjection for SqliteEventStore {
    fn snapshot(&self, scope: p::Scope) -> p::Result<p::FederationSnapshot> {
        if scope.0.trim().is_empty() {
            return Err(p::Error("federation snapshot scope is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let aggregate = p::FederationAggregateRef("federation".into());
        let version = federation_version_in_transaction(&connection, &aggregate)?;
        let epoch = authority_epoch_in_transaction(&connection)?;
        let now = current_time_ms()?;
        let mut statement = connection
            .prepare(
                "SELECT grant FROM federated_peers
                 WHERE revoked = 0 ORDER BY peer_ref, grant_version",
            )
            .map_err(|error| store_error("failed to prepare federation snapshot", error))?;
        let rows = statement
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(|error| store_error("failed to query federation snapshot", error))?;
        let mut grants = Vec::new();
        for row in rows {
            let bytes = row.map_err(|error| store_error("failed to read peer grant", error))?;
            let grant: p::FederatedPeerGrant = serde_json::from_slice(&bytes)
                .map_err(|error| store_error("failed to decode peer grant", error))?;
            grant.validate()?;
            if grant.expires_at > now && grant.scopes.binary_search(&scope).is_ok() {
                grants.push(grant.reference()?);
            }
        }
        grants.sort();
        grants.dedup();
        let registry_version = p::FederationAggregateVersion {
            schema_version: p::M4_SCHEMA_VERSION,
            aggregate,
            version,
        };
        let authority = p::AuthorityRef("authority:local".into());
        let digest = p::canonical_digest(&(&authority, epoch, &registry_version, &grants))?;
        let snapshot = p::FederationSnapshot {
            schema_version: p::M4_SCHEMA_VERSION,
            authority,
            authority_epoch: epoch,
            registry_version,
            grants,
            digest,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn peer(&self, peer: &p::FederatedPeerRef) -> p::Result<Option<p::FederatedPeerState>> {
        if peer.0.trim().is_empty() {
            return Err(p::Error("federated peer identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        connection
            .query_row(
                "SELECT grant, revoked FROM federated_peers WHERE peer_ref = ?1",
                params![&peer.0],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(|error| store_error("failed to read federated peer", error))?
            .map(|(bytes, revoked)| {
                let grant = serde_json::from_slice::<p::FederatedPeerGrant>(&bytes)
                    .map_err(|error| store_error("failed to decode federated peer", error))?;
                grant.validate()?;
                Ok(p::FederatedPeerState {
                    schema_version: p::M4_SCHEMA_VERSION,
                    grant,
                    revoked: revoked != 0,
                    last_seen_at: None,
                })
            })
            .transpose()
    }

    fn lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteExecutionLease>> {
        if lease.0.trim().is_empty() {
            return Err(p::Error("remote lease identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        connection
            .query_row(
                "SELECT lease FROM remote_leases WHERE lease_ref = ?1",
                params![&lease.0],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| store_error("failed to read remote lease", error))?
            .map(|bytes| {
                serde_json::from_slice::<p::RemoteExecutionLease>(&bytes)
                    .map_err(|error| store_error("failed to decode remote lease", error))
            })
            .transpose()
    }

    fn checkpoint(
        &self,
        peer: &p::FederatedPeerRef,
        aggregate: &p::RunId,
    ) -> p::Result<p::ReplicationCursor> {
        if peer.0.trim().is_empty() || aggregate.0.trim().is_empty() {
            return Err(p::Error(
                "replication checkpoint identity is incomplete".into(),
            ));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let stored = connection
            .query_row(
                "SELECT stream_seq, authority_epoch FROM replication_checkpoints
                 WHERE peer_ref = ?1 AND aggregate = ?2",
                params![&peer.0, &aggregate.0],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(|error| store_error("failed to read replication checkpoint", error))?;
        let stream_seq = stored.map(|(stream_seq, _)| stream_seq).unwrap_or(0);
        let epoch = authority_epoch_in_transaction(&connection)?;
        Ok(p::ReplicationCursor {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: peer.clone(),
            aggregate: aggregate.clone(),
            stream_seq: u64::try_from(stream_seq)
                .map_err(|_| p::Error("stored replication cursor is invalid".into()))?,
            authority_epoch: epoch,
        })
    }
}

impl SqliteEventStore {
    fn append_federation_internal(
        &self,
        event: p::Event,
        aggregate: &p::FederationAggregateRef,
        expected: p::FederationAggregateVersion,
        ack: Option<&p::ReplicationAck>,
    ) -> p::Result<p::ExpectedAppend> {
        expected.validate()?;
        if aggregate.0 != "federation" || expected.aggregate != *aggregate {
            return Err(p::Error(
                "federation expected version does not match the authority aggregate".into(),
            ));
        }
        validate_federation_control_event(&event, aggregate, &expected, ack.is_some())?;
        let committed = federation_control_version(&event)?.version;
        let prepared = self.prepare_event(event)?;
        let control_event = prepared.event.clone();

        let _registration = if self.core.fts_enabled {
            Some(
                self.core
                    .upcaster_registration
                    .lock()
                    .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?,
            )
        } else {
            None
        };
        let fts_upcaster_identity = if self.core.fts_enabled {
            Some(
                self.core
                    .upcasters
                    .read()
                    .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
                    .identity_bytes(),
            )
        } else {
            None
        };
        let fts_schema_identity = self
            .core
            .fts_enabled
            .then(|| schema_registry_identity(&self.core.current_schema));
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin federation append", error))?;
        let actual = federation_version_in_transaction(&transaction, aggregate)?;
        if actual != expected.version {
            if actual == committed
                && federation_history_contains(&transaction, aggregate, &control_event.event_id)?
            {
                let outcome = append_prepared(
                    &transaction,
                    prepared,
                    self.core.fts_enabled,
                    fts_schema_identity.as_deref(),
                    fts_upcaster_identity.as_deref(),
                )?;
                if outcome.disposition != AppendDisposition::Duplicate {
                    return Err(p::Error(
                        "federation history references a non-duplicate event".into(),
                    ));
                }
                transaction.commit().map_err(|error| {
                    store_error("failed to finish duplicate federation append", error)
                })?;
                return Ok(p::ExpectedAppend {
                    schema_version: p::M4_SCHEMA_VERSION,
                    status: p::ExpectedAppendStatus::Duplicate,
                    expected_version: expected.version,
                    actual_version: actual,
                    resulting_version: actual,
                    event_id: Some(outcome.event_id),
                });
            }
            transaction
                .rollback()
                .map_err(|error| store_error("failed to finish federation conflict", error))?;
            return Ok(p::ExpectedAppend {
                schema_version: p::M4_SCHEMA_VERSION,
                status: p::ExpectedAppendStatus::Conflict,
                expected_version: expected.version,
                actual_version: actual,
                resulting_version: actual,
                event_id: None,
            });
        }
        validate_federation_transition(&transaction, &control_event, ack)?;
        let outcome = append_prepared(
            &transaction,
            prepared,
            self.core.fts_enabled,
            fts_schema_identity.as_deref(),
            fts_upcaster_identity.as_deref(),
        )?;
        if outcome.disposition != AppendDisposition::Applied {
            return Err(p::Error(
                "federation event exists without its committed version".into(),
            ));
        }
        update_federation_projection(&transaction, &control_event, ack)?;
        transaction
            .execute(
                "INSERT INTO federation_versions(aggregate, version) VALUES (?1, ?2)
                 ON CONFLICT(aggregate) DO UPDATE SET version = excluded.version",
                params![
                    &aggregate.0,
                    i64::try_from(committed)
                        .map_err(|_| p::Error("federation version is too large".into()))?,
                ],
            )
            .map_err(|error| store_error("failed to advance federation version", error))?;
        transaction
            .execute(
                "INSERT INTO federation_history(aggregate, committed_version, event_id, kind)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    &aggregate.0,
                    i64::try_from(committed)
                        .map_err(|_| p::Error("federation version is too large".into()))?,
                    &control_event.event_id.0,
                    control_event.kind.as_str(),
                ],
            )
            .map_err(|error| store_error("failed to append federation history", error))?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit federation append", error))?;
        Ok(p::ExpectedAppend {
            schema_version: p::M4_SCHEMA_VERSION,
            status: p::ExpectedAppendStatus::Applied,
            expected_version: expected.version,
            actual_version: actual,
            resulting_version: committed,
            event_id: Some(outcome.event_id),
        })
    }
}

impl FederationEventStore for SqliteEventStore {
    fn federation_version(
        &self,
        aggregate: &p::FederationAggregateRef,
    ) -> p::Result<p::FederationAggregateVersion> {
        if aggregate.0 != "federation" {
            return Err(p::Error("unknown federation aggregate".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        Ok(p::FederationAggregateVersion {
            schema_version: p::M4_SCHEMA_VERSION,
            aggregate: aggregate.clone(),
            version: federation_version_in_transaction(&connection, aggregate)?,
        })
    }

    fn append_federation_expected(
        &self,
        event: p::Event,
        aggregate: &p::FederationAggregateRef,
        expected: p::FederationAggregateVersion,
    ) -> p::Result<p::ExpectedAppend> {
        if event.kind == p::EventKind::ReplicationCheckpointAdvanced {
            return Err(p::Error(
                "replication checkpoint requires an authenticated acknowledgement".into(),
            ));
        }
        self.append_federation_internal(event, aggregate, expected, None)
    }

    fn export_replication(
        &self,
        request: p::ReplicationExportRequest,
    ) -> p::Result<p::ReplicationBatch> {
        request.validate()?;
        let peer = FederationProjection::peer(self, &request.peer)?
            .ok_or_else(|| p::Error("replication peer is not registered".into()))?;
        let current_epoch = FederationProjection::snapshot(
            self,
            p::Scope(
                peer.grant
                    .scopes
                    .first()
                    .ok_or_else(|| p::Error("replication peer has no active scope".into()))?
                    .0
                    .clone(),
            ),
        )?
        .authority_epoch;
        if peer.revoked
            || !peer.grant.roles.contains(&p::FederatedPeerRole::Replica)
            || peer.grant.reference()? != request.grant
            || request.after.authority_epoch != current_epoch
            || peer.grant.expires_at <= current_time_ms()?
            || !peer
                .grant
                .scopes
                .iter()
                .any(|scope| !scope.0.trim().is_empty())
        {
            return Err(p::Error("replication peer grant is not active".into()));
        }
        let current = FederationProjection::checkpoint(self, &request.peer, &request.aggregate)?;
        if current != request.after {
            return Err(p::Error("replication export cursor is stale".into()));
        }
        let mut source = self
            .read_run(request.aggregate.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let mut workspaces = source
            .iter()
            .filter_map(|event| match &event.payload {
                p::EventPayload::SessionBound(payload) => Some(payload.workspace.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        workspaces.sort();
        workspaces.dedup();
        if workspaces.len() != 1
            || !peer
                .grant
                .scopes
                .contains(&p::Scope(workspaces[0].0.clone()))
        {
            return Err(p::Error(
                "replication aggregate is outside the peer workspace scope".into(),
            ));
        }
        source.retain(|event| event.stream_seq > request.after.stream_seq);
        source.truncate(request.limit as usize);
        if source.is_empty() {
            return Err(p::Error("replication export has no new events".into()));
        }
        let transfers = source
            .into_iter()
            .map(replication_transfer_event)
            .collect::<p::Result<Vec<_>>>()?;
        let to_seq = transfers
            .last()
            .map(|event| event.source_stream_seq)
            .ok_or_else(|| p::Error("replication export is empty".into()))?;
        let mut batch = p::ReplicationBatch {
            schema_version: p::M4_SCHEMA_VERSION,
            batch: p::ReplicationBatchRef(String::new()),
            peer: request.peer.clone(),
            peer_grant: request.grant,
            aggregate: request.aggregate.clone(),
            from: request.after.clone(),
            to: p::ReplicationCursor {
                schema_version: p::M4_SCHEMA_VERSION,
                peer: request.peer,
                aggregate: request.aggregate,
                stream_seq: to_seq,
                authority_epoch: request.after.authority_epoch,
            },
            redaction: request.redaction,
            events: transfers,
            content_digest: p::SchemaDigest(String::new()),
        };
        let batch_identity = p::canonical_digest(&(
            &batch.peer,
            &batch.aggregate,
            &batch.from,
            &batch.to,
            &batch.events,
        ))?;
        batch.batch = p::ReplicationBatchRef(format!("batch:{}", batch_identity.0));
        batch.refresh_digest()?;
        batch.validate()?;
        let semantic = serde_json::to_vec(&batch)
            .map_err(|error| store_error("failed to encode replication export", error))?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin replication export", error))?;
        let live = read_peer_grant(&transaction, &batch.peer)?
            .ok_or_else(|| p::Error("replication peer disappeared".into()))?;
        let live_epoch = authority_epoch_in_transaction(&transaction)?;
        if live.revoked
            || live.grant.reference()? != batch.peer_grant
            || batch.from.authority_epoch != live_epoch
            || live.grant.expires_at <= current_time_ms()?
        {
            return Err(p::Error("replication peer changed during export".into()));
        }
        if let Some(stored) = transaction
            .query_row(
                "SELECT semantic_bytes FROM replication_exports WHERE batch_ref = ?1",
                params![&batch.batch.0],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| store_error("failed to inspect replication export", error))?
        {
            if stored != semantic {
                return Err(p::Error(
                    "replication batch id has conflicting semantics".into(),
                ));
            }
            transaction
                .commit()
                .map_err(|error| store_error("failed to finish duplicate export", error))?;
            return serde_json::from_slice(&stored)
                .map_err(|error| store_error("failed to decode stored replication export", error));
        }
        transaction
            .execute(
                "INSERT INTO replication_exports(
                    batch_ref, peer_ref, aggregate, from_seq, to_seq,
                    authority_epoch, content_digest, semantic_bytes, acknowledged
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
                params![
                    &batch.batch.0,
                    &batch.peer.0,
                    &batch.aggregate.0,
                    i64::try_from(batch.from.stream_seq)
                        .map_err(|_| p::Error("replication cursor is too large".into()))?,
                    i64::try_from(batch.to.stream_seq)
                        .map_err(|_| p::Error("replication cursor is too large".into()))?,
                    i64::try_from(batch.from.authority_epoch.0)
                        .map_err(|_| p::Error("authority epoch is too large".into()))?,
                    &batch.content_digest.0,
                    semantic,
                ],
            )
            .map_err(|error| store_error("failed to record replication export", error))?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit replication export", error))?;
        Ok(batch)
    }

    fn acknowledge_replication(
        &self,
        event: p::Event,
        ack: p::ReplicationAck,
        expected: p::FederationAggregateVersion,
    ) -> p::Result<p::ExpectedAppend> {
        ack.validate()?;
        let aggregate = expected.aggregate.clone();
        self.append_federation_internal(event, &aggregate, expected, Some(&ack))
    }
}

impl RemoteDispatchLedger for SqliteEventStore {
    fn claim_remote_dispatch(
        &self,
        lease_ref: &p::RemoteExecutionLeaseRef,
        plan: &p::PlanDigest,
        authority_epoch: p::AuthorityEpoch,
    ) -> p::Result<p::RemoteDispatchClaim> {
        if lease_ref.0.trim().is_empty() || plan.0.trim().is_empty() || authority_epoch.0 == 0 {
            return Err(p::Error(
                "remote dispatch claim binding is incomplete".into(),
            ));
        }
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin remote dispatch claim", error))?;
        let row = transaction
            .query_row(
                "SELECT d.dispatch_id, d.plan_digest, d.attempted, l.lease
                 FROM remote_dispatches d
                 JOIN remote_leases l ON l.lease_ref = d.lease_ref
                 WHERE d.lease_ref = ?1",
                params![&lease_ref.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| store_error("failed to inspect remote dispatch claim", error))?
            .ok_or_else(|| p::Error("remote dispatch lease is not reserved".into()))?;
        let lease: p::RemoteExecutionLease = serde_json::from_slice(&row.3)
            .map_err(|error| store_error("failed to decode remote dispatch lease", error))?;
        lease.validate()?;
        let live_epoch = authority_epoch_in_transaction(&transaction)?;
        let peer = read_peer_grant(&transaction, &lease.executor)?
            .ok_or_else(|| p::Error("remote dispatch executor is not registered".into()))?;
        if lease.lease != *lease_ref
            || lease.dispatch.0 != row.0
            || lease.plan_digest != *plan
            || row.1 != plan.0
            || lease.authority_epoch != authority_epoch
            || live_epoch != authority_epoch
            || lease.state != p::RemoteLeaseState::Acquired
            || lease.expires_at <= current_time_ms()?
            || peer.revoked
            || peer.grant.reference()? != lease.peer_grant
            || peer.grant.grant_version != lease.grant_version
        {
            return Err(p::Error(
                "remote dispatch claim failed its final authority recheck".into(),
            ));
        }
        let status = if row.2 == 0 {
            let changed = transaction
                .execute(
                    "UPDATE remote_dispatches SET attempted = 1
                     WHERE dispatch_id = ?1 AND attempted = 0",
                    params![&row.0],
                )
                .map_err(|error| store_error("failed to mark remote dispatch attempted", error))?;
            if changed != 1 {
                return Err(p::Error(
                    "remote dispatch claim lost its compare-and-swap".into(),
                ));
            }
            p::RemoteDispatchClaimStatus::Claimed
        } else {
            p::RemoteDispatchClaimStatus::AlreadyAttempted
        };
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit remote dispatch claim", error))?;
        let claim = p::RemoteDispatchClaim {
            schema_version: p::M4_SCHEMA_VERSION,
            dispatch: lease.dispatch,
            lease: lease.lease,
            plan_digest: lease.plan_digest,
            authority_epoch,
            status,
        };
        claim.validate()?;
        Ok(claim)
    }

    fn remote_dispatch_claim(
        &self,
        dispatch: &p::RemoteDispatchId,
    ) -> p::Result<Option<p::RemoteDispatchClaim>> {
        if dispatch.0.trim().is_empty() {
            return Err(p::Error("remote dispatch id is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        connection
            .query_row(
                "SELECT d.plan_digest, d.attempted, l.lease
                 FROM remote_dispatches d
                 JOIN remote_leases l ON l.lease_ref = d.lease_ref
                 WHERE d.dispatch_id = ?1",
                params![&dispatch.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| store_error("failed to read remote dispatch claim", error))?
            .map(|(plan_digest, attempted, bytes)| {
                let lease: p::RemoteExecutionLease =
                    serde_json::from_slice(&bytes).map_err(|error| {
                        store_error("failed to decode remote dispatch claim", error)
                    })?;
                let claim = p::RemoteDispatchClaim {
                    schema_version: p::M4_SCHEMA_VERSION,
                    dispatch: dispatch.clone(),
                    lease: lease.lease,
                    plan_digest: p::PlanDigest(plan_digest),
                    authority_epoch: lease.authority_epoch,
                    status: if attempted == 0 {
                        p::RemoteDispatchClaimStatus::Reserved
                    } else {
                        p::RemoteDispatchClaimStatus::AlreadyAttempted
                    },
                };
                claim.validate()?;
                Ok(claim)
            })
            .transpose()
    }
}

impl FederationRuntimeLedger for SqliteEventStore {
    fn claim_control_nonce(
        &self,
        peer: &p::FederatedPeerRef,
        nonce: &p::Nonce,
        digest: &p::SchemaDigest,
    ) -> p::Result<bool> {
        if peer.0.trim().is_empty() || nonce.0.trim().is_empty() || digest.0.trim().is_empty() {
            return Err(p::Error(
                "federation control replay binding is incomplete".into(),
            ));
        }
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin control replay claim", error))?;
        let existing = transaction
            .query_row(
                "SELECT command_digest FROM federation_control_replay
                 WHERE peer_ref = ?1 AND nonce = ?2",
                params![&peer.0, &nonce.0],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| store_error("failed to inspect control replay claim", error))?;
        if let Some(existing) = existing {
            if existing != digest.0 {
                return Err(p::Error(
                    "federation control nonce changed command semantics".into(),
                ));
            }
            transaction
                .commit()
                .map_err(|error| store_error("failed to finish control replay claim", error))?;
            return Ok(false);
        }
        transaction
            .execute(
                "INSERT INTO federation_control_replay(peer_ref, nonce, command_digest)
                 VALUES (?1, ?2, ?3)",
                params![&peer.0, &nonce.0, &digest.0],
            )
            .map_err(|error| store_error("failed to persist control replay claim", error))?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit control replay claim", error))?;
        Ok(true)
    }

    fn claim_device_signal(&self, signal: &p::FederatedDeviceSignal) -> p::Result<bool> {
        signal.validate(i64::MIN)?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin device signal claim", error))?;
        let existing = transaction
            .query_row(
                "SELECT peer_ref, digest FROM federation_device_replay WHERE signal_ref = ?1",
                params![&signal.signal.0],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|error| store_error("failed to inspect device signal", error))?;
        if let Some((peer, digest)) = existing {
            if peer != signal.peer.0 || digest != signal.digest.0 {
                return Err(p::Error("federated device signal changed semantics".into()));
            }
            transaction
                .commit()
                .map_err(|error| store_error("failed to finish duplicate device signal", error))?;
            return Ok(false);
        }
        transaction
            .execute(
                "INSERT INTO federation_device_replay(signal_ref, peer_ref, digest)
                 VALUES (?1, ?2, ?3)",
                params![&signal.signal.0, &signal.peer.0, &signal.digest.0],
            )
            .map_err(|error| store_error("failed to persist device signal", error))?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit device signal", error))?;
        Ok(true)
    }

    fn record_federated_checkpoint(
        &self,
        source: &p::RunId,
        checkpoint: &p::FederatedCheckpointArtifact,
    ) -> p::Result<bool> {
        checkpoint.validate()?;
        if source.0.trim().is_empty() {
            return Err(p::Error("checkpoint source run is empty".into()));
        }
        let encoded = serde_json::to_vec(checkpoint)
            .map_err(|error| store_error("failed to encode federated checkpoint", error))?;
        persist_immutable_runtime_record(
            self,
            "federated_checkpoint_artifacts",
            "checkpoint_ref",
            &checkpoint.reference.0,
            "artifact",
            &encoded,
            Some(("source_run", &source.0)),
            "federated checkpoint",
        )
    }

    fn federated_checkpoint_artifact(
        &self,
        checkpoint: &p::FederatedCheckpointArtifactRef,
    ) -> p::Result<Option<(p::RunId, p::FederatedCheckpointArtifact)>> {
        if checkpoint.0.trim().is_empty() {
            return Err(p::Error("checkpoint reference is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        connection
            .query_row(
                "SELECT source_run, artifact FROM federated_checkpoint_artifacts
                 WHERE checkpoint_ref = ?1",
                params![&checkpoint.0],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(|error| store_error("failed to read federated checkpoint", error))?
            .map(|(source, encoded)| {
                let artifact: p::FederatedCheckpointArtifact = serde_json::from_slice(&encoded)
                    .map_err(|error| store_error("failed to decode federated checkpoint", error))?;
                artifact.validate()?;
                Ok((p::RunId(source), artifact))
            })
            .transpose()
    }

    fn record_federated_handoff(&self, handoff: &p::FederatedHandoffPlan) -> p::Result<bool> {
        handoff.validate()?;
        let encoded = serde_json::to_vec(handoff)
            .map_err(|error| store_error("failed to encode federated handoff", error))?;
        persist_immutable_runtime_record(
            self,
            "federated_handoffs",
            "next_run",
            &handoff.next_run.0,
            "plan",
            &encoded,
            None,
            "federated handoff",
        )
    }

    fn federated_handoff_for_run(
        &self,
        run: &p::RunId,
    ) -> p::Result<Option<p::FederatedHandoffPlan>> {
        read_runtime_json_record(
            self,
            "SELECT plan FROM federated_handoffs WHERE next_run = ?1",
            &run.0,
            "federated handoff",
            p::FederatedHandoffPlan::validate,
        )
    }

    fn record_retention_request(&self, request: &p::FederatedRetentionRequest) -> p::Result<bool> {
        request.validate()?;
        let mut state = p::FederatedRetentionState {
            schema_version: p::M4_SCHEMA_VERSION,
            request: request.clone(),
            status: p::RetentionStatus::Requested,
            receipt: None,
            digest: p::SchemaDigest(String::new()),
        };
        state.refresh_digest()?;
        let encoded = serde_json::to_vec(&state)
            .map_err(|error| store_error("failed to encode retention state", error))?;
        persist_immutable_runtime_record(
            self,
            "federated_retention",
            "request_ref",
            &request.request.0,
            "state",
            &encoded,
            None,
            "federated retention request",
        )
    }

    fn federated_retention_state(
        &self,
        request: &p::RetentionRequestRef,
    ) -> p::Result<Option<p::FederatedRetentionState>> {
        read_runtime_json_record(
            self,
            "SELECT state FROM federated_retention WHERE request_ref = ?1",
            &request.0,
            "federated retention state",
            p::FederatedRetentionState::validate,
        )
    }

    fn accept_retention_receipt(
        &self,
        receipt: &p::FederatedRetentionReceipt,
    ) -> p::Result<p::FederatedRetentionState> {
        receipt.validate()?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin retention receipt", error))?;
        let encoded = transaction
            .query_row(
                "SELECT state FROM federated_retention WHERE request_ref = ?1",
                params![&receipt.request.0],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| store_error("failed to inspect retention receipt", error))?
            .ok_or_else(|| p::Error("retention receipt has no authority request".into()))?;
        let mut state: p::FederatedRetentionState = serde_json::from_slice(&encoded)
            .map_err(|error| store_error("failed to decode retention state", error))?;
        state.validate()?;
        if state.status == p::RetentionStatus::Verified {
            if state.receipt.as_ref() == Some(receipt) {
                transaction
                    .commit()
                    .map_err(|error| store_error("failed to finish retention receipt", error))?;
                return Ok(state);
            }
            return Err(p::Error(
                "retention request already has a different receipt".into(),
            ));
        }
        if state.status != p::RetentionStatus::Requested
            || state.request.request != receipt.request
            || state.request.peer != receipt.peer
            || state.request.authority_epoch != receipt.authority_epoch
        {
            return Err(p::Error("retention receipt lineage is invalid".into()));
        }
        state.status = p::RetentionStatus::Verified;
        state.receipt = Some(receipt.clone());
        state.refresh_digest()?;
        let updated = serde_json::to_vec(&state)
            .map_err(|error| store_error("failed to encode verified retention state", error))?;
        let changed = transaction
            .execute(
                "UPDATE federated_retention SET state = ?2 WHERE request_ref = ?1",
                params![&receipt.request.0, updated],
            )
            .map_err(|error| store_error("failed to persist retention receipt", error))?;
        if changed != 1 {
            return Err(p::Error(
                "retention receipt lost its authority state".into(),
            ));
        }
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit retention receipt", error))?;
        Ok(state)
    }

    fn save_remote_recovery(&self, recovery: &p::RemoteActionRecoveryRecord) -> p::Result<()> {
        recovery.validate()?;
        let encoded = serde_json::to_vec(recovery)
            .map_err(|error| store_error("failed to encode remote recovery", error))?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin remote recovery write", error))?;
        let existing = transaction
            .query_row(
                "SELECT lease_ref, record FROM remote_action_recovery WHERE run_id = ?1",
                params![&recovery.run.0],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()
            .map_err(|error| store_error("failed to inspect remote recovery", error))?;
        match existing {
            None => {
                transaction
                    .execute(
                        "INSERT INTO remote_action_recovery(run_id, lease_ref, record)
                         VALUES (?1, ?2, ?3)",
                        params![&recovery.run.0, &recovery.lease.lease.0, encoded],
                    )
                    .map_err(|error| store_error("failed to persist remote recovery", error))?;
            }
            Some((lease_ref, previous)) => {
                let prior: p::RemoteActionRecoveryRecord = serde_json::from_slice(&previous)
                    .map_err(|error| store_error("failed to decode remote recovery", error))?;
                prior.validate()?;
                if prior == *recovery {
                    transaction
                        .commit()
                        .map_err(|error| store_error("failed to finish remote recovery", error))?;
                    return Ok(());
                }
                let mut prior_binding = prior.clone();
                let mut next_binding = recovery.clone();
                prior_binding.driver_receipt = None;
                next_binding.driver_receipt = None;
                prior_binding.refresh_digest()?;
                next_binding.refresh_digest()?;
                if lease_ref != recovery.lease.lease.0
                    || prior.driver_receipt.is_some()
                    || recovery.driver_receipt.is_none()
                    || prior_binding != next_binding
                {
                    return Err(p::Error(
                        "remote recovery changed immutable semantics".into(),
                    ));
                }
                transaction
                    .execute(
                        "UPDATE remote_action_recovery SET record = ?2 WHERE run_id = ?1",
                        params![&recovery.run.0, encoded],
                    )
                    .map_err(|error| store_error("failed to advance remote recovery", error))?;
            }
        }
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit remote recovery", error))?;
        Ok(())
    }

    fn remote_recovery(&self, run: &p::RunId) -> p::Result<Option<p::RemoteActionRecoveryRecord>> {
        read_runtime_json_record(
            self,
            "SELECT record FROM remote_action_recovery WHERE run_id = ?1",
            &run.0,
            "remote action recovery",
            p::RemoteActionRecoveryRecord::validate,
        )
    }

    fn remote_recovery_for_lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteActionRecoveryRecord>> {
        read_runtime_json_record(
            self,
            "SELECT record FROM remote_action_recovery WHERE lease_ref = ?1",
            &lease.0,
            "remote action recovery",
            p::RemoteActionRecoveryRecord::validate,
        )
    }
}

impl EcosystemProjection for SqliteEventStore {
    fn publisher(
        &self,
        publisher: &p::CapabilityPublisherRef,
    ) -> p::Result<Option<p::CapabilityPublisherGrant>> {
        if publisher.0.trim().is_empty() {
            return Err(p::Error("capability publisher identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_ecosystem_json(
            &connection,
            "SELECT grant FROM ecosystem_publishers WHERE publisher_ref = ?1",
            &publisher.0,
            "capability publisher grant",
            p::CapabilityPublisherGrant::validate,
        )
    }

    fn admission(
        &self,
        release: &p::CapabilityReleaseRef,
    ) -> p::Result<Option<p::CapabilityPackageAdmission>> {
        if release.0.trim().is_empty() {
            return Err(p::Error("capability release identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_ecosystem_json(
            &connection,
            "SELECT admission FROM ecosystem_admissions WHERE release_ref = ?1",
            &release.0,
            "capability package admission",
            p::CapabilityPackageAdmission::validate,
        )
    }

    fn package_state(
        &self,
        package: &p::CapabilityPackageRef,
    ) -> p::Result<Option<p::CapabilityPackageState>> {
        if package.0.trim().is_empty() {
            return Err(p::Error("capability package identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_ecosystem_json(
            &connection,
            "SELECT state FROM ecosystem_package_states WHERE package_ref = ?1",
            &package.0,
            "capability package state",
            p::CapabilityPackageState::validate,
        )
    }

    fn snapshot(&self, scope: p::Scope) -> p::Result<p::CapabilityEcosystemSnapshot> {
        if scope.0.trim().is_empty() {
            return Err(p::Error("capability ecosystem scope is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let version = p::EcosystemAggregateVersion {
            schema_version: p::M5_SCHEMA_VERSION,
            value: ecosystem_version_in_connection(&connection, &ecosystem_aggregate())?,
        };
        let publishers = read_ecosystem_refs(
            &connection,
            "SELECT grant_ref FROM ecosystem_publishers
             WHERE scope = ?1 OR scope = '*' ORDER BY grant_ref",
            &scope.0,
        )?
        .into_iter()
        .map(p::CapabilityPublisherGrantRef)
        .collect();
        let admissions = read_ecosystem_refs(
            &connection,
            "SELECT admission_ref FROM ecosystem_admissions
             WHERE scope = ?1 OR scope = '*' ORDER BY admission_ref",
            &scope.0,
        )?
        .into_iter()
        .map(p::CapabilityAdmissionRef)
        .collect();
        let packages = read_ecosystem_json_rows::<p::CapabilityPackageState>(
            &connection,
            "SELECT state FROM ecosystem_package_states
             WHERE scope = ?1 OR scope = '*' ORDER BY package_ref",
            &scope.0,
            "capability package state",
            p::CapabilityPackageState::validate,
        )?;
        let distributions = read_ecosystem_refs(
            &connection,
            "SELECT receipt_ref FROM ecosystem_distributions
             WHERE scope = ?1 OR scope = '*' ORDER BY receipt_ref",
            &scope.0,
        )?
        .into_iter()
        .map(p::CapabilityDistributionReceiptRef)
        .collect();
        let mut snapshot = p::CapabilityEcosystemSnapshot {
            schema_version: p::M5_SCHEMA_VERSION,
            scope,
            version,
            publishers,
            admissions,
            packages,
            distributions,
            digest: p::SchemaDigest(String::new()),
        };
        snapshot.refresh_digest()?;
        snapshot.validate()?;
        Ok(snapshot)
    }
}

impl EcosystemEventStore for SqliteEventStore {
    fn ecosystem_version(
        &self,
        aggregate: &p::EcosystemAggregateRef,
    ) -> p::Result<p::EcosystemAggregateVersion> {
        require_ecosystem_aggregate(aggregate)?;
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        Ok(p::EcosystemAggregateVersion {
            schema_version: p::M5_SCHEMA_VERSION,
            value: ecosystem_version_in_connection(&connection, aggregate)?,
        })
    }

    fn append_ecosystem_expected(
        &self,
        event: p::Event,
        aggregate: &p::EcosystemAggregateRef,
        expected: p::EcosystemAggregateVersion,
    ) -> p::Result<p::ExpectedAppend> {
        require_ecosystem_aggregate(aggregate)?;
        expected.validate()?;
        validate_ecosystem_control_event(&event, &expected)?;
        let committed = ecosystem_control_version(&event)?.value;
        let prepared = self.prepare_event(event)?;
        let control_event = prepared.event.clone();

        let _registration = if self.core.fts_enabled {
            Some(
                self.core
                    .upcaster_registration
                    .lock()
                    .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?,
            )
        } else {
            None
        };
        let fts_upcaster_identity = if self.core.fts_enabled {
            Some(
                self.core
                    .upcasters
                    .read()
                    .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))?
                    .identity_bytes(),
            )
        } else {
            None
        };
        let fts_schema_identity = self
            .core
            .fts_enabled
            .then(|| schema_registry_identity(&self.core.current_schema));
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin ecosystem append", error))?;
        let actual = ecosystem_version_in_connection(&transaction, aggregate)?;
        if actual != expected.value {
            if actual == committed
                && ecosystem_history_contains(&transaction, aggregate, &control_event.event_id)?
            {
                let outcome = append_prepared(
                    &transaction,
                    prepared,
                    self.core.fts_enabled,
                    fts_schema_identity.as_deref(),
                    fts_upcaster_identity.as_deref(),
                )?;
                if outcome.disposition != AppendDisposition::Duplicate {
                    return Err(p::Error(
                        "ecosystem history references a non-duplicate event".into(),
                    ));
                }
                transaction.commit().map_err(|error| {
                    store_error("failed to finish duplicate ecosystem append", error)
                })?;
                return Ok(p::ExpectedAppend {
                    schema_version: p::M5_SCHEMA_VERSION,
                    status: p::ExpectedAppendStatus::Duplicate,
                    expected_version: expected.value,
                    actual_version: actual,
                    resulting_version: actual,
                    event_id: Some(outcome.event_id),
                });
            }
            transaction
                .rollback()
                .map_err(|error| store_error("failed to finish ecosystem conflict", error))?;
            return Ok(p::ExpectedAppend {
                schema_version: p::M5_SCHEMA_VERSION,
                status: p::ExpectedAppendStatus::Conflict,
                expected_version: expected.value,
                actual_version: actual,
                resulting_version: actual,
                event_id: None,
            });
        }
        validate_ecosystem_transition(&transaction, &control_event)?;
        let outcome = append_prepared(
            &transaction,
            prepared,
            self.core.fts_enabled,
            fts_schema_identity.as_deref(),
            fts_upcaster_identity.as_deref(),
        )?;
        if outcome.disposition != AppendDisposition::Applied {
            return Err(p::Error(
                "ecosystem event exists without its committed version".into(),
            ));
        }
        update_ecosystem_projection(&transaction, &control_event)?;
        transaction
            .execute(
                "INSERT INTO ecosystem_versions(aggregate, version) VALUES (?1, ?2)
                 ON CONFLICT(aggregate) DO UPDATE SET version = excluded.version",
                params![
                    &aggregate.0,
                    i64::try_from(committed)
                        .map_err(|_| p::Error("ecosystem version is too large".into()))?,
                ],
            )
            .map_err(|error| store_error("failed to advance ecosystem version", error))?;
        transaction
            .execute(
                "INSERT INTO ecosystem_history(aggregate, committed_version, event_id, kind)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    &aggregate.0,
                    i64::try_from(committed)
                        .map_err(|_| p::Error("ecosystem version is too large".into()))?,
                    &control_event.event_id.0,
                    control_event.kind.as_str(),
                ],
            )
            .map_err(|error| store_error("failed to append ecosystem history", error))?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit ecosystem append", error))?;
        Ok(p::ExpectedAppend {
            schema_version: p::M5_SCHEMA_VERSION,
            status: p::ExpectedAppendStatus::Applied,
            expected_version: expected.value,
            actual_version: actual,
            resulting_version: committed,
            event_id: Some(outcome.event_id),
        })
    }
}

impl EcosystemRuntimeLedger for SqliteEventStore {
    fn claim_ecosystem_nonce(&self, nonce: &p::Nonce, plan: &p::PlanDigest) -> p::Result<bool> {
        if nonce.0.trim().is_empty() || plan.0.trim().is_empty() {
            return Err(p::Error("ecosystem nonce binding is incomplete".into()));
        }
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin ecosystem nonce claim", error))?;
        let existing = transaction
            .query_row(
                "SELECT plan_digest FROM ecosystem_nonces WHERE nonce = ?1",
                params![&nonce.0],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| store_error("failed to inspect ecosystem nonce", error))?;
        if let Some(existing) = existing {
            if existing != plan.0 {
                return Err(p::Error("ecosystem nonce changed plan semantics".into()));
            }
            transaction
                .commit()
                .map_err(|error| store_error("failed to finish ecosystem nonce claim", error))?;
            return Ok(false);
        }
        transaction
            .execute(
                "INSERT INTO ecosystem_nonces(nonce, plan_digest) VALUES (?1, ?2)",
                params![&nonce.0, &plan.0],
            )
            .map_err(|error| store_error("failed to persist ecosystem nonce", error))?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit ecosystem nonce", error))?;
        Ok(true)
    }

    fn record_distribution_attempt(
        &self,
        receipt: &p::CapabilityPackageDistributionReceipt,
    ) -> p::Result<bool> {
        receipt.validate()?;
        let semantic = serde_json::to_vec(receipt)
            .map_err(|error| store_error("failed to encode distribution attempt", error))?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin distribution attempt", error))?;
        let existing = transaction
            .query_row(
                "SELECT semantic FROM ecosystem_distribution_attempts WHERE receipt_ref = ?1",
                params![&receipt.reference.0],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| store_error("failed to inspect distribution attempt", error))?;
        if let Some(existing) = existing {
            if existing != semantic {
                return Err(p::Error(
                    "distribution receipt identity changed semantics".into(),
                ));
            }
            transaction
                .commit()
                .map_err(|error| store_error("failed to finish distribution attempt", error))?;
            return Ok(false);
        }
        transaction
            .execute(
                "INSERT INTO ecosystem_distribution_attempts(receipt_ref, semantic)
                 VALUES (?1, ?2)",
                params![&receipt.reference.0, semantic],
            )
            .map_err(|error| store_error("failed to persist distribution attempt", error))?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit distribution attempt", error))?;
        Ok(true)
    }

    fn distribution_receipt(
        &self,
        reference: &p::CapabilityDistributionReceiptRef,
    ) -> p::Result<Option<p::CapabilityPackageDistributionReceipt>> {
        if reference.0.trim().is_empty() {
            return Err(p::Error("distribution receipt identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_ecosystem_json(
            &connection,
            "SELECT semantic FROM ecosystem_distribution_attempts WHERE receipt_ref = ?1",
            &reference.0,
            "capability distribution receipt",
            p::CapabilityPackageDistributionReceipt::validate,
        )
    }
}

impl EcosystemPackageArchive for SqliteEventStore {
    fn archive_package(&self, package: &p::SignedCapabilityPackage) -> p::Result<bool> {
        package.validate()?;
        let encoded = serde_json::to_vec(package)
            .map_err(|error| store_error("failed to encode capability package bundle", error))?;
        let mut connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| store_error("failed to begin package archive transaction", error))?;
        let existing = transaction
            .query_row(
                "SELECT bundle FROM ecosystem_package_bundles WHERE release_ref = ?1",
                params![&package.manifest.release.0],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| store_error("failed to inspect capability package archive", error))?;
        if let Some(existing) = existing {
            let stored: p::SignedCapabilityPackage =
                serde_json::from_slice(&existing).map_err(|error| {
                    store_error("failed to decode capability package bundle", error)
                })?;
            stored.validate()?;
            if stored != *package {
                return Err(p::Error(
                    "package release identity changed immutable archived content".into(),
                ));
            }
            transaction
                .commit()
                .map_err(|error| store_error("failed to finish package archive read", error))?;
            return Ok(false);
        }
        transaction
            .execute(
                "INSERT INTO ecosystem_package_bundles(
                    release_ref, package_ref, package_digest, bundle
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    &package.manifest.release.0,
                    &package.manifest.package.0,
                    &package.package_digest.0,
                    encoded,
                ],
            )
            .map_err(|error| store_error("failed to archive capability package bundle", error))?;
        transaction
            .commit()
            .map_err(|error| store_error("failed to commit capability package archive", error))?;
        Ok(true)
    }

    fn archived_package(
        &self,
        release: &p::CapabilityReleaseRef,
    ) -> p::Result<Option<p::SignedCapabilityPackage>> {
        if release.0.trim().is_empty() {
            return Err(p::Error("capability release identity is empty".into()));
        }
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_ecosystem_json(
            &connection,
            "SELECT bundle FROM ecosystem_package_bundles WHERE release_ref = ?1",
            &release.0,
            "capability package bundle",
            p::SignedCapabilityPackage::validate,
        )
    }

    fn enabled_package_states(&self) -> p::Result<Vec<p::CapabilityPackageState>> {
        let connection = self
            .core
            .connection
            .lock()
            .map_err(|_| p::Error("event database lock is poisoned".into()))?;
        read_ecosystem_json_rows_unscoped::<p::CapabilityPackageState>(
            &connection,
            "SELECT state FROM ecosystem_package_states
             WHERE lifecycle = 'enabled' ORDER BY package_ref",
            "enabled capability package state",
            p::CapabilityPackageState::validate,
        )
    }
}

fn ecosystem_aggregate() -> p::EcosystemAggregateRef {
    p::EcosystemAggregateRef("ecosystem".into())
}

fn require_ecosystem_aggregate(aggregate: &p::EcosystemAggregateRef) -> p::Result<()> {
    if aggregate.0 != "ecosystem" {
        return Err(p::Error("unknown capability ecosystem aggregate".into()));
    }
    Ok(())
}

fn read_ecosystem_json<T>(
    connection: &Connection,
    query: &str,
    key: &str,
    label: &str,
    validate: impl Fn(&T) -> p::Result<()>,
) -> p::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    connection
        .query_row(query, params![key], |row| row.get::<_, Vec<u8>>(0))
        .optional()
        .map_err(|error| store_error(&format!("failed to read {label}"), error))?
        .map(|encoded| {
            let value = serde_json::from_slice::<T>(&encoded)
                .map_err(|error| store_error(&format!("failed to decode {label}"), error))?;
            validate(&value)?;
            Ok(value)
        })
        .transpose()
}

fn read_ecosystem_refs(
    connection: &Connection,
    query: &str,
    scope: &str,
) -> p::Result<Vec<String>> {
    let mut statement = connection
        .prepare(query)
        .map_err(|error| store_error("failed to prepare ecosystem reference scan", error))?;
    let rows = statement
        .query_map(params![scope], |row| row.get::<_, String>(0))
        .map_err(|error| store_error("failed to scan ecosystem references", error))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| store_error("failed to read ecosystem reference", error))
}

fn read_ecosystem_json_rows<T>(
    connection: &Connection,
    query: &str,
    scope: &str,
    label: &str,
    validate: impl Fn(&T) -> p::Result<()>,
) -> p::Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let mut statement = connection
        .prepare(query)
        .map_err(|error| store_error(&format!("failed to prepare {label} scan"), error))?;
    let rows = statement
        .query_map(params![scope], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|error| store_error(&format!("failed to scan {label}"), error))?;
    rows.map(|row| {
        let encoded =
            row.map_err(|error| store_error(&format!("failed to read {label}"), error))?;
        let value = serde_json::from_slice::<T>(&encoded)
            .map_err(|error| store_error(&format!("failed to decode {label}"), error))?;
        validate(&value)?;
        Ok(value)
    })
    .collect()
}

fn read_ecosystem_json_rows_unscoped<T>(
    connection: &Connection,
    query: &str,
    label: &str,
    validate: impl Fn(&T) -> p::Result<()>,
) -> p::Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let mut statement = connection
        .prepare(query)
        .map_err(|error| store_error(&format!("failed to prepare {label} scan"), error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|error| store_error(&format!("failed to scan {label}"), error))?;
    rows.map(|row| {
        let encoded =
            row.map_err(|error| store_error(&format!("failed to read {label}"), error))?;
        let value = serde_json::from_slice::<T>(&encoded)
            .map_err(|error| store_error(&format!("failed to decode {label}"), error))?;
        validate(&value)?;
        Ok(value)
    })
    .collect()
}

#[allow(clippy::too_many_arguments)]
fn persist_immutable_runtime_record(
    store: &SqliteEventStore,
    table: &str,
    key_column: &str,
    key: &str,
    value_column: &str,
    value: &[u8],
    extra: Option<(&str, &str)>,
    label: &str,
) -> p::Result<bool> {
    if key.trim().is_empty() {
        return Err(p::Error(format!("{label} identity is empty")));
    }
    let allowed = [
        (
            "federated_checkpoint_artifacts",
            "checkpoint_ref",
            "artifact",
        ),
        ("federated_handoffs", "next_run", "plan"),
        ("federated_retention", "request_ref", "state"),
    ];
    if !allowed.contains(&(table, key_column, value_column)) {
        return Err(p::Error("runtime ledger table is not allowlisted".into()));
    }
    if extra.is_some_and(|(extra_column, _)| {
        (table, extra_column) != ("federated_checkpoint_artifacts", "source_run")
    }) {
        return Err(p::Error(
            "runtime ledger extra column is not allowlisted".into(),
        ));
    }
    let mut connection = store
        .core
        .connection
        .lock()
        .map_err(|_| p::Error("event database lock is poisoned".into()))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| store_error(&format!("failed to begin {label} write"), error))?;
    let existing = match extra {
        Some((extra_column, _)) => {
            let select = format!(
                "SELECT {extra_column}, {value_column} FROM {table} WHERE {key_column} = ?1"
            );
            transaction
                .query_row(&select, params![key], |row| {
                    Ok((Some(row.get::<_, String>(0)?), row.get::<_, Vec<u8>>(1)?))
                })
                .optional()
                .map_err(|error| store_error(&format!("failed to inspect {label}"), error))?
        }
        None => {
            let select = format!("SELECT {value_column} FROM {table} WHERE {key_column} = ?1");
            transaction
                .query_row(&select, params![key], |row| {
                    Ok((None, row.get::<_, Vec<u8>>(0)?))
                })
                .optional()
                .map_err(|error| store_error(&format!("failed to inspect {label}"), error))?
        }
    };
    if let Some((stored_extra, existing)) = existing {
        if existing != value || stored_extra.as_deref() != extra.map(|(_, value)| value) {
            return Err(p::Error(format!("{label} identity changed semantics")));
        }
        transaction
            .commit()
            .map_err(|error| store_error(&format!("failed to finish duplicate {label}"), error))?;
        return Ok(false);
    }
    match extra {
        Some((extra_column, extra_value)) => {
            let insert = format!(
                "INSERT INTO {table}({key_column}, {extra_column}, {value_column}) VALUES (?1, ?2, ?3)"
            );
            transaction
                .execute(&insert, params![key, extra_value, value])
                .map_err(|error| store_error(&format!("failed to persist {label}"), error))?;
        }
        None => {
            let insert =
                format!("INSERT INTO {table}({key_column}, {value_column}) VALUES (?1, ?2)");
            transaction
                .execute(&insert, params![key, value])
                .map_err(|error| store_error(&format!("failed to persist {label}"), error))?;
        }
    }
    transaction
        .commit()
        .map_err(|error| store_error(&format!("failed to commit {label}"), error))?;
    Ok(true)
}

fn read_runtime_json_record<T>(
    store: &SqliteEventStore,
    query: &str,
    key: &str,
    label: &str,
    validate: impl FnOnce(&T) -> p::Result<()>,
) -> p::Result<Option<T>>
where
    T: serde::de::DeserializeOwned,
{
    if key.trim().is_empty() {
        return Err(p::Error(format!("{label} identity is empty")));
    }
    let connection = store
        .core
        .connection
        .lock()
        .map_err(|_| p::Error("event database lock is poisoned".into()))?;
    connection
        .query_row(query, params![key], |row| row.get::<_, Vec<u8>>(0))
        .optional()
        .map_err(|error| store_error(&format!("failed to read {label}"), error))?
        .map(|encoded| {
            let value = serde_json::from_slice(&encoded)
                .map_err(|error| store_error(&format!("failed to decode {label}"), error))?;
            validate(&value)?;
            Ok(value)
        })
        .transpose()
}

struct PreparedEvent {
    event: p::Event,
    payload: Vec<u8>,
    provenance: Vec<u8>,
    event_semantics: Vec<u8>,
    domain_key: Option<(&'static str, String)>,
    domain_semantics: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppendDisposition {
    Applied,
    Duplicate,
}

struct AppendOutcome {
    event_id: p::EventId,
    resulting_version: u64,
    disposition: AppendDisposition,
}

fn append_prepared(
    transaction: &Transaction<'_>,
    mut prepared: PreparedEvent,
    fts_enabled: bool,
    fts_schema_identity: Option<&[u8]>,
    fts_upcaster_identity: Option<&[u8]>,
) -> p::Result<AppendOutcome> {
    let duplicate_event = transaction
        .query_row(
            "SELECT run_id, turn_id, kind, payload, schema_version, ts, provenance
             FROM events WHERE event_id = ?1",
            params![&prepared.event.event_id.0],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|error| store_error("failed to check event idempotency", error))?;
    if let Some((run_id, turn_id, kind, stored_payload, schema_version, ts, stored_provenance)) =
        duplicate_event
    {
        let schema_version = u32::try_from(schema_version)
            .map_err(|_| p::Error("stored event schema version is invalid".into()))?;
        let stored_semantics = semantic_bytes(SemanticEventFields {
            event_id: Some(&prepared.event.event_id.0),
            run_id: &run_id,
            turn_id: turn_id.as_deref(),
            kind: &kind,
            payload: &stored_payload,
            schema_version,
            ts_unix_ms: Some(ts),
            provenance: &stored_provenance,
        });
        if stored_semantics != prepared.event_semantics {
            return Err(p::Error(format!(
                "event idempotency collision for event_id {}: existing event semantics differ",
                prepared.event.event_id.0
            )));
        }
        return Ok(AppendOutcome {
            event_id: prepared.event.event_id,
            resulting_version: aggregate_version_in_transaction(
                transaction,
                &prepared.event.run_id,
            )?,
            disposition: AppendDisposition::Duplicate,
        });
    }

    if let Some((namespace, key)) = prepared.domain_key.take() {
        let domain_semantics = prepared
            .domain_semantics
            .as_deref()
            .expect("domain semantics exist when the domain key exists");
        let claimed = transaction
            .execute(
                "INSERT INTO idempotency_keys(
                    namespace, \"key\", event_id, run_id, semantic_bytes
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(namespace, \"key\") DO NOTHING",
                params![
                    namespace,
                    key,
                    &prepared.event.event_id.0,
                    &prepared.event.run_id.0,
                    domain_semantics,
                ],
            )
            .map_err(|error| store_error("failed to claim event idempotency key", error))?;
        if claimed == 0 {
            let (original_id, original_semantics) = transaction
                .query_row(
                    "SELECT event_id, semantic_bytes FROM idempotency_keys
                     WHERE namespace = ?1 AND \"key\" = ?2",
                    params![namespace, key],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .map_err(|error| store_error("failed to resolve event idempotency key", error))?;
            if original_semantics != domain_semantics {
                return Err(p::Error(format!(
                    "{namespace} idempotency collision for key {key}: existing request semantics differ"
                )));
            }
            return Ok(AppendOutcome {
                event_id: p::EventId(original_id),
                resulting_version: aggregate_version_in_transaction(
                    transaction,
                    &prepared.event.run_id,
                )?,
                disposition: AppendDisposition::Duplicate,
            });
        }
    }

    let fts_was_current = if fts_enabled {
        let event_set = read_event_set_token(transaction)?;
        let schema_identity = fts_schema_identity
            .ok_or_else(|| p::Error("FTS schema identity is unavailable".into()))?;
        let upcaster_identity = fts_upcaster_identity
            .ok_or_else(|| p::Error("FTS upcaster identity is unavailable".into()))?;
        let expected = FtsCoverage::new(
            &event_set,
            schema_identity.to_vec(),
            upcaster_identity.to_vec(),
        );
        let coverage = read_fts_coverage(transaction)?;
        coverage.as_ref() == Some(&expected) || (coverage.is_none() && event_set.is_empty())
    } else {
        false
    };

    let previous_seq = aggregate_version_in_transaction(transaction, &prepared.event.run_id)?;
    let next_seq = previous_seq
        .checked_add(1)
        .ok_or_else(|| p::Error("event sequence is exhausted".into()))?;
    let next_seq_sql =
        i64::try_from(next_seq).map_err(|_| p::Error("event sequence is invalid".into()))?;
    prepared.event.stream_seq = next_seq;
    let checksum = checksum::calculate(PersistedEnvelope {
        event_id: &prepared.event.event_id.0,
        run_id: &prepared.event.run_id.0,
        stream_seq: prepared.event.stream_seq,
        turn_id: prepared
            .event
            .turn_id
            .as_ref()
            .map(|turn_id| turn_id.0.as_str()),
        kind: prepared.event.kind.as_str(),
        payload: &prepared.payload,
        schema_version: prepared.event.schema_version.0,
        ts_unix_ms: prepared.event.ts_unix_ms,
        provenance: &prepared.provenance,
    });
    let result_id = prepared.event.event_id.clone();
    transaction
        .execute(
            "INSERT INTO events(
                event_id, run_id, stream_seq, turn_id, kind, payload,
                schema_version, ts, provenance, checksum
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                &prepared.event.event_id.0,
                &prepared.event.run_id.0,
                next_seq_sql,
                prepared
                    .event
                    .turn_id
                    .as_ref()
                    .map(|turn_id| turn_id.0.as_str()),
                prepared.event.kind.as_str(),
                prepared.payload,
                i64::from(prepared.event.schema_version.0),
                prepared.event.ts_unix_ms,
                prepared.provenance,
                checksum,
            ],
        )
        .map_err(|error| store_error("failed to append event", error))?;
    let event_rowid = transaction.last_insert_rowid();
    if fts_enabled {
        if let Some(text) = searchable_text(&prepared.event) {
            transaction
                .execute(
                    "INSERT INTO events_fts(rowid, text) VALUES (?1, ?2)",
                    params![event_rowid, text],
                )
                .map_err(|error| store_error("failed to index event text", error))?;
        }
    }
    update_builtin_projections(transaction, &prepared.event)?;
    update_evolution_catalog(transaction, &prepared.event)?;
    if fts_enabled {
        if fts_was_current {
            let event_set = read_event_set_token(transaction)?;
            write_fts_coverage(
                transaction,
                &FtsCoverage::new(
                    &event_set,
                    fts_schema_identity
                        .expect("FTS schema identity exists when FTS is enabled")
                        .to_vec(),
                    fts_upcaster_identity
                        .expect("FTS upcaster identity exists when FTS is enabled")
                        .to_vec(),
                ),
            )?;
        } else {
            transaction
                .execute("DELETE FROM fts_coverage WHERE singleton = 1", [])
                .map_err(|error| {
                    store_error("failed to mark full-text search coverage stale", error)
                })?;
        }
    }
    Ok(AppendOutcome {
        event_id: result_id,
        resulting_version: next_seq,
        disposition: AppendDisposition::Applied,
    })
}

fn aggregate_version_in_transaction(
    transaction: &Transaction<'_>,
    aggregate: &p::RunId,
) -> p::Result<u64> {
    let value = transaction
        .query_row(
            "SELECT COALESCE(MAX(stream_seq), 0) FROM events WHERE run_id = ?1",
            params![&aggregate.0],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| store_error("failed to calculate aggregate version", error))?;
    u64::try_from(value).map_err(|_| p::Error("stored aggregate version is invalid".into()))
}

fn federation_version_in_transaction(
    connection: &Connection,
    aggregate: &p::FederationAggregateRef,
) -> p::Result<u64> {
    let value = connection
        .query_row(
            "SELECT version FROM federation_versions WHERE aggregate = ?1",
            params![&aggregate.0],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| store_error("failed to read federation version", error))?
        .unwrap_or(0);
    u64::try_from(value).map_err(|_| p::Error("stored federation version is invalid".into()))
}

fn authority_epoch_in_transaction(connection: &Connection) -> p::Result<p::AuthorityEpoch> {
    let value = connection
        .query_row(
            "SELECT authority_epoch FROM federation_state WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| store_error("failed to read authority epoch", error))?
        .unwrap_or(0);
    Ok(p::AuthorityEpoch(u64::try_from(value).map_err(|_| {
        p::Error("stored authority epoch is invalid".into())
    })?))
}

fn federation_control_version(event: &p::Event) -> p::Result<&p::FederationAggregateVersion> {
    match &event.payload {
        p::EventPayload::FederatedPeerRegistered(payload) => Ok(&payload.committed_version),
        p::EventPayload::FederatedPeerRevoked(payload) => Ok(&payload.committed_version),
        p::EventPayload::RemoteExecutionLeaseChanged(payload) => Ok(&payload.committed_version),
        p::EventPayload::ReplicationCheckpointAdvanced(payload) => Ok(&payload.committed_version),
        _ => Err(p::Error(
            "federation expected append only accepts federation control events".into(),
        )),
    }
}

fn ecosystem_version_in_connection(
    connection: &Connection,
    aggregate: &p::EcosystemAggregateRef,
) -> p::Result<u64> {
    let value = connection
        .query_row(
            "SELECT version FROM ecosystem_versions WHERE aggregate = ?1",
            params![&aggregate.0],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| store_error("failed to read ecosystem version", error))?
        .unwrap_or(0);
    u64::try_from(value).map_err(|_| p::Error("stored ecosystem version is invalid".into()))
}

fn ecosystem_control_version(event: &p::Event) -> p::Result<&p::EcosystemAggregateVersion> {
    match &event.payload {
        p::EventPayload::CapabilityPublisherChanged(payload) => Ok(&payload.committed_version),
        p::EventPayload::CapabilityPackageAdmitted(payload) => Ok(&payload.committed_version),
        p::EventPayload::CapabilityPackageStateChanged(payload) => Ok(&payload.committed_version),
        p::EventPayload::CapabilityPackageDistributionRecorded(payload) => {
            Ok(&payload.committed_version)
        }
        _ => Err(p::Error(
            "ecosystem expected append only accepts ecosystem control events".into(),
        )),
    }
}

fn validate_ecosystem_control_event(
    event: &p::Event,
    expected: &p::EcosystemAggregateVersion,
) -> p::Result<()> {
    event.validate_payload_kind()?;
    let committed = ecosystem_control_version(event)?;
    committed.validate()?;
    if committed.value != expected.value.checked_add(1).unwrap_or(0) {
        return Err(p::Error(
            "ecosystem event version does not match expected append".into(),
        ));
    }
    match &event.payload {
        p::EventPayload::CapabilityPublisherChanged(payload) => {
            payload.grant.validate()?;
            if event.provenance.source != p::Source::OwnerControl
                || !matches!(event.provenance.actor, p::Actor::Owner)
                || event.provenance.trust_tier != p::TrustTier::OwnerInput
            {
                return Err(p::Error(
                    "publisher mutation requires authenticated owner control".into(),
                ));
            }
        }
        p::EventPayload::CapabilityPackageAdmitted(payload) => {
            payload.admission.validate()?;
            if event.provenance.source != p::Source::Internal
                || !matches!(event.provenance.actor, p::Actor::System)
                || event.provenance.trust_tier != p::TrustTier::VerifiedProcess
            {
                return Err(p::Error(
                    "package admission requires authority verification provenance".into(),
                ));
            }
        }
        p::EventPayload::CapabilityPackageStateChanged(payload) => {
            payload.change.validate()?;
            if event.provenance.source != p::Source::OwnerControl
                || !matches!(event.provenance.actor, p::Actor::Owner)
                || event.provenance.trust_tier != p::TrustTier::OwnerInput
            {
                return Err(p::Error(
                    "package lifecycle mutation requires authenticated owner control".into(),
                ));
            }
        }
        p::EventPayload::CapabilityPackageDistributionRecorded(payload) => {
            payload.receipt.validate()?;
            if event.provenance.source != p::Source::Internal
                || !matches!(event.provenance.actor, p::Actor::System)
                || event.provenance.trust_tier != p::TrustTier::VerifiedProcess
            {
                return Err(p::Error(
                    "package distribution fact requires authority verification provenance".into(),
                ));
            }
        }
        _ => unreachable!("ecosystem kind checked above"),
    }
    Ok(())
}

fn validate_ecosystem_transition(connection: &Connection, event: &p::Event) -> p::Result<()> {
    match &event.payload {
        p::EventPayload::CapabilityPublisherChanged(payload) => {
            let existing = read_ecosystem_json::<p::CapabilityPublisherGrant>(
                connection,
                "SELECT grant FROM ecosystem_publishers WHERE publisher_ref = ?1",
                &payload.grant.publisher.0,
                "capability publisher grant",
                p::CapabilityPublisherGrant::validate,
            )?;
            match existing {
                None => {
                    if payload.previous.is_some()
                        || payload.grant.version.0 != 1
                        || payload.grant.status != p::CapabilityPublisherStatus::Active
                    {
                        return Err(p::Error("new publisher grant has stale lineage".into()));
                    }
                }
                Some(existing) => {
                    if existing.status == p::CapabilityPublisherStatus::Revoked
                        || payload.previous.as_ref() != Some(&existing.reference)
                        || payload.grant.version.0 != existing.version.0.checked_add(1).unwrap_or(0)
                    {
                        return Err(p::Error("publisher grant lineage is invalid".into()));
                    }
                }
            }
        }
        p::EventPayload::CapabilityPackageAdmitted(payload) => {
            let admission = &payload.admission;
            if connection
                .query_row(
                    "SELECT 1 FROM ecosystem_admissions WHERE release_ref = ?1 OR admission_ref = ?2",
                    params![&admission.release.0, &admission.reference.0],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|error| store_error("failed to inspect package admission", error))?
                .is_some()
            {
                return Err(p::Error(
                    "package release or admission identity is already committed".into(),
                ));
            }
            let publisher = read_publisher_by_grant(connection, &admission.publisher_grant)?
                .ok_or_else(|| p::Error("package admission publisher is not provisioned".into()))?;
            if publisher.status != p::CapabilityPublisherStatus::Active
                || publisher.version != admission.publisher_version
                || publisher.expires_at <= admission.admitted_at
                || admission.admitted_at > event.ts_unix_ms
            {
                return Err(p::Error(
                    "package admission publisher grant is inactive or stale".into(),
                ));
            }
            for dependency in &admission.dependencies {
                let admitted = read_ecosystem_json::<p::CapabilityPackageAdmission>(
                    connection,
                    "SELECT admission FROM ecosystem_admissions WHERE release_ref = ?1",
                    &dependency.release.0,
                    "package dependency admission",
                    p::CapabilityPackageAdmission::validate,
                )?
                .ok_or_else(|| p::Error("package dependency is not admitted".into()))?;
                if admitted.package != dependency.package
                    || admitted.package_digest != dependency.digest
                {
                    return Err(p::Error(
                        "package dependency admission binding does not match".into(),
                    ));
                }
            }
        }
        p::EventPayload::CapabilityPackageStateChanged(payload) => {
            let change = &payload.change;
            let admission = read_ecosystem_json::<p::CapabilityPackageAdmission>(
                connection,
                "SELECT admission FROM ecosystem_admissions WHERE release_ref = ?1",
                &change.release.0,
                "capability package admission",
                p::CapabilityPackageAdmission::validate,
            )?
            .ok_or_else(|| p::Error("package lifecycle release is not admitted".into()))?;
            if admission.package != change.package {
                return Err(p::Error(
                    "package lifecycle release belongs to another package".into(),
                ));
            }
            let publisher = read_publisher_by_grant(connection, &admission.publisher_grant)?
                .ok_or_else(|| p::Error("package lifecycle publisher is missing".into()))?;
            if publisher.status != p::CapabilityPublisherStatus::Active
                && change.to != p::CapabilityLifecycleState::Revoked
            {
                return Err(p::Error(
                    "revoked publisher fenced package lifecycle".into(),
                ));
            }
            let current = read_ecosystem_json::<p::CapabilityPackageState>(
                connection,
                "SELECT state FROM ecosystem_package_states WHERE package_ref = ?1",
                &change.package.0,
                "capability package state",
                p::CapabilityPackageState::validate,
            )?;
            let (from, generation) = current
                .as_ref()
                .map(|state| (state.lifecycle, state.active_generation))
                .unwrap_or((p::CapabilityLifecycleState::Admitted, 0));
            if from == p::CapabilityLifecycleState::Revoked
                || change.from != from
                || change.active_generation != generation.checked_add(1).unwrap_or(0)
            {
                return Err(p::Error(
                    "package lifecycle transition lost state or generation CAS".into(),
                ));
            }
            if let Some(current) = current {
                let release_changed = current.release != change.release;
                if release_changed
                    && !(change.from == p::CapabilityLifecycleState::Enabled
                        && change.to == p::CapabilityLifecycleState::Enabled)
                {
                    return Err(p::Error(
                        "package release can only switch through an approved update or rollback"
                            .into(),
                    ));
                }
            }
        }
        p::EventPayload::CapabilityPackageDistributionRecorded(payload) => {
            let receipt = &payload.receipt;
            let semantic = serde_json::to_vec(receipt)
                .map_err(|error| store_error("failed to encode distribution receipt", error))?;
            let attempt = connection
                .query_row(
                    "SELECT semantic FROM ecosystem_distribution_attempts WHERE receipt_ref = ?1",
                    params![&receipt.reference.0],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(|error| store_error("failed to inspect distribution attempt", error))?;
            if attempt.as_deref() != Some(semantic.as_slice()) {
                return Err(p::Error(
                    "verified distribution has no matching durable attempt".into(),
                ));
            }
            if connection
                .query_row(
                    "SELECT 1 FROM ecosystem_distributions WHERE receipt_ref = ?1",
                    params![&receipt.reference.0],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|error| store_error("failed to inspect distribution fact", error))?
                .is_some()
            {
                return Err(p::Error("distribution receipt is already committed".into()));
            }
            let admission = read_ecosystem_json::<p::CapabilityPackageAdmission>(
                connection,
                "SELECT admission FROM ecosystem_admissions WHERE release_ref = ?1",
                &receipt.release.0,
                "distributed package admission",
                p::CapabilityPackageAdmission::validate,
            )?
            .ok_or_else(|| p::Error("distributed package is not admitted".into()))?;
            if admission.package != receipt.package
                || admission.package_digest != receipt.package_digest
            {
                return Err(p::Error(
                    "distribution does not match package admission".into(),
                ));
            }
            let publisher = read_publisher_by_grant(connection, &admission.publisher_grant)?
                .ok_or_else(|| p::Error("distribution publisher is missing".into()))?;
            if publisher.status != p::CapabilityPublisherStatus::Active {
                return Err(p::Error(
                    "revoked publisher fenced package distribution".into(),
                ));
            }
            let state = read_ecosystem_json::<p::CapabilityPackageState>(
                connection,
                "SELECT state FROM ecosystem_package_states WHERE package_ref = ?1",
                &receipt.package.0,
                "distributed package state",
                p::CapabilityPackageState::validate,
            )?;
            if state
                .as_ref()
                .is_some_and(|state| state.lifecycle == p::CapabilityLifecycleState::Revoked)
            {
                return Err(p::Error("revoked package cannot be distributed".into()));
            }
            let peer = read_peer_grant(connection, &receipt.peer)?
                .ok_or_else(|| p::Error("distribution executor is not registered".into()))?;
            let epoch = authority_epoch_in_transaction(connection)?;
            let lease = read_ecosystem_json::<p::RemoteExecutionLease>(
                connection,
                "SELECT lease FROM remote_leases WHERE lease_ref = ?1",
                &receipt.lease.0,
                "distribution remote lease",
                p::RemoteExecutionLease::validate,
            )?
            .ok_or_else(|| p::Error("distribution lease is not authoritative".into()))?;
            if peer.revoked
                || !peer.grant.roles.contains(&p::FederatedPeerRole::Executor)
                || peer.grant.reference()? != receipt.peer_grant
                || epoch != receipt.authority_epoch
                || lease.executor != receipt.peer
                || lease.peer_grant != receipt.peer_grant
                || lease.authority_epoch != receipt.authority_epoch
                || lease.plan_digest != receipt.plan_digest
                || lease.fence.0 != receipt.fence_token
                || lease.state != p::RemoteLeaseState::Acquired
            {
                return Err(p::Error(
                    "distribution receipt failed its M4 authority binding".into(),
                ));
            }
        }
        _ => unreachable!("ecosystem kind checked before transition"),
    }
    Ok(())
}

fn update_ecosystem_projection(transaction: &Transaction<'_>, event: &p::Event) -> p::Result<()> {
    match &event.payload {
        p::EventPayload::CapabilityPublisherChanged(payload) => {
            let encoded = serde_json::to_vec(&payload.grant)
                .map_err(|error| store_error("failed to encode capability publisher", error))?;
            transaction
                .execute(
                    "INSERT INTO ecosystem_publishers(
                        publisher_ref, grant_ref, grant_version, scope, status,
                        expires_at, grant, last_event_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(publisher_ref) DO UPDATE SET
                        grant_ref = excluded.grant_ref,
                        grant_version = excluded.grant_version,
                        scope = excluded.scope,
                        status = excluded.status,
                        expires_at = excluded.expires_at,
                        grant = excluded.grant,
                        last_event_id = excluded.last_event_id",
                    params![
                        &payload.grant.publisher.0,
                        &payload.grant.reference.0,
                        i64::from(payload.grant.version.0),
                        &payload.grant.scope.0,
                        publisher_status_name(payload.grant.status),
                        payload.grant.expires_at,
                        encoded,
                        &event.event_id.0,
                    ],
                )
                .map_err(|error| store_error("failed to update capability publisher", error))?;
        }
        p::EventPayload::CapabilityPackageAdmitted(payload) => {
            let publisher =
                read_publisher_by_grant(transaction, &payload.admission.publisher_grant)?
                    .ok_or_else(|| p::Error("admission publisher disappeared".into()))?;
            let encoded = serde_json::to_vec(&payload.admission)
                .map_err(|error| store_error("failed to encode package admission", error))?;
            transaction
                .execute(
                    "INSERT INTO ecosystem_admissions(
                        release_ref, admission_ref, package_ref, package_digest,
                        scope, admission, last_event_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        &payload.admission.release.0,
                        &payload.admission.reference.0,
                        &payload.admission.package.0,
                        &payload.admission.package_digest.0,
                        &publisher.scope.0,
                        encoded,
                        &event.event_id.0,
                    ],
                )
                .map_err(|error| store_error("failed to persist package admission", error))?;
        }
        p::EventPayload::CapabilityPackageStateChanged(payload) => {
            let admission = read_ecosystem_json::<p::CapabilityPackageAdmission>(
                transaction,
                "SELECT admission FROM ecosystem_admissions WHERE release_ref = ?1",
                &payload.change.release.0,
                "package state admission",
                p::CapabilityPackageAdmission::validate,
            )?
            .ok_or_else(|| p::Error("package state admission disappeared".into()))?;
            let publisher = read_publisher_by_grant(transaction, &admission.publisher_grant)?
                .ok_or_else(|| p::Error("package state publisher disappeared".into()))?;
            let state = p::CapabilityPackageState {
                schema_version: p::M5_SCHEMA_VERSION,
                package: payload.change.package.clone(),
                release: payload.change.release.clone(),
                package_digest: admission.package_digest,
                lifecycle: payload.change.to,
                active_generation: payload.change.active_generation,
                plan: Some(payload.change.plan.clone()),
                approval: Some(payload.change.approval.clone()),
            };
            state.validate()?;
            let encoded = serde_json::to_vec(&state)
                .map_err(|error| store_error("failed to encode package state", error))?;
            transaction
                .execute(
                    "INSERT INTO ecosystem_package_states(
                        package_ref, release_ref, package_digest, scope, lifecycle,
                        active_generation, state, last_event_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(package_ref) DO UPDATE SET
                        release_ref = excluded.release_ref,
                        package_digest = excluded.package_digest,
                        scope = excluded.scope,
                        lifecycle = excluded.lifecycle,
                        active_generation = excluded.active_generation,
                        state = excluded.state,
                        last_event_id = excluded.last_event_id",
                    params![
                        &state.package.0,
                        &state.release.0,
                        &state.package_digest.0,
                        &publisher.scope.0,
                        lifecycle_state_name(state.lifecycle),
                        i64::try_from(state.active_generation)
                            .map_err(|_| p::Error("active generation is too large".into()))?,
                        encoded,
                        &event.event_id.0,
                    ],
                )
                .map_err(|error| store_error("failed to update package state", error))?;
        }
        p::EventPayload::CapabilityPackageDistributionRecorded(payload) => {
            let admission = read_ecosystem_json::<p::CapabilityPackageAdmission>(
                transaction,
                "SELECT admission FROM ecosystem_admissions WHERE release_ref = ?1",
                &payload.receipt.release.0,
                "distribution admission",
                p::CapabilityPackageAdmission::validate,
            )?
            .ok_or_else(|| p::Error("distribution admission disappeared".into()))?;
            let publisher = read_publisher_by_grant(transaction, &admission.publisher_grant)?
                .ok_or_else(|| p::Error("distribution publisher disappeared".into()))?;
            let encoded = serde_json::to_vec(&payload.receipt)
                .map_err(|error| store_error("failed to encode distribution receipt", error))?;
            transaction
                .execute(
                    "INSERT INTO ecosystem_distributions(
                        receipt_ref, package_ref, release_ref, scope, peer_ref,
                        receipt, last_event_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        &payload.receipt.reference.0,
                        &payload.receipt.package.0,
                        &payload.receipt.release.0,
                        &publisher.scope.0,
                        &payload.receipt.peer.0,
                        encoded,
                        &event.event_id.0,
                    ],
                )
                .map_err(|error| store_error("failed to persist distribution fact", error))?;
        }
        _ => unreachable!("ecosystem kind checked before projection update"),
    }
    Ok(())
}

fn read_publisher_by_grant(
    connection: &Connection,
    grant: &p::CapabilityPublisherGrantRef,
) -> p::Result<Option<p::CapabilityPublisherGrant>> {
    read_ecosystem_json(
        connection,
        "SELECT grant FROM ecosystem_publishers WHERE grant_ref = ?1",
        &grant.0,
        "capability publisher grant",
        p::CapabilityPublisherGrant::validate,
    )
}

fn publisher_status_name(status: p::CapabilityPublisherStatus) -> &'static str {
    match status {
        p::CapabilityPublisherStatus::Active => "active",
        p::CapabilityPublisherStatus::Revoked => "revoked",
    }
}

fn lifecycle_state_name(state: p::CapabilityLifecycleState) -> &'static str {
    match state {
        p::CapabilityLifecycleState::Quarantined => "quarantined",
        p::CapabilityLifecycleState::Admitted => "admitted",
        p::CapabilityLifecycleState::Installed => "installed",
        p::CapabilityLifecycleState::Enabled => "enabled",
        p::CapabilityLifecycleState::Disabled => "disabled",
        p::CapabilityLifecycleState::Revoked => "revoked",
    }
}

fn ecosystem_history_contains(
    connection: &Connection,
    aggregate: &p::EcosystemAggregateRef,
    event: &p::EventId,
) -> p::Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM ecosystem_history WHERE aggregate = ?1 AND event_id = ?2",
            params![&aggregate.0, &event.0],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| store_error("failed to inspect ecosystem history", error))
}

fn validate_federation_control_event(
    event: &p::Event,
    aggregate: &p::FederationAggregateRef,
    expected: &p::FederationAggregateVersion,
    has_ack: bool,
) -> p::Result<()> {
    event.validate_payload_kind()?;
    let committed = federation_control_version(event)?;
    committed.validate()?;
    if committed.aggregate != *aggregate
        || committed.version != expected.version.checked_add(1).unwrap_or(0)
    {
        return Err(p::Error(
            "federation event version fields do not match the expected append".into(),
        ));
    }
    let trusted = matches!(
        event.provenance.trust_tier,
        p::TrustTier::OwnerInput | p::TrustTier::VerifiedProcess
    );
    if !trusted {
        return Err(p::Error(
            "federation control event requires authority provenance".into(),
        ));
    }
    match &event.payload {
        p::EventPayload::FederatedPeerRegistered(payload) => {
            payload.grant.validate()?;
            if has_ack
                || event.provenance.source != p::Source::OwnerControl
                || !matches!(event.provenance.actor, p::Actor::Owner)
                || event.provenance.trust_tier != p::TrustTier::OwnerInput
            {
                return Err(p::Error(
                    "peer registration requires authenticated owner control".into(),
                ));
            }
        }
        p::EventPayload::FederatedPeerRevoked(payload) => {
            if has_ack
                || payload.peer.0.trim().is_empty()
                || payload.revoked_grant.0.trim().is_empty()
                || payload.new_authority_epoch.0 == 0
                || event.provenance.source != p::Source::OwnerControl
                || !matches!(event.provenance.actor, p::Actor::Owner)
                || event.provenance.trust_tier != p::TrustTier::OwnerInput
            {
                return Err(p::Error(
                    "peer revocation requires authenticated owner control".into(),
                ));
            }
        }
        p::EventPayload::RemoteExecutionLeaseChanged(payload) => {
            if has_ack || payload.reason.0.trim().is_empty() {
                return Err(p::Error("remote lease event is incomplete".into()));
            }
            payload.lease.validate()?;
        }
        p::EventPayload::ReplicationCheckpointAdvanced(payload) => {
            if !has_ack
                || payload.peer.0.trim().is_empty()
                || payload.aggregate.0.trim().is_empty()
                || payload.from_stream_seq >= payload.to_stream_seq
                || payload.batch_digest.0.trim().is_empty()
                || payload.redaction.0.trim().is_empty()
                || payload.authority_epoch.0 == 0
            {
                return Err(p::Error(
                    "replication checkpoint requires a valid acknowledgement".into(),
                ));
            }
        }
        _ => unreachable!("federation kind checked above"),
    }
    Ok(())
}

fn validate_federation_transition(
    connection: &Connection,
    event: &p::Event,
    ack: Option<&p::ReplicationAck>,
) -> p::Result<()> {
    let current_epoch = authority_epoch_in_transaction(connection)?;
    match &event.payload {
        p::EventPayload::FederatedPeerRegistered(payload) => {
            if payload.grant.authority_epoch != current_epoch.next()? {
                return Err(p::Error(
                    "peer registration epoch is not the next epoch".into(),
                ));
            }
            let existing = read_peer_grant(connection, &payload.grant.peer)?;
            match existing {
                None => {
                    if payload.previous.is_some() || payload.grant.grant_version.0 != 1 {
                        return Err(p::Error("new peer registration has stale lineage".into()));
                    }
                }
                Some(existing) => {
                    let previous = existing.grant.reference()?;
                    if existing.revoked
                        || payload.previous.as_ref() != Some(&previous)
                        || payload.grant.grant_version.0
                            != existing.grant.grant_version.0.checked_add(1).unwrap_or(0)
                    {
                        return Err(p::Error("peer grant update lineage is invalid".into()));
                    }
                }
            }
        }
        p::EventPayload::FederatedPeerRevoked(payload) => {
            let existing = read_peer_grant(connection, &payload.peer)?
                .ok_or_else(|| p::Error("cannot revoke an unknown peer".into()))?;
            if existing.revoked
                || existing.grant.reference()? != payload.revoked_grant
                || payload.new_authority_epoch != current_epoch.next()?
            {
                return Err(p::Error("peer revocation lineage is invalid".into()));
            }
        }
        p::EventPayload::RemoteExecutionLeaseChanged(payload) => {
            let lease = &payload.lease;
            if lease.authority_epoch != current_epoch || current_epoch.0 == 0 {
                return Err(p::Error("remote lease uses a stale authority epoch".into()));
            }
            let peer = read_peer_grant(connection, &lease.executor)?
                .ok_or_else(|| p::Error("remote executor peer is not registered".into()))?;
            if peer.revoked
                || !peer.grant.roles.contains(&p::FederatedPeerRole::Executor)
                || peer.grant.reference()? != lease.peer_grant
                || peer.grant.grant_version != lease.grant_version
            {
                return Err(p::Error("remote executor grant is not active".into()));
            }
            let existing = read_remote_lease(connection, &lease.lease)?;
            match existing {
                None if lease.state == p::RemoteLeaseState::Acquired => {}
                Some(previous)
                    if !previous.state.terminal()
                        && lease.state.terminal()
                        && same_lease_binding(&previous, lease) => {}
                _ => {
                    return Err(p::Error(
                        "remote lease transition is not monotonic or one-shot".into(),
                    ));
                }
            }
        }
        p::EventPayload::ReplicationCheckpointAdvanced(payload) => {
            let ack = ack.ok_or_else(|| p::Error("replication ack is missing".into()))?;
            let bytes = connection
                .query_row(
                    "SELECT semantic_bytes FROM replication_exports WHERE batch_ref = ?1",
                    params![&ack.batch.0],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()
                .map_err(|error| store_error("failed to read acknowledged export", error))?
                .ok_or_else(|| p::Error("replication acknowledgement has no export".into()))?;
            let batch: p::ReplicationBatch = serde_json::from_slice(&bytes)
                .map_err(|error| store_error("failed to decode acknowledged export", error))?;
            if ack.peer != batch.peer
                || ack.aggregate != batch.aggregate
                || ack.applied != batch.to
                || payload.peer != batch.peer
                || payload.aggregate != batch.aggregate
                || payload.from_stream_seq != batch.from.stream_seq
                || payload.to_stream_seq != batch.to.stream_seq
                || payload.batch_digest != batch.content_digest
                || payload.redaction != batch.redaction
                || payload.authority_epoch != batch.to.authority_epoch
                || payload.authority_epoch != current_epoch
            {
                return Err(p::Error(
                    "replication acknowledgement binding is invalid".into(),
                ));
            }
            let current = replication_cursor_in_transaction(
                connection,
                &batch.peer,
                &batch.aggregate,
                current_epoch,
            )?;
            if current.stream_seq != batch.from.stream_seq {
                return Err(p::Error("replication checkpoint cursor has a gap".into()));
            }
        }
        _ => unreachable!("federation kind checked before transition"),
    }
    Ok(())
}

fn update_federation_projection(
    transaction: &Transaction<'_>,
    event: &p::Event,
    ack: Option<&p::ReplicationAck>,
) -> p::Result<()> {
    match &event.payload {
        p::EventPayload::FederatedPeerRegistered(payload) => {
            let grant_ref = payload.grant.reference()?;
            let bytes = serde_json::to_vec(&payload.grant)
                .map_err(|error| store_error("failed to encode peer grant", error))?;
            transaction
                .execute(
                    "INSERT INTO federated_peers(
                        peer_ref, grant_ref, grant_version, authority_epoch,
                        revoked, grant, last_event_id
                     ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)
                     ON CONFLICT(peer_ref) DO UPDATE SET
                        grant_ref = excluded.grant_ref,
                        grant_version = excluded.grant_version,
                        authority_epoch = excluded.authority_epoch,
                        revoked = 0,
                        grant = excluded.grant,
                        last_event_id = excluded.last_event_id",
                    params![
                        &payload.grant.peer.0,
                        &grant_ref.0,
                        i64::try_from(payload.grant.grant_version.0)
                            .map_err(|_| p::Error("grant version is too large".into()))?,
                        i64::try_from(payload.grant.authority_epoch.0)
                            .map_err(|_| p::Error("authority epoch is too large".into()))?,
                        bytes,
                        &event.event_id.0,
                    ],
                )
                .map_err(|error| store_error("failed to update peer registry", error))?;
            set_authority_epoch(transaction, payload.grant.authority_epoch)?;
            fence_old_epoch_leases(transaction, payload.grant.authority_epoch)?;
        }
        p::EventPayload::FederatedPeerRevoked(payload) => {
            transaction
                .execute(
                    "UPDATE federated_peers SET revoked = 1, last_event_id = ?2
                     WHERE peer_ref = ?1",
                    params![&payload.peer.0, &event.event_id.0],
                )
                .map_err(|error| store_error("failed to revoke peer", error))?;
            set_authority_epoch(transaction, payload.new_authority_epoch)?;
            fence_old_epoch_leases(transaction, payload.new_authority_epoch)?;
        }
        p::EventPayload::RemoteExecutionLeaseChanged(payload) => {
            let bytes = serde_json::to_vec(&payload.lease)
                .map_err(|error| store_error("failed to encode remote lease", error))?;
            transaction
                .execute(
                    "INSERT INTO remote_leases(
                        lease_ref, dispatch_id, executor_peer, grant_ref,
                        authority_epoch, fence_token, state, lease, last_event_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                     ON CONFLICT(lease_ref) DO UPDATE SET
                        state = excluded.state,
                        lease = excluded.lease,
                        last_event_id = excluded.last_event_id",
                    params![
                        &payload.lease.lease.0,
                        &payload.lease.dispatch.0,
                        &payload.lease.executor.0,
                        &payload.lease.peer_grant.0,
                        i64::try_from(payload.lease.authority_epoch.0)
                            .map_err(|_| p::Error("lease epoch is too large".into()))?,
                        i64::try_from(payload.lease.fence.0)
                            .map_err(|_| p::Error("fence token is too large".into()))?,
                        remote_lease_state_name(payload.lease.state),
                        bytes,
                        &event.event_id.0,
                    ],
                )
                .map_err(|error| store_error("failed to update remote lease", error))?;
            if payload.lease.state == p::RemoteLeaseState::Acquired {
                transaction
                    .execute(
                        "INSERT INTO remote_dispatches(
                            dispatch_id, lease_ref, plan_digest, attempted
                         ) VALUES (?1, ?2, ?3, 0)",
                        params![
                            &payload.lease.dispatch.0,
                            &payload.lease.lease.0,
                            &payload.lease.plan_digest.0,
                        ],
                    )
                    .map_err(|error| store_error("failed to reserve remote dispatch", error))?;
            }
        }
        p::EventPayload::ReplicationCheckpointAdvanced(payload) => {
            let ack = ack.ok_or_else(|| p::Error("replication ack is missing".into()))?;
            transaction
                .execute(
                    "UPDATE replication_exports SET acknowledged = 1 WHERE batch_ref = ?1",
                    params![&ack.batch.0],
                )
                .map_err(|error| store_error("failed to acknowledge replication export", error))?;
            transaction
                .execute(
                    "INSERT INTO replication_checkpoints(
                        peer_ref, aggregate, stream_seq, authority_epoch,
                        batch_ref, batch_digest, projection_digest
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(peer_ref, aggregate) DO UPDATE SET
                        stream_seq = excluded.stream_seq,
                        authority_epoch = excluded.authority_epoch,
                        batch_ref = excluded.batch_ref,
                        batch_digest = excluded.batch_digest,
                        projection_digest = excluded.projection_digest",
                    params![
                        &payload.peer.0,
                        &payload.aggregate.0,
                        i64::try_from(payload.to_stream_seq)
                            .map_err(|_| p::Error("replication cursor is too large".into()))?,
                        i64::try_from(payload.authority_epoch.0)
                            .map_err(|_| p::Error("authority epoch is too large".into()))?,
                        &ack.batch.0,
                        &payload.batch_digest.0,
                        &ack.projection_digest.0,
                    ],
                )
                .map_err(|error| store_error("failed to advance replication checkpoint", error))?;
        }
        _ => unreachable!("federation kind checked before projection update"),
    }
    Ok(())
}

fn set_authority_epoch(transaction: &Transaction<'_>, epoch: p::AuthorityEpoch) -> p::Result<()> {
    transaction
        .execute(
            "INSERT INTO federation_state(singleton, authority_epoch) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET authority_epoch = excluded.authority_epoch",
            params![i64::try_from(epoch.0)
                .map_err(|_| p::Error("authority epoch is too large".into()))?],
        )
        .map_err(|error| store_error("failed to advance authority epoch", error))?;
    Ok(())
}

fn fence_old_epoch_leases(
    transaction: &Transaction<'_>,
    current: p::AuthorityEpoch,
) -> p::Result<()> {
    let leases = {
        let mut statement = transaction
            .prepare(
                "SELECT lease_ref, lease FROM remote_leases
                 WHERE state IN ('reserved', 'acquired') AND authority_epoch < ?1",
            )
            .map_err(|error| store_error("failed to prepare lease fencing", error))?;
        let rows = statement
            .query_map(
                params![i64::try_from(current.0)
                    .map_err(|_| p::Error("authority epoch is too large".into()))?],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .map_err(|error| store_error("failed to read leases for fencing", error))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| store_error("failed to materialize leases for fencing", error))?
    };
    for (lease_ref, bytes) in leases {
        let mut lease: p::RemoteExecutionLease = serde_json::from_slice(&bytes)
            .map_err(|error| store_error("failed to decode lease for fencing", error))?;
        lease.state = p::RemoteLeaseState::Fenced;
        let encoded = serde_json::to_vec(&lease)
            .map_err(|error| store_error("failed to encode fenced lease", error))?;
        transaction
            .execute(
                "UPDATE remote_leases SET state = 'fenced', lease = ?2 WHERE lease_ref = ?1",
                params![lease_ref, encoded],
            )
            .map_err(|error| store_error("failed to fence remote lease", error))?;
    }
    Ok(())
}

fn read_peer_grant(
    connection: &Connection,
    peer: &p::FederatedPeerRef,
) -> p::Result<Option<p::FederatedPeerState>> {
    connection
        .query_row(
            "SELECT grant, revoked FROM federated_peers WHERE peer_ref = ?1",
            params![&peer.0],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|error| store_error("failed to read peer registry", error))?
        .map(|(bytes, revoked)| {
            let grant = serde_json::from_slice::<p::FederatedPeerGrant>(&bytes)
                .map_err(|error| store_error("failed to decode peer registry", error))?;
            Ok(p::FederatedPeerState {
                schema_version: p::M4_SCHEMA_VERSION,
                grant,
                revoked: revoked != 0,
                last_seen_at: None,
            })
        })
        .transpose()
}

fn read_remote_lease(
    connection: &Connection,
    lease: &p::RemoteExecutionLeaseRef,
) -> p::Result<Option<p::RemoteExecutionLease>> {
    connection
        .query_row(
            "SELECT lease FROM remote_leases WHERE lease_ref = ?1",
            params![&lease.0],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(|error| store_error("failed to read remote lease", error))?
        .map(|bytes| {
            serde_json::from_slice(&bytes)
                .map_err(|error| store_error("failed to decode remote lease", error))
        })
        .transpose()
}

fn same_lease_binding(left: &p::RemoteExecutionLease, right: &p::RemoteExecutionLease) -> bool {
    left.schema_version == right.schema_version
        && left.lease == right.lease
        && left.dispatch == right.dispatch
        && left.intent == right.intent
        && left.plan_digest == right.plan_digest
        && left.placement == right.placement
        && left.executor == right.executor
        && left.peer_grant == right.peer_grant
        && left.grant_version == right.grant_version
        && left.authority_epoch == right.authority_epoch
        && left.fence == right.fence
        && left.expires_at == right.expires_at
}

fn remote_lease_state_name(state: p::RemoteLeaseState) -> &'static str {
    match state {
        p::RemoteLeaseState::Reserved => "reserved",
        p::RemoteLeaseState::Acquired => "acquired",
        p::RemoteLeaseState::Released => "released",
        p::RemoteLeaseState::Expired => "expired",
        p::RemoteLeaseState::Fenced => "fenced",
    }
}

fn federation_history_contains(
    connection: &Connection,
    aggregate: &p::FederationAggregateRef,
    event: &p::EventId,
) -> p::Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM federation_history WHERE aggregate = ?1 AND event_id = ?2",
            params![&aggregate.0, &event.0],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| store_error("failed to inspect federation history", error))
}

fn replication_cursor_in_transaction(
    connection: &Connection,
    peer: &p::FederatedPeerRef,
    aggregate: &p::RunId,
    current_epoch: p::AuthorityEpoch,
) -> p::Result<p::ReplicationCursor> {
    let stream_seq = connection
        .query_row(
            "SELECT stream_seq FROM replication_checkpoints
             WHERE peer_ref = ?1 AND aggregate = ?2",
            params![&peer.0, &aggregate.0],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| store_error("failed to read replication checkpoint", error))?
        .unwrap_or(0);
    Ok(p::ReplicationCursor {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: peer.clone(),
        aggregate: aggregate.clone(),
        stream_seq: u64::try_from(stream_seq)
            .map_err(|_| p::Error("stored replication cursor is invalid".into()))?,
        authority_epoch: current_epoch,
    })
}

fn replication_transfer_event(event: p::Event) -> p::Result<p::SyncTransferEvent> {
    let kind = event.kind;
    let sensitive = matches!(
        kind,
        p::EventKind::RunAccepted
            | p::EventKind::ModelCallDelta
            | p::EventKind::ToolCallProposed
            | p::EventKind::ActionOutputDelta
            | p::EventKind::FederatedPeerRegistered
            | p::EventKind::FederatedPeerRevoked
            | p::EventKind::RemoteExecutionLeaseChanged
            | p::EventKind::ReplicationCheckpointAdvanced
            | p::EventKind::CapabilityPublisherChanged
            | p::EventKind::CapabilityPackageAdmitted
            | p::EventKind::CapabilityPackageStateChanged
            | p::EventKind::CapabilityPackageDistributionRecorded
    );
    let payload = if sensitive {
        p::SyncTransferPayload::Redacted {
            kind,
            reason: p::ReasonRef("authority redaction profile".into()),
            digest: p::canonical_digest(&event.payload)?,
        }
    } else {
        p::SyncTransferPayload::Full(Box::new(event.payload))
    };
    Ok(p::SyncTransferEvent {
        schema_version: p::M4_SCHEMA_VERSION,
        event_id: event.event_id,
        aggregate: event.run_id,
        source_stream_seq: event.stream_seq,
        turn_id: event.turn_id,
        kind,
        payload,
        event_schema_version: event.schema_version,
        ts_unix_ms: event.ts_unix_ms,
        provenance: event.provenance,
    })
}

fn current_time_ms() -> p::Result<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| p::Error("system clock is before the Unix epoch".into()))?
        .as_millis();
    i64::try_from(millis).map_err(|_| p::Error("system clock exceeds timestamp range".into()))
}

fn reject_non_evolution_write_path(event: &p::Event) -> p::Result<()> {
    if matches!(
        event.kind,
        p::EventKind::StrategyActivated
            | p::EventKind::StrategyRolledBack
            | p::EventKind::FederatedPeerRegistered
            | p::EventKind::FederatedPeerRevoked
            | p::EventKind::RemoteExecutionLeaseChanged
            | p::EventKind::ReplicationCheckpointAdvanced
            | p::EventKind::CapabilityPublisherChanged
            | p::EventKind::CapabilityPackageAdmitted
            | p::EventKind::CapabilityPackageStateChanged
            | p::EventKind::CapabilityPackageDistributionRecorded
    ) {
        return Err(p::Error(
            "control events require their dedicated expected-version append".into(),
        ));
    }
    Ok(())
}

fn evolution_control_versions(
    event: &p::Event,
) -> p::Result<(
    &p::EvolutionAggregateRef,
    &p::EvolutionAggregateVersion,
    &p::EvolutionAggregateVersion,
)> {
    match &event.payload {
        p::EventPayload::StrategyActivated(payload) => {
            payload.activation.validate()?;
            Ok((
                &payload.activation.aggregate,
                &payload.activation.expected_version,
                &payload.activation.committed_version,
            ))
        }
        p::EventPayload::StrategyRolledBack(payload) => {
            payload.rollback.validate()?;
            Ok((
                &payload.rollback.aggregate,
                &payload.rollback.expected_version,
                &payload.rollback.committed_version,
            ))
        }
        _ => Err(p::Error(
            "evolution expected-version append only accepts activation or rollback events".into(),
        )),
    }
}

fn validate_evolution_control_provenance(event: &p::Event) -> p::Result<()> {
    match &event.payload {
        p::EventPayload::StrategyActivated(payload) => {
            let owner_required = matches!(
                payload.activation.impact,
                p::EvolutionImpact::Bounded | p::EvolutionImpact::Expansive
            );
            if owner_required && !is_owner_evolution_provenance(&event.provenance) {
                return Err(p::Error(
                    "bounded or expansive activation requires owner-authenticated provenance"
                        .into(),
                ));
            }
            if !owner_required && !is_trusted_evolution_provenance(&event.provenance) {
                return Err(p::Error(
                    "automatic cautious activation requires verified process provenance".into(),
                ));
            }
        }
        p::EventPayload::StrategyRolledBack(_) => {
            if !is_trusted_evolution_provenance(&event.provenance) {
                return Err(p::Error(
                    "strategy rollback requires owner or verified process provenance".into(),
                ));
            }
        }
        _ => {
            return Err(p::Error(
                "non-control event has no evolution control provenance".into(),
            ));
        }
    }
    Ok(())
}

fn update_evolution_catalog(transaction: &Transaction<'_>, event: &p::Event) -> p::Result<()> {
    match &event.payload {
        p::EventPayload::CandidateCreated(payload) => {
            let Some(candidate) = payload.strategy_candidate.as_ref() else {
                return Ok(());
            };
            candidate.validate()?;
            validate_evolution_fact_provenance(&event.provenance)?;
            if candidate.candidate != payload.candidate_id
                || candidate.target_tier != payload.target_tier
                || candidate.evidence != payload.evidence_refs
                || candidate.provenance != payload.provenance
                || candidate.provenance != event.provenance
            {
                return Err(p::Error(
                    "strategy candidate envelope does not match its verified candidate facts"
                        .into(),
                ));
            }
            let encoded = serde_json::to_vec(candidate)
                .map_err(|error| store_error("failed to encode strategy candidate", error))?;
            if let Some((created_event, stored)) = transaction
                .query_row(
                    "SELECT created_event_id, candidate FROM strategy_candidates
                     WHERE candidate_id = ?1",
                    params![&candidate.candidate.0],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .optional()
                .map_err(|error| store_error("failed to inspect strategy candidate", error))?
            {
                if created_event != event.event_id.0 || stored != encoded {
                    return Err(p::Error(format!(
                        "conflicting strategy candidate {}",
                        candidate.candidate.0
                    )));
                }
                return Ok(());
            }
            validate_strategy_candidate_identity(transaction, candidate)?;
            transaction
                .execute(
                    "INSERT INTO strategy_candidates(candidate_id, created_event_id, candidate)
                     VALUES (?1, ?2, ?3)",
                    params![&candidate.candidate.0, &event.event_id.0, encoded],
                )
                .map_err(|error| store_error("failed to project strategy candidate", error))?;
        }
        p::EventPayload::EvolutionEvaluationRecorded(payload) => {
            validate_evolution_fact_provenance(&event.provenance)?;
            if payload.evaluation.0.trim().is_empty()
                || payload.baseline.0.trim().is_empty()
                || payload.candidate.0.trim().is_empty()
                || payload.baseline == payload.candidate
                || payload.hard_invariants.is_empty()
                || payload
                    .hard_invariants
                    .iter()
                    .any(|reference| reference.0.trim().is_empty())
                || payload.ground_truth.is_empty()
                || payload
                    .ground_truth
                    .iter()
                    .any(|reference| reference.0.trim().is_empty())
            {
                return Err(p::Error(
                    "evolution evaluation summary is incomplete".into(),
                ));
            }
            let encoded = serde_json::to_vec(payload)
                .map_err(|error| store_error("failed to encode evolution evaluation", error))?;
            if let Some((stored_event, stored)) = transaction
                .query_row(
                    "SELECT event_id, evaluation FROM evolution_evaluations
                     WHERE evaluation_ref = ?1",
                    params![&payload.evaluation.0],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .optional()
                .map_err(|error| store_error("failed to inspect evolution evaluation", error))?
            {
                if stored_event != event.event_id.0 || stored != encoded {
                    return Err(p::Error(format!(
                        "conflicting evolution evaluation {}",
                        payload.evaluation.0
                    )));
                }
                return Ok(());
            }
            transaction
                .execute(
                    "INSERT INTO evolution_evaluations(evaluation_ref, event_id, evaluation)
                     VALUES (?1, ?2, ?3)",
                    params![&payload.evaluation.0, &event.event_id.0, encoded],
                )
                .map_err(|error| store_error("failed to project evolution evaluation", error))?;
        }
        p::EventPayload::CandidatePromoted(payload) => {
            let Some((created_event, candidate_bytes)) = transaction
                .query_row(
                    "SELECT created_event_id, candidate FROM strategy_candidates
                     WHERE candidate_id = ?1",
                    params![&payload.candidate_id.0],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
                )
                .optional()
                .map_err(|error| store_error("failed to resolve promoted candidate", error))?
            else {
                return Ok(());
            };
            validate_evolution_fact_provenance(&event.provenance)?;
            if (payload.by == p::DecisionActor::User
                && (!matches!(event.provenance.actor, p::Actor::Owner)
                    || event.provenance.trust_tier != p::TrustTier::OwnerInput))
                || (payload.by == p::DecisionActor::Auto
                    && event.provenance.trust_tier != p::TrustTier::VerifiedProcess)
            {
                return Err(p::Error(
                    "strategy promotion actor does not match its verified provenance".into(),
                ));
            }
            let candidate = serde_json::from_slice::<p::StrategyCandidate>(&candidate_bytes)
                .map_err(|error| store_error("failed to decode promoted strategy", error))?;
            candidate.validate()?;
            if !has_passing_evolution_evaluation(transaction, &candidate)? {
                return Err(p::Error(
                    "strategy promotion requires a recorded passing evaluation".into(),
                ));
            }
            if let Some((stored_created, stored_promotion, stored_candidate)) = transaction
                .query_row(
                    "SELECT created_event_id, promotion_event_id, candidate
                     FROM stable_strategies WHERE candidate_id = ?1",
                    params![&payload.candidate_id.0],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|error| store_error("failed to inspect stable strategy", error))?
            {
                if stored_created != created_event
                    || stored_promotion != event.event_id.0
                    || stored_candidate != candidate_bytes
                {
                    return Err(p::Error(format!(
                        "conflicting stable strategy promotion {}",
                        payload.candidate_id.0
                    )));
                }
                return Ok(());
            }
            transaction
                .execute(
                    "INSERT INTO stable_strategies(
                        candidate_id, created_event_id, promotion_event_id, candidate
                     ) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        &payload.candidate_id.0,
                        created_event,
                        &event.event_id.0,
                        candidate_bytes,
                    ],
                )
                .map_err(|error| store_error("failed to project stable strategy", error))?;
        }
        _ => {}
    }
    Ok(())
}

fn strategy_domain_name(domain: p::StrategyDomain) -> &'static str {
    match domain {
        p::StrategyDomain::Loop => "loop",
        p::StrategyDomain::Coordination => "coordination",
        p::StrategyDomain::CapabilitySelection => "capability-selection",
        p::StrategyDomain::ModelSelection => "model-selection",
        p::StrategyDomain::BackendSelection => "backend-selection",
        p::StrategyDomain::ModelAdaptation => "model-adaptation",
        p::StrategyDomain::StrategyMemory => "strategy-memory",
        p::StrategyDomain::AgentSelf => "agent-self",
        p::StrategyDomain::Partnership => "partnership",
        p::StrategyDomain::TrustDelegation => "trust-delegation",
        p::StrategyDomain::Proactivity => "proactivity",
        p::StrategyDomain::Communication => "communication",
    }
}

fn read_stable_strategy(
    connection: &Connection,
    domain: p::StrategyDomain,
    scope: &p::Scope,
    version: &p::StrategyVersionRef,
) -> p::Result<Option<StableStrategyRecord>> {
    let mut statement = connection
        .prepare("SELECT created_event_id, promotion_event_id, candidate FROM stable_strategies")
        .map_err(|error| store_error("failed to prepare stable strategy lookup", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(|error| store_error("failed to query stable strategies", error))?;
    let mut matched = None;
    for row in rows {
        let (created_event, promotion_event, candidate) =
            row.map_err(|error| store_error("failed to read stable strategy", error))?;
        let candidate = serde_json::from_slice::<p::StrategyCandidate>(&candidate)
            .map_err(|error| store_error("failed to decode stable strategy", error))?;
        candidate.validate()?;
        if candidate.domain == domain
            && candidate.scope == *scope
            && candidate.proposed_version == *version
        {
            if matched.is_some() {
                return Err(p::Error(
                    "stable strategy lookup is ambiguous for domain, scope, and version".into(),
                ));
            }
            matched = Some(StableStrategyRecord {
                candidate,
                created_event: p::EventId(created_event),
                promotion_event: p::EventId(promotion_event),
            });
        }
    }
    Ok(matched)
}

fn validate_evolution_fact_provenance(provenance: &p::Provenance) -> p::Result<()> {
    if !is_trusted_evolution_provenance(provenance) {
        return Err(p::Error(
            "evolution catalog facts require owner or verified-process provenance".into(),
        ));
    }
    Ok(())
}

fn validate_strategy_candidate_identity(
    transaction: &Transaction<'_>,
    candidate: &p::StrategyCandidate,
) -> p::Result<()> {
    let mut statement = transaction
        .prepare("SELECT candidate FROM strategy_candidates")
        .map_err(|error| store_error("failed to inspect strategy version identity", error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|error| store_error("failed to read strategy version identities", error))?;
    for row in rows {
        let encoded =
            row.map_err(|error| store_error("failed to read strategy version identity", error))?;
        let existing = serde_json::from_slice::<p::StrategyCandidate>(&encoded)
            .map_err(|error| store_error("failed to decode strategy version identity", error))?;
        existing.validate()?;
        if existing.proposed_version != candidate.proposed_version {
            continue;
        }
        if existing.domain != candidate.domain
            || existing.spec_ref != candidate.spec_ref
            || existing.spec_digest != candidate.spec_digest
        {
            return Err(p::Error(format!(
                "strategy version {} is already bound to different immutable content",
                candidate.proposed_version.0
            )));
        }
        if existing.scope == candidate.scope {
            return Err(p::Error(format!(
                "strategy version {} already has a candidate in this scope",
                candidate.proposed_version.0
            )));
        }
    }
    Ok(())
}

fn is_trusted_evolution_provenance(provenance: &p::Provenance) -> bool {
    is_owner_evolution_provenance(provenance)
        || (matches!(provenance.actor, p::Actor::System)
            && provenance.trust_tier == p::TrustTier::VerifiedProcess)
}

fn is_owner_evolution_provenance(provenance: &p::Provenance) -> bool {
    matches!(provenance.actor, p::Actor::Owner) && provenance.trust_tier == p::TrustTier::OwnerInput
}

fn has_passing_evolution_evaluation(
    transaction: &Transaction<'_>,
    candidate: &p::StrategyCandidate,
) -> p::Result<bool> {
    let mut statement = transaction
        .prepare("SELECT evaluation FROM evolution_evaluations")
        .map_err(|error| store_error("failed to inspect strategy evaluations", error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|error| store_error("failed to read strategy evaluations", error))?;
    for row in rows {
        let encoded =
            row.map_err(|error| store_error("failed to read strategy evaluation", error))?;
        let evaluation = serde_json::from_slice::<p::EvolutionEvaluationRecordedPayload>(&encoded)
            .map_err(|error| store_error("failed to decode strategy evaluation", error))?;
        if evaluation.verdict == p::EvaluationVerdict::Pass
            && evaluation.baseline == candidate.baseline
            && evaluation.candidate == candidate.proposed_version
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_evolution_evaluation(
    connection: &Connection,
    evaluation: &p::EvolutionEvaluationRef,
) -> p::Result<Option<p::EvolutionEvaluationRecordedPayload>> {
    connection
        .query_row(
            "SELECT evaluation FROM evolution_evaluations WHERE evaluation_ref = ?1",
            params![&evaluation.0],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(|error| store_error("failed to read evolution evaluation", error))?
        .map(|encoded| {
            serde_json::from_slice(&encoded)
                .map_err(|error| store_error("failed to decode evolution evaluation", error))
        })
        .transpose()
}

fn read_active_strategy(
    connection: &Connection,
    aggregate: &p::EvolutionAggregateRef,
    domain: p::StrategyDomain,
    scope: &p::Scope,
) -> p::Result<Option<p::ActiveStrategyRef>> {
    connection
        .query_row(
            "SELECT active FROM active_strategies
             WHERE aggregate = ?1 AND domain = ?2 AND scope = ?3",
            params![&aggregate.0, strategy_domain_name(domain), &scope.0],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(|error| store_error("failed to read active strategy", error))?
        .map(|encoded| {
            let active = serde_json::from_slice::<p::ActiveStrategyRef>(&encoded)
                .map_err(|error| store_error("failed to decode active strategy", error))?;
            active.validate()?;
            Ok(active)
        })
        .transpose()
}

fn read_active_strategies(
    connection: &Connection,
    domain: Option<p::StrategyDomain>,
    scope: &p::Scope,
    aggregate: Option<&p::EvolutionAggregateRef>,
) -> p::Result<Vec<p::ActiveStrategyRef>> {
    let mut statement = connection
        .prepare("SELECT active FROM active_strategies")
        .map_err(|error| store_error("failed to prepare active strategy scan", error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|error| store_error("failed to query active strategies", error))?;
    let mut active = Vec::new();
    for row in rows {
        let encoded = row.map_err(|error| store_error("failed to read active strategy", error))?;
        let strategy = serde_json::from_slice::<p::ActiveStrategyRef>(&encoded)
            .map_err(|error| store_error("failed to decode active strategy", error))?;
        strategy.validate()?;
        if domain.is_none_or(|expected| expected == strategy.domain)
            && aggregate.is_none_or(|expected| *expected == strategy.aggregate)
            && scope_contains(&strategy.scope, scope)
        {
            active.push(strategy);
        }
    }
    active.sort_by(|left, right| {
        (
            &left.aggregate,
            left.domain,
            &left.scope,
            &left.version,
            &left.activation_event,
        )
            .cmp(&(
                &right.aggregate,
                right.domain,
                &right.scope,
                &right.version,
                &right.activation_event,
            ))
    });
    Ok(active)
}

fn scope_contains(granted: &p::Scope, requested: &p::Scope) -> bool {
    requested.0 == granted.0
        || requested
            .0
            .strip_prefix(&granted.0)
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with(':'))
}

fn evolution_version_in_transaction(
    connection: &Connection,
    aggregate: &p::EvolutionAggregateRef,
) -> p::Result<u64> {
    let value = connection
        .query_row(
            "SELECT version FROM evolution_versions WHERE aggregate = ?1",
            params![&aggregate.0],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| store_error("failed to read evolution aggregate version", error))?
        .unwrap_or(0);
    u64::try_from(value)
        .map_err(|_| p::Error("stored evolution aggregate version is invalid".into()))
}

fn evolution_history_contains(
    connection: &Connection,
    aggregate: &p::EvolutionAggregateRef,
    event: &p::EventId,
) -> p::Result<bool> {
    connection
        .query_row(
            "SELECT 1 FROM evolution_history WHERE aggregate = ?1 AND event_id = ?2",
            params![&aggregate.0, &event.0],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| store_error("failed to inspect evolution history", error))
}

fn validate_evolution_transition(connection: &Connection, event: &p::Event) -> p::Result<()> {
    validate_evolution_control_provenance(event)?;
    match &event.payload {
        p::EventPayload::StrategyActivated(payload) => {
            let activation = &payload.activation;
            activation.validate()?;
            let stable = read_stable_strategy(
                connection,
                activation.domain,
                &activation.scope,
                &activation.to,
            )?
            .ok_or_else(|| {
                p::Error("activation target is not a promoted stable strategy".into())
            })?;
            if stable.promotion_event != activation.promotion
                || stable.candidate.spec_ref != activation.spec_ref
                || stable.candidate.spec_digest != activation.spec_digest
            {
                return Err(p::Error(
                    "activation target does not match the promoted immutable strategy".into(),
                ));
            }
            let evaluation = read_evolution_evaluation(connection, &activation.evaluation)?
                .ok_or_else(|| p::Error("activation evaluation is not recorded".into()))?;
            if evaluation.verdict != p::EvaluationVerdict::Pass
                || evaluation.candidate != activation.to
                || activation
                    .from
                    .as_ref()
                    .is_some_and(|baseline| evaluation.baseline != *baseline)
            {
                return Err(p::Error(
                    "activation evaluation does not prove the promoted transition".into(),
                ));
            }
            let current = read_active_strategy(
                connection,
                &activation.aggregate,
                activation.domain,
                &activation.scope,
            )?;
            if current.as_ref().map(|active| &active.version) != activation.from.as_ref() {
                return Err(p::Error(
                    "activation from-version does not match the active projection".into(),
                ));
            }
        }
        p::EventPayload::StrategyRolledBack(payload) => {
            let rollback = &payload.rollback;
            rollback.validate()?;
            let current = read_active_strategy(
                connection,
                &rollback.aggregate,
                rollback.domain,
                &rollback.scope,
            )?
            .ok_or_else(|| p::Error("rollback has no active strategy to replace".into()))?;
            if current.version != rollback.failed {
                return Err(p::Error(
                    "rollback failed-version does not match the active projection".into(),
                ));
            }
            let restored = read_stable_strategy(
                connection,
                rollback.domain,
                &rollback.scope,
                &rollback.restored,
            )?
            .ok_or_else(|| p::Error("rollback target is not a known stable strategy".into()))?;
            if restored.candidate.spec_ref != rollback.restored_spec_ref
                || restored.candidate.spec_digest != rollback.restored_spec_digest
            {
                return Err(p::Error(
                    "rollback target does not match its immutable stable strategy".into(),
                ));
            }
        }
        _ => {
            return Err(p::Error(
                "non-control event cannot change evolution projection".into(),
            ));
        }
    }
    Ok(())
}

fn apply_evolution_control_event(
    transaction: &Transaction<'_>,
    event: &p::Event,
    verify_snapshot: bool,
) -> p::Result<()> {
    let (aggregate, committed_version, active, scope, expected_snapshot) = match &event.payload {
        p::EventPayload::StrategyActivated(payload) => {
            let activation = &payload.activation;
            (
                &activation.aggregate,
                activation.committed_version.value,
                p::ActiveStrategyRef {
                    schema_version: p::SchemaVersion(1),
                    id: p::ActiveStrategyId(format!("active:{}", event.event_id.0)),
                    aggregate: activation.aggregate.clone(),
                    domain: activation.domain,
                    scope: activation.scope.clone(),
                    version: activation.to.clone(),
                    spec_ref: activation.spec_ref.clone(),
                    spec_digest: activation.spec_digest.clone(),
                    activation_event: event.event_id.clone(),
                },
                &activation.scope,
                &payload.active_snapshot,
            )
        }
        p::EventPayload::StrategyRolledBack(payload) => {
            let rollback = &payload.rollback;
            (
                &rollback.aggregate,
                rollback.committed_version.value,
                p::ActiveStrategyRef {
                    schema_version: p::SchemaVersion(1),
                    id: p::ActiveStrategyId(format!("active:{}", event.event_id.0)),
                    aggregate: rollback.aggregate.clone(),
                    domain: rollback.domain,
                    scope: rollback.scope.clone(),
                    version: rollback.restored.clone(),
                    spec_ref: rollback.restored_spec_ref.clone(),
                    spec_digest: rollback.restored_spec_digest.clone(),
                    activation_event: event.event_id.clone(),
                },
                &rollback.scope,
                &payload.active_snapshot,
            )
        }
        _ => {
            return Err(p::Error(
                "non-control event cannot update evolution projection".into(),
            ));
        }
    };
    active.validate()?;
    let encoded = serde_json::to_vec(&active)
        .map_err(|error| store_error("failed to encode active strategy", error))?;
    let committed_i64 = i64::try_from(committed_version)
        .map_err(|_| p::Error("evolution aggregate version is too large".into()))?;
    transaction
        .execute(
            "INSERT INTO evolution_versions(aggregate, version) VALUES (?1, ?2)
             ON CONFLICT(aggregate) DO UPDATE SET version = excluded.version",
            params![&aggregate.0, committed_i64],
        )
        .map_err(|error| store_error("failed to advance evolution aggregate version", error))?;
    transaction
        .execute(
            "INSERT INTO active_strategies(
                aggregate, domain, scope, aggregate_version, activation_event, active
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(aggregate, domain, scope) DO UPDATE SET
                aggregate_version = excluded.aggregate_version,
                activation_event = excluded.activation_event,
                active = excluded.active",
            params![
                &aggregate.0,
                strategy_domain_name(active.domain),
                &active.scope.0,
                committed_i64,
                &event.event_id.0,
                &encoded,
            ],
        )
        .map_err(|error| store_error("failed to update active strategy", error))?;
    transaction
        .execute(
            "INSERT INTO evolution_history(
                aggregate, committed_version, event_id, kind, active
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &aggregate.0,
                committed_i64,
                &event.event_id.0,
                event.kind.as_str(),
                &encoded,
            ],
        )
        .map_err(|error| store_error("failed to append evolution history", error))?;

    if verify_snapshot {
        let snapshot = evolution_snapshot_in_transaction(transaction, scope, Some(aggregate))?;
        if snapshot.snapshot != *expected_snapshot {
            return Err(p::Error(
                "evolution control event active snapshot does not match committed projection"
                    .into(),
            ));
        }
    }
    Ok(())
}

fn evolution_snapshot_in_transaction(
    connection: &Connection,
    scope: &p::Scope,
    aggregate: Option<&p::EvolutionAggregateRef>,
) -> p::Result<p::EvolutionSnapshot> {
    let strategies = read_active_strategies(connection, None, scope, aggregate)?;
    if strategies.is_empty() {
        return Err(p::Error(
            "no active strategy projection exists for the requested scope".into(),
        ));
    }
    let mut aggregate_refs = strategies
        .iter()
        .map(|strategy| strategy.aggregate.clone())
        .collect::<Vec<_>>();
    aggregate_refs.sort();
    aggregate_refs.dedup();
    let mut aggregates = Vec::with_capacity(aggregate_refs.len());
    for reference in aggregate_refs {
        aggregates.push(p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            value: evolution_version_in_transaction(connection, &reference)?,
            aggregate: reference,
        });
    }
    build_evolution_snapshot(aggregates, strategies)
}

fn preview_evolution_snapshot_in_transaction(
    connection: &Connection,
    event: &p::Event,
) -> p::Result<p::EvolutionSnapshot> {
    let (aggregate, committed, domain, scope, version, spec_ref, spec_digest) = match &event.payload
    {
        p::EventPayload::StrategyActivated(payload) => (
            &payload.activation.aggregate,
            payload.activation.committed_version.value,
            payload.activation.domain,
            &payload.activation.scope,
            &payload.activation.to,
            &payload.activation.spec_ref,
            &payload.activation.spec_digest,
        ),
        p::EventPayload::StrategyRolledBack(payload) => (
            &payload.rollback.aggregate,
            payload.rollback.committed_version.value,
            payload.rollback.domain,
            &payload.rollback.scope,
            &payload.rollback.restored,
            &payload.rollback.restored_spec_ref,
            &payload.rollback.restored_spec_digest,
        ),
        _ => {
            return Err(p::Error(
                "non-control event cannot preview an evolution snapshot".into(),
            ));
        }
    };
    let mut strategies = read_active_strategies(connection, None, scope, Some(aggregate))?;
    strategies.retain(|strategy| strategy.domain != domain || strategy.scope != *scope);
    strategies.push(p::ActiveStrategyRef {
        schema_version: p::SchemaVersion(1),
        id: p::ActiveStrategyId(format!("active:{}", event.event_id.0)),
        aggregate: aggregate.clone(),
        domain,
        scope: scope.clone(),
        version: version.clone(),
        spec_ref: spec_ref.clone(),
        spec_digest: spec_digest.clone(),
        activation_event: event.event_id.clone(),
    });
    strategies.sort_by(|left, right| {
        (
            &left.aggregate,
            left.domain,
            &left.scope,
            &left.version,
            &left.activation_event,
        )
            .cmp(&(
                &right.aggregate,
                right.domain,
                &right.scope,
                &right.version,
                &right.activation_event,
            ))
    });
    build_evolution_snapshot(
        vec![p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: aggregate.clone(),
            value: committed,
        }],
        strategies,
    )
}

fn build_evolution_snapshot(
    aggregates: Vec<p::EvolutionAggregateVersion>,
    strategies: Vec<p::ActiveStrategyRef>,
) -> p::Result<p::EvolutionSnapshot> {
    let identity = serde_json::to_vec(&(&aggregates, &strategies))
        .map_err(|error| store_error("failed to encode evolution snapshot identity", error))?;
    let digest = checksum::calculate_bytes(&identity);
    let snapshot = p::EvolutionSnapshot {
        schema_version: p::SchemaVersion(1),
        snapshot: p::EvolutionSnapshotRef(format!("evolution-snapshot:fnv64:{digest}")),
        aggregates,
        strategies,
        digest: p::SchemaDigest(format!("fnv64:{digest}")),
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn read_evolution_history(
    connection: &Connection,
    aggregate: &p::EvolutionAggregateRef,
) -> p::Result<Vec<EvolutionHistoryEntry>> {
    let mut statement = connection
        .prepare(
            "SELECT committed_version, event_id, kind, active
             FROM evolution_history WHERE aggregate = ?1 ORDER BY committed_version",
        )
        .map_err(|error| store_error("failed to prepare evolution history", error))?;
    let rows = statement
        .query_map(params![&aggregate.0], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })
        .map_err(|error| store_error("failed to query evolution history", error))?;
    let mut history = Vec::new();
    for row in rows {
        let (committed_version, event_id, kind, active) =
            row.map_err(|error| store_error("failed to read evolution history", error))?;
        let committed_version = u64::try_from(committed_version)
            .map_err(|_| p::Error("stored evolution history version is invalid".into()))?;
        if committed_version != history.len() as u64 + 1 {
            return Err(p::Error(
                "evolution history contains a non-contiguous aggregate version".into(),
            ));
        }
        let kind = kind.parse::<p::EventKind>()?;
        if !matches!(
            kind,
            p::EventKind::StrategyActivated | p::EventKind::StrategyRolledBack
        ) {
            return Err(p::Error(
                "evolution history contains a non-control event".into(),
            ));
        }
        let active = serde_json::from_slice::<p::ActiveStrategyRef>(&active)
            .map_err(|error| store_error("failed to decode evolution history", error))?;
        active.validate()?;
        history.push(EvolutionHistoryEntry {
            aggregate: aggregate.clone(),
            committed_version,
            event_id: p::EventId(event_id),
            kind,
            active,
        });
    }
    Ok(history)
}

fn rebuild_evolution_projection_in_transaction(
    transaction: &Transaction<'_>,
    current_schema: &BTreeMap<PayloadType, p::SchemaVersion>,
    upcasters: &UpcasterGraph,
) -> p::Result<()> {
    let events = read_all_stored_events(transaction)?
        .into_iter()
        .map(|event| event.decode(current_schema, upcasters))
        .collect::<p::Result<Vec<_>>>()?;
    transaction
        .execute_batch(
            "DELETE FROM active_strategies;
             DELETE FROM evolution_history;
             DELETE FROM evolution_versions;
             DELETE FROM stable_strategies;
             DELETE FROM evolution_evaluations;
             DELETE FROM strategy_candidates;",
        )
        .map_err(|error| store_error("failed to clear evolution projections", error))?;

    for event in events
        .iter()
        .filter(|event| matches!(event.kind, p::EventKind::CandidateCreated))
    {
        update_evolution_catalog(transaction, event)?;
    }
    for event in events
        .iter()
        .filter(|event| matches!(event.kind, p::EventKind::EvolutionEvaluationRecorded))
    {
        update_evolution_catalog(transaction, event)?;
    }
    for event in events
        .iter()
        .filter(|event| matches!(event.kind, p::EventKind::CandidatePromoted))
    {
        update_evolution_catalog(transaction, event)?;
    }

    let mut control_events = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                p::EventKind::StrategyActivated | p::EventKind::StrategyRolledBack
            )
        })
        .collect::<Vec<_>>();
    control_events.sort_by(|left, right| {
        let (left_aggregate, _, left_committed) =
            evolution_control_versions(left).expect("filtered control event has versions");
        let (right_aggregate, _, right_committed) =
            evolution_control_versions(right).expect("filtered control event has versions");
        (&left_aggregate.0, left_committed.value).cmp(&(&right_aggregate.0, right_committed.value))
    });
    for event in control_events {
        let (aggregate, expected, committed) = evolution_control_versions(event)?;
        let actual = evolution_version_in_transaction(transaction, aggregate)?;
        if expected.value != actual || committed.value != actual.checked_add(1).unwrap_or(0) {
            return Err(p::Error(format!(
                "evolution history for {} is not contiguous at committed version {}",
                aggregate.0, committed.value
            )));
        }
        validate_evolution_transition(transaction, event)?;
        apply_evolution_control_event(transaction, event, true)?;
    }
    Ok(())
}

fn sync_batch_semantic_bytes(batch: &p::SyncWriteBatch) -> p::Result<Vec<u8>> {
    serde_json::to_vec(batch)
        .map_err(|error| store_error("failed to encode sync batch semantics", error))
}

fn sync_peer_semantic_bytes(peer: &p::SyncPeer) -> p::Result<Vec<u8>> {
    serde_json::to_vec(peer)
        .map_err(|error| store_error("failed to encode sync peer semantics", error))
}

fn configure_sync_peer(transaction: &Transaction<'_>, peer: Option<&p::SyncPeer>) -> p::Result<()> {
    let Some(peer) = peer else {
        return Ok(());
    };
    peer.validate()?;
    let semantics = sync_peer_semantic_bytes(peer)?;
    let existing = transaction
        .query_row(
            "SELECT peer_ref, owner_ref, semantic_bytes FROM sync_peers WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| store_error("failed to inspect configured sync peer", error))?;
    if let Some((peer_ref, owner_ref, stored_semantics)) = existing {
        if peer_ref != peer.reference.0
            || owner_ref != peer.owner.0
            || stored_semantics != semantics
        {
            return Err(p::Error(
                "only the configured owner-bound sync peer may use this store".into(),
            ));
        }
        return Ok(());
    }
    transaction
        .execute(
            "INSERT INTO sync_peers(singleton, peer_ref, owner_ref, semantic_bytes)
             VALUES (1, ?1, ?2, ?3)",
            params![&peer.reference.0, &peer.owner.0, semantics],
        )
        .map_err(|error| store_error("failed to configure sync peer", error))?;
    Ok(())
}

fn verify_sync_peer(transaction: &Transaction<'_>, peer: &p::SyncPeer) -> p::Result<()> {
    peer.validate()?;
    let semantics = sync_peer_semantic_bytes(peer)?;
    let existing = transaction
        .query_row(
            "SELECT peer_ref, owner_ref, semantic_bytes FROM sync_peers WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| store_error("failed to inspect configured sync peer", error))?;
    match existing {
        Some((peer_ref, owner_ref, stored_semantics))
            if peer_ref == peer.reference.0
                && owner_ref == peer.owner.0
                && stored_semantics == semantics =>
        {
            Ok(())
        }
        _ => Err(p::Error(
            "sync peer is not the preconfigured owner-bound peer".into(),
        )),
    }
}

fn update_sync_cursor(
    transaction: &Transaction<'_>,
    peer: &p::SyncPeerRef,
    aggregate: &p::RunId,
    applied: Option<u64>,
    exported: Option<u64>,
) -> p::Result<()> {
    let current = transaction
        .query_row(
            "SELECT applied_version, exported_version
             FROM sync_cursors WHERE peer_ref = ?1 AND aggregate = ?2",
            params![&peer.0, &aggregate.0],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|error| store_error("failed to inspect sync cursor", error))?;
    let (current_applied, current_exported) = match current {
        Some((applied, exported)) => (
            u64::try_from(applied)
                .map_err(|_| p::Error("stored applied sync cursor is invalid".into()))?,
            u64::try_from(exported)
                .map_err(|_| p::Error("stored export sync cursor is invalid".into()))?,
        ),
        None => (0, 0),
    };
    let next_applied = applied.map_or(current_applied, |value| current_applied.max(value));
    let next_exported = exported.map_or(current_exported, |value| current_exported.max(value));
    transaction
        .execute(
            "INSERT INTO sync_cursors(peer_ref, aggregate, applied_version, exported_version)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(peer_ref, aggregate) DO UPDATE SET
                applied_version = excluded.applied_version,
                exported_version = excluded.exported_version",
            params![
                &peer.0,
                &aggregate.0,
                i64::try_from(next_applied)
                    .map_err(|_| p::Error("applied sync cursor is too large".into()))?,
                i64::try_from(next_exported)
                    .map_err(|_| p::Error("export sync cursor is too large".into()))?,
            ],
        )
        .map_err(|error| store_error("failed to update sync cursor", error))?;
    Ok(())
}

fn sync_event_allowed(peer: &p::SyncPeer, event: &p::Event) -> bool {
    sync_event_scope(event).is_none_or(|scope| {
        peer.allowed_scopes
            .iter()
            .any(|allowed| sync_scope_contains(allowed, &scope))
    })
}

fn sync_event_scope(event: &p::Event) -> Option<p::Scope> {
    match &event.payload {
        p::EventPayload::ApprovalRequested(payload) => Some(payload.scope.clone()),
        p::EventPayload::ActionPlanned(payload) => Some(payload.scope.clone()),
        p::EventPayload::ActionStarted(payload) => Some(payload.scope.clone()),
        p::EventPayload::ActionOutputDelta(payload) => Some(payload.scope.clone()),
        p::EventPayload::FailureEvidenceRecorded(payload) => Some(payload.scope.clone()),
        p::EventPayload::ObservationRecorded(payload) => Some(payload.scope.clone()),
        p::EventPayload::CommunicationEventReceived(payload) => Some(payload.scope.clone()),
        p::EventPayload::MemoryNodeAppended(payload) => Some(payload.scope.clone()),
        p::EventPayload::UserAttributeCandidateCreated(payload) => Some(payload.scope.clone()),
        p::EventPayload::GoalFramed(payload) => {
            payload.long_term.as_ref().map(|goal| goal.scope.clone())
        }
        p::EventPayload::CandidateCreated(payload) => payload
            .capability_update
            .as_ref()
            .map(|proposal| proposal.gap.scope.clone()),
        p::EventPayload::SessionBound(payload) => Some(p::Scope(payload.workspace.0.clone())),
        _ => None,
    }
}

fn sync_scope_contains(granted: &p::Scope, requested: &p::Scope) -> bool {
    requested.0 == granted.0
        || requested
            .0
            .strip_prefix(&granted.0)
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with(':'))
}

fn sync_redaction_reason(event: &p::Event, request: &p::SyncExportRequest) -> Option<&'static str> {
    if !sync_event_allowed(&request.peer, event) {
        return Some("event scope omitted by sync grant");
    }
    if matches!(
        event.kind,
        p::EventKind::RunAccepted
            | p::EventKind::ModelCallDelta
            | p::EventKind::ToolCallProposed
            | p::EventKind::ActionOutputDelta
    ) {
        return Some("raw or action payload omitted by sync policy");
    }
    let Ok(value) = serde_json::to_value(&event.payload) else {
        return Some("uninspectable payload omitted by sync policy");
    };
    payload_contains_sensitive_data(&value, &request.redaction)
        .then_some("sensitive payload omitted by sync policy")
}

fn payload_contains_sensitive_data(
    value: &serde_json::Value,
    policy: &p::SyncRedactionPolicy,
) -> bool {
    const SENSITIVE_KEYS: &[&str] = &[
        "api_key",
        "api-key",
        "authorization",
        "credential",
        "credential_ref",
        "password",
        "secret",
        "secret_ref",
        "secrets",
        "secret_bindings",
        "access_token",
        "refresh_token",
        "token",
    ];
    match value {
        serde_json::Value::Object(fields) => fields.iter().any(|(key, value)| {
            let lower = key.to_ascii_lowercase();
            SENSITIVE_KEYS.contains(&lower.as_str())
                || policy
                    .forbidden_keys
                    .iter()
                    .any(|forbidden| forbidden.eq_ignore_ascii_case(key))
                || payload_contains_sensitive_data(value, policy)
        }),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| payload_contains_sensitive_data(value, policy)),
        serde_json::Value::String(value) => sync_string_contains_sensitive_marker(value, policy),
        _ => false,
    }
}

fn sync_peer_contains_sensitive_identity(
    peer: &p::SyncPeer,
    policy: &p::SyncRedactionPolicy,
) -> bool {
    sync_string_contains_sensitive_marker(&peer.reference.0, policy)
        || sync_string_contains_sensitive_marker(&peer.owner.0, policy)
        || peer
            .allowed_scopes
            .iter()
            .any(|scope| sync_string_contains_sensitive_marker(&scope.0, policy))
}

fn sync_event_contains_sensitive_identity(
    event: &p::Event,
    policy: &p::SyncRedactionPolicy,
) -> bool {
    sync_string_contains_sensitive_marker(&event.event_id.0, policy)
        || sync_string_contains_sensitive_marker(&event.run_id.0, policy)
        || event
            .turn_id
            .as_ref()
            .is_some_and(|turn| sync_string_contains_sensitive_marker(&turn.0, policy))
        || event
            .provenance
            .caused_by
            .as_ref()
            .is_some_and(|cause| sync_string_contains_sensitive_marker(&cause.0, policy))
        || match &event.provenance.actor {
            p::Actor::Subagent(run) => sync_string_contains_sensitive_marker(&run.0, policy),
            p::Actor::External(participant) => {
                sync_string_contains_sensitive_marker(&participant.0, policy)
            }
            _ => false,
        }
}

fn sync_string_contains_sensitive_marker(value: &str, policy: &p::SyncRedactionPolicy) -> bool {
    const SENSITIVE_MARKERS: &[&str] = &[
        "secret:",
        "credential:",
        "bearer ",
        "authorization:",
        "api_key=",
        "api-key=",
    ];
    let lower = value.to_ascii_lowercase();
    (policy.forbid_secret_refs
        && SENSITIVE_MARKERS
            .iter()
            .any(|marker| lower.contains(marker)))
        || policy
            .forbidden_value_markers
            .iter()
            .any(|marker| !marker.is_empty() && lower.contains(&marker.to_ascii_lowercase()))
}

fn sync_export_batch_id(
    peer: &p::SyncPeerRef,
    aggregate: &p::RunId,
    from_version: u64,
    to_version: u64,
) -> p::SyncBatchId {
    let mut identity = Vec::new();
    push_semantic_field(&mut identity, b"forme-sync-export-v1");
    push_semantic_field(&mut identity, peer.0.as_bytes());
    push_semantic_field(&mut identity, aggregate.0.as_bytes());
    push_semantic_field(&mut identity, &from_version.to_le_bytes());
    push_semantic_field(&mut identity, &to_version.to_le_bytes());
    p::SyncBatchId(format!(
        "sync-export:fnv64:{}",
        checksum::calculate_bytes(&identity)
    ))
}

pub(crate) struct StoreCore {
    connection: Mutex<Connection>,
    current_schema: BTreeMap<PayloadType, p::SchemaVersion>,
    fts_enabled: bool,
    sync_peer: Option<p::SyncPeer>,
    upcasters: RwLock<UpcasterGraph>,
    upcaster_registration: Mutex<()>,
}

impl StoreCore {
    fn require_sync_peer(&self, peer: &p::SyncPeer) -> p::Result<()> {
        match self.sync_peer.as_ref() {
            Some(configured) if configured == peer => Ok(()),
            Some(_) => Err(p::Error(
                "sync request does not match this store instance's configured peer".into(),
            )),
            None => Err(p::Error(
                "sync is disabled because no owner-bound peer was configured".into(),
            )),
        }
    }

    fn require_sync_peer_ref(&self, peer: &p::SyncPeerRef) -> p::Result<()> {
        match self.sync_peer.as_ref() {
            Some(configured) if configured.reference == *peer => Ok(()),
            Some(_) => Err(p::Error(
                "sync cursor does not match this store instance's configured peer".into(),
            )),
            None => Err(p::Error(
                "sync is disabled because no owner-bound peer was configured".into(),
            )),
        }
    }

    fn current_schema_for(&self, payload_type: PayloadType) -> p::Result<p::SchemaVersion> {
        self.current_schema
            .get(&payload_type)
            .copied()
            .ok_or_else(|| {
                p::Error(format!(
                    "current schema registry is missing version for {payload_type}"
                ))
            })
    }

    fn upcaster_snapshot(&self) -> p::Result<UpcasterGraph> {
        let _registration = self
            .upcaster_registration
            .lock()
            .map_err(|_| p::Error("upcaster registration lock is poisoned".into()))?;
        self.upcasters
            .read()
            .map(|graph| graph.clone())
            .map_err(|_| p::Error("upcaster graph lock is poisoned".into()))
    }

    pub(crate) fn load_page(
        &self,
        run_id: &p::RunId,
        first_seq: u64,
        page_size: usize,
    ) -> p::Result<CursorPage> {
        let first_seq = i64::try_from(first_seq)
            .map_err(|_| p::Error("event cursor sequence is invalid".into()))?;
        let page_size = i64::try_from(page_size)
            .map_err(|_| p::Error("event cursor page size is invalid".into()))?;
        let stored_events = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| p::Error("event database lock is poisoned".into()))?;
            let mut statement = connection
                .prepare(
                    "SELECT event_id, run_id, stream_seq, turn_id, kind, payload,
                            schema_version, ts, provenance, checksum
                     FROM events
                     WHERE run_id = ?1 AND stream_seq >= ?2
                     ORDER BY stream_seq
                     LIMIT ?3",
                )
                .map_err(|error| store_error("failed to prepare event cursor", error))?;
            let rows = statement
                .query_map(
                    params![&run_id.0, first_seq, page_size],
                    StoredEvent::from_row,
                )
                .map_err(|error| store_error("failed to query event cursor", error))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| store_error("failed to read event cursor row", error))?
        };

        let Some(last_event) = stored_events.last() else {
            return Ok(CursorPage {
                next_seq: u64::try_from(first_seq)
                    .map_err(|_| p::Error("event cursor sequence is invalid".into()))?,
                items: VecDeque::new(),
            });
        };
        let last_seq = u64::try_from(last_event.stream_seq)
            .map_err(|_| p::Error("stored event sequence is invalid".into()))?;
        let next_seq = last_seq
            .checked_add(1)
            .ok_or_else(|| p::Error("event cursor sequence is exhausted".into()))?;
        let upcasters = self.upcaster_snapshot()?;
        let items = stored_events
            .into_iter()
            .map(|event| event.decode(&self.current_schema, &upcasters))
            .collect::<VecDeque<_>>();

        Ok(CursorPage { next_seq, items })
    }
}

struct StoredEvent {
    event_id: String,
    run_id: String,
    stream_seq: i64,
    turn_id: Option<String>,
    kind: String,
    payload: Vec<u8>,
    schema_version: i64,
    ts_unix_ms: i64,
    provenance: Vec<u8>,
    checksum: String,
}

impl StoredEvent {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            event_id: row.get(0)?,
            run_id: row.get(1)?,
            stream_seq: row.get(2)?,
            turn_id: row.get(3)?,
            kind: row.get(4)?,
            payload: row.get(5)?,
            schema_version: row.get(6)?,
            ts_unix_ms: row.get(7)?,
            provenance: row.get(8)?,
            checksum: row.get(9)?,
        })
    }

    fn decode(
        self,
        current_schema: &BTreeMap<PayloadType, p::SchemaVersion>,
        upcasters: &UpcasterGraph,
    ) -> p::Result<p::Event> {
        self.decode_with_target(upcasters, |payload_type, _| {
            current_schema.get(&payload_type).copied().ok_or_else(|| {
                p::Error(format!(
                    "current schema registry is missing version for {payload_type}"
                ))
            })
        })
    }

    fn decode_at(
        self,
        snapshot: &SchemaSnapshot,
        upcasters: &UpcasterGraph,
    ) -> p::Result<p::Event> {
        self.decode_with_target(upcasters, |payload_type, _| {
            snapshot.schema.get(&payload_type).copied().ok_or_else(|| {
                p::Error(format!(
                    "schema snapshot is missing version for {payload_type}"
                ))
            })
        })
    }

    fn decode_with_target(
        self,
        upcasters: &UpcasterGraph,
        target_for: impl FnOnce(PayloadType, p::SchemaVersion) -> p::Result<p::SchemaVersion>,
    ) -> p::Result<p::Event> {
        let stream_seq = u64::try_from(self.stream_seq)
            .map_err(|_| p::Error("stored event sequence is invalid".into()))?;
        let schema_version = u32::try_from(self.schema_version)
            .map_err(|_| p::Error("stored event schema version is invalid".into()))?;
        let expected_checksum = checksum::calculate(PersistedEnvelope {
            event_id: &self.event_id,
            run_id: &self.run_id,
            stream_seq,
            turn_id: self.turn_id.as_deref(),
            kind: &self.kind,
            payload: &self.payload,
            schema_version,
            ts_unix_ms: self.ts_unix_ms,
            provenance: &self.provenance,
        });
        if expected_checksum != self.checksum {
            return Err(p::Error(format!(
                "event {} checksum mismatch",
                self.event_id
            )));
        }

        let stored_schema = p::SchemaVersion(schema_version);
        let kind = self.kind.parse::<p::EventKind>()?;
        let payload_type = PayloadType::from_event_kind(kind);
        let target_schema = target_for(payload_type, stored_schema)?;
        let mut payload_bytes = self.payload;
        for step in upcasters.plan(payload_type, stored_schema, target_schema)? {
            payload_bytes = step.apply(payload_bytes)?;
        }
        let payload = serde_json::from_slice::<p::EventPayload>(&payload_bytes)
            .map_err(|error| store_error("failed to deserialize event payload", error))?;
        let provenance = serde_json::from_slice::<p::Provenance>(&self.provenance)
            .map_err(|error| store_error("failed to deserialize event provenance", error))?;
        let event = p::Event {
            event_id: p::EventId(self.event_id),
            run_id: p::RunId(self.run_id),
            stream_seq,
            turn_id: self.turn_id.map(p::TurnId),
            kind,
            payload,
            schema_version: target_schema,
            ts_unix_ms: self.ts_unix_ms,
            provenance,
        };
        event.validate_payload_kind()?;
        Ok(event)
    }
}

fn update_builtin_projections(transaction: &Transaction<'_>, event: &p::Event) -> p::Result<()> {
    let expected_watermark = event
        .stream_seq
        .checked_sub(1)
        .ok_or_else(|| p::Error("appended event sequence must start at one".into()))?;
    let mut session_state = match read_session_state(transaction, &event.run_id)? {
        Some(state) if state.last_stream_seq == expected_watermark => state,
        None if event.stream_seq == 1 => SessionStateProjection::empty(),
        Some(state) => {
            return Err(p::Error(format!(
                "session_state projection cache discontinuity for run {}: expected watermark {expected_watermark}, found {}",
                event.run_id.0, state.last_stream_seq
            )))
        }
        None => {
            return Err(p::Error(format!(
                "session_state projection cache discontinuity for run {}: expected watermark {expected_watermark}, found no row",
                event.run_id.0
            )))
        }
    };
    let mut transcript = match read_transcript(transaction, &event.run_id)? {
        Some(state) if state.last_stream_seq == expected_watermark => state,
        None if event.stream_seq == 1 => TranscriptProjection::empty(),
        Some(state) => {
            return Err(p::Error(format!(
                "transcript projection cache discontinuity for run {}: expected watermark {expected_watermark}, found {}",
                event.run_id.0, state.last_stream_seq
            )))
        }
        None => {
            return Err(p::Error(format!(
                "transcript projection cache discontinuity for run {}: expected watermark {expected_watermark}, found no row",
                event.run_id.0
            )))
        }
    };

    SessionStateProjection::apply(&mut session_state, event);
    TranscriptProjection::apply(&mut transcript, event);
    write_session_state(transaction, &session_state)?;
    write_transcript(transaction, &transcript)
}

fn read_session_state(
    connection: &Connection,
    run_id: &p::RunId,
) -> p::Result<Option<SessionState>> {
    let stored = connection
        .query_row(
            "SELECT state, last_stream_seq FROM session_state WHERE run_id = ?1",
            params![&run_id.0],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|error| store_error("failed to load session projection", error))?;
    stored
        .map(|(state, last_stream_seq)| {
            let state = serde_json::from_slice::<SessionState>(&state)
                .map_err(|error| store_error("failed to deserialize session projection", error))?;
            validate_projection_row(
                "session",
                run_id,
                state.run_id.as_ref(),
                state.schema_version,
                state.last_stream_seq,
                last_stream_seq,
            )?;
            Ok(state)
        })
        .transpose()
}

fn read_transcript(connection: &Connection, run_id: &p::RunId) -> p::Result<Option<Transcript>> {
    let stored = connection
        .query_row(
            "SELECT state, last_stream_seq FROM transcript WHERE run_id = ?1",
            params![&run_id.0],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .map_err(|error| store_error("failed to load transcript projection", error))?;
    stored
        .map(|(state, last_stream_seq)| {
            let state = serde_json::from_slice::<Transcript>(&state).map_err(|error| {
                store_error("failed to deserialize transcript projection", error)
            })?;
            validate_projection_row(
                "transcript",
                run_id,
                state.run_id.as_ref(),
                state.schema_version,
                state.last_stream_seq,
                last_stream_seq,
            )?;
            Ok(state)
        })
        .transpose()
}

fn validate_projection_row(
    projection: &str,
    expected_run_id: &p::RunId,
    state_run_id: Option<&p::RunId>,
    schema_version: p::SchemaVersion,
    state_last_stream_seq: u64,
    stored_last_stream_seq: i64,
) -> p::Result<()> {
    if state_run_id != Some(expected_run_id) {
        return Err(p::Error(format!(
            "{projection} projection run id does not match its cache key"
        )));
    }
    if schema_version != p::SchemaVersion(1) {
        return Err(p::Error(format!(
            "{projection} projection schema version {} is unsupported",
            schema_version.0
        )));
    }
    let stored_last_stream_seq = u64::try_from(stored_last_stream_seq)
        .map_err(|_| p::Error(format!("{projection} projection sequence is invalid")))?;
    if state_last_stream_seq != stored_last_stream_seq {
        return Err(p::Error(format!(
            "{projection} projection sequence does not match its cache metadata"
        )));
    }
    Ok(())
}

fn write_session_state(transaction: &Transaction<'_>, state: &SessionState) -> p::Result<()> {
    let run_id = state
        .run_id
        .as_ref()
        .ok_or_else(|| p::Error("session projection has no run id".into()))?;
    let last_stream_seq = i64::try_from(state.last_stream_seq)
        .map_err(|_| p::Error("session projection sequence is invalid".into()))?;
    let serialized = serde_json::to_vec(state)
        .map_err(|error| store_error("failed to serialize session projection", error))?;
    transaction
        .execute(
            "INSERT INTO session_state(run_id, state, last_stream_seq)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(run_id) DO UPDATE SET
                 state = excluded.state,
                 last_stream_seq = excluded.last_stream_seq",
            params![&run_id.0, serialized, last_stream_seq],
        )
        .map_err(|error| store_error("failed to write session projection", error))?;
    Ok(())
}

fn write_transcript(transaction: &Transaction<'_>, state: &Transcript) -> p::Result<()> {
    let run_id = state
        .run_id
        .as_ref()
        .ok_or_else(|| p::Error("transcript projection has no run id".into()))?;
    let last_stream_seq = i64::try_from(state.last_stream_seq)
        .map_err(|_| p::Error("transcript projection sequence is invalid".into()))?;
    let serialized = serde_json::to_vec(state)
        .map_err(|error| store_error("failed to serialize transcript projection", error))?;
    transaction
        .execute(
            "INSERT INTO transcript(run_id, state, last_stream_seq)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(run_id) DO UPDATE SET
                 state = excluded.state,
                 last_stream_seq = excluded.last_stream_seq",
            params![&run_id.0, serialized, last_stream_seq],
        )
        .map_err(|error| store_error("failed to write transcript projection", error))?;
    Ok(())
}

struct BuiltinProjectionState {
    session: SessionState,
    transcript: Transcript,
    fts_entries: Vec<FtsEntry>,
}

struct FtsEntry {
    event_id: String,
    text: String,
}

#[derive(Debug, PartialEq, Eq)]
struct FtsCoverage {
    event_set: Vec<u8>,
    schema_registry: Vec<u8>,
    upcaster_graph: Vec<u8>,
}

impl FtsCoverage {
    fn new(event_set: &EventSetToken, schema_registry: Vec<u8>, upcaster_graph: Vec<u8>) -> Self {
        Self {
            event_set: event_set.identity_bytes(),
            schema_registry,
            upcaster_graph,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct EventSetToken {
    streams: BTreeMap<String, Vec<EventToken>>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EventToken {
    stream_seq: i64,
    event_id: String,
    checksum: String,
}

impl EventSetToken {
    fn from_stored_events(events: &[StoredEvent]) -> Self {
        let mut token = Self::default();
        for event in events {
            token.insert(
                event.run_id.clone(),
                EventToken {
                    stream_seq: event.stream_seq,
                    event_id: event.event_id.clone(),
                    checksum: event.checksum.clone(),
                },
            );
        }
        token.normalized()
    }

    fn insert(&mut self, run_id: String, event: EventToken) {
        self.streams.entry(run_id).or_default().push(event);
    }

    fn normalized(mut self) -> Self {
        for stream in self.streams.values_mut() {
            stream.sort();
        }
        self
    }

    fn is_empty(&self) -> bool {
        self.streams.is_empty()
    }

    fn identity_bytes(&self) -> Vec<u8> {
        let mut encoded = Vec::new();
        push_identity_field(&mut encoded, b"forme-event-set-v1");
        for (run_id, stream) in &self.streams {
            push_identity_field(&mut encoded, run_id.as_bytes());
            for event in stream {
                push_identity_field(&mut encoded, &event.stream_seq.to_le_bytes());
                push_identity_field(&mut encoded, event.event_id.as_bytes());
                push_identity_field(&mut encoded, event.checksum.as_bytes());
            }
        }
        encoded
    }
}

fn capture_rebuild_events(
    connection: &mut Connection,
) -> p::Result<(Vec<StoredEvent>, EventSetToken)> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| store_error("failed to begin projection rebuild snapshot", error))?;
    let events = read_all_stored_events(&transaction)?;
    let token = EventSetToken::from_stored_events(&events);
    transaction
        .commit()
        .map_err(|error| store_error("failed to finish projection rebuild snapshot", error))?;
    Ok((events, token))
}

fn read_all_stored_events(connection: &Connection) -> p::Result<Vec<StoredEvent>> {
    let mut statement = connection
        .prepare(
            "SELECT event_id, run_id, stream_seq, turn_id, kind, payload,
                    schema_version, ts, provenance, checksum
             FROM events",
        )
        .map_err(|error| store_error("failed to prepare projection rebuild capture", error))?;
    let rows = statement
        .query_map([], StoredEvent::from_row)
        .map_err(|error| store_error("failed to query projection rebuild capture", error))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| store_error("failed to read projection rebuild event", error))
}

fn fold_builtin_projections(
    stored_events: Vec<StoredEvent>,
    current_schema: &BTreeMap<PayloadType, p::SchemaVersion>,
    upcasters: &UpcasterGraph,
) -> p::Result<Vec<BuiltinProjectionState>> {
    let mut streams = BTreeMap::<String, Vec<StoredEvent>>::new();
    for event in stored_events {
        streams.entry(event.run_id.clone()).or_default().push(event);
    }

    let mut rebuilt = Vec::with_capacity(streams.len());
    for mut stream in streams.into_values() {
        stream.sort_by_key(|event| event.stream_seq);
        let mut session = SessionStateProjection::empty();
        let mut transcript = TranscriptProjection::empty();
        let mut fts_entries = Vec::new();
        for stored_event in stream {
            let event = stored_event.decode(current_schema, upcasters)?;
            if let Some(text) = searchable_text(&event) {
                fts_entries.push(FtsEntry {
                    event_id: event.event_id.0.clone(),
                    text: text.to_owned(),
                });
            }
            SessionStateProjection::apply(&mut session, &event);
            TranscriptProjection::apply(&mut transcript, &event);
        }
        rebuilt.push(BuiltinProjectionState {
            session,
            transcript,
            fts_entries,
        });
    }
    Ok(rebuilt)
}

fn read_event_set_token(connection: &Connection) -> p::Result<EventSetToken> {
    let mut statement = connection
        .prepare("SELECT run_id, stream_seq, event_id, checksum FROM events")
        .map_err(|error| store_error("failed to prepare projection rebuild token", error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                EventToken {
                    stream_seq: row.get(1)?,
                    event_id: row.get(2)?,
                    checksum: row.get(3)?,
                },
            ))
        })
        .map_err(|error| store_error("failed to query projection rebuild token", error))?;
    let mut token = EventSetToken::default();
    for row in rows {
        let (run_id, event) =
            row.map_err(|error| store_error("failed to read projection rebuild token", error))?;
        token.insert(run_id, event);
    }
    Ok(token.normalized())
}

fn schema_registry_identity(current_schema: &BTreeMap<PayloadType, p::SchemaVersion>) -> Vec<u8> {
    let mut encoded = Vec::new();
    push_identity_field(&mut encoded, b"forme-schema-registry-v1");
    for (payload_type, version) in current_schema {
        push_identity_field(&mut encoded, payload_type.as_str().as_bytes());
        push_identity_field(&mut encoded, &version.0.to_le_bytes());
    }
    encoded
}

fn push_identity_field(encoded: &mut Vec<u8>, value: &[u8]) {
    encoded.extend_from_slice(&(value.len() as u64).to_le_bytes());
    encoded.extend_from_slice(value);
}

fn read_fts_coverage(connection: &Connection) -> p::Result<Option<FtsCoverage>> {
    connection
        .query_row(
            "SELECT event_set, schema_registry, upcaster_graph\n             FROM fts_coverage WHERE singleton = 1",
            [],
            |row| {
                Ok(FtsCoverage {
                    event_set: row.get(0)?,
                    schema_registry: row.get(1)?,
                    upcaster_graph: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|error| store_error("failed to read full-text search coverage", error))
}

fn write_fts_coverage(transaction: &Transaction<'_>, coverage: &FtsCoverage) -> p::Result<()> {
    transaction
        .execute(
            "INSERT INTO fts_coverage(\n                singleton, event_set, schema_registry, upcaster_graph\n             ) VALUES (1, ?1, ?2, ?3)\n             ON CONFLICT(singleton) DO UPDATE SET\n                event_set = excluded.event_set,\n                schema_registry = excluded.schema_registry,\n                upcaster_graph = excluded.upcaster_graph",
            params![
                &coverage.event_set,
                &coverage.schema_registry,
                &coverage.upcaster_graph,
            ],
        )
        .map_err(|error| store_error("failed to write full-text search coverage", error))?;
    Ok(())
}

fn initialize_connection(
    connection: &mut Connection,
    file_database: bool,
    fts_enabled: bool,
    sync_peer: Option<&p::SyncPeer>,
) -> p::Result<()> {
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| store_error("failed to enable SQLite foreign keys", error))?;
    let foreign_keys: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(|error| store_error("failed to verify SQLite foreign keys", error))?;
    if foreign_keys != 1 {
        return Err(p::Error("SQLite foreign keys are not enabled".into()));
    }

    if file_database {
        let journal_mode = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| store_error("failed to enable SQLite WAL mode", error))?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(p::Error(format!(
                "SQLite WAL mode was not enabled: {journal_mode}"
            )));
        }
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| store_error("failed to begin event database schema migration", error))?;
    let user_version: i64 = transaction
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| store_error("failed to read event database schema version", error))?;
    if user_version > DATABASE_SCHEMA_VERSION {
        return Err(p::Error(format!(
            "event database schema version {user_version} is newer than supported version {DATABASE_SCHEMA_VERSION}"
        )));
    }

    transaction
        .execute_batch(SCHEMA)
        .map_err(|error| store_error("failed to initialize event database schema", error))?;
    if fts_enabled {
        transaction
            .execute_batch(FTS_SCHEMA)
            .map_err(|error| store_error("failed to initialize full-text search", error))?;
    }
    if user_version < DATABASE_SCHEMA_VERSION {
        migrate_pre_versioned_catalog(&transaction)?;
    }
    validate_catalog(&transaction)?;
    configure_sync_peer(&transaction, sync_peer)?;
    transaction
        .pragma_update(None, "user_version", DATABASE_SCHEMA_VERSION)
        .map_err(|error| store_error("failed to record event database schema version", error))?;
    transaction
        .commit()
        .map_err(|error| store_error("failed to commit event database schema migration", error))
}

fn migrate_pre_versioned_catalog(transaction: &Transaction<'_>) -> p::Result<()> {
    let idempotency_is_current =
        table_has_column(transaction, "idempotency_keys", "semantic_bytes")?;
    let migrations_are_current =
        table_has_column(transaction, "schema_migrations", "implementation_identity")?;

    if !migrations_are_current {
        let migration_count: i64 = transaction
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .map_err(|error| store_error("failed to inspect legacy schema migrations", error))?;
        if migration_count != 0 {
            return Err(p::Error(
                "legacy schema migrations lack a stable implementation identity; restore with an explicit migration identity before opening this database"
                    .into(),
            ));
        }
    }

    if !idempotency_is_current {
        rebuild_legacy_idempotency_keys(transaction)?;
    }
    if !migrations_are_current {
        rebuild_empty_legacy_schema_migrations(transaction)?;
    }
    Ok(())
}

fn rebuild_legacy_idempotency_keys(transaction: &Transaction<'_>) -> p::Result<()> {
    let legacy_count: i64 = transaction
        .query_row("SELECT COUNT(*) FROM idempotency_keys", [], |row| {
            row.get(0)
        })
        .map_err(|error| store_error("failed to count legacy idempotency claims", error))?;
    let rows = {
        let mut statement = transaction
            .prepare(
                "SELECT i.namespace, i.\"key\", i.event_id, i.run_id,
                        e.kind, e.payload, e.schema_version, e.provenance
                 FROM idempotency_keys AS i
                 JOIN events AS e ON e.event_id = i.event_id",
            )
            .map_err(|error| {
                store_error("failed to prepare legacy idempotency migration", error)
            })?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                ))
            })
            .map_err(|error| store_error("failed to query legacy idempotency claims", error))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| store_error("failed to read legacy idempotency claim", error))?
    };
    if i64::try_from(rows.len()).ok() != Some(legacy_count) {
        return Err(p::Error(
            "legacy idempotency claims reference missing authoritative events".into(),
        ));
    }

    transaction
        .execute_batch(
            r#"
            CREATE TABLE idempotency_keys_catalog_v1 (
                namespace      TEXT NOT NULL,
                "key"          TEXT NOT NULL,
                event_id       TEXT NOT NULL,
                run_id         TEXT NOT NULL,
                semantic_bytes BLOB NOT NULL,
                UNIQUE(namespace, "key"),
                FOREIGN KEY(event_id) REFERENCES events(event_id)
                    DEFERRABLE INITIALLY DEFERRED
            );
            "#,
        )
        .map_err(|error| store_error("failed to create migrated idempotency catalog", error))?;
    for (namespace, key, event_id, run_id, kind, payload, schema_version, provenance) in rows {
        let schema_version = u32::try_from(schema_version)
            .map_err(|_| p::Error("legacy event schema version is invalid".into()))?;
        let semantic_bytes =
            domain_semantic_bytes_from_parts(&kind, &payload, schema_version, &provenance);
        transaction
            .execute(
                "INSERT INTO idempotency_keys_catalog_v1(
                    namespace, \"key\", event_id, run_id, semantic_bytes
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![namespace, key, event_id, run_id, semantic_bytes],
            )
            .map_err(|error| store_error("failed to migrate idempotency claim", error))?;
    }
    transaction
        .execute_batch(
            "DROP TABLE idempotency_keys;
             ALTER TABLE idempotency_keys_catalog_v1 RENAME TO idempotency_keys;",
        )
        .map_err(|error| store_error("failed to replace legacy idempotency catalog", error))?;
    Ok(())
}

fn rebuild_empty_legacy_schema_migrations(transaction: &Transaction<'_>) -> p::Result<()> {
    transaction
        .execute_batch(
            r#"
            CREATE TABLE schema_migrations_catalog_v1 (
                payload_type            TEXT NOT NULL,
                from_version            INTEGER NOT NULL,
                to_version              INTEGER NOT NULL,
                note                    TEXT NOT NULL,
                implementation_identity TEXT NOT NULL,
                PRIMARY KEY(payload_type, from_version),
                CHECK(length(trim(note)) > 0),
                CHECK(length(trim(implementation_identity)) > 0),
                CHECK(to_version > from_version)
            );
            DROP TABLE schema_migrations;
            ALTER TABLE schema_migrations_catalog_v1 RENAME TO schema_migrations;
            "#,
        )
        .map_err(|error| store_error("failed to replace legacy schema migration catalog", error))?;
    Ok(())
}

fn validate_catalog(connection: &Connection) -> p::Result<()> {
    for (table, column) in [
        ("idempotency_keys", "semantic_bytes"),
        ("schema_migrations", "implementation_identity"),
        ("sync_peers", "semantic_bytes"),
        ("sync_batches", "applied_event_ids"),
        ("sync_cursors", "exported_version"),
        ("strategy_candidates", "candidate"),
        ("evolution_evaluations", "evaluation"),
        ("stable_strategies", "promotion_event_id"),
        ("evolution_versions", "version"),
        ("active_strategies", "aggregate_version"),
        ("evolution_history", "committed_version"),
        ("ecosystem_versions", "version"),
        ("ecosystem_publishers", "grant"),
        ("ecosystem_admissions", "admission"),
        ("ecosystem_package_states", "state"),
        ("ecosystem_distributions", "receipt"),
        ("ecosystem_history", "committed_version"),
        ("ecosystem_nonces", "plan_digest"),
        ("ecosystem_distribution_attempts", "semantic"),
        ("ecosystem_package_bundles", "bundle"),
    ] {
        if !table_has_column(connection, table, column)? {
            return Err(p::Error(format!(
                "event database catalog {table} is missing required column {column}"
            )));
        }
    }
    Ok(())
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> p::Result<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| store_error("failed to inspect event database catalog", error))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| store_error("failed to query event database catalog", error))?;
    for stored_column in columns {
        if stored_column
            .map_err(|error| store_error("failed to read event database catalog", error))?
            == column
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_options(options: &StoreOptions) -> p::Result<()> {
    if options.cursor_page_size == 0 {
        return Err(p::Error(
            "cursor page size must be greater than zero".into(),
        ));
    }
    if options.current_schema.len() != PayloadType::ALL.len()
        || PayloadType::ALL
            .into_iter()
            .any(|payload_type| !options.current_schema.contains_key(&payload_type))
    {
        return Err(p::Error(format!(
            "current schema registry must contain all {} payload types",
            PayloadType::ALL.len()
        )));
    }
    if let Some(peer) = options.sync_peer.as_ref() {
        peer.validate()?;
    }
    Ok(())
}

fn event_semantic_bytes(event: &p::Event, payload: &[u8], provenance: &[u8]) -> Vec<u8> {
    semantic_bytes(SemanticEventFields {
        event_id: Some(&event.event_id.0),
        run_id: &event.run_id.0,
        turn_id: event.turn_id.as_ref().map(|turn_id| turn_id.0.as_str()),
        kind: event.kind.as_str(),
        payload,
        schema_version: event.schema_version.0,
        ts_unix_ms: Some(event.ts_unix_ms),
        provenance,
    })
}

fn domain_semantic_bytes(event: &p::Event, payload: &[u8], provenance: &[u8]) -> Vec<u8> {
    domain_semantic_bytes_from_parts(
        event.kind.as_str(),
        payload,
        event.schema_version.0,
        provenance,
    )
}

fn domain_semantic_bytes_from_parts(
    kind: &str,
    payload: &[u8],
    schema_version: u32,
    provenance: &[u8],
) -> Vec<u8> {
    let mut encoded = Vec::new();
    push_semantic_field(&mut encoded, b"forme-domain-semantic-v1");
    push_semantic_field(&mut encoded, kind.as_bytes());
    push_semantic_field(&mut encoded, payload);
    push_semantic_field(&mut encoded, &schema_version.to_le_bytes());
    push_semantic_field(&mut encoded, provenance);
    encoded
}

struct SemanticEventFields<'a> {
    event_id: Option<&'a str>,
    run_id: &'a str,
    turn_id: Option<&'a str>,
    kind: &'a str,
    payload: &'a [u8],
    schema_version: u32,
    ts_unix_ms: Option<i64>,
    provenance: &'a [u8],
}

fn semantic_bytes(fields: SemanticEventFields<'_>) -> Vec<u8> {
    let mut encoded = Vec::new();
    push_semantic_field(&mut encoded, b"forme-semantic-v1");
    push_optional_semantic_field(&mut encoded, fields.event_id.map(str::as_bytes));
    push_semantic_field(&mut encoded, fields.run_id.as_bytes());
    push_optional_semantic_field(&mut encoded, fields.turn_id.map(str::as_bytes));
    push_semantic_field(&mut encoded, fields.kind.as_bytes());
    push_semantic_field(&mut encoded, fields.payload);
    push_semantic_field(&mut encoded, &fields.schema_version.to_le_bytes());
    match fields.ts_unix_ms {
        Some(value) => {
            encoded.push(1);
            push_semantic_field(&mut encoded, &value.to_le_bytes());
        }
        None => encoded.push(0),
    }
    push_semantic_field(&mut encoded, fields.provenance);
    encoded
}

fn push_optional_semantic_field(encoded: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(value) => {
            encoded.push(1);
            push_semantic_field(encoded, value);
        }
        None => encoded.push(0),
    }
}

fn push_semantic_field(encoded: &mut Vec<u8>, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("semantic field length fits in u64");
    encoded.extend_from_slice(&length.to_le_bytes());
    encoded.extend_from_slice(value);
}

fn domain_idempotency_key(event: &p::Event) -> Option<(&'static str, String)> {
    match &event.payload {
        p::EventPayload::RunAccepted(payload) => payload
            .idempotency_key
            .as_ref()
            .map(|key| ("run_request", key.0.clone())),
        p::EventPayload::ActionPlanned(payload) => {
            Some(("action_intent", payload.intent_id.0.clone()))
        }
        p::EventPayload::MemoryMaintenanceApplied(payload) => payload
            .deltas_ref
            .0
            .strip_prefix("intention-lease|")
            .and_then(|value| {
                let mut fields = value.split('|');
                let generation = fields.next()?;
                let _lease_until = fields.next()?;
                let intention = fields.next()?;
                fields
                    .next()
                    .is_none()
                    .then(|| ("intention_lease", format!("{generation}|{intention}")))
            }),
        _ => None,
    }
}

fn store_error(context: &str, error: impl std::fmt::Display) -> p::Error {
    p::Error(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn append_rolls_back_all_derived_writes() {
        let store = SqliteEventStore::open_in_memory(StoreOptions {
            fts_enabled: true,
            ..StoreOptions::default()
        })
        .unwrap();
        {
            let connection = store.core.connection.lock().unwrap();
            connection
                .execute_batch(
                    "CREATE TRIGGER reject_session_state_projection
                     BEFORE INSERT ON session_state
                     WHEN NEW.run_id = 'rollback-run'
                     BEGIN
                         SELECT RAISE(ABORT, 'forced session projection failure');
                     END;",
                )
                .unwrap();
        }

        let result = store.append(p::Event::new(
            p::EventId("rollback-event".into()),
            p::RunId("rollback-run".into()),
            None,
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::UserTurn,
                session_ref: p::SessionId("rollback-session".into()),
                input_ref: p::InputRef("rollback searchable input".into()),
                idempotency_key: Some(p::IdempotencyKey("rollback-key".into())),
            }),
            p::SchemaVersion(1),
            1,
            p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        ));

        let error = result.expect_err("the injected projection trigger must abort append");
        assert!(
            error.0.contains("forced session projection failure"),
            "append failed before reaching the injected trigger: {error}"
        );
        let connection = store.core.connection.lock().unwrap();
        for table in [
            "events",
            "idempotency_keys",
            "events_fts",
            "session_state",
            "transcript",
        ] {
            let count: i64 = connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{table} must roll back with the append");
        }
    }

    #[test]
    fn replay_uses_one_snapshot_when_a_second_connection_commits_between_reads() {
        let database = UnitTestDatabase::new("replay-snapshot");
        let run_id = p::RunId("deterministic-snapshot-run".into());
        let writer = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
        writer
            .append(run_accepted_event(
                "snapshot-before",
                &run_id,
                "before-commit",
            ))
            .unwrap();
        let reader = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
        let late_event = run_accepted_event("snapshot-after", &run_id, "after-commit");

        install_replay_after_events_materialized_hook(move || {
            writer
                .append(late_event)
                .expect("the second connection must commit before cache reads");
        });
        let report = reader
            .replay(
                run_id.clone(),
                SchemaSnapshot {
                    schema: BTreeMap::from([(PayloadType::RunAccepted, p::SchemaVersion(1))]),
                    policy_version: p::Version(1),
                    loop_version: p::Version(1),
                    model_profile: p::ModelProfileRef("snapshot-model".into()),
                    tool_schema: p::Version(1),
                },
            )
            .unwrap();

        assert_eq!(report.replayed_session_state.last_stream_seq, 1);
        assert_eq!(report.replayed_transcript.last_stream_seq, 1);
        assert_eq!(report.replayed_transcript.entries.len(), 1);
        assert!(report.diff_vs_current.is_empty());

        let current_session = reader.load_session_state(run_id.clone()).unwrap().unwrap();
        let current_transcript = reader.load_transcript(run_id.clone()).unwrap().unwrap();
        assert_eq!(current_session.last_stream_seq, 2);
        assert_eq!(current_transcript.last_stream_seq, 2);
        assert_eq!(current_transcript.entries.len(), 2);
        assert_eq!(
            reader
                .read_run(run_id)
                .collect::<p::Result<Vec<_>>>()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn search_matches_the_same_snapshot_as_its_validated_coverage() {
        let database = UnitTestDatabase::new("search-snapshot");
        let seed_run = p::RunId("search-snapshot-seed".into());
        let late_run = p::RunId("search-snapshot-late".into());
        let writer = SqliteEventStore::open(
            database.path(),
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
        )
        .unwrap();
        writer
            .append(run_accepted_event(
                "search-snapshot-before",
                &seed_run,
                "snapshot seed text",
            ))
            .unwrap();
        let reader = SqliteEventStore::open(
            database.path(),
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
        )
        .unwrap();
        let late_event =
            run_accepted_event("search-snapshot-after", &late_run, "latecoverageuniqueterm");

        install_search_after_coverage_validated_hook(move || {
            writer
                .append(late_event)
                .expect("the second connection must commit after coverage validation");
        });
        let first = reader.search("latecoverageuniqueterm", 10).unwrap();
        assert!(
            first.is_empty(),
            "the search must not move past its validated snapshot: {first:?}"
        );

        let second = reader.search("latecoverageuniqueterm", 10).unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(
            second[0].event_id,
            p::EventId("search-snapshot-after".into())
        );
    }

    fn run_accepted_event(event_id: &str, run_id: &p::RunId, input: &str) -> p::Event {
        p::Event::new(
            p::EventId(event_id.into()),
            run_id.clone(),
            None,
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::UserTurn,
                session_ref: p::SessionId(format!("session-{}", run_id.0)),
                input_ref: p::InputRef(input.into()),
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

    struct UnitTestDatabase {
        path: PathBuf,
    }

    impl UnitTestDatabase {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "forme-store-{label}-{}-{nonce}.sqlite",
                std::process::id()
            ));
            remove_unit_test_database(&path);
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for UnitTestDatabase {
        fn drop(&mut self) {
            remove_unit_test_database(&self.path);
        }
    }

    fn remove_unit_test_database(path: &PathBuf) {
        let _ = fs::remove_file(path);
        let raw_path = path.to_string_lossy();
        let _ = fs::remove_file(format!("{raw_path}-wal"));
        let _ = fs::remove_file(format!("{raw_path}-shm"));
    }
}
