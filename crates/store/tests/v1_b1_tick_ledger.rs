//! B1: durable owner-brain tick claims (prd/23 B1, requirements/16 R-TICK).

#[allow(dead_code)]
mod support;

use std::collections::BTreeMap;

use forme_protocol as p;
use forme_store::{
    BrainAuthorityClaim, BrainAuthorityStore, CognitiveTickClaimRequest, CognitiveTickLedger,
    CognitiveTickWindowState, EventStore, SqliteEventStore, StoreOptions, TickEventCursor,
    TickEventRange,
};
use support::TestDatabase;

const OWNER: &str = "owner:tick-ledger";

fn owner_digest() -> p::SchemaDigest {
    p::sha256_content_digest(OWNER.as_bytes())
}

fn guarded_store(database: &TestDatabase) -> (SqliteEventStore, forme_store::AuthorityLeaseGuard) {
    let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let binding = store
        .claim_brain_authority(BrainAuthorityClaim {
            owner_digest: owner_digest(),
            process_nonce: p::Nonce("runtime:tick-ledger".into()),
            now_ms: 1,
            lease_ttl: p::DurationMs(600_000),
        })
        .unwrap();
    let guard = store
        .install_authority_lease_guard(&binding, p::DurationMs(600_000))
        .unwrap();
    (store, guard)
}

