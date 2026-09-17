#[allow(dead_code)]
mod support;

use std::sync::Arc;

use forme_protocol as p;
use forme_store::{
    ApprovalConsumptionClaim, ApprovalConsumptionLedger, BrainAuthorityClaim, BrainAuthorityStore,
    SqliteEventStore, StoreOptions, APPROVAL_CONSUMPTION_SCHEMA_VERSION,
};
use rusqlite::Connection;
use support::TestDatabase;

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

fn owner(value: &str) -> p::VerifiedPrincipal {
    p::VerifiedPrincipal(value.into())
}

fn claim(approver: &p::VerifiedPrincipal, suffix: &str, now: i64) -> ApprovalConsumptionClaim {
    ApprovalConsumptionClaim {
        schema_version: APPROVAL_CONSUMPTION_SCHEMA_VERSION,
        approval_id: p::ApprovalId(format!("approval:{suffix}")),
        approver: approver.clone(),
        nonce: p::Nonce(format!("nonce:{suffix}")),
        plan_digest: p::PlanDigest(p::sha256_content_digest(format!("plan:{suffix}").as_bytes()).0),
        normalized_action_digest: p::sha256_content_digest(format!("action:{suffix}").as_bytes()),
        affected_resources: vec![
            p::ResourceRef(format!("file:{suffix}:a")),
            p::ResourceRef(format!("file:{suffix}:b")),
        ],
        policy_version: p::Version(3),
        tool_schema_version: p::Version(7),
        now,
    }
}

fn guarded_store(
    database: &TestDatabase,
    configured_owner: &p::VerifiedPrincipal,
) -> (SqliteEventStore, forme_store::AuthorityLeaseGuard) {
    let store = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let now = now_ms();
    let binding = store
        .claim_brain_authority(BrainAuthorityClaim {
            owner_digest: p::sha256_content_digest(configured_owner.0.as_bytes()),
            process_nonce: p::Nonce(format!("runtime:{}", configured_owner.0)),
            now_ms: now,
            lease_ttl: p::DurationMs(60_000),
        })
        .unwrap();
    let guard = store
        .install_authority_lease_guard(&binding, p::DurationMs(60_000))
        .unwrap();
    (store, guard)
}

fn consumption_count(database: &TestDatabase) -> i64 {
    Connection::open(database.path())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM approval_consumptions", [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn claim_is_restart_durable_and_exactly_once() {
    let database = TestDatabase::new();
    let configured_owner = owner("owner:approval-restart");
    let claim = claim(&configured_owner, "restart", now_ms());

    {
        let (store, guard) = guarded_store(&database, &configured_owner);
        assert!(store.claim_approval(&claim).unwrap());

        let mut later_retry = claim.clone();
        later_retry.now += 100;
        assert!(!store.claim_approval(&later_retry).unwrap());
        assert_eq!(consumption_count(&database), 1);

        drop(guard);
        drop(store);
    }

    let (store, guard) = guarded_store(&database, &configured_owner);
    assert!(!store.claim_approval(&claim).unwrap());
    assert_eq!(consumption_count(&database), 1);
    drop(guard);
}

#[test]
fn same_nonce_with_changed_semantics_is_rejected_without_a_second_row() {
    let database = TestDatabase::new();
    let configured_owner = owner("owner:approval-conflict");
    let (store, guard) = guarded_store(&database, &configured_owner);
    let original = claim(&configured_owner, "conflict", now_ms());
    assert!(store.claim_approval(&original).unwrap());

    let mut variants = Vec::new();
    let mut approval = original.clone();
    approval.approval_id = p::ApprovalId("approval:changed".into());
    variants.push(approval);
    let mut plan = original.clone();
    plan.plan_digest = p::PlanDigest(p::sha256_content_digest(b"plan:changed").0);
    variants.push(plan);
    let mut action = original.clone();
    action.normalized_action_digest = p::sha256_content_digest(b"action:changed");
    variants.push(action);
    let mut resources = original.clone();
    resources.affected_resources[1] = p::ResourceRef("file:changed".into());
    variants.push(resources);
    let mut policy = original.clone();
    policy.policy_version = p::Version(4);
    variants.push(policy);
    let mut tool_schema = original.clone();
    tool_schema.tool_schema_version = p::Version(8);
    variants.push(tool_schema);
    let mut approver = original.clone();
    approver.approver = owner("owner:other");
    variants.push(approver);

    for variant in variants {
        assert!(
            store.claim_approval(&variant).is_err(),
            "a changed approval binding must conflict"
        );
    }
    assert_eq!(consumption_count(&database), 1);
    drop(guard);
}

#[test]
fn missing_guard_and_wrong_owner_fail_before_writing() {
    let database = TestDatabase::new();
    let configured_owner = owner("owner:approval-guard");
    let unguarded = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    let candidate = claim(&configured_owner, "no-guard", now_ms());
    assert!(unguarded.claim_approval(&candidate).is_err());
    assert_eq!(consumption_count(&database), 0);
    drop(unguarded);

    let (store, guard) = guarded_store(&database, &configured_owner);
    let wrong_owner = claim(&owner("owner:wrong"), "wrong-owner", now_ms());
    assert!(store.claim_approval(&wrong_owner).is_err());
    assert_eq!(consumption_count(&database), 0);
    drop(guard);
}

#[test]
fn a_taken_over_writer_guard_cannot_claim() {
    let database = TestDatabase::new();
    let configured_owner = owner("owner:approval-fence");
    let (store, guard) = guarded_store(&database, &configured_owner);
    let now = now_ms();
    let takeover = SqliteEventStore::open(database.path(), StoreOptions::default()).unwrap();
    takeover
        .claim_brain_authority(BrainAuthorityClaim {
            owner_digest: p::sha256_content_digest(configured_owner.0.as_bytes()),
            process_nonce: p::Nonce("runtime:takeover".into()),
            now_ms: now.saturating_add(120_000),
            lease_ttl: p::DurationMs(60_000),
        })
        .unwrap();

    let candidate = claim(&configured_owner, "fenced", now);
    assert!(store.claim_approval(&candidate).is_err());
    assert_eq!(consumption_count(&database), 0);
    drop(guard);
    drop(takeover);
}

#[test]
fn concurrent_claims_have_one_first_success() {
    let database = TestDatabase::new();
    let configured_owner = owner("owner:approval-concurrent");
    let (store, guard) = guarded_store(&database, &configured_owner);
    let claim = Arc::new(claim(&configured_owner, "concurrent", now_ms()));
    let left_store = store.clone();
    let right_store = store.clone();
    let left_claim = claim.clone();
    let right_claim = claim.clone();

    let (left, right) = std::thread::scope(|scope| {
        let left = scope.spawn(move || left_store.claim_approval(&left_claim));
        let right = scope.spawn(move || right_store.claim_approval(&right_claim));
        (left.join().unwrap(), right.join().unwrap())
    });

    assert!(left.is_ok());
    assert!(right.is_ok());
    assert_eq!(usize::from(left.unwrap()) + usize::from(right.unwrap()), 1);
    assert_eq!(consumption_count(&database), 1);
    drop(guard);
}
