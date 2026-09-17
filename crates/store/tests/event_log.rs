mod support;

use std::sync::{Arc, Barrier};

use forme_protocol as p;
use forme_store::{EventStore, Projection, ProjectionScope, SqliteEventStore, StoreOptions};
use rusqlite::{params, Connection};
use support::{action_planned_event, collect, event, run_accepted_event, test_store, TestDatabase};

#[test]
fn store_options_default_to_m0_schema_and_cursor_page_size() {
    let options = StoreOptions::default();

    assert!(!options.fts_enabled);
    assert_eq!(options.current_schema.len(), p::EventKind::ALL.len());
    for payload_type in forme_store::PayloadType::ALL {
        assert_eq!(
            options.current_schema.get(&payload_type),
            Some(&p::SchemaVersion(1))
        );
    }
    assert_eq!(options.cursor_page_size, 128);
}

#[test]
fn open_in_memory_rejects_zero_cursor_page_size() {
    let error = SqliteEventStore::open_in_memory(StoreOptions {
        cursor_page_size: 0,
        ..StoreOptions::default()
    })
    .err()
    .expect("zero cursor page size must be rejected");

    assert_eq!(
        error,
        p::Error("cursor page size must be greater than zero".into())
    );
}

#[test]
fn open_rejects_zero_cursor_page_size_before_creating_database() {
    let database = TestDatabase::new();
    assert!(!database.path().exists());

    let error = SqliteEventStore::open(
        database.path(),
        StoreOptions {
            cursor_page_size: 0,
            ..StoreOptions::default()
        },
    )
    .err()
    .expect("zero cursor page size must be rejected");

    assert_eq!(
        error,
        p::Error("cursor page size must be greater than zero".into())
    );
    assert!(!database.path().exists());
}

#[test]
fn append_assigns_stream_seq_and_ignores_clock_order() {
    let store = test_store();
    store.append(event("e-late", "run-1", 9_000)).unwrap();
    store.append(event("e-early", "run-1", 1)).unwrap();

    let events = collect(store.read_run(p::RunId("run-1".into()))).unwrap();

    assert_eq!(
        events
            .iter()
            .map(|event| event.stream_seq)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["e-late", "e-early"]
    );
}

