//! D4 uses actual legacy SQLite bytes, the production AEAD resolver, the
//! official backup API and real store replay. No encryption/replay stand-ins.
use forme_protocol as p;
use forme_store::{
    BrainAuthorityClaim, BrainAuthorityStore, ContentKeyResolver, EventStore, EvolutionEventStore,
    FileContentKeyResolver, LegacyPlaintextMigration, SchemaSnapshot, SourceBackupDisposition,
    SqliteEventStore, StoreContentProtection, StoreOptions,
};
use rusqlite::{params, Connection};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};

const PRIVATE: &str = "legacy-personal-content-4e16d239";
const FREE_PAGE: &str = "legacy-deleted-page-content-a97432c5";
const AUX_PRIVATE: &str = "legacy-auxiliary-content-7f04592b";
const PREFIX: &[u8] = b"FORME-AEAD-1\n";
static NEXT: AtomicU64 = AtomicU64::new(1);

fn owner() -> p::VerifiedPrincipal {
    p::VerifiedPrincipal("owner:legacy-migration".into())
}
fn scope() -> p::Scope {
    p::Scope("workspace:legacy-migration".into())
}
fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::UserTurn,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}
fn snapshot() -> SchemaSnapshot {
    SchemaSnapshot {
        schema: StoreOptions::default().current_schema,
        policy_version: p::Version(1),
        loop_version: p::Version(1),
        model_profile: p::ModelProfileRef("fixture:legacy-replay".into()),
        tool_schema: p::Version(1),
    }
}
fn accepted() -> p::Event {
    p::Event::new(
        p::EventId("event:legacy:first".into()),
        p::RunId("run:legacy".into()),
        None,
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId("session:legacy".into()),
            input_ref: p::InputRef(PRIVATE.into()),
            idempotency_key: Some(p::IdempotencyKey("command:legacy".into())),
        }),
        p::SchemaVersion(1),
        17,
        provenance(),
    )
}

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-legacy 测试-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self {
            source: root.join("legacy.db"),
            target: root.join("protected.db"),
            root,
        }
    }
    fn legacy(&self, bind_owner: bool) {
        let store = SqliteEventStore::open(
            &self.source,
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
        )
        .unwrap();
        if bind_owner {
            store
                .claim_brain_authority(BrainAuthorityClaim {
                    owner_digest: p::sha256_content_digest(owner().0.as_bytes()),
                    process_nonce: p::Nonce("legacy-offline-process".into()),
                    now_ms: 1,
                    lease_ttl: p::DurationMs(1),
                })
                .unwrap();
        }
        store.append(accepted()).unwrap();
        let mut next = p::Event::new(
            p::EventId("event:legacy:second".into()),
            p::RunId("run:legacy".into()),
            None,
            p::EventPayload::GoalFramed(p::GoalFramedPayload {
                goal_frame: p::GoalFrameRef("goal:legacy".into()),
                long_term: None,
                knowledge: None,
            }),
            p::SchemaVersion(1),
            2,
            provenance(),
        );
        next.provenance.caused_by = Some(accepted().event_id);
        store.append(next).unwrap();
        assert_eq!(
            store.search(&format!("\"{PRIVATE}\""), 10).unwrap().len(),
            1
        );
        assert!(store
            .replay(accepted().run_id, snapshot())
            .unwrap()
            .diff_vs_current
            .is_empty());
        drop(store);
        // A dropped ordinary table leaves recoverable plaintext in free pages
        // without changing the supported schema. Only target VACUUM may erase it.
        let c = Connection::open(&self.source).unwrap();
        c.execute_batch("PRAGMA secure_delete=OFF; CREATE TABLE dropped_legacy_cache(body TEXT);")
            .unwrap();
        c.execute(
            "INSERT INTO dropped_legacy_cache(body) VALUES(?1)",
            params![FREE_PAGE.repeat(600)],
        )
        .unwrap();
        c.execute_batch("DROP TABLE dropped_legacy_cache;").unwrap();
        drop(c);
        let bytes = fs::read(&self.source).unwrap();
        assert!(contains(&bytes, PRIVATE));
        assert!(contains(&bytes, FREE_PAGE));
        self.assert_cold();
    }
    fn resolver(&self, name: &str) -> Arc<FileContentKeyResolver> {
        Arc::new(FileContentKeyResolver::new(self.root.join(name)).unwrap())
    }
    fn protection(&self) -> StoreContentProtection {
        StoreContentProtection::new(&owner(), scope(), self.resolver("keys")).unwrap()
    }
    fn assert_cold(&self) {
        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(!sidecar(&self.source, suffix).exists());
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}
fn contains(bytes: &[u8], text: &str) -> bool {
    bytes.windows(text.len()).any(|w| w == text.as_bytes())
}
fn scan_target(path: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        if let Ok(bytes) = fs::read(sidecar(path, suffix)) {
            for marker in [PRIVATE, FREE_PAGE, AUX_PRIVATE] {
                assert!(
                    !contains(&bytes, marker),
                    "plaintext {marker} in target{suffix}"
                );
            }
        }
    }
}

