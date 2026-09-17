use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore, StoreOptions, VersionedEventStore};

static NEXT_DATABASE_ID: AtomicU64 = AtomicU64::new(1);

struct TestDatabase {
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        let id = NEXT_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "forme-m2-c-sync-{}-{id}.sqlite3",
            std::process::id()
        ));
        remove_sqlite_files(&path);
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        remove_sqlite_files(&self.path);
    }
}

fn remove_sqlite_files(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let candidate = PathBuf::from(format!("{}{suffix}", path.display()));
        let _ = std::fs::remove_file(candidate);
    }
}

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn peer(reference: &str) -> p::SyncPeer {
    p::SyncPeer {
        schema_version: p::SchemaVersion(1),
        reference: p::SyncPeerRef(reference.into()),
        owner: p::VerifiedPrincipal("owner:local".into()),
        allowed_scopes: vec![p::Scope("workspace:alpha".into())],
    }
}

fn sync_options(peer: p::SyncPeer) -> StoreOptions {
    StoreOptions {
        sync_peer: Some(peer),
        ..StoreOptions::default()
    }
}

fn redaction_policy() -> p::SyncRedactionPolicy {
    p::SyncRedactionPolicy {
        schema_version: p::SchemaVersion(1),
        reference: p::RedactionPolicyRef("sync-redaction:m2-c".into()),
        redact_raw_content: true,
        forbid_secret_refs: true,
        forbidden_keys: vec!["authorization".into(), "api_key".into()],
        forbidden_value_markers: vec!["private-sync-marker".into()],
    }
}

fn event(id: &str, run: &p::RunId, sequence: u64, payload: p::EventPayload) -> p::Event {
    let mut event = p::Event::new(
        p::EventId(id.into()),
        run.clone(),
        None,
        payload,
        p::SchemaVersion(1),
        100 + sequence as i64,
        provenance(),
    );
    event.stream_seq = sequence;
    event
}

#[test]
fn s52_expected_append_uses_atomic_compare_and_zero_write_conflicts() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let run = p::RunId("sync:expected-append".into());
    let first = event(
        "expected-event:1",
        &run,
        0,
        p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 1 }),
    );
    let applied = store
        .append_expected(
            first,
            p::AggregateVersion {
                schema_version: p::SchemaVersion(1),
                aggregate: run.clone(),
                value: 0,
            },
        )
        .unwrap();
    assert_eq!(applied.status, p::ExpectedAppendStatus::Applied);
    assert_eq!(applied.resulting_version, 1);

    let stale = store
        .append_expected(
            event(
                "expected-event:2",
                &run,
                0,
                p::EventPayload::TurnComplete(p::TurnCompletePayload { turn_index: 1 }),
            ),
            p::AggregateVersion {
                schema_version: p::SchemaVersion(1),
                aggregate: run.clone(),
                value: 0,
            },
        )
        .unwrap();
    assert_eq!(stale.status, p::ExpectedAppendStatus::Conflict);
    assert_eq!(stale.actual_version, 1);
    assert!(stale.event_id.is_none());
    assert_eq!(
        store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()
            .unwrap()
            .len(),
        1
    );

    let applied = store
        .append_expected(
            event(
                "expected-event:2",
                &run,
                0,
                p::EventPayload::TurnComplete(p::TurnCompletePayload { turn_index: 1 }),
            ),
            p::AggregateVersion {
                schema_version: p::SchemaVersion(1),
                aggregate: run.clone(),
                value: 1,
            },
        )
        .unwrap();
    assert_eq!(applied.status, p::ExpectedAppendStatus::Applied);
    assert_eq!(store.aggregate_version(run).unwrap().value, 2);
}

