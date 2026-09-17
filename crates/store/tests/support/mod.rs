use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use forme_protocol as p;
use forme_store::{EventCursor, SqliteEventStore, StoreOptions};

static NEXT_DATABASE_ID: AtomicU64 = AtomicU64::new(1);

pub fn test_store() -> SqliteEventStore {
    SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap()
}

pub fn event(event_id: &str, run_id: &str, ts_unix_ms: i64) -> p::Event {
    run_accepted_event(event_id, run_id, ts_unix_ms, None)
}

pub fn run_accepted_event(
    event_id: &str,
    run_id: &str,
    ts_unix_ms: i64,
    idempotency_key: Option<&str>,
) -> p::Event {
    p::Event::new(
        p::EventId(event_id.into()),
        p::RunId(run_id.into()),
        None,
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId(format!("session-{run_id}")),
            input_ref: p::InputRef(format!("input-{event_id}")),
            idempotency_key: idempotency_key.map(|key| p::IdempotencyKey(key.into())),
        }),
        p::SchemaVersion(1),
        ts_unix_ms,
        provenance(),
    )
}

pub fn action_planned_event(
    event_id: &str,
    run_id: &str,
    intent_id: &str,
    ts_unix_ms: i64,
) -> p::Event {
    p::Event::new(
        p::EventId(event_id.into()),
        p::RunId(run_id.into()),
        None,
        p::EventPayload::ActionPlanned(p::ActionPlannedPayload {
            intent_id: p::ActionId(intent_id.into()),
            plan_digest: p::PlanDigest(format!("digest-{event_id}")),
            backend: p::BackendKind::File,
            expected_effect: p::ExpectedEffect::Internal,
            source: p::Source::UserTurn,
            scope: p::Scope("workspace".into()),
            approval_ref: None,
            remote_placement: None,
        }),
        p::SchemaVersion(1),
        ts_unix_ms,
        provenance(),
    )
}

pub fn collect(cursor: EventCursor) -> p::Result<Vec<p::Event>> {
    cursor.collect()
}

pub struct TestDatabase {
    path: PathBuf,
}

impl TestDatabase {
    pub fn new() -> Self {
        let id = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("forme-store-{}-{id}.sqlite3", std::process::id()));
        remove_sqlite_files(&path);
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        remove_sqlite_files(&self.path);
    }
}

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::UserTurn,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}

fn remove_sqlite_files(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
        let _ = std::fs::remove_file(candidate);
    }
}
