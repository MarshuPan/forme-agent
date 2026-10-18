#[allow(dead_code)]
mod support;
use forme_protocol as p;
use forme_store::{
    BrainAuthorityClaim, BrainAuthorityStore, ContentKeyResolver, DataLifecycleRequest, EventStore,
    FileContentKeyResolver, SchemaSnapshot, SqliteEventStore, StoreContentProtection, StoreOptions,
};
use std::sync::Arc;
use support::{run_accepted_event, TestDatabase};

const PRIVATE: &str = "private-c22-content-c6e8b06b";
fn owner() -> p::VerifiedPrincipal {
    p::VerifiedPrincipal("owner:test-d".into())
}
fn scope() -> p::Scope {
    p::Scope("workspace:test-d".into())
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
fn protected(
    db: &TestDatabase,
) -> (
    SqliteEventStore,
    Arc<FileContentKeyResolver>,
    forme_store::AuthorityLeaseGuard,
) {
    let s = SqliteEventStore::open(
        db.path(),
        StoreOptions {
            fts_enabled: true,
            ..StoreOptions::default()
        },
    )
    .unwrap();
    let keys = Arc::new(FileContentKeyResolver::new(db.path().with_extension("keys")).unwrap());
    s.enable_content_protection(
        StoreContentProtection::new(&owner(), scope(), keys.clone()).unwrap(),
    )
    .unwrap();
    let binding = s
        .claim_brain_authority(BrainAuthorityClaim {
            owner_digest: p::sha256_content_digest(owner().0.as_bytes()),
            process_nonce: p::Nonce(format!("test:{}", now())),
            now_ms: now(),
            lease_ttl: p::DurationMs(60_000),
        })
        .unwrap();
    let guard = s
        .install_authority_lease_guard(&binding, p::DurationMs(60_000))
        .unwrap();
    (s, keys, guard)
}
fn input(id: &str, run: &str) -> p::Event {
    let mut e = run_accepted_event(id, run, now(), Some(run));
    if let p::EventPayload::RunAccepted(p) = &mut e.payload {
        p.input_ref = p::InputRef(PRIVATE.into());
    }
    e
}
fn request(run: &str, version: u64) -> DataLifecycleRequest {
    DataLifecycleRequest {
        schema_version: p::V1_CLOSURE_SCHEMA_VERSION,
        owner: owner(),
        scope: scope(),
        subject_digest: p::sha256_content_digest(run.as_bytes()),
        operation: p::DataLifecycleOperation::CryptoShred,
        expected_version: version,
        event_id: p::EventId(format!("lifecycle:{run}:{version}")),
        timestamp: now(),
        remote_disposition: p::RemoteDeletionDisposition::Unknown,
        evidence: vec![p::EvidenceRef("evidence:local-cleanup".into())],
    }
}
fn snapshot() -> SchemaSnapshot {
    SchemaSnapshot {
        schema: StoreOptions::default().current_schema,
        policy_version: p::Version(1),
        loop_version: p::Version(1),
        model_profile: p::ModelProfileRef("fixture:d".into()),
        tool_schema: p::Version(1),
    }
}
fn absent(bytes: &[u8]) {
    assert!(!bytes
        .windows(PRIVATE.len())
        .any(|w| w == PRIVATE.as_bytes()));
}

#[test]
fn c22_events_projections_idempotency_backup_and_restart_contain_no_plaintext() {
    let db = TestDatabase::new();
    let (s, keys, guard) = protected(&db);
    let event = input("event:d:private", "run:d:private");
    s.append(event.clone()).unwrap();
    s.append(event.clone()).unwrap();
    assert_eq!(s.search(PRIVATE, 10).unwrap().len(), 1);
    assert_eq!(
        s.read_run(event.run_id.clone())
            .next()
            .unwrap()
            .unwrap()
            .payload,
        event.payload
    );
    assert!(s
        .replay(event.run_id.clone(), snapshot())
        .unwrap()
        .diff_vs_current
        .is_empty());
    s.rebuild_builtin_projections().unwrap();
    let backup = db.path().with_extension("backup");
    let manifest = s.backup_service().create_cold_backup(&backup).unwrap();
    assert!(s
        .backup_service()
        .verify_backup(&backup)
        .unwrap()
        .is_valid());
    absent(&std::fs::read(&backup).unwrap());
    absent(&serde_json::to_vec(&manifest).unwrap());
    let raw = rusqlite::Connection::open(db.path()).unwrap();
    for table in ["events", "idempotency_keys", "transcript", "session_state"] {
        let column = match table {
            "events" => "payload",
            "idempotency_keys" => "semantic_bytes",
            _ => "state",
        };
        let mut stmt = raw
            .prepare(&format!("SELECT {column} FROM {table}"))
            .unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0)).unwrap();
        for row in rows {
            absent(&row.unwrap());
        }
    }
    drop(raw);
    drop(guard);
    drop(s);
    let (s, _, _guard) = protected(&db);
    assert_eq!(
        s.read_run(event.run_id).next().unwrap().unwrap().payload,
        event.payload
    );
    drop(keys);
}

