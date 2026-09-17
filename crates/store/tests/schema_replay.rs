#[allow(dead_code)]
mod support;

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};

use forme_protocol as p;
use forme_store::{
    EventStore, PayloadType, ProjectionName, ProjectionPath, ProjectionValue, SchemaSnapshot,
    SessionStatePath, SqliteEventStore, StoreOptions, TranscriptEntry, TranscriptPath, Upcaster,
};
use rusqlite::{params, Connection};
use serde_json::Value;
use support::{run_accepted_event, TestDatabase};

#[test]
fn payload_type_is_a_total_one_to_one_event_kind_mapping() {
    assert_eq!(PayloadType::ALL.len(), p::EventKind::ALL.len());

    let mut payload_types = BTreeSet::new();
    for kind in p::EventKind::ALL {
        let payload_type = PayloadType::from_event_kind(kind);
        assert_eq!(payload_type.event_kind(), kind);
        assert_eq!(payload_type.as_str(), kind.as_str());
        assert!(payload_types.insert(payload_type));
    }

    assert_eq!(payload_types.len(), p::EventKind::ALL.len());
    for payload_type in PayloadType::ALL {
        assert!(p::EventKind::ALL.contains(&payload_type.event_kind()));
    }
}

#[test]
fn current_schema_versions_are_independent_per_payload_type() {
    let database = TestDatabase::new();
    let config_run = p::RunId("per-payload-config-run".into());
    {
        let seed = SqliteEventStore::open(database.path(), options(1)).unwrap();
        seed.append(config_doctor_event(
            "per-payload-config-v1",
            &config_run,
            p::SchemaVersion(1),
        ))
        .unwrap();
    }

    let store = SqliteEventStore::open(database.path(), options(3)).unwrap();
    let unrelated_run = p::RunId("per-payload-unrelated-run".into());
    store
        .append(run_accepted_event(
            "per-payload-run-v1",
            &unrelated_run.0,
            2,
            None,
        ))
        .unwrap();
    let unrelated = store
        .read_run(unrelated_run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(unrelated[0].schema_version, p::SchemaVersion(1));

    let missing = store
        .read_run(config_run.clone())
        .next()
        .unwrap()
        .unwrap_err();
    assert_error_contains(&missing, &["missing upcaster", "ConfigDoctorReport"]);
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("config v1 to v2", "config-v1-v2@1", add_v2_finding),
        )
        .unwrap();
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(2),
            p::SchemaVersion(3),
            Upcaster::new("config v2 to v3", "config-v2-v3@1", add_v3_check),
        )
        .unwrap();
    let config = store
        .read_run(config_run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(config[0].schema_version, p::SchemaVersion(3));
}