#[test]
fn s52_sync_batch_is_atomic_idempotent_single_peer_and_cursor_bound() {
    let run = p::RunId("sync:target".into());
    let configured_peer = peer("peer:laptop");
    let store = SqliteEventStore::open_in_memory(sync_options(configured_peer.clone())).unwrap();
    let batch = p::SyncWriteBatch {
        schema_version: p::SchemaVersion(1),
        batch_id: p::SyncBatchId("sync-batch:1".into()),
        peer: configured_peer.clone(),
        aggregate: run.clone(),
        expected_version: p::AggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: run.clone(),
            value: 0,
        },
        events: vec![
            event(
                "sync-event:1",
                &run,
                1,
                p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 1 }),
            ),
            event(
                "sync-event:2",
                &run,
                2,
                p::EventPayload::TurnComplete(p::TurnCompletePayload { turn_index: 1 }),
            ),
        ],
    };
    let applied = store.apply_sync_batch(batch.clone()).unwrap();
    assert_eq!(applied.status, p::SyncApplyStatus::Applied);
    assert_eq!(applied.resulting_version, 2);
    assert_eq!(applied.applied_events.len(), 2);
    let duplicate = store.apply_sync_batch(batch.clone()).unwrap();
    assert_eq!(duplicate.status, p::SyncApplyStatus::Duplicate);
    assert_eq!(duplicate.resulting_version, 2);

    let mut changed = batch;
    changed.events[1].payload =
        p::EventPayload::TurnComplete(p::TurnCompletePayload { turn_index: 9 });
    assert!(store.apply_sync_batch(changed).is_err());
    assert_eq!(store.aggregate_version(run.clone()).unwrap().value, 2);

    let conflict = p::SyncWriteBatch {
        schema_version: p::SchemaVersion(1),
        batch_id: p::SyncBatchId("sync-batch:conflict".into()),
        peer: configured_peer.clone(),
        aggregate: run.clone(),
        expected_version: p::AggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: run.clone(),
            value: 0,
        },
        events: vec![event(
            "sync-event:conflict",
            &run,
            1,
            p::EventPayload::OutputClassified(p::OutputClassifiedPayload {
                kind: p::OutputKind::Final,
            }),
        )],
    };
    let conflict = store.apply_sync_batch(conflict).unwrap();
    assert_eq!(conflict.status, p::SyncApplyStatus::Conflict);
    assert!(conflict.applied_events.is_empty());
    assert_eq!(store.aggregate_version(run.clone()).unwrap().value, 2);

    let atomic_failure = p::SyncWriteBatch {
        schema_version: p::SchemaVersion(1),
        batch_id: p::SyncBatchId("sync-batch:atomic-failure".into()),
        peer: configured_peer.clone(),
        aggregate: run.clone(),
        expected_version: p::AggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: run.clone(),
            value: 2,
        },
        events: vec![
            event(
                "sync-event:3",
                &run,
                3,
                p::EventPayload::OutputClassified(p::OutputClassifiedPayload {
                    kind: p::OutputKind::Final,
                }),
            ),
            event(
                "sync-event:1",
                &run,
                4,
                p::EventPayload::TurnComplete(p::TurnCompletePayload { turn_index: 9 }),
            ),
        ],
    };
    assert!(store.apply_sync_batch(atomic_failure).is_err());
    assert_eq!(store.aggregate_version(run.clone()).unwrap().value, 2);
    assert_eq!(
        store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()
            .unwrap()
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>(),
        vec![
            p::EventId("sync-event:1".into()),
            p::EventId("sync-event:2".into()),
        ]
    );
    let cursor = store
        .sync_cursor(&configured_peer.reference, &run)
        .unwrap()
        .unwrap();
    assert_eq!(cursor.applied_version, 2);
    assert_eq!(cursor.exported_version, 0);

    let other_peer_batch = p::SyncWriteBatch {
        schema_version: p::SchemaVersion(1),
        batch_id: p::SyncBatchId("sync-batch:other-peer".into()),
        peer: peer("peer:phone"),
        aggregate: p::RunId("sync:other".into()),
        expected_version: p::AggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: p::RunId("sync:other".into()),
            value: 0,
        },
        events: vec![event(
            "sync-other-event:1",
            &p::RunId("sync:other".into()),
            1,
            p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 1 }),
        )],
    };
    assert!(store.apply_sync_batch(other_peer_batch).is_err());
}