fn cursor(entries: &[(&str, u64)]) -> TickEventCursor {
    TickEventCursor(
        entries
            .iter()
            .map(|(run, seq)| (p::RunId((*run).into()), *seq))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn range(from: &[(&str, u64)], to: &[(&str, u64)]) -> TickEventRange {
    TickEventRange {
        from: cursor(from),
        to: cursor(to),
    }
}

fn request(window: i64, nonce: &str, now: i64, range: TickEventRange) -> CognitiveTickClaimRequest {
    CognitiveTickClaimRequest {
        owner_digest: owner_digest(),
        window,
        lease_nonce: p::Nonce(nonce.into()),
        now_ms: now,
        lease_ttl: p::DurationMs(100),
        range,
        min_spacing: p::DurationMs(0),
    }
}

fn digest(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}

#[test]
fn a_window_is_fulfilled_once_and_duplicate_signals_never_rerun_it() {
    let database = TestDatabase::new();
    let (store, _guard) = guarded_store(&database);
    let window_range = range(&[], &[("run:a", 3)]);

    let claim = store
        .claim_tick(request(1_000, "nonce:first", 10, window_range.clone()))
        .unwrap()
        .expect("the first signal claims the window");
    assert_eq!(claim.attempt, 1);
    assert_eq!(claim.range, window_range);
    assert!(
        store
            .claim_tick(request(1_000, "nonce:duplicate", 20, window_range.clone()))
            .unwrap()
            .is_none(),
        "a live claim is never shared"
    );
    store.finish_tick(&claim, digest("report:first")).unwrap();
    assert!(
        store
            .claim_tick(request(1_000, "nonce:late", 10_000, window_range))
            .unwrap()
            .is_none(),
        "a finished window is never run again, even long after"
    );
    assert_eq!(
        store.tick_window(&owner_digest(), 1_000).unwrap(),
        Some(CognitiveTickWindowState::Finished {
            attempt: 1,
            report_digest: digest("report:first"),
            consumed: cursor(&[("run:a", 3)]),
        })
    );
    assert!(store.finish_tick(&claim, digest("report:again")).is_err());
}

#[test]
fn a_takeover_reads_the_recorded_range_and_fences_the_lost_attempt() {
    let database = TestDatabase::new();
    let (store, _guard) = guarded_store(&database);
    let recorded = range(&[], &[("run:a", 2)]);
    let lost = store
        .claim_tick(request(2_000, "nonce:crashed", 0, recorded.clone()))
        .unwrap()
        .unwrap();

    let newer_snapshot = range(&[], &[("run:a", 9), ("run:b", 4)]);
    let retry = store
        .claim_tick(request(2_000, "nonce:retry", 150, newer_snapshot))
        .unwrap()
        .expect("an expired claim can be taken over");
    assert_eq!(retry.attempt, 2);
    assert_eq!(
        retry.range, recorded,
        "a retried window reads exactly the inputs of the attempt it replaces"
    );
    assert!(
        store.finish_tick(&lost, digest("report:lost")).is_err(),
        "the replaced attempt can no longer finish"
    );
    store.finish_tick(&retry, digest("report:retry")).unwrap();
}

#[test]
fn abandoned_windows_retry_and_consumption_stays_inside_the_range() {
    let database = TestDatabase::new();
    let (store, _guard) = guarded_store(&database);
    let window_range = range(&[("run:a", 1)], &[("run:a", 5), ("run:b", 2)]);
    store
        .claim_tick(request(3_000, "nonce:stuck", 0, window_range.clone()))
        .unwrap()
        .unwrap();
    store.abandon_expired_tick(&owner_digest(), 50).unwrap();
    assert_eq!(
        store.tick_window(&owner_digest(), 3_000).unwrap(),
        Some(CognitiveTickWindowState::Claimed {
            attempt: 1,
            expires_at: 100
        }),
        "a live claim is never abandoned"
    );
    store.abandon_expired_tick(&owner_digest(), 100).unwrap();
    assert_eq!(
        store.tick_window(&owner_digest(), 3_000).unwrap(),
        Some(CognitiveTickWindowState::Abandoned { attempt: 1 })
    );
    let retry = store
        .claim_tick(request(3_000, "nonce:resumed", 120, window_range))
        .unwrap()
        .unwrap();
    assert_eq!(retry.attempt, 2);

    assert!(
        store
            .finish_tick_consuming(&retry, digest("report:beyond"), cursor(&[("run:a", 6)]))
            .is_err(),
        "a tick cannot consume past its own range"
    );
    assert!(
        store
            .finish_tick_consuming(&retry, digest("report:backwards"), cursor(&[("run:a", 0)]))
            .is_err(),
        "a tick cannot move a cursor backwards"
    );
    // A deferred window keeps its inputs for the next window.
    store
        .finish_tick_consuming(&retry, digest("report:deferred"), cursor(&[("run:a", 1)]))
        .unwrap();
    assert_eq!(
        store.consumed_tick_cursor(&owner_digest()).unwrap(),
        cursor(&[("run:a", 1)])
    );
}

#[test]
fn finishing_binds_the_recorded_range_attempt_expiry_and_schema() {
    let database = TestDatabase::new();
    let (store, _guard) = guarded_store(&database);
    let claim = store
        .claim_tick(request(
            2_500,
            "nonce:bound",
            0,
            range(&[], &[("run:a", 2)]),
        ))
        .unwrap()
        .unwrap();
    let mut variants = Vec::new();
    let mut changed = claim.clone();
    changed.range.to = cursor(&[("run:a", 99)]);
    variants.push(changed);
    let mut changed = claim.clone();
    changed.attempt += 1;
    variants.push(changed);
    let mut changed = claim.clone();
    changed.expires_at += 1;
    variants.push(changed);
    let mut changed = claim.clone();
    changed.schema_version = p::SchemaVersion(2);
    variants.push(changed);
    for changed in variants {
        assert!(store
            .finish_tick(&changed, digest("report:changed"))
            .is_err());
        assert_eq!(
            store.consumed_tick_cursor(&owner_digest()).unwrap(),
            TickEventCursor::default()
        );
        assert_eq!(
            store.tick_window(&owner_digest(), claim.window).unwrap(),
            Some(CognitiveTickWindowState::Claimed {
                attempt: 1,
                expires_at: 100
            })
        );
    }

    // Reusing a nonce must not allow the old attempt to finish its successor.
    let successor = store
        .claim_tick(request(
            claim.window,
            "nonce:bound",
            150,
            TickEventRange::default(),
        ))
        .unwrap()
        .unwrap();
    assert!(store
        .finish_tick(&claim, digest("report:old-attempt"))
        .is_err());
    store
        .finish_tick(&successor, digest("report:recorded"))
        .unwrap();
    assert_eq!(
        store.consumed_tick_cursor(&owner_digest()).unwrap(),
        cursor(&[("run:a", 2)])
    );
}

#[test]
fn windows_are_sequential_and_honour_the_cadence() {
    let database = TestDatabase::new();
    let (store, _guard) = guarded_store(&database);
    let spaced = |window, nonce: &str, now| {
        let mut request = request(window, nonce, now, range(&[], &[]));
        request.min_spacing = p::DurationMs(500);
        request
    };
    let first = store
        .claim_tick(spaced(1_000, "nonce:w1", 1_000))
        .unwrap()
        .unwrap();
    assert!(
        store
            .claim_tick(spaced(2_000, "nonce:w2-early", 1_010))
            .unwrap()
            .is_none(),
        "no newer window opens while an earlier one is unfinished"
    );
    store.finish_tick(&first, digest("report:w1")).unwrap();
    assert!(
        store
            .claim_tick(spaced(1_200, "nonce:inside", 1_200))
            .unwrap()
            .is_none(),
        "a signal inside the cadence never opens a second window"
    );
    assert!(
        store
            .claim_tick(spaced(900, "nonce:backwards", 1_300))
            .unwrap()
            .is_none(),
        "windows only move forward"
    );
    let second = store
        .claim_tick(spaced(1_500, "nonce:w2", 1_500))
        .unwrap()
        .unwrap();
    assert_eq!(second.attempt, 1);
    assert_eq!(
        store.latest_tick_window(&owner_digest()).unwrap(),
        Some((
            1_500,
            CognitiveTickWindowState::Claimed {
                attempt: 1,
                expires_at: 1_600
            }
        ))
    );

    // The runtime crashed inside window 1_500: nothing newer may read past
    // it until the same window is retried with its recorded inputs.
    store.abandon_expired_tick(&owner_digest(), 5_000).unwrap();
    assert!(store
        .claim_tick(spaced(5_000, "nonce:w3", 5_000))
        .unwrap()
        .is_none());
    let retry = store
        .claim_tick(spaced(1_500, "nonce:w2-retry", 5_000))
        .unwrap()
        .expect("the unfinished window is retried first");
    assert_eq!(retry.attempt, 2);
    store.finish_tick(&retry, digest("report:w2")).unwrap();
    assert!(store
        .claim_tick(spaced(5_000, "nonce:w3", 5_000))
        .unwrap()
        .is_some());
}

#[test]
fn tick_claims_survive_restart_and_require_the_owner_fence() {
    let database = TestDatabase::new();
    {
        let (store, _guard) = guarded_store(&database);
        let claim = store
            .claim_tick(request(
                4_000,
                "nonce:durable",
                10,
                range(&[], &[("run:a", 1)]),
            ))
            .unwrap()
            .unwrap();
        store.finish_tick(&claim, digest("report:durable")).unwrap();
    }
    let (store, _guard) = guarded_store(&database);
    assert!(store
        .claim_tick(request(
            4_000,
            "nonce:after-restart",
            99_999,
            range(&[], &[])
        ))
        .unwrap()
        .is_none());
    assert_eq!(
        store.consumed_tick_cursor(&owner_digest()).unwrap(),
        cursor(&[("run:a", 1)])
    );

    let mut foreign = request(5_000, "nonce:foreign", 10, range(&[], &[]));
    foreign.owner_digest = digest("owner:someone-else");
    assert!(store.claim_tick(foreign).is_err());
    drop(store);

    let unfenced = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    assert!(unfenced
        .claim_tick(request(5_000, "nonce:unfenced", 10, range(&[], &[])))
        .is_err());
}

#[test]
fn the_consumed_cursor_grows_by_run_and_never_moves_back() {
    let database = TestDatabase::new();
    let (store, _guard) = guarded_store(&database);
    let first = store
        .claim_tick(request(
            1_000,
            "nonce:c1",
            1_000,
            range(&[], &[("run:a", 3)]),
        ))
        .unwrap()
        .unwrap();
    store.finish_tick(&first, digest("report:c1")).unwrap();
    // The next window records only the run that advanced since.
    let second = store
        .claim_tick(request(
            2_000,
            "nonce:c2",
            2_000,
            range(&[("run:a", 3)], &[("run:a", 5), ("run:b", 2)]),
        ))
        .unwrap()
        .unwrap();
    store.finish_tick(&second, digest("report:c2")).unwrap();
    assert_eq!(
        store.consumed_tick_cursor(&owner_digest()).unwrap(),
        cursor(&[("run:a", 5), ("run:b", 2)])
    );
    // A deferred window leaves every position where it was.
    let third = store
        .claim_tick(request(
            3_000,
            "nonce:c3",
            3_000,
            range(&[("run:a", 5)], &[("run:a", 7)]),
        ))
        .unwrap()
        .unwrap();
    store
        .finish_tick_consuming(&third, digest("report:c3"), cursor(&[("run:a", 5)]))
        .unwrap();
    assert_eq!(
        store.consumed_tick_cursor(&owner_digest()).unwrap(),
        cursor(&[("run:a", 5), ("run:b", 2)])
    );
}

#[test]
fn run_heads_snapshot_every_run_in_one_read() {
    let database = TestDatabase::new();
    let (store, _guard) = guarded_store(&database);
    for (id, run) in [("e1", "run:a"), ("e2", "run:a"), ("e3", "run:b")] {
        store
            .append(p::Event::new(
                p::EventId(id.into()),
                p::RunId(run.into()),
                None,
                p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                    source: p::Source::UserTurn,
                    session_ref: p::SessionId(format!("session:{id}")),
                    input_ref: p::InputRef("input".into()),
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
            ))
            .unwrap();
    }
    assert_eq!(
        store.run_heads().unwrap(),
        cursor(&[("run:a", 2), ("run:b", 1)])
    );
    assert_eq!(store.run_head(&p::RunId("run:a".into())).unwrap(), 2);
    assert_eq!(store.run_head(&p::RunId("run:none".into())).unwrap(), 0);
    let tail = store
        .read_run(p::RunId("run:a".into()))
        .after(1)
        .map(|event| event.unwrap().event_id.0)
        .collect::<Vec<_>>();
    assert_eq!(tail, vec!["e2".to_owned()]);
}