#[test]
fn independent_handles_concurrently_register_the_same_migration_idempotently() {
    const HANDLES: usize = 16;
    let database = TestDatabase::new();
    let stores = (0..HANDLES)
        .map(|_| SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap())
        .collect::<Vec<_>>();
    let barrier = Arc::new(std::sync::Barrier::new(HANDLES));
    let handles = stores
        .into_iter()
        .map(|store| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store.register_upcaster(
                    PayloadType::ConfigDoctorReport,
                    p::SchemaVersion(1),
                    p::SchemaVersion(2),
                    Upcaster::new(
                        "shared concurrent migration",
                        "shared-concurrent-migration@1",
                        identity,
                    ),
                )
            })
        })
        .collect::<Vec<_>>();

    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert!(
        results.iter().all(Result::is_ok),
        "same-identity registrations must all succeed: {results:?}"
    );

    let count: i64 = Connection::open(database.path())
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations \
             WHERE payload_type = 'ConfigDoctorReport' AND from_version = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn s21_read_time_upcast_and_replay_leave_authoritative_history_unchanged() {
    let database = TestDatabase::new();
    let run_id = p::RunId("schema-run".into());
    let original_event = config_doctor_event("schema-event", &run_id, p::SchemaVersion(1));
    let original_payload = serde_json::to_vec(&original_event.payload).unwrap();

    {
        let store = SqliteEventStore::open(database.path(), options(1)).unwrap();
        store.append(original_event).unwrap();
    }

    let original_row = raw_event(&Connection::open(database.path()).unwrap(), "schema-event");
    assert_eq!(original_row.schema_version, 1);
    assert_eq!(original_row.payload, original_payload);

    let store = SqliteEventStore::open(database.path(), options(3)).unwrap();
    let error = store
        .read_run(run_id.clone())
        .next()
        .expect("the stored event must exist")
        .expect_err("v1 cannot be read by a v3 reader without an upcaster");
    assert_error_contains(
        &error,
        &["missing upcaster", "ConfigDoctorReport", "1", "3"],
    );

    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new(
                "record the v2 migration",
                "config-v1-v2/add-finding@1",
                add_v2_finding,
            ),
        )
        .unwrap();
    let error = store
        .read_run(run_id.clone())
        .next()
        .expect("the stored event must exist")
        .expect_err("a partial chain must fail at its missing edge");
    assert_error_contains(
        &error,
        &["missing upcaster", "ConfigDoctorReport", "2", "3"],
    );

    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(2),
            p::SchemaVersion(3),
            Upcaster::new(
                "record the v3 migration",
                "config-v2-v3/add-check@1",
                add_v3_check,
            ),
        )
        .unwrap();

    let events = store
        .read_run(run_id.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stream_seq, 1);
    assert_eq!(events[0].schema_version, p::SchemaVersion(3));
    let p::EventPayload::ConfigDoctorReport(payload) = &events[0].payload else {
        panic!("upcast changed the payload discriminator");
    };
    assert_eq!(
        payload.checks,
        vec![p::ConfigCheck::Provider, p::ConfigCheck::Shell]
    );
    assert_eq!(
        payload.findings,
        vec![p::ConfigFindingRef("added-by-v2".into())]
    );

    let connection = Connection::open(database.path()).unwrap();
    assert_eq!(
        migration_notes(&connection),
        vec![
            (
                "ConfigDoctorReport".to_string(),
                1,
                2,
                "record the v2 migration".to_string(),
                "config-v1-v2/add-finding@1".to_string(),
            ),
            (
                "ConfigDoctorReport".to_string(),
                2,
                3,
                "record the v3 migration".to_string(),
                "config-v2-v3/add-check@1".to_string(),
            ),
        ]
    );
    assert_eq!(raw_event(&connection, "schema-event"), original_row);

    let mut corrupted_transcript = store
        .load_transcript(run_id.clone())
        .unwrap()
        .expect("append must maintain the transcript cache");
    corrupted_transcript.entries.push(TranscriptEntry {
        event_id: p::EventId("cache-only-event".into()),
        run_id: run_id.clone(),
        turn_id: None,
        stream_seq: 1,
        kind: p::EventKind::ConfigDoctorReport,
        text: "not-authoritative".into(),
    });
    connection
        .execute(
            "UPDATE transcript SET state = ?1 WHERE run_id = ?2",
            params![
                serde_json::to_vec(&corrupted_transcript).unwrap(),
                &run_id.0
            ],
        )
        .unwrap();

    let before_replay = persistence_snapshot(&connection);
    let snapshot = SchemaSnapshot {
        schema: BTreeMap::from([(PayloadType::ConfigDoctorReport, p::SchemaVersion(3))]),
        policy_version: p::Version(7),
        loop_version: p::Version(11),
        model_profile: p::ModelProfileRef("replay-model".into()),
        tool_schema: p::Version(13),
    };

    let report = store.replay(run_id.clone(), snapshot.clone()).unwrap();

    assert_eq!(report.run, run_id);
    assert_eq!(report.schema_snapshot, snapshot);
    assert_eq!(report.replayed_session_state.run_id, Some(run_id.clone()));
    assert_eq!(report.replayed_session_state.last_stream_seq, 1);
    assert_eq!(report.replayed_transcript.run_id, Some(run_id.clone()));
    assert!(report.replayed_transcript.entries.is_empty());
    assert_eq!(report.replayed_transcript.last_stream_seq, 1);
    assert_eq!(report.diff_vs_current.len(), 1);
    let transcript_diff = &report.diff_vs_current[0];
    assert_eq!(transcript_diff.projection, ProjectionName::Transcript);
    assert_eq!(
        transcript_diff.path,
        ProjectionPath::Transcript(TranscriptPath::Entries)
    );
    assert_eq!(
        transcript_diff.current,
        ProjectionValue::TranscriptEntries(corrupted_transcript.entries)
    );
    assert_eq!(
        transcript_diff.replayed,
        ProjectionValue::TranscriptEntries(Vec::new())
    );

    assert_eq!(persistence_snapshot(&connection), before_replay);
    assert_eq!(raw_event(&connection, "schema-event"), original_row);
}

