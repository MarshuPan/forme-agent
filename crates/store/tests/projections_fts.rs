#[allow(dead_code)]
mod support;

use forme_protocol as p;
use forme_store::{
    EventStore, ProjectionScope, SessionStateProjection, SqliteEventStore, StoreOptions,
};
use rusqlite::{params, Connection};
use support::{collect, TestDatabase};

#[test]
fn s1_shape_keeps_online_and_pure_projections_equivalent() {
    let store = SqliteEventStore::open_in_memory(StoreOptions {
        fts_enabled: true,
        ..StoreOptions::default()
    })
    .unwrap();
    let run_a = p::RunId("run-a".into());
    let run_b = p::RunId("run-b".into());

    append_all(
        &store,
        s1_events(
            &run_a,
            "a",
            p::Source::UserTurn,
            90_000,
            "azimuthneedle model answer",
        ),
    );
    append_all(
        &store,
        s1_events(
            &run_b,
            "b",
            p::Source::Communication,
            10,
            "second model answer",
        ),
    );

    let events_a = collect(store.read_run(run_a.clone())).unwrap();
    let events_b = collect(store.read_run(run_b.clone())).unwrap();
    assert_eq!(
        events_a.iter().map(|event| event.kind).collect::<Vec<_>>(),
        events_b.iter().map(|event| event.kind).collect::<Vec<_>>()
    );
    assert_ne!(events_a[0].event_id, events_b[0].event_id);
    assert_ne!(events_a[0].ts_unix_ms, events_b[0].ts_unix_ms);
    assert_ne!(events_a[0].provenance.source, events_b[0].provenance.source);

    let online = store
        .load_session_state(run_a.clone())
        .unwrap()
        .expect("online session projection must exist");
    assert_eq!(online.status, p::RunStatus::Complete);
    assert_eq!(online.last_stream_seq, events_a.last().unwrap().stream_seq);
    assert_eq!(online.session_ref, Some(p::SessionId("session-a".into())));
    assert_eq!(
        online.workspace,
        Some(p::WorkspaceRef("workspace-a".into()))
    );
    assert_eq!(online.result_ref, Some(p::EventId("result-a".into())));

    let folded = store
        .project::<SessionStateProjection>(ProjectionScope::Run(run_a.clone()))
        .unwrap();
    assert_eq!(folded, online);

    let transcript = store
        .load_transcript(run_a)
        .unwrap()
        .expect("online transcript projection must exist");
    assert_eq!(
        transcript.last_stream_seq,
        events_a.last().unwrap().stream_seq
    );
    assert_eq!(
        transcript
            .entries
            .iter()
            .map(|entry| (entry.kind, entry.text.as_str(), entry.stream_seq))
            .collect::<Vec<_>>(),
        vec![
            (p::EventKind::RunAccepted, "input-a", 1),
            (
                p::EventKind::ModelCallDelta,
                "azimuthneedle model answer",
                7,
            ),
            (p::EventKind::ActionOutputDelta, "action output a", 8),
        ]
    );
}

#[test]
fn generic_projection_rejects_all_scope_without_cross_run_order() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    append_all(
        &store,
        s1_events(
            &p::RunId("run-a".into()),
            "a",
            p::Source::UserTurn,
            100,
            "model answer",
        ),
    );

    let error = store
        .project::<SessionStateProjection>(ProjectionScope::All)
        .unwrap_err();

    assert!(error.0.contains("cross-run ordering is undefined"));
}

