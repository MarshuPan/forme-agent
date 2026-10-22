//! Real protected SQLite, external keys and replay. These tests neither claim
//! native service acceptance nor use the owner's installed data.
#![forbid(unsafe_code)]
use forme_protocol as p;
use forme_store::{
    BrainAuthorityClaim, BrainAuthorityStore, ContentKeyResolver, EventStore,
    FileContentKeyResolver, OfflineBrainSource, SqliteEventStore, StoreAccessGuard,
    StoreContentProtection, StoreOptions,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const PERSONAL: &str = "offline-recovery-private-fixture-8739ab";
fn owner() -> p::VerifiedPrincipal {
    p::VerifiedPrincipal("owner:offline-test".into())
}
fn scope() -> p::Scope {
    p::Scope("workspace:offline-test".into())
}
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    keys: PathBuf,
}
impl Fixture {
    fn new(protected: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-offline 恢复-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let f = Self {
            source: root.join("source.db"),
            keys: root.join("keys"),
            root,
        };
        let keys = Arc::new(FileContentKeyResolver::new_managed(&f.keys).unwrap());
        let store = SqliteEventStore::open(
            &f.source,
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
        )
        .unwrap();
        store
            .claim_brain_authority(BrainAuthorityClaim {
                owner_digest: p::sha256_content_digest(owner().0.as_bytes()),
                process_nonce: p::Nonce("fixture:writer".into()),
                now_ms: 1,
                lease_ttl: p::DurationMs(1),
            })
            .unwrap();
        if protected {
            store
                .enable_content_protection(
                    StoreContentProtection::new(&owner(), scope(), keys).unwrap(),
                )
                .unwrap();
        }
        store
            .append(p::Event::new(
                p::EventId("event:offline".into()),
                p::RunId("run:offline".into()),
                None,
                p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                    source: p::Source::UserTurn,
                    session_ref: p::SessionId("session:offline".into()),
                    input_ref: p::InputRef(PERSONAL.into()),
                    idempotency_key: Some(p::IdempotencyKey("command:offline".into())),
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
        drop(store);
        f
    }
    fn open(&self) -> forme_protocol::Result<OfflineBrainSource> {
        OfflineBrainSource::open(&self.source, &owner(), scope(), &self.keys)
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(&self.source).unwrap()
    }
    fn keys(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut v = fs::read_dir(&self.keys)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                let b = fs::read(&p).unwrap();
                (p, b)
            })
            .collect::<Vec<_>>();
        v.sort();
        v
    }
    fn assert_cold(path: &Path) {
        for s in ["-wal", "-shm", "-journal"] {
            assert!(!PathBuf::from(format!("{}{s}", path.display())).exists());
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn offline_and_online_backup_manifests_are_interoperable() {
    let f = Fixture::new(true);
    let keys = Arc::new(FileContentKeyResolver::new_managed(&f.keys).unwrap());
    let store = SqliteEventStore::open(
        &f.source,
        StoreOptions {
            fts_enabled: true,
            ..StoreOptions::default()
        },
    )
    .unwrap();
    store
        .enable_content_protection(StoreContentProtection::new(&owner(), scope(), keys).unwrap())
        .unwrap();
    let online_backup = f.root.join("online.db");
    store
        .backup_service()
        .create_cold_backup(&online_backup)
        .unwrap();
    let offline_backup = f.root.join("offline.db");
    // Close the source connection before taking the exclusive offline guard.
    drop(store);
    f.open().unwrap().create_backup(&offline_backup).unwrap();
    let online = OfflineBrainSource::open(&online_backup, &owner(), scope(), &f.keys).unwrap();
    assert!(online.verify_backup().unwrap().is_valid());
    drop(online);
    let store = SqliteEventStore::open(&f.source, StoreOptions::default()).unwrap();
    let keys = Arc::new(FileContentKeyResolver::new_managed(&f.keys).unwrap());
    store
        .enable_content_protection(StoreContentProtection::new(&owner(), scope(), keys).unwrap())
        .unwrap();
    assert!(store
        .backup_service()
        .verify_backup(&offline_backup)
        .unwrap()
        .is_valid());
}

#[test]
fn offline_restore_authenticates_original_brain_and_preserves_external_keys() {
    let f = Fixture::new(true);
    let before = f.bytes();
    let keys = f.keys();
    assert!(!before
        .windows(PERSONAL.len())
        .any(|w| w == PERSONAL.as_bytes()));
    let brain = f.open().unwrap();
    assert_eq!(brain.identity().event_count, 1);
    assert_eq!(brain.identity().authenticated_content_rows, 3);
    assert!(brain.verify_backup().is_err()); // an old brain is not a manifest-backed backup
    assert!(StoreAccessGuard::online(&f.source).is_err());
    let backup = f.root.join("backup.db");
    let verified = brain.create_backup(&backup).unwrap();
    assert!(verified.is_valid());
    assert!(brain.create_backup(&backup).is_err());
    drop(brain);
    let backup_reader = OfflineBrainSource::open(&backup, &owner(), scope(), &f.keys).unwrap();
    let target = f.root.join("restored.db");
    let plan = backup_reader.plan_restore(&target).unwrap();
    let receipt = backup_reader
        .restore_to(&target, &plan, &plan.digest)
        .unwrap();
    assert!(receipt.verification.is_valid());
    assert!(receipt.source_unchanged);
    assert!(!receipt.authority_switched);
    assert_eq!(
        receipt.source.store_identity,
        receipt.restored.store_identity
    );
    assert!(backup_reader
        .restore_to(&target, &plan, &plan.digest)
        .is_err());
    assert!(!target.with_extension("content-keys").exists());
    drop(backup_reader);
    let restored = OfflineBrainSource::open(&target, &owner(), scope(), &f.keys).unwrap();
    assert_eq!(restored.identity().event_count, 1);
    drop(restored);
    assert_eq!(f.bytes(), before);
    assert_eq!(f.keys(), keys);
    Fixture::assert_cold(&f.source);
    Fixture::assert_cold(&backup);
    Fixture::assert_cold(&target);
}

#[test]
fn offline_validation_refuses_missing_wrong_keys_owner_plaintext_and_busy_source() {
    let f = Fixture::new(true);
    let before = f.bytes();
    let keys = f.keys();
    let missing = f.root.join("absent-keys");
    assert!(OfflineBrainSource::open(&f.source, &owner(), scope(), &missing).is_err());
    assert!(!missing.exists());
    assert!(OfflineBrainSource::open(
        &f.source,
        &p::VerifiedPrincipal("owner:other".into()),
        scope(),
        &f.keys
    )
    .is_err());
    let empty = f.root.join("wrong-keys");
    FileContentKeyResolver::new_managed(&empty).unwrap();
    assert!(OfflineBrainSource::open(&f.source, &owner(), scope(), &empty).is_err());
    assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
    let online = StoreAccessGuard::online(&f.source).unwrap();
    assert!(f.open().is_err());
    drop(online);
    let old_writer = rusqlite::Connection::open(&f.source).unwrap();
    old_writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(f.open().is_err());
    drop(old_writer);
    assert_eq!(f.bytes(), before);
    assert_eq!(f.keys(), keys);
    let legacy = Fixture::new(false);
    let original = legacy.bytes();
    assert!(legacy.open().is_err());
    assert_eq!(legacy.bytes(), original);
}

#[test]
fn offline_restore_rejects_rebound_plan_stale_artifact_and_preexisting_target() {
    let f = Fixture::new(true);
    let backup = f.root.join("backup.db");
    f.open().unwrap().create_backup(&backup).unwrap();
    let source = OfflineBrainSource::open(&backup, &owner(), scope(), &f.keys).unwrap();
    let target = f.root.join("restored.db");
    let plan = source.plan_restore(&target).unwrap();
    assert!(source
        .restore_to(&f.root.join("other.db"), &plan, &plan.digest)
        .is_err());
    assert!(!f.root.join("other.db").exists());
    let wrong = p::sha256_content_digest(b"wrong approval");
    assert!(source.restore_to(&target, &plan, &wrong).is_err());
    assert!(!target.exists());
    fs::write(&target, b"preserved unrelated file").unwrap();
    assert!(source.restore_to(&target, &plan, &plan.digest).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"preserved unrelated file");
    let candidate = f.root.join("another.db");
    let plan = source.plan_restore(&candidate).unwrap();
    let manifest = PathBuf::from(format!("{}.manifest.json", backup.display()));
    let mut m: serde_json::Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    m["created_at"] = serde_json::json!(2);
    fs::write(manifest, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(source.restore_to(&candidate, &plan, &plan.digest).is_err());
    assert!(!candidate.exists());
}

#[test]
fn offline_validation_checks_catalog_lineage_and_rechecks_key_revocation() {
    let f = Fixture::new(true);
    let before = f.bytes();
    let source = f.open().unwrap();
    let keys = FileContentKeyResolver::new_managed(&f.keys).unwrap();
    let first = f.keys()[0].1.clone();
    let digest = p::sha256_content_digest(&first);
    keys.destroy(&digest).unwrap();
    let target = f.root.join("backup.db");
    assert!(source.create_backup(&target).is_err());
    assert!(!target.exists());
    drop(source);
    assert_eq!(f.bytes(), before);
    let other = Fixture::new(true);
    let c = rusqlite::Connection::open(&other.source).unwrap();
    c.execute("DELETE FROM protected_key_history", []).unwrap();
    drop(c);
    let damaged = other.bytes();
    assert!(other.open().is_err());
    assert_eq!(other.bytes(), damaged);
}

fn append_fact(
    store: &SqliteEventStore,
    run: &str,
    id: &str,
    payload: p::EventPayload,
) -> p::Result<p::EventId> {
    store.append(p::Event::new(
        p::EventId(id.into()),
        p::RunId(run.into()),
        None,
        payload,
        p::SchemaVersion(1),
        2,
        p::Provenance {
            source: p::Source::OwnerControl,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    ))
}

fn restored(f: &Fixture, target_name: &str) -> (PathBuf, forme_store::OfflineRestoreReceipt) {
    let backup = f.root.join(format!("{target_name}-backup.db"));
    f.open().unwrap().create_backup(&backup).unwrap();
    let source = OfflineBrainSource::open(&backup, &owner(), scope(), &f.keys).unwrap();
    let target = f.root.join(target_name);
    let plan = source.plan_restore(&target).unwrap();
    let receipt = source.restore_to(&target, &plan, &plan.digest).unwrap();
    (target, receipt)
}

fn open_restored(f: &Fixture, target: &Path) -> SqliteEventStore {
    let store = SqliteEventStore::open(target, StoreOptions::default()).unwrap();
    let keys = Arc::new(FileContentKeyResolver::new_managed(&f.keys).unwrap());
    store
        .enable_content_protection(StoreContentProtection::new(&owner(), scope(), keys).unwrap())
        .unwrap();
    store
}

#[test]
fn restored_history_never_regains_model_action_resume_or_intention_authority() {
    let f = Fixture::new(true);
    let source = open_restored(&f, &f.source);
    append_fact(
        &source,
        "run:offline",
        "event:old-intention",
        p::EventPayload::ProspectiveIntentionCreated(p::ProspectiveIntentionCreatedPayload {
            intention_id: p::IntentionId("intention:old".into()),
            source: p::IntentionSource::Commitment,
            trigger: p::IntentionTriggerRef("at:3".into()),
            schedule: None,
            goal_frame: None,
        }),
    )
    .unwrap();
    drop(source);
    let original = f.bytes();
    let (target, receipt) = restored(&f, "history.db");
    assert_eq!(f.bytes(), original);
    let store = open_restored(&f, &target);
    let fence = store.restored_execution_fence().unwrap().unwrap();
    assert_eq!(fence.digest().unwrap(), receipt.execution_fence_digest);
    assert_eq!(fence.plan_digest, receipt.plan_digest);
    assert_eq!(fence.streams[&p::RunId("run:offline".into())], 2);
    assert!(store
        .quarantined_intentions()
        .unwrap()
        .contains(&p::IntentionId("intention:old".into())));
    let forbidden = vec![
        p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
            call_id: p::ModelCallId("call:historical".into()),
            model_profile: p::ModelProfileRef("model:fixture".into()),
        }),
        p::EventPayload::ActionStarted(p::ActionStartedPayload {
            intent_id: p::ActionId("action:historical".into()),
            backend: p::BackendKind::Shell,
            scope: scope(),
            remote_lease: None,
        }),
        p::EventPayload::RunResumed(p::RunResumedPayload {
            wait_reason: p::WaitReason("approval".into()),
            resume_ref: p::ResumeRef("resume:old".into()),
        }),
    ];
    for (index, payload) in forbidden.into_iter().enumerate() {
        assert!(append_fact(
            &store,
            "run:offline",
            &format!("event:blocked:{index}"),
            payload
        )
        .is_err());
        assert_eq!(store.read_run(p::RunId("run:offline".into())).count(), 2);
    }
    // Moving the old intention into a fresh aggregate cannot reactivate it.
    assert!(append_fact(
        &store,
        "run:new",
        "event:blocked:fire",
        p::EventPayload::ProspectiveIntentionResolved(p::ProspectiveIntentionResolvedPayload {
            intention_id: p::IntentionId("intention:old".into()),
            outcome: p::IntentionOutcome::Fired,
        })
    )
    .is_err());
    append_fact(
        &store,
        "run:offline",
        "event:old-cancel",
        p::EventPayload::ProspectiveIntentionResolved(p::ProspectiveIntentionResolvedPayload {
            intention_id: p::IntentionId("intention:old".into()),
            outcome: p::IntentionOutcome::Cancelled,
        }),
    )
    .unwrap();
    append_fact(
        &store,
        "run:new",
        "event:new-call",
        p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
            call_id: p::ModelCallId("call:fresh".into()),
            model_profile: p::ModelProfileRef("model:fixture".into()),
        }),
    )
    .unwrap();
    assert!(store
        .require_current_execution(&p::RunId("run:new".into()))
        .is_ok());
}

#[test]
fn repeated_restores_extend_fence_lineage_and_reject_tampering() {
    let f = Fixture::new(true);
    let (first, receipt) = restored(&f, "first.db");
    let first_source = OfflineBrainSource::open(&first, &owner(), scope(), &f.keys).unwrap();
    let second = f.root.join("second.db");
    let plan = first_source.plan_restore(&second).unwrap();
    let second_receipt = first_source
        .restore_to(&second, &plan, &plan.digest)
        .unwrap();
    drop(first_source);
    let store = open_restored(&f, &second);
    let fence = store.restored_execution_fence().unwrap().unwrap();
    assert_eq!(fence.generation, 2);
    assert_eq!(fence.previous_digest, Some(receipt.execution_fence_digest));
    assert_eq!(
        fence.digest().unwrap(),
        second_receipt.execution_fence_digest
    );
    assert!(store
        .require_current_execution(&p::RunId("run:offline".into()))
        .is_err());
    drop(store);
    let c = rusqlite::Connection::open(&second).unwrap();
    c.execute(
        "UPDATE brain_restore_fences SET digest=?1 WHERE generation=2",
        [p::sha256_content_digest(b"tampered").0],
    )
    .unwrap();
    drop(c);
    assert!(SqliteEventStore::open(&second, StoreOptions::default()).is_err());
    assert!(OfflineBrainSource::open(&second, &owner(), scope(), &f.keys).is_err());
}

#[test]
fn a_sealed_restore_candidate_cannot_be_committed_after_it_has_run_again() {
    let f = Fixture::new(true);
    let (target, _) = restored(&f, "active-candidate.db");
    let candidate = OfflineBrainSource::open(&target, &owner(), scope(), &f.keys).unwrap();
    assert!(candidate.restored_candidate_fence_digest().is_ok());
    drop(candidate);
    let store = open_restored(&f, &target);
    append_fact(
        &store,
        "run:after-seal",
        "event:after-seal",
        p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
            call_id: p::ModelCallId("call:after-seal".into()),
            model_profile: p::ModelProfileRef("model:fixture".into()),
        }),
    )
    .unwrap();
    drop(store);
    let candidate = OfflineBrainSource::open(&target, &owner(), scope(), &f.keys).unwrap();
    assert!(candidate.execution_fence_digest().is_ok());
    assert!(candidate.restored_candidate_fence_digest().is_err());
}