#[test]
fn upcaster_graph_rejects_empty_notes_downgrades_cycles_and_conflicts() {
    let store = SqliteEventStore::open_in_memory(options(3)).unwrap();

    let empty_note = store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("   ", "invalid-empty-note@1", identity),
        )
        .unwrap_err();
    assert_error_contains(&empty_note, &["migration note", "empty"]);

    let empty_identity = store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("valid note", "   ", identity),
        )
        .unwrap_err();
    assert_error_contains(
        &empty_identity,
        &["migration implementation identity", "empty"],
    );

    let downgrade = store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(3),
            p::SchemaVersion(2),
            Upcaster::new("invalid downgrade", "invalid-downgrade@1", identity),
        )
        .unwrap_err();
    assert_error_contains(&downgrade, &["downgrade", "3", "2"]);

    let cycle = store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(2),
            p::SchemaVersion(2),
            Upcaster::new("invalid self edge", "invalid-self-edge@1", identity),
        )
        .unwrap_err();
    assert_error_contains(&cycle, &["cycle", "2"]);

    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("accepted edge", "accepted-edge@1", identity),
        )
        .unwrap();
    let conflict = store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(3),
            Upcaster::new("different edge", "different-edge@1", identity),
        )
        .unwrap_err();
    assert_error_contains(&conflict, &["conflicting", "ConfigDoctorReport", "1"]);
}

#[test]
fn same_edge_different_migration_implementation_identity_is_rejected() {
    let store = SqliteEventStore::open_in_memory(options(2)).unwrap();
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("same note", "implementation-a@1", add_v2_finding),
        )
        .unwrap();

    let error = store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("same note", "implementation-b@1", identity),
        )
        .unwrap_err();

    assert_error_contains(
        &error,
        &["conflicting", "implementation", "ConfigDoctorReport", "1"],
    );
}

#[test]
fn same_identity_rebind_does_not_replace_the_bound_transform() {
    let database = TestDatabase::new();
    let run_id = p::RunId("same-identity-rebind-run".into());
    {
        let seed = SqliteEventStore::open(database.path(), options(1)).unwrap();
        seed.append(config_doctor_event(
            "same-identity-rebind-event",
            &run_id,
            p::SchemaVersion(1),
        ))
        .unwrap();
    }
    let store = SqliteEventStore::open(database.path(), options(2)).unwrap();
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("stable edge", "stable-edge@1", add_v2_finding),
        )
        .unwrap();
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("stable edge", "stable-edge@1", identity),
        )
        .unwrap();

    let event = store.read_run(run_id).next().unwrap().unwrap();
    let p::EventPayload::ConfigDoctorReport(report) = event.payload else {
        panic!("upcaster changed the payload discriminator");
    };
    assert_eq!(
        report.findings,
        vec![p::ConfigFindingRef("added-by-v2".into())]
    );
}

#[test]
fn upcaster_chain_rejects_an_edge_that_overshoots_the_target() {
    let database = TestDatabase::new();
    let run_id = p::RunId("overshoot-run".into());
    {
        let store = SqliteEventStore::open(database.path(), options(1)).unwrap();
        store
            .append(config_doctor_event(
                "overshoot-event",
                &run_id,
                p::SchemaVersion(1),
            ))
            .unwrap();
    }

    let store = SqliteEventStore::open(database.path(), options(2)).unwrap();
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(3),
            Upcaster::new(
                "edge beyond requested target",
                "overshooting-edge@1",
                identity,
            ),
        )
        .unwrap();

    let error = store
        .read_run(run_id)
        .next()
        .expect("the stored event must exist")
        .unwrap_err();
    assert_error_contains(
        &error,
        &[
            "missing upcaster chain",
            "ConfigDoctorReport",
            "1",
            "3",
            "2",
        ],
    );
}