#[test]
fn rebuild_replaces_only_builtin_projection_caches() {
    let database = TestDatabase::new();
    let run_a = p::RunId("run-rebuild-a".into());
    let run_b = p::RunId("run-rebuild-b".into());
    let expected_events_a;
    let expected_events_b;
    let expected_session_a;
    let expected_session_b;
    let expected_transcript_a;
    let expected_transcript_b;
    {
        let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
        append_all(
            &store,
            s1_events(
                &run_a,
                "rebuild-a",
                p::Source::UserTurn,
                5_000,
                "first rebuild model answer",
            ),
        );
        append_all(
            &store,
            s1_events(
                &run_b,
                "rebuild-b",
                p::Source::Communication,
                50,
                "second rebuild model answer",
            ),
        );
        expected_events_a = collect(store.read_run(run_a.clone())).unwrap();
        expected_events_b = collect(store.read_run(run_b.clone())).unwrap();
        expected_session_a = store.load_session_state(run_a.clone()).unwrap().unwrap();
        expected_session_b = store.load_session_state(run_b.clone()).unwrap().unwrap();
        expected_transcript_a = store.load_transcript(run_a.clone()).unwrap().unwrap();
        expected_transcript_b = store.load_transcript(run_b.clone()).unwrap().unwrap();
    }

    let connection = Connection::open(database.path()).unwrap();
    connection.execute("DELETE FROM session_state", []).unwrap();
    connection.execute("DELETE FROM transcript", []).unwrap();
    connection
        .execute(
            "INSERT INTO session_state(run_id, state, last_stream_seq) VALUES (?1, ?2, ?3)",
            params!["stale-run", b"{}", 999_i64],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO transcript(run_id, state, last_stream_seq) VALUES (?1, ?2, ?3)",
            params!["stale-run", b"{}", 999_i64],
        )
        .unwrap();
    drop(connection);

    let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    assert!(store.load_session_state(run_a.clone()).unwrap().is_none());
    assert!(store.load_session_state(run_b.clone()).unwrap().is_none());
    store.rebuild_builtin_projections().unwrap();

    assert_eq!(
        store.load_session_state(run_a.clone()).unwrap(),
        Some(expected_session_a)
    );
    assert_eq!(
        store.load_session_state(run_b.clone()).unwrap(),
        Some(expected_session_b)
    );
    assert_eq!(
        store.load_transcript(run_a.clone()).unwrap(),
        Some(expected_transcript_a)
    );
    assert_eq!(
        store.load_transcript(run_b.clone()).unwrap(),
        Some(expected_transcript_b)
    );
    assert!(store
        .load_session_state(p::RunId("stale-run".into()))
        .unwrap()
        .is_none());
    assert_eq!(collect(store.read_run(run_a)).unwrap(), expected_events_a);
    assert_eq!(collect(store.read_run(run_b)).unwrap(), expected_events_b);
}

#[test]
fn missing_projection_cache_row_aborts_append_until_rebuild() {
    let database = TestDatabase::new();
    let run_id = p::RunId("run-missing-cache".into());
    let store = SqliteEventStore::open(
        database.path(),
        StoreOptions {
            fts_enabled: true,
            ..StoreOptions::default()
        },
    )
    .unwrap();
    let events = s1_events(
        &run_id,
        "missing-cache",
        p::Source::UserTurn,
        1_000,
        "missingcachemodeltoken",
    );
    store.append(events[0].clone()).unwrap();
    let connection = Connection::open(database.path()).unwrap();
    connection
        .execute(
            "DELETE FROM transcript WHERE run_id = ?1",
            params![&run_id.0],
        )
        .unwrap();
    drop(connection);

    let error = store.append(events[6].clone()).unwrap_err();
    assert!(
        error.0.contains("projection cache discontinuity"),
        "{error}"
    );
    assert!(error.0.contains("transcript"), "{error}");
    let connection = Connection::open(database.path()).unwrap();
    let event_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(event_count, 1);
    drop(connection);
    assert!(store
        .search("missingcachemodeltoken", 10)
        .unwrap()
        .is_empty());

    store.rebuild_builtin_projections().unwrap();
    store.append(events[6].clone()).unwrap();
    assert_eq!(store.search("missingcachemodeltoken", 10).unwrap().len(), 1);
    assert_eq!(
        store
            .load_session_state(run_id.clone())
            .unwrap()
            .unwrap()
            .last_stream_seq,
        2
    );
    assert_eq!(
        store
            .load_transcript(run_id)
            .unwrap()
            .unwrap()
            .last_stream_seq,
        2
    );
}