#[test]
fn c22_missing_resolver_wrong_owner_and_historical_plaintext_fail_closed() {
    let db = TestDatabase::new();
    let (s, _keys, guard) = protected(&db);
    s.append(input("event:d:missing", "run:d:missing")).unwrap();
    drop(guard);
    drop(s);
    let raw = SqliteEventStore::open(db.path(), StoreOptions::default()).unwrap();
    assert!(raw
        .read_run(p::RunId("run:d:missing".into()))
        .next()
        .unwrap()
        .is_err());
    let other =
        Arc::new(FileContentKeyResolver::new(db.path().with_extension("other-keys")).unwrap());
    raw.enable_content_protection(
        StoreContentProtection::new(&p::VerifiedPrincipal("other-owner".into()), scope(), other)
            .unwrap(),
    )
    .unwrap();
    assert!(raw
        .read_run(p::RunId("run:d:missing".into()))
        .next()
        .unwrap()
        .is_err());
    let legacy = TestDatabase::new();
    let s = SqliteEventStore::open(legacy.path(), StoreOptions::default()).unwrap();
    s.append(input("event:d:legacy", "run:d:legacy")).unwrap();
    let keys = Arc::new(FileContentKeyResolver::new(legacy.path().with_extension("keys")).unwrap());
    assert!(s
        .enable_content_protection(StoreContentProtection::new(&owner(), scope(), keys).unwrap())
        .is_err());
}

#[test]
fn c23_scoped_cleanup_removes_real_content_caches_keys_and_survives_replay() {
    let db = TestDatabase::new();
    let (s, keys, _guard) = protected(&db);
    s.append(input("event:d:erase", "run:d:erase")).unwrap();
    let service = s.data_lifecycle_service(keys.clone());
    let r = request("run:d:erase", 0);
    let plan = service.plan(&r).unwrap();
    assert!(plan.objects.iter().any(|o| o.table == "transcript"));
    let receipt = service.apply_plan(r.clone(), &plan.digest).unwrap();
    assert!(!receipt.destroyed_key_digests.is_empty());
    for key in &receipt.destroyed_key_digests {
        assert!(keys.resolve(key).is_err());
    }
    assert_eq!(
        receipt.remote_disposition,
        p::RemoteDeletionDisposition::Unknown
    );
    assert!(s.search(PRIVATE, 10).unwrap().is_empty());
    assert!(s.read_run(p::RunId("run:d:erase".into())).next().is_none());
    assert!(s
        .load_transcript(p::RunId("run:d:erase".into()))
        .unwrap()
        .is_none());
    assert!(s.append(input("event:d:revive", "run:d:erase")).is_err());
    assert_eq!(service.apply_plan(r, &plan.digest).unwrap(), receipt);
    service.rebuild_tombstone().unwrap();
    assert_eq!(service.tombstone().unwrap(), Some(receipt));
    s.rebuild_builtin_projections().unwrap();
    assert!(s.search(PRIVATE, 10).unwrap().is_empty());
}

#[test]
fn c23_wrong_owner_scope_stale_and_changed_plan_have_zero_changes() {
    let db = TestDatabase::new();
    let (s, keys, _guard) = protected(&db);
    s.append(input("event:d:fences", "run:d:fences")).unwrap();
    let service = s.data_lifecycle_service(keys);
    let r = request("run:d:fences", 0);
    let p = service.plan(&r).unwrap();
    let mut wrong = r.clone();
    wrong.owner = p::VerifiedPrincipal("other-owner".into());
    assert!(service.apply_plan(wrong, &p.digest).is_err());
    let mut wrong = r.clone();
    wrong.scope = p::Scope("workspace:other".into());
    assert!(service.plan(&wrong).is_err());
    let mut wrong = r.clone();
    wrong.expected_version = 1;
    assert!(service.apply_plan(wrong, &p.digest).is_err());
    s.append(run_accepted_event(
        "event:d:more",
        "run:d:fences",
        now(),
        None,
    ))
    .unwrap();
    assert!(service.apply_plan(r, &p.digest).is_err());
    assert_eq!(s.search(PRIVATE, 10).unwrap().len(), 1);
    assert!(service.tombstone().unwrap().is_none());
}

