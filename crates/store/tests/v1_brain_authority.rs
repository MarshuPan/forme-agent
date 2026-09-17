#[allow(dead_code)]
mod support;

use forme_protocol as p;
use forme_store::{
    BrainAggregateKind, BrainAuthorityClaim, BrainAuthorityStore, EventStore, SqliteEventStore,
    StoreOptions,
};
use rusqlite::Connection;
use support::TestDatabase;

fn digest(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}

fn claim(owner: &str, nonce: &str, now_ms: i64, ttl_ms: u64) -> BrainAuthorityClaim {
    BrainAuthorityClaim {
        owner_digest: digest(owner),
        process_nonce: p::Nonce(nonce.into()),
        now_ms,
        lease_ttl: p::DurationMs(ttl_ms),
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

#[test]
fn authority_binding_is_persistent_deterministic_and_event_free() {
    let database = TestDatabase::new();
    let first = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let binding = first
        .claim_brain_authority(claim("private-owner", "runtime-one", 1_000, 500))
        .unwrap();

    assert_eq!(binding.aggregates.len(), BrainAggregateKind::ALL.len());
    assert!(first.run_ids().unwrap().is_empty());
    assert!(
        first
            .release_brain_authority_lease(
                &binding.store_identity,
                &binding.writer_lease.process_nonce,
            )
            .unwrap()
    );
    drop(first);

    let reopened = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let rebound = reopened
        .claim_brain_authority(claim("private-owner", "runtime-two", 2_000, 500))
        .unwrap();
    assert_eq!(binding.owner_digest, rebound.owner_digest);
    assert_eq!(binding.store_identity, rebound.store_identity);
    assert_eq!(binding.aggregates, rebound.aggregates);
    assert_eq!(binding.created_at, rebound.created_at);
    assert!(reopened.run_ids().unwrap().is_empty());

    let metadata = reopened.brain_authority().unwrap().unwrap();
    assert_eq!(metadata.aggregates, binding.aggregates);
    assert_eq!(
        metadata
            .aggregates
            .get(&BrainAggregateKind::MemoryCognition),
        binding.aggregates.get(&BrainAggregateKind::MemoryCognition)
    );

    let raw = std::fs::read(database.path()).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("private-owner"));
}

#[test]
fn wrong_owner_and_live_second_writer_fail_closed_without_mutation() {
    let database = TestDatabase::new();
    let first = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let binding = first
        .claim_brain_authority(claim("owner-a", "runtime-a", 1_000, 1_000))
        .unwrap();
    let second = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();

    let writer_error = second
        .claim_brain_authority(claim("owner-a", "runtime-b", 1_500, 1_000))
        .unwrap_err();
    assert!(writer_error.0.contains("held by another runtime"));

    let owner_error = second
        .claim_brain_authority(claim("owner-b", "runtime-b", 1_500, 1_000))
        .unwrap_err();
    assert!(owner_error.0.contains("does not match"));
    assert_eq!(
        second.brain_authority().unwrap().unwrap().owner_digest,
        binding.owner_digest
    );
    assert!(second.run_ids().unwrap().is_empty());
}

#[test]
fn writer_lease_supports_exact_release_and_expiry_recovery() {
    let database = TestDatabase::new();
    let first = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let binding = first
        .claim_brain_authority(claim("owner", "runtime-a", 1_000, 100))
        .unwrap();
    let second = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();

    assert!(!second
        .release_brain_authority_lease(&binding.store_identity, &p::Nonce("wrong-runtime".into()),)
        .unwrap());
    second
        .claim_brain_authority(claim("owner", "runtime-b", 1_099, 100))
        .expect_err("an unexpired writer lease must fence a second runtime");

    let recovered = second
        .claim_brain_authority(claim("owner", "runtime-b", 1_100, 100))
        .expect("the expired writer lease must be recoverable");
    assert_eq!(recovered.store_identity, binding.store_identity);
    assert_eq!(recovered.aggregates, binding.aggregates);
    assert_eq!(recovered.writer_lease.expires_at, 1_200);
    assert!(second
        .release_brain_authority_lease(
            &recovered.store_identity,
            &recovered.writer_lease.process_nonce,
        )
        .unwrap());
}

#[test]
fn installed_guard_renews_the_exact_writer_and_fences_a_later_takeover() {
    let database = TestDatabase::new();
    let first = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let started_at = now_ms();
    let binding = first
        .claim_brain_authority(claim("owner", "runtime-a", started_at, 1))
        .unwrap();
    let guard = first
        .install_authority_lease_guard(&binding, p::DurationMs(60_000))
        .unwrap();

    first
        .append(support::event("guarded-a", "guarded-run", started_at))
        .expect("the installed guard must renew its exact lease before append");

    let second = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    second
        .claim_brain_authority(claim("owner", "runtime-b", started_at + 120_000, 60_000))
        .expect("an expired lease can be taken over by a new runtime");

    let stale_error = first
        .append(support::event("stale-a", "guarded-run", started_at + 1))
        .expect_err("the stale runtime must fail closed after authority takeover");
    assert!(stale_error.0.contains("released or taken over"));

    drop(guard);
    let third = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    third
        .claim_brain_authority(claim("owner", "runtime-c", started_at + 120_001, 60_000))
        .expect_err("dropping a stale guard must not release the new writer's lease");
}

#[test]
fn legacy_store_upgrade_adds_empty_authority_catalog_without_synthetic_facts() {
    let database = TestDatabase::new();
    {
        let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
        assert!(store.brain_authority().unwrap().is_none());
    }
    let connection = Connection::open(database.path()).unwrap();
    for table in [
        "brain_authority",
        "brain_aggregates",
        "authority_writer_lease",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table} must not contain synthetic facts");
    }
}
