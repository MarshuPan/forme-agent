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
    root: PathBuf,
    path: PathBuf,
}

impl TestDatabase {
    pub fn new() -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        loop {
            let id = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "forme-store-{}-{timestamp}-{id}",
                std::process::id()
            ));
            match std::fs::create_dir(&root) {
                Ok(()) => {
                    let path = root.join("brain.sqlite3");
                    return Self { root, path };
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create an exclusive test directory: {error}"),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        // Only this fixture owns this directory. Include its backups and keys;
        // never remove or reuse paths left by an earlier process with this PID.
        let _ = std::fs::remove_dir_all(&self.root);
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