#[test]
fn s52_sync_peer_must_be_preconfigured_and_cannot_be_claimed_by_first_request() {
    let database = TestDatabase::new();
    let run = p::RunId("sync:preconfigured-peer".into());
    let configured_peer = peer("peer:laptop");
    let batch = p::SyncWriteBatch {
        schema_version: p::SchemaVersion(1),
        batch_id: p::SyncBatchId("sync-batch:preconfigured-peer".into()),
        peer: configured_peer.clone(),
        aggregate: run.clone(),
        expected_version: p::AggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: run.clone(),
            value: 0,
        },
        events: vec![event(
            "sync-preconfigured-event:1",
            &run,
            1,
            p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 1 }),
        )],
    };
    let export = p::SyncExportRequest {
        schema_version: p::SchemaVersion(1),
        peer: configured_peer.clone(),
        aggregate: run.clone(),
        after_version: p::AggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: run.clone(),
            value: 0,
        },
        limit: 1,
        redaction: redaction_policy(),
    };

    let unconfigured = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    assert!(unconfigured.apply_sync_batch(batch.clone()).is_err());
    assert!(unconfigured.export_sync_batch(export).is_err());
    assert_eq!(
        unconfigured.aggregate_version(run.clone()).unwrap().value,
        0
    );
    drop(unconfigured);

    let configured =
        SqliteEventStore::open(database.path(), sync_options(configured_peer.clone())).unwrap();
    assert_eq!(
        configured.apply_sync_batch(batch).unwrap().status,
        p::SyncApplyStatus::Applied
    );
    drop(configured);

    let mismatch = SqliteEventStore::open(database.path(), sync_options(peer("peer:phone")));
    assert!(mismatch.is_err());

    let disabled = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    assert!(disabled
        .sync_cursor(&configured_peer.reference, &run)
        .is_err());
}