#[test]
fn c22_legacy_migration_encrypts_real_store_and_free_pages_and_restarts() {
    let f = Fixture::new();
    f.legacy(true);
    let before = fs::read(&f.source).unwrap();
    let protection = f.protection();
    let migration = LegacyPlaintextMigration::new(protection.clone());
    let plan = migration.plan(&owner(), &f.source, &f.target).unwrap();
    assert_eq!(fs::read(&f.source).unwrap(), before);
    assert!(!f.target.exists());
    let receipt = migration
        .execute(&owner(), &f.source, &f.target, &plan, &plan.digest)
        .unwrap();
    assert_eq!(receipt.event_count, 2);
    assert!(receipt.integrity_verified && receipt.replay_verified && receipt.source_unchanged);
    assert_eq!(
        receipt.source_backup,
        SourceBackupDisposition::UnprotectedBackupOutsideProtectedDatabase
    );
    assert!(!receipt.source_deleted && !receipt.authority_switched);
    assert_eq!(
        receipt.source_database_digest,
        p::sha256_content_digest(&before)
    );
    assert_eq!(
        receipt.target_database_digest,
        p::sha256_content_digest(&fs::read(&f.target).unwrap())
    );
    assert_eq!(fs::read(&f.source).unwrap(), before);
    let portable = serde_json::to_string(&(&plan, &receipt)).unwrap();
    for marker in [PRIVATE, FREE_PAGE, f.root.to_str().unwrap(), "SecretRef"] {
        assert!(!portable.contains(marker));
    }
    scan_target(&f.target);
    {
        let raw = Connection::open(&f.target).unwrap();
        assert_eq!(
            raw.query_row("PRAGMA freelist_count", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            raw.query_row("SELECT COUNT(*) FROM events_fts", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            raw.query_row(
                "SELECT COUNT(*) FROM authority_writer_lease WHERE expires_at>2",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        let sealed: Vec<u8> = raw
            .query_row(
                "SELECT payload FROM events WHERE event_id=?1",
                params![accepted().event_id.0],
                |r| r.get(0),
            )
            .unwrap();
        assert!(sealed.starts_with(PREFIX));
        assert!(raw.execute("UPDATE events SET ts=1", []).is_err());
        assert!(raw.execute("DELETE FROM events", []).is_err());
    }
    for _ in 0..2 {
        let store = SqliteEventStore::open(
            &f.target,
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
        )
        .unwrap();
        // Construct a fresh file resolver, exercising restart key discovery.
        store.enable_content_protection(f.protection()).unwrap();
        let read = store
            .read_run(accepted().run_id.clone())
            .collect::<p::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            read.iter().map(|e| e.kind).collect::<Vec<_>>(),
            vec![p::EventKind::RunAccepted, p::EventKind::GoalFramed]
        );
        assert_eq!(
            read.iter().map(|e| e.stream_seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(read[0].payload, accepted().payload);
        assert!(store
            .replay(accepted().run_id, snapshot())
            .unwrap()
            .diff_vs_current
            .is_empty());
        assert_eq!(store.search(PRIVATE, 10).unwrap().len(), 1);
        assert!(store.load_transcript(accepted().run_id).unwrap().is_some());
        store.append(accepted()).unwrap();
        let mut same_command = accepted();
        same_command.event_id = p::EventId("event:legacy:duplicate-command".into());
        same_command.ts_unix_ms = 99;
        assert_eq!(store.append(same_command).unwrap(), accepted().event_id);
        assert_eq!(store.run_head(&accepted().run_id).unwrap(), 2);
        scan_target(&f.target);
    }
    scan_target(&f.target);
    assert_eq!(fs::read(&f.source).unwrap(), before);
    f.assert_cold();
}

#[test]
fn c22_migration_requires_owner_source_authority_and_exact_approved_plan() {
    let f = Fixture::new();
    f.legacy(true);
    let migration = LegacyPlaintextMigration::new(f.protection());
    let before = fs::read(&f.source).unwrap();
    for invalid in [
        p::VerifiedPrincipal(String::new()),
        p::VerifiedPrincipal("owner:other".into()),
    ] {
        assert!(migration.plan(&invalid, &f.source, &f.target).is_err());
    }
    let plan = migration.plan(&owner(), &f.source, &f.target).unwrap();
    let wrong = p::sha256_content_digest(b"unapproved-plan");
    assert!(migration
        .execute(&owner(), &f.source, &f.target, &plan, &wrong)
        .is_err());
    assert!(migration
        .execute(
            &p::VerifiedPrincipal(String::new()),
            &f.source,
            &f.target,
            &plan,
            &plan.digest
        )
        .is_err());
    let mut altered = plan.clone();
    altered.target_scope = p::Scope("workspace:other".into());
    assert!(migration
        .execute(&owner(), &f.source, &f.target, &altered, &plan.digest)
        .is_err());
    assert!(migration
        .execute(
            &owner(),
            &f.source,
            &f.root.join("other.db"),
            &plan,
            &plan.digest
        )
        .is_err());
    assert!(!f.target.exists());
    assert_eq!(fs::read(&f.source).unwrap(), before);
    let missing = Fixture::new();
    missing.legacy(false);
    assert!(LegacyPlaintextMigration::new(missing.protection())
        .plan(&owner(), &missing.source, &missing.target)
        .unwrap_err()
        .0
        .contains("authority is missing"));
    let other_protection = StoreContentProtection::new(
        &p::VerifiedPrincipal("owner:other".into()),
        scope(),
        f.resolver("other-owner-keys"),
    )
    .unwrap();
    assert!(LegacyPlaintextMigration::new(other_protection)
        .plan(
            &p::VerifiedPrincipal("owner:other".into()),
            &f.source,
            &f.target
        )
        .is_err());
}

#[test]
fn c22_migration_rechecks_source_after_approval_and_never_reuses_a_target() {
    let f = Fixture::new();
    f.legacy(true);
    let migration = LegacyPlaintextMigration::new(f.protection());
    let plan = migration.plan(&owner(), &f.source, &f.target).unwrap();
    let store = SqliteEventStore::open(&f.source, StoreOptions::default()).unwrap();
    let mut additional = accepted();
    additional.event_id = p::EventId("event:legacy:changed".into());
    if let p::EventPayload::RunAccepted(p) = &mut additional.payload {
        p.idempotency_key = None;
    }
    store.append(additional).unwrap();
    drop(store);
    let changed = fs::read(&f.source).unwrap();
    assert!(migration
        .execute(&owner(), &f.source, &f.target, &plan, &plan.digest)
        .unwrap_err()
        .0
        .contains("source changed"));
    assert!(!f.target.exists());
    assert_eq!(fs::read(&f.source).unwrap(), changed);
    let plan = migration.plan(&owner(), &f.source, &f.target).unwrap();
    fs::write(&f.target, b"existing destination must survive").unwrap();
    assert!(migration
        .execute(&owner(), &f.source, &f.target, &plan, &plan.digest)
        .is_err());
    assert_eq!(
        fs::read(&f.target).unwrap(),
        b"existing destination must survive"
    );
    assert!(migration.plan(&owner(), &f.source, &f.source).is_err());
    assert_eq!(fs::read(&f.source).unwrap(), changed);
}

#[test]
fn c22_migration_rejects_live_source_lease_and_target_sidecars() {
    let f = Fixture::new();
    f.legacy(true);
    let migration = LegacyPlaintextMigration::new(f.protection());
    let c = Connection::open(&f.source).unwrap();
    c.execute(
        "UPDATE authority_writer_lease SET expires_at=?1",
        params![i64::MAX],
    )
    .unwrap();
    drop(c);
    let before = fs::read(&f.source).unwrap();
    assert!(migration
        .plan(&owner(), &f.source, &f.target)
        .unwrap_err()
        .0
        .contains("lease has not expired"));
    assert_eq!(fs::read(&f.source).unwrap(), before);
    let c = Connection::open(&f.source).unwrap();
    c.execute("UPDATE authority_writer_lease SET expires_at=2", [])
        .unwrap();
    drop(c);
    let online = SqliteEventStore::open(&f.source, StoreOptions::default()).unwrap();
    assert!(migration.plan(&owner(), &f.source, &f.target).is_err());
    drop(online);
    fs::write(sidecar(&f.target, "-wal"), b"existing sidecar").unwrap();
    assert!(migration.plan(&owner(), &f.source, &f.target).is_err());
    assert!(!f.target.exists());
}

#[test]
fn c22_migration_fails_closed_for_unknown_schema_and_unhandled_content() {
    for sql in [
        "CREATE TABLE private_legacy_notes(body TEXT); INSERT INTO private_legacy_notes VALUES('unhandled plaintext')",
        "ALTER TABLE transcript ADD COLUMN private_text TEXT",
        "CREATE TRIGGER unrelated_copy AFTER INSERT ON transcript BEGIN SELECT 1; END",
        "PRAGMA user_version=1234",
        "INSERT INTO schema_migrations VALUES('run_accepted',1,2,'custom migration','custom:implementation')",
        "INSERT INTO data_lifecycle_deleted_objects VALUES('unhandled:history')",
    ] {
        let f=Fixture::new();f.legacy(true);
        let c=Connection::open(&f.source).unwrap();c.execute_batch(sql).unwrap();drop(c);
        let before=fs::read(&f.source).unwrap();
        let migration=LegacyPlaintextMigration::new(f.protection());
        assert!(migration.plan(&owner(),&f.source,&f.target).is_err(),"{sql}");
        assert!(!f.target.exists());assert_eq!(fs::read(&f.source).unwrap(),before);
    }
}

struct FailingKeys {
    actual: Arc<FileContentKeyResolver>,
    calls: AtomicUsize,
}
impl ContentKeyResolver for FailingKeys {
    fn resolve(&self, digest: &p::SchemaDigest) -> p::Result<Vec<u8>> {
        self.actual.resolve(digest)
    }
    fn subject_key(
        &self,
        binding: &p::ContentBinding,
        subject: &p::SchemaDigest,
    ) -> p::Result<p::SchemaDigest> {
        if self.calls.fetch_add(1, Ordering::SeqCst) >= 2 {
            return Err(p::Error("fixture: resolver interrupted".into()));
        }
        self.actual.subject_key(binding, subject)
    }
    fn destroy(&self, digest: &p::SchemaDigest) -> p::Result<()> {
        self.actual.destroy(digest)
    }
}

#[test]
fn c22_failed_migration_retains_source_and_incomplete_target_then_recovers_to_new_path() {
    let f = Fixture::new();
    f.legacy(true);
    let before = fs::read(&f.source).unwrap();
    let keys = Arc::new(FailingKeys {
        actual: f.resolver("keys"),
        calls: AtomicUsize::new(0),
    });
    let service = LegacyPlaintextMigration::new(
        StoreContentProtection::new(&owner(), scope(), keys).unwrap(),
    );
    let plan = service.plan(&owner(), &f.source, &f.target).unwrap();
    let error = service
        .execute(&owner(), &f.source, &f.target, &plan, &plan.digest)
        .unwrap_err();
    assert!(
        error.0.contains("migration incomplete")
            && error.0.contains("source remains an unprotected backup")
    );
    assert!(f.target.exists());
    assert_eq!(fs::read(&f.source).unwrap(), before);
    let incomplete = SqliteEventStore::open(&f.target, StoreOptions::default()).unwrap();
    assert!(incomplete
        .enable_content_protection(f.protection())
        .is_err());
    drop(incomplete);
    assert!(service
        .execute(&owner(), &f.source, &f.target, &plan, &plan.digest)
        .is_err());
    let recovered = f.root.join("recovered.db");
    let service = LegacyPlaintextMigration::new(f.protection());
    let plan = service.plan(&owner(), &f.source, &recovered).unwrap();
    let receipt = service
        .execute(&owner(), &f.source, &recovered, &plan, &plan.digest)
        .unwrap();
    assert!(!receipt.source_deleted && !receipt.authority_switched);
    scan_target(&recovered);
    assert_eq!(fs::read(&f.source).unwrap(), before);
}

#[test]
fn c22_migrated_ciphertext_still_requires_the_real_owner_key_and_authentication() {
    let f = Fixture::new();
    f.legacy(true);
    let service = LegacyPlaintextMigration::new(f.protection());
    let plan = service.plan(&owner(), &f.source, &f.target).unwrap();
    service
        .execute(&owner(), &f.source, &f.target, &plan, &plan.digest)
        .unwrap();
    let store = SqliteEventStore::open(&f.target, StoreOptions::default()).unwrap();
    assert!(store.read_run(accepted().run_id).next().unwrap().is_err());
    store
        .enable_content_protection(
            StoreContentProtection::new(&owner(), scope(), f.resolver("wrong-keys")).unwrap(),
        )
        .unwrap();
    assert!(store.read_run(accepted().run_id).next().unwrap().is_err());
    drop(store);
    let c = Connection::open(&f.target).unwrap();
    let bytes: Vec<u8> = c
        .query_row("SELECT state FROM transcript", [], |r| r.get(0))
        .unwrap();
    let mut envelope: serde_json::Value =
        serde_json::from_slice(bytes.strip_prefix(PREFIX).unwrap()).unwrap();
    envelope["ciphertext"][0] = serde_json::json!(envelope["ciphertext"][0].as_u64().unwrap() ^ 1);
    let mut corrupt = PREFIX.to_vec();
    corrupt.extend(serde_json::to_vec(&envelope).unwrap());
    c.execute("UPDATE transcript SET state=?1", params![corrupt])
        .unwrap();
    drop(c);
    let store = SqliteEventStore::open(&f.target, StoreOptions::default()).unwrap();
    store.enable_content_protection(f.protection()).unwrap();
    assert!(store.load_transcript(accepted().run_id).is_err());
}

// All existing auxiliary blob surfaces, including rows whose SQLite rowid
// changes during VACUUM. Their bytes remain data, never owner instructions.
const AUXILIARY: &[(&str, &str, &str)] = &[
    ("sync_peers", "singleton", "semantic_bytes"),
    ("sync_batches", "batch_id", "semantic_bytes"),
    ("federated_peers", "peer_ref", "grant"),
    ("remote_leases", "lease_ref", "lease"),
    ("replication_exports", "batch_ref", "semantic_bytes"),
    (
        "federated_checkpoint_artifacts",
        "checkpoint_ref",
        "artifact",
    ),
    ("federated_handoffs", "next_run", "plan"),
    ("federated_retention", "request_ref", "state"),
    ("remote_action_recovery", "run_id", "record"),
    ("ecosystem_publishers", "publisher_ref", "grant"),
    ("ecosystem_admissions", "release_ref", "admission"),
    ("ecosystem_package_states", "package_ref", "state"),
    ("ecosystem_distributions", "receipt_ref", "receipt"),
    ("ecosystem_distribution_attempts", "receipt_ref", "semantic"),
    ("ecosystem_package_bundles", "release_ref", "bundle"),
];
fn seed_auxiliary(path: &Path) {
    let c = Connection::open(path).unwrap();
    for &(table, _, column) in AUXILIARY {
        let fields = c
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let values=fields.iter().map(|(name,kind)| {
            use rusqlite::types::Value;
            if name==column { return Value::Blob(serde_json::to_vec(&serde_json::json!({"private":AUX_PRIVATE,"source":"event:legacy:first"})).unwrap()); }
            if kind=="INTEGER" { return Value::Integer(1); }
            Value::Text(match name.as_str() {
                "last_event_id"=>"event:legacy:first",
                "aggregate"|"run_id"|"source_run"|"next_run"=>"run:legacy",
                "scope"=>"workspace:legacy-migration",_=>"fixture:auxiliary",
            }.into())
        }).collect::<Vec<_>>();
        let columns = fields
            .iter()
            .map(|f| f.0.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let placeholders = vec!["?"; fields.len()].join(",");
        c.execute(
            &format!("INSERT INTO {table}(rowid,{columns}) VALUES(7,{placeholders})"),
            rusqlite::params_from_iter(values),
        )
        .unwrap_or_else(|e| panic!("{table}: {e}"));
    }
}

#[test]
fn c22_migration_covers_every_auxiliary_blob_with_row_binding_history_and_lineage() {
    let f = Fixture::new();
    f.legacy(true);
    seed_auxiliary(&f.source);
    let before = fs::read(&f.source).unwrap();
    let protection = f.protection();
    let service = LegacyPlaintextMigration::new(protection.clone());
    let plan = service.plan(&owner(), &f.source, &f.target).unwrap();
    let receipt = service
        .execute(&owner(), &f.source, &f.target, &plan, &plan.digest)
        .unwrap();
    assert_eq!(receipt.protected_rows, 19); // 2 events, 2 builtins, 15 auxiliary.
    let c = Connection::open(&f.target).unwrap();
    for &(table, key, column) in AUXILIARY {
        let (row_key, bytes): (String, Vec<u8>) = c
            .query_row(
                &format!("SELECT CAST({key} AS TEXT),{column} FROM {table}"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let envelope: p::EncryptedContentEnvelope =
            serde_json::from_slice(bytes.strip_prefix(PREFIX).unwrap()).unwrap();
        assert_eq!(
            envelope.object.0,
            format!(
                "content:{}",
                p::sha256_content_digest(format!("{table}:{row_key}").as_bytes()).0
            )
        );
        let plain = protection.open(&bytes).unwrap();
        assert!(contains(&plain, AUX_PRIVATE));
        let history:i64=c.query_row("SELECT COUNT(*) FROM protected_rows r JOIN protected_key_history h ON r.object_ref=h.object_ref AND r.key_digest=h.key_digest WHERE r.table_name=?1 AND r.row_key=?2",params![table,row_key],|r|r.get(0)).unwrap();
        assert_eq!(history, 1, "{table}");
        let dependencies:i64=c.query_row("SELECT COUNT(*) FROM protected_dependencies d JOIN protected_rows r ON d.object_ref=r.object_ref JOIN protected_rows s ON d.source_object=s.object_ref WHERE r.table_name=?1 AND s.table_name='events' AND s.row_key='event:legacy:first'",params![table],|r|r.get(0)).unwrap();
        assert_eq!(dependencies, 1, "{table}");
    }
    drop(c);
    scan_target(&f.target);
    assert_eq!(fs::read(&f.source).unwrap(), before);
}

#[test]
fn c22_legacy_migration_checks_real_replay_and_checksums_before_any_target_write() {
    for corrupt_checksum in [false, true] {
        let f = Fixture::new();
        f.legacy(true);
        let c = Connection::open(&f.source).unwrap();
        if corrupt_checksum {
            // Restore the exact original SQL, so this case gets through the
            // schema whitelist and actually exercises checksum verification.
            let immutable_guard: String = c
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name='events_immutable_update'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            c.execute_batch(
                "DROP TRIGGER events_immutable_update; UPDATE events SET checksum='invalid';",
            )
            .unwrap();
            c.execute_batch(&immutable_guard).unwrap();
        } else {
            c.execute("UPDATE transcript SET state=x'7b7d'", [])
                .unwrap();
        }
        drop(c);
        let before = fs::read(&f.source).unwrap();
        let service = LegacyPlaintextMigration::new(f.protection());
        let error = service.plan(&owner(), &f.source, &f.target).unwrap_err();
        assert!(
            error.0.contains(if corrupt_checksum {
                "event checksum/schema"
            } else {
                "transcript projection"
            }),
            "{error}"
        );
        assert!(!f.target.exists());
        assert_eq!(fs::read(&f.source).unwrap(), before);
    }
}

#[test]
fn c22_legacy_migration_encrypts_strategy_catalog_and_distinct_activation_history_bindings() {
    let f = Fixture::new();
    f.legacy(true);
    let store = SqliteEventStore::open(&f.source, StoreOptions::default()).unwrap();
    let candidate = p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId("candidate:legacy".into()),
        domain: p::StrategyDomain::Loop,
        scope: scope(),
        target_tier: p::StabilityTier::Stable,
        proposed_version: p::StrategyVersionRef("strategy:legacy:1".into()),
        baseline: p::StrategyVersionRef("strategy:legacy:0".into()),
        spec_ref: p::ContentRef(PRIVATE.into()),
        spec_digest: p::sha256_content_digest(b"fixture:legacy-strategy"),
        evidence: vec![p::EvidenceRef("evidence:legacy".into())],
        provenance: provenance(),
        impact: p::EvolutionImpact::Cautious,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef("strategy:legacy:0".into()),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    };
    let make = |id: &str, payload| {
        p::Event::new(
            p::EventId(id.into()),
            p::RunId("run:strategy-legacy".into()),
            None,
            payload,
            p::SchemaVersion(1),
            31,
            provenance(),
        )
    };
    store
        .append(make(
            "event:legacy:candidate",
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: candidate.candidate.clone(),
                target: p::CandidateTargetRef("strategy:legacy".into()),
                evidence_refs: candidate.evidence.clone(),
                confidence: p::Confidence(0.9),
                provenance: provenance(),
                target_tier: p::StabilityTier::Stable,
                capability_update: None,
                strategy_candidate: Some(candidate.clone()),
            }),
        ))
        .unwrap();
    store
        .append(make(
            "event:legacy:evaluation",
            p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
                evaluation: p::EvolutionEvaluationRef("evaluation:legacy".into()),
                baseline: candidate.baseline.clone(),
                candidate: candidate.proposed_version.clone(),
                verdict: p::EvaluationVerdict::Pass,
                hard_invariants: vec![p::InvariantResultRef("invariant:legacy".into())],
                ground_truth: vec![p::EvidenceRef("ground:legacy".into())],
            }),
        ))
        .unwrap();
    store
        .append(make(
            "event:legacy:promoted",
            p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                candidate_id: candidate.candidate.clone(),
                by: p::DecisionActor::User,
                reason: p::ReasonRef("owner:confirmed".into()),
                stable: None,
            }),
        ))
        .unwrap();
    let aggregate = p::EvolutionAggregateRef("evolution:legacy".into());
    let version = |value| p::EvolutionAggregateVersion {
        schema_version: p::SchemaVersion(1),
        aggregate: aggregate.clone(),
        value,
    };
    let mut activate = make(
        "event:legacy:activation",
        p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
            activation: p::StrategyActivation {
                schema_version: p::SchemaVersion(1),
                aggregate: aggregate.clone(),
                domain: p::StrategyDomain::Loop,
                scope: scope(),
                from: None,
                to: candidate.proposed_version.clone(),
                spec_ref: candidate.spec_ref.clone(),
                spec_digest: candidate.spec_digest.clone(),
                evaluation: p::EvolutionEvaluationRef("evaluation:legacy".into()),
                promotion: p::EventId("event:legacy:promoted".into()),
                owner_confirmation: None,
                impact: p::EvolutionImpact::Cautious,
                expected_version: version(0),
                committed_version: version(1),
            },
            active_snapshot: p::EvolutionSnapshotRef("preview:pending".into()),
        }),
    );
    let preview = store.preview_evolution_snapshot(&activate).unwrap();
    if let p::EventPayload::StrategyActivated(payload) = &mut activate.payload {
        payload.active_snapshot = preview.snapshot;
    }
    store
        .append_evolution_expected(activate, &aggregate, version(0))
        .unwrap();
    let original = store.evolution_history(&aggregate).unwrap();
    drop(store);
    let before = fs::read(&f.source).unwrap();
    let service = LegacyPlaintextMigration::new(f.protection());
    let plan = service.plan(&owner(), &f.source, &f.target).unwrap();
    service
        .execute(&owner(), &f.source, &f.target, &plan, &plan.digest)
        .unwrap();
    scan_target(&f.target);
    let store = SqliteEventStore::open(&f.target, StoreOptions::default()).unwrap();
    store.enable_content_protection(f.protection()).unwrap();
    assert_eq!(store.evolution_history(&aggregate).unwrap(), original);
    assert_eq!(
        store
            .strategy_candidate(&candidate.candidate)
            .unwrap()
            .unwrap(),
        candidate
    );
    assert!(store
        .stable_strategy(
            candidate.domain,
            &candidate.scope,
            &candidate.proposed_version
        )
        .unwrap()
        .is_some());
    let raw = Connection::open(&f.target).unwrap();
    for table in [
        "strategy_candidates",
        "stable_strategies",
        "evolution_evaluations",
        "active_strategies",
        "evolution_history",
    ] {
        let count:i64=raw.query_row("SELECT COUNT(*) FROM protected_rows r JOIN protected_key_history h ON r.object_ref=h.object_ref AND r.key_digest=h.key_digest WHERE r.table_name=?1",params![table],|r|r.get(0)).unwrap();
        assert_eq!(count, 1, "{table}");
    }
    drop(raw);
    assert_eq!(fs::read(&f.source).unwrap(), before);
}