#[test]
fn duplicate_event_id_returns_original_id_and_leaves_one_row() {
    let store = test_store();
    let original = event("event-id", "run-original", 1);
    let duplicate = original.clone();

    assert_eq!(
        store.append(original).unwrap(),
        p::EventId("event-id".into())
    );
    assert_eq!(
        store.append(duplicate).unwrap(),
        p::EventId("event-id".into())
    );

    assert_eq!(
        collect(store.read_run(p::RunId("run-original".into())))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn duplicate_event_id_with_different_semantics_is_a_collision() {
    let store = test_store();
    store.append(event("event-id", "run-original", 1)).unwrap();

    let error = store
        .append(event("event-id", "run-collision", 2))
        .unwrap_err();

    assert!(error.0.contains("idempotency collision"), "{error}");
    assert_eq!(
        collect(store.read_run(p::RunId("run-original".into())))
            .unwrap()
            .len(),
        1
    );
    assert!(collect(store.read_run(p::RunId("run-collision".into())))
        .unwrap()
        .is_empty());
}

#[test]
fn duplicate_run_idempotency_key_returns_original_event_id() {
    let store = SqliteEventStore::open_in_memory(StoreOptions {
        fts_enabled: true,
        ..StoreOptions::default()
    })
    .unwrap();

    let original = run_accepted_event("run-event-1", "run-1", 1, Some("request-key"));
    let mut retry = original.clone();
    retry.event_id = p::EventId("run-event-2".into());
    retry.ts_unix_ms = 2;
    let first = store.append(original).unwrap();
    let duplicate = store.append(retry).unwrap();

    assert_eq!(duplicate, first);
    assert_eq!(
        collect(store.read_run(p::RunId("run-1".into())))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn duplicate_run_request_ignores_new_envelope_ids_for_the_same_semantics() {
    let store = test_store();
    let original = run_accepted_event("run-event-1", "run-1", 1, Some("request-key"));
    let mut retry = original.clone();
    retry.event_id = p::EventId("run-event-2".into());
    retry.run_id = p::RunId("run-2".into());
    retry.ts_unix_ms = 2;

    let original_id = store.append(original).unwrap();
    assert_eq!(store.append(retry).unwrap(), original_id);
    assert!(collect(store.read_run(p::RunId("run-2".into())))
        .unwrap()
        .is_empty());
}

#[test]
fn duplicate_run_idempotency_key_with_different_request_is_a_collision() {
    let store = SqliteEventStore::open_in_memory(StoreOptions {
        fts_enabled: true,
        ..StoreOptions::default()
    })
    .unwrap();
    let first = store
        .append(run_accepted_event(
            "run-event-1",
            "run-1",
            1,
            Some("request-key"),
        ))
        .unwrap();

    let error = store
        .append(run_accepted_event(
            "run-event-2",
            "run-2",
            2,
            Some("request-key"),
        ))
        .unwrap_err();

    assert!(error.0.contains("idempotency collision"), "{error}");
    assert_eq!(
        store
            .load_session_state(p::RunId("run-1".into()))
            .unwrap()
            .unwrap()
            .last_stream_seq,
        1
    );
    assert!(store
        .load_session_state(p::RunId("run-2".into()))
        .unwrap()
        .is_none());
    assert!(store
        .search("\"input-run-event-2\"", 10)
        .unwrap()
        .is_empty());
    let mut retry = run_accepted_event("run-event-3", "run-1", 3, Some("request-key"));
    retry.payload = run_accepted_event("run-event-1", "run-1", 1, Some("request-key")).payload;
    assert_eq!(store.append(retry).unwrap(), first);
}

#[test]
fn duplicate_action_intent_returns_original_event_id() {
    let store = test_store();

    let original = action_planned_event("action-1", "run-1", "intent-1", 1);
    let mut retry = original.clone();
    retry.event_id = p::EventId("action-2".into());
    retry.ts_unix_ms = 2;
    let first = store.append(original).unwrap();
    let duplicate = store.append(retry).unwrap();

    assert_eq!(duplicate, first);
    assert_eq!(
        collect(store.read_run(p::RunId("run-1".into())))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn duplicate_action_intent_ignores_new_envelope_ids_for_the_same_plan() {
    let store = test_store();
    let original = action_planned_event("action-1", "run-1", "intent-1", 1);
    let mut retry = original.clone();
    retry.event_id = p::EventId("action-2".into());
    retry.run_id = p::RunId("run-2".into());
    retry.ts_unix_ms = 2;

    let original_id = store.append(original).unwrap();
    assert_eq!(store.append(retry).unwrap(), original_id);
    assert!(collect(store.read_run(p::RunId("run-2".into())))
        .unwrap()
        .is_empty());
}

#[test]
fn duplicate_action_intent_with_different_plan_is_a_collision() {
    let store = test_store();
    store
        .append(action_planned_event("action-1", "run-1", "intent-1", 1))
        .unwrap();

    let error = store
        .append(action_planned_event("action-2", "run-2", "intent-1", 2))
        .unwrap_err();

    assert!(error.0.contains("idempotency collision"), "{error}");
    assert_eq!(
        collect(store.read_run(p::RunId("run-1".into())))
            .unwrap()
            .len(),
        1
    );
    assert!(collect(store.read_run(p::RunId("run-2".into())))
        .unwrap()
        .is_empty());
}

#[test]
fn payload_kind_mismatch_fails_before_writing() {
    let store = test_store();
    let mut mismatched = event("mismatch", "run-1", 1);
    mismatched.kind = p::EventKind::TurnStarted;

    let error = store.append(mismatched).unwrap_err();

    assert!(error.0.contains("does not match payload kind"));
    assert!(collect(store.read_run(p::RunId("run-1".into())))
        .unwrap()
        .is_empty());
}

#[test]
fn non_current_schema_fails_before_writing() {
    let store = test_store();
    let mut future = event("future", "run-1", 1);
    future.schema_version = p::SchemaVersion(2);

    let error = store.append(future).unwrap_err();

    assert!(error.0.contains("schema version"));
    assert!(collect(store.read_run(p::RunId("run-1".into())))
        .unwrap()
        .is_empty());
}

#[test]
fn concurrent_appends_to_one_run_are_contiguous() {
    const APPENDS: usize = 32;
    let store = Arc::new(test_store());
    let barrier = Arc::new(Barrier::new(APPENDS));
    let handles = (0..APPENDS)
        .map(|index| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .append(event(
                        &format!("event-{index:02}"),
                        "run-concurrent",
                        index as i64,
                    ))
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().unwrap();
    }

    let events = collect(store.read_run(p::RunId("run-concurrent".into()))).unwrap();
    assert_eq!(events.len(), APPENDS);
    assert_eq!(
        events
            .iter()
            .map(|event| event.stream_seq)
            .collect::<Vec<_>>(),
        (1..=APPENDS as u64).collect::<Vec<_>>()
    );
}

#[test]
fn independent_store_handles_serialize_appends_to_one_run() {
    const APPENDS: usize = 16;
    let database = TestDatabase::new();
    let stores = [
        Arc::new(SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap()),
        Arc::new(SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap()),
    ];
    let barrier = Arc::new(Barrier::new(APPENDS));
    let handles = (0..APPENDS)
        .map(|index| {
            let store = Arc::clone(&stores[index % stores.len()]);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.append(event(
                    &format!("independent-event-{index:02}"),
                    "run-independent-handles",
                    index as i64,
                ))
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().unwrap().unwrap();
    }

    let events = collect(stores[0].read_run(p::RunId("run-independent-handles".into()))).unwrap();
    assert_eq!(events.len(), APPENDS);
    assert_eq!(
        events
            .iter()
            .map(|event| event.stream_seq)
            .collect::<Vec<_>>(),
        (1..=APPENDS as u64).collect::<Vec<_>>()
    );
}

#[test]
fn independent_store_handles_coalesce_concurrent_exact_domain_retries() {
    let database = TestDatabase::new();
    let stores = [
        SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap(),
        SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap(),
    ];
    let original = run_accepted_event("claim-1", "run-claim", 1, Some("shared-key"));
    let mut retry = original.clone();
    retry.event_id = p::EventId("claim-2".into());
    retry.ts_unix_ms = 2;
    let barrier = Arc::new(Barrier::new(2));
    let handles = stores
        .into_iter()
        .zip([original, retry])
        .map(|(store, event)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.append(event)
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(results[0], results[1]);
    let reader = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    assert_eq!(
        collect(reader.read_run(p::RunId("run-claim".into())))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn independent_store_handles_reject_concurrent_conflicting_domain_claims() {
    let database = TestDatabase::new();
    let stores = [
        SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap(),
        SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap(),
    ];
    let barrier = Arc::new(Barrier::new(2));
    let handles = stores
        .into_iter()
        .zip([
            run_accepted_event("claim-a", "run-a", 1, Some("shared-key")),
            run_accepted_event("claim-b", "run-b", 2, Some("shared-key")),
        ])
        .map(|(store, event)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.append(event)
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
    let error = results.into_iter().find_map(Result::err).unwrap();
    assert!(error.0.contains("idempotency collision"), "{error}");
    let reader = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let total = ["run-a", "run-b"]
        .into_iter()
        .map(|run| {
            collect(reader.read_run(p::RunId(run.into())))
                .unwrap()
                .len()
        })
        .sum::<usize>();
    assert_eq!(total, 1);
}

#[test]
fn independent_runs_each_start_at_one() {
    let store = test_store();
    store.append(event("a-1", "run-a", 1)).unwrap();
    store.append(event("b-1", "run-b", 2)).unwrap();
    store.append(event("a-2", "run-a", 3)).unwrap();
    store.append(event("b-2", "run-b", 4)).unwrap();

    for run_id in ["run-a", "run-b"] {
        let events = collect(store.read_run(p::RunId(run_id.into()))).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.stream_seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }
}

#[test]
fn cursor_uses_keyset_pages_without_losing_events() {
    let store = SqliteEventStore::open_in_memory(StoreOptions {
        cursor_page_size: 3,
        ..StoreOptions::default()
    })
    .unwrap();
    for index in 1..=8 {
        store
            .append(event(&format!("event-{index}"), "run-paged", 10 - index))
            .unwrap();
    }

    let events = collect(store.read_run(p::RunId("run-paged".into()))).unwrap();

    assert_eq!(events.len(), 8);
    assert_eq!(
        events
            .iter()
            .map(|event| event.stream_seq)
            .collect::<Vec<_>>(),
        (1..=8).collect::<Vec<_>>()
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_id.0.clone())
            .collect::<Vec<_>>(),
        (1..=8)
            .map(|index| format!("event-{index}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn reopening_a_file_database_preserves_rows() {
    let database = TestDatabase::new();
    {
        let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
        store.append(event("persisted", "run-1", 1)).unwrap();
    }

    let reopened = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let events = collect(reopened.read_run(p::RunId("run-1".into()))).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id, p::EventId("persisted".into()));
    assert_eq!(events[0].stream_seq, 1);
}

#[test]
fn failed_event_insert_rolls_back_idempotency_claim() {
    let database = TestDatabase::new();
    let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let connection = Connection::open(database.path()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_forced_event
             BEFORE INSERT ON events
             WHEN NEW.event_id = 'forced-failure'
             BEGIN
                 SELECT RAISE(ABORT, 'forced insert failure');
             END;",
        )
        .unwrap();

    let failed = store.append(run_accepted_event(
        "forced-failure",
        "run-1",
        1,
        Some("rollback-key"),
    ));
    assert!(failed.is_err());

    connection
        .execute_batch("DROP TRIGGER reject_forced_event;")
        .unwrap();
    let retried = store
        .append(run_accepted_event(
            "after-rollback",
            "run-1",
            2,
            Some("rollback-key"),
        ))
        .unwrap();

    assert_eq!(retried, p::EventId("after-rollback".into()));
    let events = collect(store.read_run(p::RunId("run-1".into()))).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id, p::EventId("after-rollback".into()));
    assert_eq!(events[0].stream_seq, 1);
}

#[test]
fn sqlite_schema_matches_contract_and_events_are_immutable() {
    let database = TestDatabase::new();
    let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    store.append(event("immutable", "run-1", 1)).unwrap();
    let connection = Connection::open(database.path()).unwrap();

    let event_columns = table_columns(&connection, "events");
    assert_eq!(
        event_columns,
        vec![
            "event_id",
            "run_id",
            "stream_seq",
            "turn_id",
            "kind",
            "payload",
            "schema_version",
            "ts",
            "provenance",
            "checksum",
        ]
    );
    assert_eq!(
        table_columns(&connection, "idempotency_keys"),
        vec!["namespace", "key", "event_id", "run_id", "semantic_bytes"]
    );

    let update_error = connection
        .execute(
            "UPDATE events SET ts = ts + 1 WHERE event_id = ?1",
            params!["immutable"],
        )
        .unwrap_err();
    let delete_error = connection
        .execute(
            "DELETE FROM events WHERE event_id = ?1",
            params!["immutable"],
        )
        .unwrap_err();
    assert!(update_error.to_string().contains("events are immutable"));
    assert!(delete_error.to_string().contains("events are immutable"));

    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
}

#[test]
fn open_transactionally_upgrades_the_supported_legacy_catalog() {
    let database = TestDatabase::new();
    let original = run_accepted_event("legacy-event", "legacy-run", 1, Some("legacy-request-key"));
    {
        let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
        store.append(original.clone()).unwrap();
    }
    replace_catalog_with_legacy_schema(database.path());

    let store = SqliteEventStore::open(database.path(), StoreOptions::default())
        .expect("supported legacy catalogs must upgrade during open");
    let mut retry = original;
    retry.event_id = p::EventId("legacy-retry-envelope".into());
    retry.run_id = p::RunId("legacy-retry-run".into());
    retry.ts_unix_ms = 9_999;
    assert_eq!(
        store.append(retry).unwrap(),
        p::EventId("legacy-event".into())
    );

    let connection = Connection::open(database.path()).unwrap();
    assert_eq!(
        table_columns(&connection, "idempotency_keys"),
        vec!["namespace", "key", "event_id", "run_id", "semantic_bytes"]
    );
    assert_eq!(
        table_columns(&connection, "schema_migrations"),
        vec![
            "payload_type",
            "from_version",
            "to_version",
            "note",
            "implementation_identity",
        ]
    );
    let user_version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(user_version, 6);
    assert_eq!(
        table_columns(&connection, "sync_peers"),
        vec!["singleton", "peer_ref", "owner_ref", "semantic_bytes"]
    );
    assert_eq!(
        table_columns(&connection, "sync_batches"),
        vec![
            "batch_id",
            "peer_ref",
            "aggregate",
            "expected_version",
            "semantic_bytes",
            "resulting_version",
            "applied_event_ids",
        ]
    );
    assert_eq!(
        table_columns(&connection, "sync_cursors"),
        vec![
            "peer_ref",
            "aggregate",
            "applied_version",
            "exported_version",
        ]
    );
    assert_eq!(
        table_columns(&connection, "ecosystem_versions"),
        vec!["aggregate", "version"]
    );
    assert_eq!(
        table_columns(&connection, "ecosystem_package_bundles"),
        vec!["release_ref", "package_ref", "package_digest", "bundle"]
    );
}

#[test]
fn open_blocks_legacy_migrations_without_an_implementation_identity() {
    let database = TestDatabase::new();
    {
        SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    }
    replace_catalog_with_legacy_schema(database.path());
    let connection = Connection::open(database.path()).unwrap();
    connection
        .execute(
            "INSERT INTO schema_migrations(payload_type, from_version, to_version, note)\
             VALUES ('ConfigDoctorReport', 1, 2, 'legacy migration')",
            [],
        )
        .unwrap();
    drop(connection);

    let error = SqliteEventStore::open(database.path(), StoreOptions::default())
        .err()
        .expect("an unverifiable legacy migration must fail during open");
    assert!(error.0.contains("implementation identity"), "{error}");

    let connection = Connection::open(database.path()).unwrap();
    assert!(!table_columns(&connection, "schema_migrations")
        .iter()
        .any(|column| column == "implementation_identity"));
}

#[test]
fn append_rejects_non_finite_floating_payloads_without_writing() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let store = test_store();
        let error = store
            .append(memory_node_event("non-finite-node", value, 0.5))
            .expect_err("non-finite confidence must be rejected");
        assert!(error.0.contains("finite"), "{error}");
        assert!(collect(store.read_run(p::RunId("non-finite-run".into())))
            .unwrap()
            .is_empty());

        let store = test_store();
        let error = store
            .append(memory_node_event("non-finite-resting", 0.5, value))
            .expect_err("non-finite resting activation must be rejected");
        assert!(error.0.contains("finite"), "{error}");
        assert!(collect(store.read_run(p::RunId("non-finite-run".into())))
            .unwrap()
            .is_empty());

        let store = test_store();
        let error = store
            .append(memory_edge_event("non-finite-edge", value))
            .expect_err("non-finite weight must be rejected");
        assert!(error.0.contains("finite"), "{error}");
        assert!(collect(store.read_run(p::RunId("non-finite-run".into())))
            .unwrap()
            .is_empty());
    }
}

#[test]
fn checksum_corruption_is_yielded_as_a_cursor_error() {
    let database = TestDatabase::new();
    {
        let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
        store.append(event("corrupt", "run-1", 1)).unwrap();
    }
    let connection = Connection::open(database.path()).unwrap();
    let update_trigger: String = connection
        .query_row(
            "SELECT name
             FROM sqlite_master
             WHERE type = 'trigger'
               AND tbl_name = 'events'
               AND lower(sql) LIKE '%before update%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let quoted_trigger = update_trigger.replace('"', "\"\"");
    connection
        .execute_batch(&format!("DROP TRIGGER \"{quoted_trigger}\";"))
        .unwrap();
    connection
        .execute(
            "UPDATE events SET payload = X'00' WHERE event_id = ?1",
            params!["corrupt"],
        )
        .unwrap();
    drop(connection);

    let reopened = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let error = reopened
        .read_run(p::RunId("run-1".into()))
        .next()
        .unwrap()
        .unwrap_err();

    assert!(error.0.contains("checksum mismatch"));
}

#[test]
fn checksum_is_deterministic_for_the_same_persisted_envelope() {
    let first_database = TestDatabase::new();
    let second_database = TestDatabase::new();
    let same_event = event("same-event", "same-run", 42);

    for database in [&first_database, &second_database] {
        let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
        store.append(same_event.clone()).unwrap();
    }

    let checksum = |database: &TestDatabase| {
        Connection::open(database.path())
            .unwrap()
            .query_row(
                "SELECT checksum FROM events WHERE event_id = ?1",
                params!["same-event"],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
    };

    assert_eq!(checksum(&first_database), checksum(&second_database));
}

#[test]
fn checksum_matches_fnv1a_known_vector_for_a_complete_envelope() {
    let database = TestDatabase::new();
    let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let complete_event = p::Event::new(
        p::EventId("vector-event-7f3a".into()),
        p::RunId("vector-run-c91d".into()),
        Some(p::TurnId("vector-turn-4e2b".into())),
        p::EventPayload::ActionPlanned(p::ActionPlannedPayload {
            intent_id: p::ActionId("vector-intent-a52c".into()),
            plan_digest: p::PlanDigest("sha256:7d4f91c0".into()),
            backend: p::BackendKind::Mcp,
            expected_effect: p::ExpectedEffect::Outward,
            source: p::Source::ProactiveJob,
            scope: p::Scope("workspace:alpha/resource:42".into()),
            approval_ref: Some(p::ApprovalId("approval-39bd".into())),
            remote_placement: None,
        }),
        p::SchemaVersion(1),
        1_725_987_654_321,
        p::Provenance {
            source: p::Source::ProactiveJob,
            actor: p::Actor::Subagent(p::RunId("actor-run-8ac1".into())),
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: Some(p::EventId("cause-event-f011".into())),
        },
    );

    store.append(complete_event).unwrap();
    let checksum: String = Connection::open(database.path())
        .unwrap()
        .query_row(
            "SELECT checksum FROM events WHERE event_id = ?1",
            params!["vector-event-7f3a"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(checksum, "6f644afe66322546");
}

#[test]
fn project_is_a_pure_ordered_fold_over_events() {
    struct SequenceProjection;

    impl Projection for SequenceProjection {
        type State = Vec<u64>;

        fn empty() -> Self::State {
            Vec::new()
        }

        fn apply(state: &mut Self::State, event: &p::Event) {
            state.push(event.stream_seq);
        }
    }

    let store = test_store();
    store.append(event("event-1", "run-1", 2)).unwrap();
    store.append(event("event-2", "run-1", 1)).unwrap();

    let state = store
        .project::<SequenceProjection>(ProjectionScope::run(p::RunId("run-1".into())))
        .unwrap();

    assert_eq!(state, vec![1, 2]);
}

fn table_columns(connection: &Connection, table: &str) -> Vec<String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    statement
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn replace_catalog_with_legacy_schema(path: &std::path::Path) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            r#"
            PRAGMA foreign_keys = OFF;
            BEGIN IMMEDIATE;

            ALTER TABLE idempotency_keys RENAME TO idempotency_keys_current;
            CREATE TABLE idempotency_keys (
                namespace TEXT NOT NULL,
                "key"     TEXT NOT NULL,
                event_id  TEXT NOT NULL,
                run_id    TEXT NOT NULL,
                UNIQUE(namespace, "key"),
                FOREIGN KEY(event_id) REFERENCES events(event_id)
                    DEFERRABLE INITIALLY DEFERRED
            );
            INSERT INTO idempotency_keys(namespace, "key", event_id, run_id)
            SELECT namespace, "key", event_id, run_id FROM idempotency_keys_current;
            DROP TABLE idempotency_keys_current;

            ALTER TABLE schema_migrations RENAME TO schema_migrations_current;
            CREATE TABLE schema_migrations (
                payload_type TEXT NOT NULL,
                from_version INTEGER NOT NULL,
                to_version   INTEGER NOT NULL,
                note         TEXT NOT NULL,
                PRIMARY KEY(payload_type, from_version),
                CHECK(length(trim(note)) > 0),
                CHECK(to_version > from_version)
            );
            INSERT INTO schema_migrations(payload_type, from_version, to_version, note)
            SELECT payload_type, from_version, to_version, note
            FROM schema_migrations_current;
            DROP TABLE schema_migrations_current;

            PRAGMA user_version = 0;
            COMMIT;
            PRAGMA foreign_keys = ON;
            "#,
        )
        .unwrap();
}

fn memory_node_event(event_id: &str, confidence: f32, resting_activation: f32) -> p::Event {
    p::Event::new(
        p::EventId(event_id.into()),
        p::RunId("non-finite-run".into()),
        None,
        p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
            node_id: p::NodeId(format!("node-{event_id}")),
            kind: p::MemoryNodeType("episode".into()),
            content_ref: p::ContentRef("content".into()),
            tier: p::StabilityTier::Working,
            confidence: p::Confidence(confidence),
            scope: p::Scope("workspace".into()),
            resting_activation: p::RestingActivation(resting_activation),
            recency: p::Recency(1),
        }),
        p::SchemaVersion(1),
        1,
        test_provenance(),
    )
}

fn memory_edge_event(event_id: &str, weight: f32) -> p::Event {
    p::Event::new(
        p::EventId(event_id.into()),
        p::RunId("non-finite-run".into()),
        None,
        p::EventPayload::MemoryEdgeAppended(p::MemoryEdgeAppendedPayload {
            edge_id: p::EdgeId(format!("edge-{event_id}")),
            from: p::NodeId("from".into()),
            to: p::NodeId("to".into()),
            kind: p::MemoryEdgeType("supports".into()),
            weight: p::Weight(weight),
        }),
        p::SchemaVersion(1),
        1,
        test_provenance(),
    )
}

fn test_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::UserTurn,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}