#[test]
fn reopening_rebinds_identical_metadata_and_rejects_persisted_conflicts() {
    let database = TestDatabase::new();
    let run_id = p::RunId("persisted-rebind-run".into());
    {
        let store = SqliteEventStore::open(database.path(), options(1)).unwrap();
        store
            .append(config_doctor_event(
                "persisted-rebind-event",
                &run_id,
                p::SchemaVersion(1),
            ))
            .unwrap();
        store
            .register_upcaster(
                PayloadType::ConfigDoctorReport,
                p::SchemaVersion(1),
                p::SchemaVersion(2),
                Upcaster::new("persisted edge", "persisted-edge@1", add_v2_finding),
            )
            .unwrap();
    }

    {
        let rebound = SqliteEventStore::open(database.path(), options(2)).unwrap();
        rebound
            .register_upcaster(
                PayloadType::ConfigDoctorReport,
                p::SchemaVersion(1),
                p::SchemaVersion(2),
                Upcaster::new("persisted edge", "persisted-edge@1", add_v2_finding),
            )
            .unwrap();
        let event = rebound
            .read_run(run_id.clone())
            .next()
            .expect("the v1 event must still exist")
            .unwrap();
        assert_eq!(event.schema_version, p::SchemaVersion(2));
        let p::EventPayload::ConfigDoctorReport(report) = event.payload else {
            panic!("rebound upcaster changed the payload discriminator");
        };
        assert_eq!(
            report.findings,
            vec![p::ConfigFindingRef("added-by-v2".into())]
        );
    }

    let conflicting_identity = SqliteEventStore::open(database.path(), options(2)).unwrap();
    let identity_error = conflicting_identity
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("persisted edge", "persisted-edge@2", add_v2_finding),
        )
        .unwrap_err();
    assert_error_contains(
        &identity_error,
        &[
            "conflicting persisted",
            "implementation",
            "ConfigDoctorReport",
            "1",
        ],
    );

    let conflicting_target = SqliteEventStore::open(database.path(), options(3)).unwrap();
    let target_error = conflicting_target
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(3),
            Upcaster::new("persisted edge", "persisted-target-conflict@1", identity),
        )
        .unwrap_err();
    assert_error_contains(
        &target_error,
        &["conflicting persisted", "ConfigDoctorReport", "1"],
    );

    let conflicting_note = SqliteEventStore::open(database.path(), options(2)).unwrap();
    let note_error = conflicting_note
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("different note", "persisted-edge@1", identity),
        )
        .unwrap_err();
    assert_error_contains(
        &note_error,
        &["conflicting persisted", "ConfigDoctorReport", "1"],
    );

    assert_eq!(
        migration_notes(&Connection::open(database.path()).unwrap()),
        vec![(
            "ConfigDoctorReport".to_string(),
            1,
            2,
            "persisted edge".to_string(),
            "persisted-edge@1".to_string(),
        )]
    );
}

#[test]
fn replay_transform_can_reenter_upcaster_registration_without_deadlock() {
    let database = TestDatabase::new();
    let run_id = p::RunId("reentrant-registration-run".into());
    {
        let seed = SqliteEventStore::open(database.path(), options(1)).unwrap();
        seed.append(config_doctor_event(
            "reentrant-registration-event",
            &run_id,
            p::SchemaVersion(1),
        ))
        .unwrap();
    }

    let store = SqliteEventStore::open(database.path(), options(2)).unwrap();
    let registrar_slot = Arc::new(Mutex::new(Some(store.clone())));
    let transform_slot = Arc::clone(&registrar_slot);
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new(
                "reenter registration",
                "reenter-registration@1",
                move |bytes| {
                    let registrar = transform_slot
                        .lock()
                        .map_err(|_| p::Error("registrar test slot is poisoned".into()))?
                        .as_ref()
                        .ok_or_else(|| p::Error("registrar test slot is empty".into()))?
                        .clone();
                    registrar.register_upcaster(
                        PayloadType::RunAccepted,
                        p::SchemaVersion(1),
                        p::SchemaVersion(2),
                        Upcaster::new(
                            "registered from replay transform",
                            "registered-from-replay@1",
                            identity,
                        ),
                    )?;
                    Ok(bytes)
                },
            ),
        )
        .unwrap();

    let replay_store = store.clone();
    let replay_run = run_id.clone();
    let report = run_with_timeout("reentrant replay registration", move || {
        replay_store.replay(
            replay_run,
            snapshot(BTreeMap::from([(
                PayloadType::ConfigDoctorReport,
                p::SchemaVersion(2),
            )])),
        )
    })
    .unwrap();
    assert_eq!(report.replayed_session_state.last_stream_seq, 1);
    registrar_slot.lock().unwrap().take();

    assert_eq!(
        migration_notes(&Connection::open(database.path()).unwrap()),
        vec![
            (
                "ConfigDoctorReport".to_string(),
                1,
                2,
                "reenter registration".to_string(),
                "reenter-registration@1".to_string(),
            ),
            (
                "RunAccepted".to_string(),
                1,
                2,
                "registered from replay transform".to_string(),
                "registered-from-replay@1".to_string(),
            ),
        ]
    );
}