struct InterruptedKeys {
    inner: Arc<FileContentKeyResolver>,
    fail: std::sync::atomic::AtomicBool,
}
impl ContentKeyResolver for InterruptedKeys {
    fn resolve(&self, key: &p::SchemaDigest) -> p::Result<Vec<u8>> {
        self.inner.resolve(key)
    }
    fn destroy(&self, key: &p::SchemaDigest) -> p::Result<()> {
        if self.fail.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(p::Error(
                "self-authored interrupted key-store fixture".into(),
            ));
        }
        self.inner.destroy(key)
    }
}
#[test]
fn c23_interrupted_key_destruction_has_no_success_and_resumes_its_immutable_plan() {
    let db = TestDatabase::new();
    let (s, keys, guard) = protected(&db);
    s.append(input("event:d:interrupted", "run:d:interrupted"))
        .unwrap();
    let service = s.data_lifecycle_service(Arc::new(InterruptedKeys {
        inner: keys.clone(),
        fail: std::sync::atomic::AtomicBool::new(true),
    }));
    let r = request("run:d:interrupted", 0);
    let plan = service.plan(&r).unwrap();
    assert!(service.apply_plan(r.clone(), &plan.digest).is_err());
    assert!(s.has_pending_data_cleanup().unwrap());
    assert!(service.tombstone().unwrap().is_none());
    assert!(s
        .append(input("event:d:forbidden-recreation", "run:d:interrupted"))
        .is_err());
    assert_eq!(service.plan(&r).unwrap(), plan);
    drop(service);
    drop(guard);
    drop(s);
    let (s, keys, _guard) = protected(&db);
    let service = s.data_lifecycle_service(keys);
    assert_eq!(service.plan(&r).unwrap(), plan);
    let receipt = service.apply_plan(r.clone(), &plan.digest).unwrap();
    assert!(!s.has_pending_data_cleanup().unwrap());
    assert_eq!(
        service.apply_plan(r.clone(), &plan.digest).unwrap(),
        receipt
    );
    let mut changed = r;
    changed.expected_version = 1;
    assert!(service.apply_plan(changed, &plan.digest).is_err());
}
#[test]
fn c23_retain_keeps_content_and_predeletion_backup_cannot_restore_destroyed_keys() {
    let db = TestDatabase::new();
    let (s, keys, _guard) = protected(&db);
    s.append(input("event:d:retain", "run:d:retain")).unwrap();
    let service = s.data_lifecycle_service(keys);
    let mut keep = request("run:d:retain", 0);
    keep.operation = p::DataLifecycleOperation::Retain;
    let receipt = service.apply(keep).unwrap();
    assert!(receipt.destroyed_key_digests.is_empty());
    assert_eq!(s.search(PRIVATE, 5).unwrap().len(), 1);
    let backup = db.path().with_extension("before-delete");
    s.backup_service().create_cold_backup(&backup).unwrap();
    service.apply(request("run:d:retain", 1)).unwrap();
    assert!(s.backup_service().verify_backup(&backup).is_err());
    assert!(s.search(PRIVATE, 5).unwrap().is_empty());
}

#[test]
fn c23_prefixed_evidence_and_transitive_aggregate_suffix_are_all_invalidated() {
    let db = TestDatabase::new();
    let (s, keys, _guard) = protected(&db);
    s.append(input("event:d:lineage-root", "run:d:lineage-root"))
        .unwrap();
    let derivative = p::RunId("aggregate:d:derived".into());
    let provenance = p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    };
    for index in 0..12 {
        let inputs = if index == 0 {
            vec![p::EvidenceRef("event:event:d:lineage-root".into())]
        } else {
            vec![]
        };
        s.append(p::Event::new(
            p::EventId(format!("event:d:derived:{index}")),
            derivative.clone(),
            None,
            p::EventPayload::ReflectionProduced(p::ReflectionProducedPayload {
                inputs,
                candidate_refs: vec![],
            }),
            p::SchemaVersion(1),
            now(),
            provenance.clone(),
        ))
        .unwrap();
    }
    assert_eq!(s.read_run(derivative.clone()).count(), 12);
    let service = s.data_lifecycle_service(keys);
    let receipt = service.apply(request("run:d:lineage-root", 0)).unwrap();
    assert!(receipt.destroyed_key_digests.len() >= 13);
    assert!(s.read_run(derivative).next().is_none());
}

#[test]
fn c22_protected_sync_bootstrap_reopens_with_bound_metadata_and_no_plaintext() {
    let db = TestDatabase::new();
    let keys = Arc::new(FileContentKeyResolver::new(db.path().with_extension("keys")).unwrap());
    let protection = StoreContentProtection::new(&owner(), scope(), keys).unwrap();
    let peer = p::SyncPeer {
        schema_version: p::SchemaVersion(1),
        reference: p::SyncPeerRef("peer:d-local".into()),
        owner: owner(),
        allowed_scopes: vec![scope()],
    };
    let options = StoreOptions {
        sync_peer: Some(peer.clone()),
        ..StoreOptions::default()
    };
    let s = SqliteEventStore::open_with_content_protection(
        db.path(),
        options.clone(),
        protection.clone(),
    )
    .unwrap();
    s.append(input("event:d:sync-protected", "run:d:sync-protected"))
        .unwrap();
    drop(s);
    let s = SqliteEventStore::open_with_content_protection(
        db.path(),
        options.clone(),
        protection.clone(),
    )
    .unwrap();
    assert_eq!(
        s.read_run(p::RunId("run:d:sync-protected".into())).count(),
        1
    );
    drop(s);
    let mut wrong = options;
    wrong.sync_peer.as_mut().unwrap().reference = p::SyncPeerRef("peer:changed".into());
    assert!(SqliteEventStore::open_with_content_protection(db.path(), wrong, protection).is_err());
    absent(&std::fs::read(db.path()).unwrap());
}
