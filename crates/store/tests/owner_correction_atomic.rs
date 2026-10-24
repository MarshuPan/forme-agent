#![forbid(unsafe_code)]
#[allow(dead_code)]
mod support;
use forme_protocol as p;
use forme_store::{
    EventStore, FileContentKeyResolver, ProtectedContentRecord, SqliteEventStore,
    StoreContentProtection, StoreOptions,
};
use std::sync::Arc;
use support::TestDatabase;

struct Batch {
    expected: Vec<p::AggregateVersion>,
    events: Vec<p::Event>,
    sources: Vec<ProtectedContentRecord>,
    dependencies: Vec<p::EventId>,
}
fn store(db: &TestDatabase) -> SqliteEventStore {
    let store = SqliteEventStore::open(
        db.path(),
        StoreOptions {
            fts_enabled: true,
            ..StoreOptions::default()
        },
    )
    .unwrap();
    let keys = Arc::new(FileContentKeyResolver::new(db.path().with_extension("keys")).unwrap());
    store
        .enable_content_protection(
            StoreContentProtection::new(
                &p::VerifiedPrincipal("owner:atomic".into()),
                p::Scope("workspace:atomic".into()),
                keys,
            )
            .unwrap(),
        )
        .unwrap();
    store
}
fn event(id: &str, run: &str, payload: p::EventPayload) -> p::Event {
    p::Event::new(
        p::EventId(id.into()),
        p::RunId(run.into()),
        None,
        payload,
        p::SchemaVersion(1),
        10,
        p::Provenance {
            source: p::Source::OwnerControl,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    )
}
fn batch(store: &SqliteEventStore) -> Batch {
    let protection = store.content_protection().unwrap().unwrap();
    let scope = p::Scope("workspace:atomic".into());
    let subject = p::sha256_content_digest(b"owner correction source");
    let (_, body) = protection
        .seal_record(
            b"private correction text",
            &scope,
            "correction-body",
            &subject,
        )
        .unwrap();
    let (_, receipt) = protection
        .seal_record(b"private receipt", &scope, "correction-material", &subject)
        .unwrap();
    let events = vec![
        event(
            "event:retract",
            "run:source",
            p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                target_object: p::ObjectRef("frame:old".into()),
                evidence_lineage: p::LineageRef("lineage:owner".into()),
            }),
        ),
        event(
            "event:new",
            "run:aggregate",
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: p::CandidateId("candidate:new".into()),
                target: p::CandidateTargetRef("private correction text".into()),
                evidence_refs: vec![p::EvidenceRef("evidence:owner".into())],
                confidence: p::Confidence(0.4),
                provenance: p::Provenance {
                    source: p::Source::OwnerControl,
                    actor: p::Actor::Owner,
                    trust_tier: p::TrustTier::OwnerInput,
                    caused_by: None,
                },
                target_tier: p::StabilityTier::Working,
                capability_update: None,
                strategy_candidate: None,
            }),
        ),
        event(
            "event:receipt",
            "run:aggregate",
            p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
                deltas_ref: p::MemoryDeltasRef("workbench-correction-v1|safe test receipt".into()),
            }),
        ),
    ];
    Batch {
        expected: ["run:source", "run:aggregate"]
            .into_iter()
            .map(|run| p::AggregateVersion {
                schema_version: p::SchemaVersion(1),
                aggregate: p::RunId(run.into()),
                value: 0,
            })
            .collect(),
        events,
        sources: vec![body, receipt],
        dependencies: vec![p::EventId("event:new".into())],
    }
}
fn counts(db: &TestDatabase) -> Vec<i64> {
    let raw = rusqlite::Connection::open(db.path()).unwrap();
    [
        "events",
        "protected_content",
        "protected_rows",
        "protected_key_history",
        "protected_dependencies",
    ]
    .iter()
    .map(|table| {
        raw.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    })
    .collect()
}
fn append(store: &SqliteEventStore, b: Batch) -> p::Result<Vec<p::EventId>> {
    store.append_owner_correction(b.expected, b.events, b.sources, b.dependencies)
}

#[test]
fn late_sql_failure_rolls_back_sources_both_streams_projections_and_fts() {
    let db = TestDatabase::new();
    let store = store(&db);
    let original = batch(&store);
    let before = counts(&db);
    let connection = rusqlite::Connection::open(db.path()).unwrap();
    connection.execute_batch("CREATE TRIGGER correction_fault BEFORE INSERT ON events WHEN NEW.event_id='event:new' BEGIN SELECT RAISE(ABORT,'fixture late write fault'); END;").unwrap();
    assert!(append(&store, original).is_err());
    assert_eq!(counts(&db), before);
    assert!(store.run_ids().unwrap().is_empty());
    assert!(store.search("private", 10).unwrap().is_empty());
    connection
        .execute_batch("DROP TRIGGER correction_fault")
        .unwrap();
    drop(connection);
    let b = batch(&store);
    assert_eq!(append(&store, b).unwrap().len(), 3);
    assert_eq!(store.run_head(&p::RunId("run:source".into())).unwrap(), 1);
    assert_eq!(
        store.run_head(&p::RunId("run:aggregate".into())).unwrap(),
        2
    );
    assert_eq!(store.read_run(p::RunId("run:aggregate".into())).count(), 2);
}

#[test]
fn either_stream_cas_conflict_rejects_the_whole_batch_before_source_insertion() {
    for stale in [0, 1] {
        let db = TestDatabase::new();
        let store = store(&db);
        let mut b = batch(&store);
        b.expected[stale].value = 1;
        let before = counts(&db);
        assert!(append(&store, b)
            .unwrap_err()
            .0
            .contains("source version is stale"));
        assert_eq!(counts(&db), before);
        assert!(store.run_ids().unwrap().is_empty());
        assert!(store.search("private", 10).unwrap().is_empty());
    }
}

#[test]
fn inadmissible_events_or_unbound_dependencies_never_reach_the_writer() {
    let db = TestDatabase::new();
    let store = store(&db);
    let before = counts(&db);
    let mut b = batch(&store);
    b.dependencies = vec![p::EventId("event:outside".into())];
    assert!(append(&store, b).is_err());
    let mut b = batch(&store);
    b.events
        .push(support::event("event:unrelated", "run:source", 11));
    assert!(append(&store, b).is_err());
    let mut b = batch(&store);
    b.sources[1].subject_digest = p::sha256_content_digest(b"other");
    assert!(append(&store, b).is_err());
    assert_eq!(counts(&db), before);
}