#[test]
fn lagging_projection_caches_abort_append_until_rebuild() {
    let database = TestDatabase::new();
    let run_id = p::RunId("run-lagging-cache".into());
    let store = SqliteEventStore::open(
        database.path(),
        StoreOptions {
            fts_enabled: true,
            ..StoreOptions::default()
        },
    )
    .unwrap();
    let events = s1_events(
        &run_id,
        "lagging-cache",
        p::Source::UserTurn,
        2_000,
        "laggingcachemodeltoken",
    );
    store.append(events[0].clone()).unwrap();
    let connection = Connection::open(database.path()).unwrap();
    let cached_at_one = connection
        .query_row(
            "SELECT session_state.state, transcript.state
             FROM session_state JOIN transcript USING(run_id)
             WHERE run_id = ?1",
            params![&run_id.0],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .unwrap();
    drop(connection);
    store.append(events[6].clone()).unwrap();
    let connection = Connection::open(database.path()).unwrap();
    connection
        .execute(
            "UPDATE session_state SET state = ?1, last_stream_seq = 1 WHERE run_id = ?2",
            params![cached_at_one.0, &run_id.0],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE transcript SET state = ?1, last_stream_seq = 1 WHERE run_id = ?2",
            params![cached_at_one.1, &run_id.0],
        )
        .unwrap();
    drop(connection);

    let error = store.append(events[7].clone()).unwrap_err();
    assert!(
        error.0.contains("projection cache discontinuity"),
        "{error}"
    );
    assert!(error.0.contains("expected watermark 2"), "{error}");
    let connection = Connection::open(database.path()).unwrap();
    let event_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(event_count, 2);
    drop(connection);
    assert!(store.search("action", 10).unwrap().is_empty());

    store.rebuild_builtin_projections().unwrap();
    store.append(events[7].clone()).unwrap();
    assert_eq!(store.search("action", 10).unwrap().len(), 1);
    assert_eq!(
        store
            .load_session_state(run_id.clone())
            .unwrap()
            .unwrap()
            .last_stream_seq,
        3
    );
    assert_eq!(
        store
            .load_transcript(run_id)
            .unwrap()
            .unwrap()
            .last_stream_seq,
        3
    );
}

#[test]
fn enabling_fts_after_disabled_appends_requires_and_supports_backfill() {
    let database = TestDatabase::new();
    let run_id = p::RunId("run-fts-backfill".into());
    let historical = s1_events(
        &run_id,
        "fts-backfill",
        p::Source::UserTurn,
        3_000,
        "historicalbackfilltoken",
    );
    {
        let disabled = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
        disabled.append(historical[0].clone()).unwrap();
        disabled.append(historical[6].clone()).unwrap();
    }

    let store = SqliteEventStore::open(
        database.path(),
        StoreOptions {
            fts_enabled: true,
            ..StoreOptions::default()
        },
    )
    .unwrap();
    let stale = store.search("historicalbackfilltoken", 10).unwrap_err();
    assert!(stale.0.contains("stale"), "{stale}");
    assert!(stale.0.contains("rebuild"), "{stale}");

    store.rebuild_builtin_projections().unwrap();
    assert_eq!(
        store.search("historicalbackfilltoken", 10).unwrap().len(),
        1
    );

    let live_run = p::RunId("run-fts-live".into());
    let live = s1_events(
        &live_run,
        "fts-live",
        p::Source::UserTurn,
        4_000,
        "livecoverageafterrebuild",
    );
    store.append(live[6].clone()).unwrap();
    assert_eq!(
        store.search("livecoverageafterrebuild", 10).unwrap().len(),
        1
    );
}

#[test]
fn fts_search_returns_event_identity_and_disabled_store_errors() {
    let (enabled, run) = searchable_store();

    let hits = enabled.search("uniquemodeltoken", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].event_id, p::EventId("search-model-delta".into()));
    assert_eq!(hits[0].run_id, run);
    assert_eq!(hits[0].stream_seq, 7);
    assert_eq!(hits[0].kind, p::EventKind::ModelCallDelta);

    let disabled = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let error = disabled.search("anything", 10).unwrap_err();
    assert!(error.0.contains("full-text search is disabled"));
}

#[test]
fn fts_search_handles_zero_and_overflowing_limits() {
    let (store, _) = searchable_store();

    assert!(store.search("uniquemodeltoken", 0).unwrap().is_empty());

    #[cfg(target_pointer_width = "64")]
    {
        let error = store.search("uniquemodeltoken", usize::MAX).unwrap_err();
        assert_eq!(error, p::Error("full-text search limit is invalid".into()));
    }
}

#[test]
fn fts_search_propagates_empty_and_invalid_query_errors() {
    let (store, _) = searchable_store();

    for query in ["", "("] {
        let error = store.search(query, 10).unwrap_err();
        assert!(
            error.0.contains("full-text search") && error.0.contains("syntax error"),
            "unexpected error for query {query:?}: {error}"
        );
    }
}

fn append_all(store: &SqliteEventStore, events: Vec<p::Event>) {
    for event in events {
        store.append(event).unwrap();
    }
}

fn searchable_store() -> (SqliteEventStore, p::RunId) {
    let store = SqliteEventStore::open_in_memory(StoreOptions {
        fts_enabled: true,
        ..StoreOptions::default()
    })
    .unwrap();
    let run = p::RunId("run-search".into());
    append_all(
        &store,
        s1_events(
            &run,
            "search",
            p::Source::UserTurn,
            1_000,
            "uniquemodeltoken searchable answer",
        ),
    );
    (store, run)
}