#[test]
fn rebuild_upcaster_can_reenter_the_same_store_without_deadlock() {
    let database = TestDatabase::new();
    let run_id = p::RunId("reentrant-rebuild-run".into());
    {
        let seed = SqliteEventStore::open(database.path(), options(1)).unwrap();
        seed.append(config_doctor_event(
            "reentrant-rebuild-event",
            &run_id,
            p::SchemaVersion(1),
        ))
        .unwrap();
    }

    let store = SqliteEventStore::open(database.path(), options(2)).unwrap();
    let reentrant_slot = Arc::new(Mutex::new(Some(store.clone())));
    let transform_slot = Arc::clone(&reentrant_slot);
    let reentrant_run = run_id.clone();
    let transform_ran = Arc::new(AtomicBool::new(false));
    let transform_flag = Arc::clone(&transform_ran);
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new(
                "reentrant rebuild read",
                "reentrant-rebuild-read@1",
                move |bytes| {
                    let reentrant_store = transform_slot
                        .lock()
                        .map_err(|_| p::Error("reentrant test slot is poisoned".into()))?
                        .as_ref()
                        .ok_or_else(|| p::Error("reentrant test slot is empty".into()))?
                        .clone();
                    reentrant_store
                        .load_session_state(reentrant_run.clone())?
                        .ok_or_else(|| p::Error("reentrant cache read returned no state".into()))?;
                    transform_flag.store(true, Ordering::SeqCst);
                    Ok(bytes)
                },
            ),
        )
        .unwrap();

    let rebuild_store = store.clone();
    run_with_timeout("reentrant projection rebuild", move || {
        rebuild_store.rebuild_builtin_projections()
    })
    .unwrap();
    assert!(transform_ran.load(Ordering::SeqCst));
    reentrant_slot.lock().unwrap().take();
}

#[test]
fn rebuild_aborts_without_replacing_caches_when_the_event_capture_changes() {
    let database = TestDatabase::new();
    let run_id = p::RunId("stale-rebuild-run".into());
    {
        let seed = SqliteEventStore::open(database.path(), options(1)).unwrap();
        seed.append(config_doctor_event(
            "stale-rebuild-seed",
            &run_id,
            p::SchemaVersion(1),
        ))
        .unwrap();
    }

    let store = SqliteEventStore::open(database.path(), options(2)).unwrap();
    let writer = SqliteEventStore::open(database.path(), options(2)).unwrap();
    let late_event = run_accepted_event("stale-rebuild-late", &run_id.0, 2, None);
    let append_completed = Arc::new(AtomicBool::new(false));
    let append_flag = Arc::clone(&append_completed);
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new(
                "append after rebuild capture",
                "append-after-rebuild-capture@1",
                move |bytes| {
                    writer.append(late_event.clone())?;
                    append_flag.store(true, Ordering::SeqCst);
                    Ok(bytes)
                },
            ),
        )
        .unwrap();

    let rebuild_store = store.clone();
    let error = run_with_timeout("stale projection rebuild", move || {
        rebuild_store.rebuild_builtin_projections()
    })
    .unwrap_err();
    assert_error_contains(&error, &["event set", "changed", "stale"]);
    assert!(append_completed.load(Ordering::SeqCst));

    let events = store
        .read_run(run_id.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(events.len(), 2);
    let session = store.load_session_state(run_id.clone()).unwrap().unwrap();
    assert_eq!(session.source, Some(p::Source::UserTurn));
    assert_eq!(session.last_stream_seq, 2);
    let transcript = store.load_transcript(run_id).unwrap().unwrap();
    assert_eq!(transcript.last_stream_seq, 2);
    assert_eq!(transcript.entries.len(), 1);
    assert_eq!(transcript.entries[0].text, "input-stale-rebuild-late");
}