#[test]
fn online_restore_binds_the_approved_plan_and_final_post_sealing_manifest() {
    let f = Fixture::new(true);
    let store = open_restored(&f, &f.source);
    let service = store.backup_service();
    let backup = f.root.join("online-restore-source.db");
    service.create_cold_backup(&backup).unwrap();
    let original = fs::read(&backup).unwrap();
    let target = f.root.join("online-restored.db");
    let plan = p::sha256_content_digest(b"persisted-owner-plan-fixture");
    let report = service
        .restore_approved_to(&backup, &target, &plan)
        .unwrap();
    assert!(report.verification.is_valid());
    assert_eq!(fs::read(&backup).unwrap(), original);
    assert_eq!(
        p::sha256_content_digest(&fs::read(&target).unwrap()),
        report.manifest.database_digest
    );
    let restored = open_restored(&f, &target);
    let fence = restored.restored_execution_fence().unwrap().unwrap();
    assert_eq!(fence.plan_digest, plan);
    assert_eq!(Some(fence.digest().unwrap()), report.execution_fence_digest);
    assert_eq!(Some(fence.store_identity), report.store_identity);
    assert!(restored
        .require_current_execution(&p::RunId("run:offline".into()))
        .is_err());
    assert!(service
        .restore_approved_to(&backup, &target, &plan)
        .is_err());
    drop(restored);
    let busy = StoreAccessGuard::online(&backup).unwrap();
    let other = f.root.join("must-not-be-created.db");
    assert!(service.restore_approved_to(&backup, &other, &plan).is_err());
    assert!(!other.exists());
    drop(busy);
}