fn s1_events(
    run_id: &p::RunId,
    prefix: &str,
    source: p::Source,
    base_ts: i64,
    model_delta: &str,
) -> Vec<p::Event> {
    let turn_id = p::TurnId(format!("turn-{prefix}"));
    let model_call_id = p::ModelCallId(format!("model-call-{prefix}"));
    let action_id = p::ActionId(format!("action-{prefix}"));
    let provenance = p::Provenance {
        source,
        actor: match source {
            p::Source::UserTurn => p::Actor::Owner,
            _ => p::Actor::Agent,
        },
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    };
    let payloads = vec![
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source,
            session_ref: p::SessionId(format!("session-{prefix}")),
            input_ref: p::InputRef(format!("input-{prefix}")),
            idempotency_key: Some(p::IdempotencyKey(format!("idempotency-{prefix}"))),
        }),
        p::EventPayload::SessionBound(p::SessionBoundPayload {
            policy_profile: p::PolicyProfileRef(format!("policy-{prefix}")),
            model_profile: p::ModelProfileRef(format!("model-{prefix}")),
            toolset_ref: p::ToolsetRef(format!("toolset-{prefix}")),
            workspace: p::WorkspaceRef(format!("workspace-{prefix}")),
            effect_mode: None,
            evolution_snapshot: None,
            federation_snapshot: None,
        }),
        p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 0 }),
        p::EventPayload::ContextBuildStarted(p::ContextBuildStartedPayload {
            sources: vec![p::ContextSource::Rules, p::ContextSource::History],
            slice_refs: vec![p::ContextSliceRef(format!("slice-{prefix}"))],
        }),
        p::EventPayload::ContextBuildFinished(p::ContextBuildFinishedPayload {
            sources: vec![p::ContextSource::Rules, p::ContextSource::History],
            slice_refs: vec![p::ContextSliceRef(format!("slice-{prefix}"))],
        }),
        p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
            call_id: model_call_id.clone(),
            model_profile: p::ModelProfileRef(format!("model-{prefix}")),
        }),
        p::EventPayload::ModelCallDelta(p::ModelCallDeltaPayload {
            call_id: model_call_id.clone(),
            delta: model_delta.into(),
        }),
        p::EventPayload::ActionOutputDelta(p::ActionOutputDeltaPayload {
            intent_id: action_id,
            backend: p::BackendKind::File,
            scope: p::Scope(format!("workspace-{prefix}")),
            delta: format!("action output {prefix}"),
            truncated: false,
            trust: p::TrustTier::Untrusted,
            content_ref: None,
            remote_lease: None,
        }),
        p::EventPayload::ModelCallFinished(p::ModelCallFinishedPayload {
            call_id: model_call_id,
            model_profile: p::ModelProfileRef(format!("model-{prefix}")),
            usage: p::ModelUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
            finish_reason: p::FinishReason("stop".into()),
        }),
        p::EventPayload::OutputClassified(p::OutputClassifiedPayload {
            kind: p::OutputKind::Final,
        }),
        p::EventPayload::TurnComplete(p::TurnCompletePayload { turn_index: 0 }),
        p::EventPayload::RunComplete(p::RunCompletePayload {
            stop_reason: p::StopReason("final-output".into()),
            result_ref: Some(p::EventId(format!("result-{prefix}"))),
        }),
    ];

    payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            let turn_id = match payload.kind() {
                p::EventKind::RunAccepted
                | p::EventKind::SessionBound
                | p::EventKind::RunComplete => None,
                _ => Some(turn_id.clone()),
            };
            p::Event::new(
                p::EventId(format!("{prefix}-{}", event_suffix(payload.kind()))),
                run_id.clone(),
                turn_id,
                payload,
                p::SchemaVersion(1),
                base_ts - i64::try_from(index).unwrap(),
                provenance.clone(),
            )
        })
        .collect()
}

fn event_suffix(kind: p::EventKind) -> &'static str {
    match kind {
        p::EventKind::RunAccepted => "accepted",
        p::EventKind::SessionBound => "bound",
        p::EventKind::TurnStarted => "turn-started",
        p::EventKind::ContextBuildStarted => "context-started",
        p::EventKind::ContextBuildFinished => "context-finished",
        p::EventKind::ModelCallStarted => "model-started",
        p::EventKind::ModelCallDelta => "model-delta",
        p::EventKind::ActionOutputDelta => "action-delta",
        p::EventKind::ModelCallFinished => "model-finished",
        p::EventKind::OutputClassified => "classified",
        p::EventKind::TurnComplete => "turn-complete",
        p::EventKind::RunComplete => "complete",
        _ => unreachable!("fixture only emits the S1-shaped sequence"),
    }
}