#[test]
fn s52_export_redacts_secrets_rejects_redacted_import_and_resumes_from_cursor() {
    let database = TestDatabase::new();
    let run = p::RunId("sync:source".into());
    let configured_peer = peer("peer:laptop");
    {
        let source =
            SqliteEventStore::open(database.path(), sync_options(configured_peer.clone())).unwrap();
        source
            .append(event(
                "source-event:1",
                &run,
                0,
                p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 1 }),
            ))
            .unwrap();
        source
            .append(event(
                "source-event:2",
                &run,
                0,
                p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                    source: p::Source::UserTurn,
                    session_ref: p::SessionId("session:sync".into()),
                    input_ref: p::InputRef("secret:private-sync-marker".into()),
                    idempotency_key: None,
                }),
            ))
            .unwrap();
        source
            .append(event(
                "source-event:3",
                &run,
                0,
                p::EventPayload::ToolCallProposed(p::ToolCallProposedPayload {
                    call_id: p::ToolCallId("tool-call:sync".into()),
                    tool: p::ToolRef("tool:project-api".into()),
                    args: serde_json::json!({
                        "authorization": "Bearer private-sync-marker"
                    }),
                }),
            ))
            .unwrap();
        source
            .append(event(
                "source-event:4",
                &run,
                0,
                p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                    node_id: p::NodeId("memory:secret-ref".into()),
                    kind: p::MemoryNodeType("episode".into()),
                    content_ref: p::ContentRef("credential:private-sync-marker".into()),
                    tier: p::StabilityTier::Working,
                    confidence: p::Confidence(0.8),
                    scope: p::Scope("workspace:alpha".into()),
                    resting_activation: p::RestingActivation(0.1),
                    recency: p::Recency(104),
                }),
            ))
            .unwrap();
        source
            .append(event(
                "source-event:5",
                &run,
                0,
                p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                    node_id: p::NodeId("memory:safe-ref".into()),
                    kind: p::MemoryNodeType("episode".into()),
                    content_ref: p::ContentRef("artifact:safe-summary".into()),
                    tier: p::StabilityTier::Working,
                    confidence: p::Confidence(0.8),
                    scope: p::Scope("workspace:alpha/project:release".into()),
                    resting_activation: p::RestingActivation(0.1),
                    recency: p::Recency(105),
                }),
            ))
            .unwrap();

        let first = source
            .export_sync_batch(p::SyncExportRequest {
                schema_version: p::SchemaVersion(1),
                peer: configured_peer.clone(),
                aggregate: run.clone(),
                after_version: p::AggregateVersion {
                    schema_version: p::SchemaVersion(1),
                    aggregate: run.clone(),
                    value: 0,
                },
                limit: 2,
                redaction: redaction_policy(),
            })
            .unwrap();
        assert_eq!(first.from_version, 0);
        assert_eq!(first.to_version, 2);
        assert!(matches!(
            first.events[0].payload,
            p::SyncTransferPayload::Full(_)
        ));
        assert!(matches!(
            first.events[1].payload,
            p::SyncTransferPayload::Redacted { .. }
        ));
        assert!(first
            .clone()
            .into_authoritative_write(configured_peer.clone())
            .is_err());
        let cursor = source
            .sync_cursor(&configured_peer.reference, &run)
            .unwrap()
            .unwrap();
        assert_eq!(cursor.exported_version, 2);
    }

    let reopened =
        SqliteEventStore::open(database.path(), sync_options(configured_peer.clone())).unwrap();
    let persisted_cursor = reopened
        .sync_cursor(&configured_peer.reference, &run)
        .unwrap()
        .unwrap();
    assert_eq!(persisted_cursor.exported_version, 2);
    let second = reopened
        .export_sync_batch(p::SyncExportRequest {
            schema_version: p::SchemaVersion(1),
            peer: configured_peer.clone(),
            aggregate: run.clone(),
            after_version: p::AggregateVersion {
                schema_version: p::SchemaVersion(1),
                aggregate: run.clone(),
                value: 2,
            },
            limit: 10,
            redaction: redaction_policy(),
        })
        .unwrap();
    assert_eq!(second.from_version, 2);
    assert_eq!(second.to_version, 5);
    assert!(matches!(
        second.events[0].payload,
        p::SyncTransferPayload::Redacted { .. }
    ));
    assert!(matches!(
        second.events[1].payload,
        p::SyncTransferPayload::Redacted { .. }
    ));
    assert!(matches!(
        second.events[2].payload,
        p::SyncTransferPayload::Full(_)
    ));
    let exported = serde_json::to_string(&second).unwrap();
    for forbidden in [
        "private-sync-marker",
        "secret:private-sync-marker",
        "credential:private-sync-marker",
        "Bearer private-sync-marker",
    ] {
        assert!(!exported.contains(forbidden), "leaked marker: {forbidden}");
    }
    assert_eq!(
        reopened
            .sync_cursor(&configured_peer.reference, &run)
            .unwrap()
            .unwrap()
            .exported_version,
        5
    );
    reopened
        .append(event(
            "secret:forbidden-envelope-ref",
            &run,
            0,
            p::EventPayload::TurnComplete(p::TurnCompletePayload { turn_index: 1 }),
        ))
        .unwrap();
    assert!(reopened
        .export_sync_batch(p::SyncExportRequest {
            schema_version: p::SchemaVersion(1),
            peer: configured_peer.clone(),
            aggregate: run.clone(),
            after_version: p::AggregateVersion {
                schema_version: p::SchemaVersion(1),
                aggregate: run.clone(),
                value: 5,
            },
            limit: 1,
            redaction: redaction_policy(),
        })
        .is_err());
    assert_eq!(
        reopened
            .sync_cursor(&configured_peer.reference, &run)
            .unwrap()
            .unwrap()
            .exported_version,
        5
    );
    assert!(reopened
        .export_sync_batch(p::SyncExportRequest {
            schema_version: p::SchemaVersion(1),
            peer: peer("peer:phone"),
            aggregate: run.clone(),
            after_version: p::AggregateVersion {
                schema_version: p::SchemaVersion(1),
                aggregate: run,
                value: 5,
            },
            limit: 1,
            redaction: redaction_policy(),
        })
        .is_err());
}