#[test]
fn rebuild_aborts_without_replacing_caches_when_upcaster_graph_changes() {
    let database = TestDatabase::new();
    let run_id = p::RunId("upcaster-token-rebuild-run".into());
    {
        let seed = SqliteEventStore::open(database.path(), options(1)).unwrap();
        seed.append(config_doctor_event(
            "upcaster-token-rebuild-event",
            &run_id,
            p::SchemaVersion(1),
        ))
        .unwrap();
    }
    let store = SqliteEventStore::open(database.path(), options(2)).unwrap();
    let session_before = store.load_session_state(run_id.clone()).unwrap();
    let transcript_before = store.load_transcript(run_id.clone()).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new(
                "blocking rebuild transform",
                "blocking-rebuild-transform@1",
                move |bytes| {
                    started_tx
                        .send(())
                        .map_err(|_| p::Error("failed to signal rebuild transform".into()))?;
                    release_rx
                        .lock()
                        .map_err(|_| p::Error("release channel lock is poisoned".into()))?
                        .recv()
                        .map_err(|_| p::Error("failed to release rebuild transform".into()))?;
                    add_v2_finding(bytes)
                },
            ),
        )
        .unwrap();

    let rebuild_store = store.clone();
    let rebuild = thread::spawn(move || rebuild_store.rebuild_builtin_projections());
    started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("rebuild did not reach the captured upcaster graph");
    store
        .register_upcaster(
            PayloadType::RunAccepted,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new(
                "unrelated graph change",
                "unrelated-graph-change@1",
                identity,
            ),
        )
        .unwrap();
    release_tx.send(()).unwrap();

    let error = rebuild.join().unwrap().unwrap_err();
    assert_error_contains(&error, &["upcaster graph", "changed", "stale"]);
    assert_eq!(
        store.load_session_state(run_id.clone()).unwrap(),
        session_before
    );
    assert_eq!(store.load_transcript(run_id).unwrap(), transcript_before);
}

#[test]
fn replay_rejects_missing_snapshot_versions_and_downgrades() {
    let missing_store = SqliteEventStore::open_in_memory(options(1)).unwrap();
    let missing_run = p::RunId("missing-snapshot-run".into());
    missing_store
        .append(config_doctor_event(
            "missing-snapshot-event",
            &missing_run,
            p::SchemaVersion(1),
        ))
        .unwrap();
    let missing = missing_store
        .replay(missing_run, snapshot(BTreeMap::new()))
        .unwrap_err();
    assert_error_contains(
        &missing,
        &["schema snapshot", "ConfigDoctorReport", "missing"],
    );

    let downgrade_store = SqliteEventStore::open_in_memory(options(3)).unwrap();
    let downgrade_run = p::RunId("downgrade-run".into());
    downgrade_store
        .append(config_doctor_event(
            "downgrade-event",
            &downgrade_run,
            p::SchemaVersion(3),
        ))
        .unwrap();
    let downgrade = downgrade_store
        .replay(
            downgrade_run,
            snapshot(BTreeMap::from([(
                PayloadType::ConfigDoctorReport,
                p::SchemaVersion(2),
            )])),
        )
        .unwrap_err();
    assert_error_contains(&downgrade, &["downgrade", "3", "2"]);
}

#[test]
fn upcaster_transform_errors_remain_explicit_read_errors() {
    let database = TestDatabase::new();
    let run_id = p::RunId("transform-error-run".into());
    {
        let store = SqliteEventStore::open(database.path(), options(1)).unwrap();
        store
            .append(config_doctor_event(
                "transform-error-event",
                &run_id,
                p::SchemaVersion(1),
            ))
            .unwrap();
    }

    let store = SqliteEventStore::open(database.path(), options(2)).unwrap();
    store
        .register_upcaster(
            PayloadType::ConfigDoctorReport,
            p::SchemaVersion(1),
            p::SchemaVersion(2),
            Upcaster::new("failing transform", "failing-transform@1", |_| {
                Err(p::Error("transform deliberately failed".into()))
            }),
        )
        .unwrap();

    let error = store
        .read_run(run_id)
        .next()
        .expect("the stored event must exist")
        .unwrap_err();
    assert_error_contains(
        &error,
        &[
            "upcaster transform failed",
            "ConfigDoctorReport",
            "1",
            "2",
            "transform deliberately failed",
        ],
    );
}

#[test]
fn projection_diff_order_is_stable_and_typed() {
    let database = TestDatabase::new();
    let run_id = p::RunId("diff-order-run".into());
    let store = SqliteEventStore::open(database.path(), options(1)).unwrap();
    store
        .append(config_doctor_event(
            "diff-order-event",
            &run_id,
            p::SchemaVersion(1),
        ))
        .unwrap();

    let connection = Connection::open(database.path()).unwrap();
    let mut state = store.load_session_state(run_id.clone()).unwrap().unwrap();
    state.source = Some(p::Source::Internal);
    state.status = p::RunStatus::Failed;
    connection
        .execute(
            "UPDATE session_state SET state = ?1 WHERE run_id = ?2",
            params![serde_json::to_vec(&state).unwrap(), &run_id.0],
        )
        .unwrap();
    let mut transcript = store.load_transcript(run_id.clone()).unwrap().unwrap();
    transcript.entries.push(TranscriptEntry {
        event_id: p::EventId("diff-cache-event".into()),
        run_id: run_id.clone(),
        turn_id: None,
        stream_seq: 1,
        kind: p::EventKind::ConfigDoctorReport,
        text: "cache-only".into(),
    });
    connection
        .execute(
            "UPDATE transcript SET state = ?1 WHERE run_id = ?2",
            params![serde_json::to_vec(&transcript).unwrap(), &run_id.0],
        )
        .unwrap();

    let report = store
        .replay(
            run_id,
            snapshot(BTreeMap::from([(
                PayloadType::ConfigDoctorReport,
                p::SchemaVersion(1),
            )])),
        )
        .unwrap();

    assert_eq!(
        report
            .diff_vs_current
            .iter()
            .map(|diff| diff.path.clone())
            .collect::<Vec<_>>(),
        vec![
            ProjectionPath::SessionState(SessionStatePath::Source),
            ProjectionPath::SessionState(SessionStatePath::Status),
            ProjectionPath::Transcript(TranscriptPath::Entries),
        ]
    );
    assert_eq!(
        report.diff_vs_current[0].current,
        ProjectionValue::Source(p::Source::Internal)
    );
    assert_eq!(report.diff_vs_current[0].replayed, ProjectionValue::Absent);
    assert_eq!(
        report.diff_vs_current[1].current,
        ProjectionValue::RunStatus(p::RunStatus::Failed)
    );
    assert_eq!(
        report.diff_vs_current[1].replayed,
        ProjectionValue::RunStatus(p::RunStatus::Accepted)
    );
}

fn options(current_schema: u32) -> StoreOptions {
    let mut options = StoreOptions {
        fts_enabled: true,
        ..StoreOptions::default()
    };
    options.current_schema.insert(
        PayloadType::ConfigDoctorReport,
        p::SchemaVersion(current_schema),
    );
    options
}

fn snapshot(schema: BTreeMap<PayloadType, p::SchemaVersion>) -> SchemaSnapshot {
    SchemaSnapshot {
        schema,
        policy_version: p::Version(1),
        loop_version: p::Version(1),
        model_profile: p::ModelProfileRef("test-model".into()),
        tool_schema: p::Version(1),
    }
}

fn config_doctor_event(
    event_id: &str,
    run_id: &p::RunId,
    schema_version: p::SchemaVersion,
) -> p::Event {
    p::Event::new(
        p::EventId(event_id.into()),
        run_id.clone(),
        None,
        p::EventPayload::ConfigDoctorReport(p::ConfigDoctorReportPayload {
            checks: vec![p::ConfigCheck::Provider],
            findings: Vec::new(),
        }),
        schema_version,
        1,
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        },
    )
}

fn add_v2_finding(bytes: Vec<u8>) -> p::Result<Vec<u8>> {
    edit_config_doctor(bytes, |report| {
        report
            .get_mut("findings")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| p::Error("v1 findings are not an array".into()))?
            .push(Value::String("added-by-v2".into()));
        Ok(())
    })
}

fn add_v3_check(bytes: Vec<u8>) -> p::Result<Vec<u8>> {
    edit_config_doctor(bytes, |report| {
        report
            .get_mut("checks")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| p::Error("v2 checks are not an array".into()))?
            .push(Value::String("Shell".into()));
        Ok(())
    })
}

fn edit_config_doctor(
    bytes: Vec<u8>,
    edit: impl FnOnce(&mut serde_json::Map<String, Value>) -> p::Result<()>,
) -> p::Result<Vec<u8>> {
    let mut value = serde_json::from_slice::<Value>(&bytes)
        .map_err(|error| p::Error(format!("failed to parse migration input: {error}")))?;
    let report = value
        .get_mut("ConfigDoctorReport")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| p::Error("migration input is not ConfigDoctorReport".into()))?;
    edit(report)?;
    serde_json::to_vec(&value)
        .map_err(|error| p::Error(format!("failed to encode migration output: {error}")))
}

fn identity(bytes: Vec<u8>) -> p::Result<Vec<u8>> {
    Ok(bytes)
}

fn assert_error_contains(error: &p::Error, fragments: &[&str]) {
    for fragment in fragments {
        assert!(
            error
                .0
                .to_ascii_lowercase()
                .contains(&fragment.to_ascii_lowercase()),
            "expected error to contain {fragment:?}, got: {error}"
        );
    }
}

fn run_with_timeout<T: Send + 'static>(
    operation_name: &str,
    operation: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = operation();
        let _ = sender.send(result);
    });
    match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => {
            worker.join().expect("timed operation panicked");
            result
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("{operation_name} did not complete within two seconds")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            worker.join().expect("timed operation panicked");
            unreachable!("timed operation disconnected without a result")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawEvent {
    payload: Vec<u8>,
    schema_version: i64,
    checksum: String,
}

fn raw_event(connection: &Connection, event_id: &str) -> RawEvent {
    connection
        .query_row(
            "SELECT payload, schema_version, checksum FROM events WHERE event_id = ?1",
            params![event_id],
            |row| {
                Ok(RawEvent {
                    payload: row.get(0)?,
                    schema_version: row.get(1)?,
                    checksum: row.get(2)?,
                })
            },
        )
        .unwrap()
}

fn migration_notes(connection: &Connection) -> Vec<(String, i64, i64, String, String)> {
    let mut statement = connection
        .prepare(
            "SELECT payload_type, from_version, to_version, note, implementation_identity
             FROM schema_migrations
             ORDER BY payload_type, from_version",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct PersistenceSnapshot {
    events: Vec<(String, String, i64, Vec<u8>, i64, String)>,
    idempotency_keys: Vec<(String, String, String, String)>,
    session_state: Vec<(String, Vec<u8>, i64)>,
    transcript: Vec<(String, Vec<u8>, i64)>,
    fts_rows: i64,
    migrations: Vec<(String, i64, i64, String, String)>,
}

fn persistence_snapshot(connection: &Connection) -> PersistenceSnapshot {
    PersistenceSnapshot {
        events: query_rows(
            connection,
            "SELECT event_id, kind, stream_seq, payload, schema_version, checksum
             FROM events ORDER BY run_id, stream_seq",
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        ),
        idempotency_keys: query_rows(
            connection,
            "SELECT namespace, \"key\", event_id, run_id
             FROM idempotency_keys ORDER BY namespace, \"key\"",
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ),
        session_state: query_rows(
            connection,
            "SELECT run_id, state, last_stream_seq FROM session_state ORDER BY run_id",
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ),
        transcript: query_rows(
            connection,
            "SELECT run_id, state, last_stream_seq FROM transcript ORDER BY run_id",
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ),
        fts_rows: connection
            .query_row("SELECT COUNT(*) FROM events_fts", [], |row| row.get(0))
            .unwrap(),
        migrations: migration_notes(connection),
    }
}

fn query_rows<T>(
    connection: &Connection,
    sql: &str,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Vec<T> {
    let mut statement = connection.prepare(sql).unwrap();
    statement
        .query_map([], map)
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}
