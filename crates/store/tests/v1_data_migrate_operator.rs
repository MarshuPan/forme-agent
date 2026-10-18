//! Exercise the actual offline operator process against self-owned raw SQLite.
#![forbid(unsafe_code)]

use forme_protocol as p;
use forme_store::{
    BrainAuthorityClaim, BrainAuthorityStore, EventStore, FileContentKeyResolver,
    LegacyMigrationPlan, LegacyMigrationReceipt, SchemaSnapshot, SourceBackupDisposition,
    SqliteEventStore, StoreContentProtection, StoreOptions,
};
use rusqlite::{params, Connection};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

const OWNER: &str = "owner:operator-fixture";
const PRIVATE: &str = "operator-private-input-8c632ac5";
const MAX_PLAN_BYTES: usize = 256 * 1024;
const FAILURE: &str = "forme-data-migrate: migration not completed; check owner, exact approval, offline source and NEW target. Stop the authority daemon. Source and any incomplete target/key vault remain sensitive; source is unprotected and authority is unchanged.\n";
static NEXT: AtomicU64 = AtomicU64::new(1);

fn accepted() -> p::Event {
    p::Event::new(
        p::EventId("event:operator:accepted".into()),
        p::RunId("run:operator".into()),
        None,
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId("session:operator".into()),
            input_ref: p::InputRef(PRIVATE.into()),
            idempotency_key: Some(p::IdempotencyKey("command:operator".into())),
        }),
        p::SchemaVersion(1),
        17,
        p::Provenance {
            source: p::Source::UserTurn,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    )
}

fn framed(id: &str) -> p::Event {
    let mut event = accepted();
    event.event_id = p::EventId(id.into());
    event.ts_unix_ms = 2; // stream_seq, not wall clock, orders replay.
    event.provenance.caused_by = Some(accepted().event_id);
    event.payload = p::EventPayload::GoalFramed(p::GoalFramedPayload {
        goal_frame: p::GoalFrameRef("goal:operator".into()),
        long_term: None,
        knowledge: None,
    });
    event.kind = p::EventKind::GoalFramed;
    event
}

fn snapshot() -> SchemaSnapshot {
    SchemaSnapshot {
        schema: StoreOptions::default().current_schema,
        policy_version: p::Version(1),
        loop_version: p::Version(1),
        model_profile: p::ModelProfileRef("fixture:operator-replay".into()),
        tool_schema: p::Version(1),
    }
}

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new(bind_owner: bool) -> Self {
        let tag = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forme-operator 私有测试-{}-{tag}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let fixture = Self {
            source: root.join("legacy source.db"),
            target: root.join("protected target.db"),
            root,
        };
        let store = SqliteEventStore::open(
            &fixture.source,
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
        )
        .unwrap();
        if bind_owner {
            store
                .claim_brain_authority(BrainAuthorityClaim {
                    owner_digest: p::sha256_content_digest(OWNER.as_bytes()),
                    process_nonce: p::Nonce("operator-fixture-stopped".into()),
                    now_ms: 1,
                    lease_ttl: p::DurationMs(1),
                })
                .unwrap();
        }
        store.append(accepted()).unwrap();
        store.append(framed("event:operator:framed")).unwrap();
        assert!(store
            .replay(accepted().run_id, snapshot())
            .unwrap()
            .diff_vs_current
            .is_empty());
        drop(store);
        assert!(contains(&fs::read(&fixture.source).unwrap(), PRIVATE));
        fixture
    }

    fn command(&self, operation: &str, source: &Path, target: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forme-data-migrate"));
        // Inherited runtime configuration must not point a child at real data.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("FORME_") {
                command.env_remove(key);
            }
        }
        command
            .current_dir(&self.root)
            .env("FORME_OWNER_ID", OWNER)
            .arg(operation)
            .arg(source)
            .arg(target);
        command
    }

    fn plan(&self) -> (LegacyMigrationPlan, Vec<u8>) {
        let before = tree(&self.root);
        let output = invoke(self.command("plan", &self.source, &self.target), b"");
        self.assert_safe(&output);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let plan: LegacyMigrationPlan = serde_json::from_slice(&output.stdout).unwrap();
        plan.validate().unwrap();
        assert_eq!(tree(&self.root), before, "planning changed owner files");
        (plan, output.stdout)
    }

    fn apply_command(&self, plan: &LegacyMigrationPlan) -> Command {
        let mut command = self.command("apply", &self.source, &self.target);
        command.arg(&plan.digest.0);
        command
    }

    fn rejects_without_writes(&self, command: Command, input: &[u8]) {
        let before = tree(&self.root);
        self.rejects(command, input);
        assert_eq!(tree(&self.root), before, "rejection changed owner files");
    }

    fn rejects(&self, command: Command, input: &[u8]) {
        let output = invoke(command, input);
        self.assert_safe(&output);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, FAILURE.as_bytes());
    }

    fn assert_safe(&self, output: &Output) {
        for bytes in [&output.stdout, &output.stderr] {
            for marker in [PRIVATE, OWNER, self.root.to_str().unwrap()] {
                assert!(!contains(bytes, marker), "operator disclosed private data");
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn invoke(mut command: Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    if let Err(error) = stdin.write_all(input) {
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
    drop(stdin);
    child.wait_with_output().unwrap()
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Option<p::SchemaDigest>> {
    fn walk(path: &Path, result: &mut BTreeMap<PathBuf, Option<p::SchemaDigest>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                result.insert(path.clone(), None);
                walk(&path, result);
            } else {
                result.insert(
                    path.clone(),
                    Some(p::sha256_content_digest(&fs::read(path).unwrap())),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    walk(root, &mut result);
    result
}

fn contains(bytes: &[u8], marker: &str) -> bool {
    bytes
        .windows(marker.len())
        .any(|part| part == marker.as_bytes())
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut path = path.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

fn redigest(plan: &mut LegacyMigrationPlan) {
    plan.digest = p::SchemaDigest(String::new());
    plan.digest = p::canonical_digest(plan).unwrap();
}

#[test]
fn c22_operator_plan_apply_preserves_source_and_restarts_protected_store() {
    let fixture = Fixture::new(true);
    let source_before = p::sha256_content_digest(&fs::read(&fixture.source).unwrap());
    let (plan, mut input) = fixture.plan();
    assert_eq!(plan.target_scope, p::Scope("workspace:default".into()));
    assert_eq!(
        plan.owner_digest,
        p::sha256_content_digest(OWNER.as_bytes())
    );
    assert_eq!(plan.event_count, 2);
    assert_eq!(plan.content_rows, 4);
    // Exactly 256 KiB is accepted; one byte beyond is tested separately.
    input.resize(MAX_PLAN_BYTES, b' ');
    let output = invoke(fixture.apply_command(&plan), &input);
    fixture.assert_safe(&output);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let receipt: LegacyMigrationReceipt = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt.plan_digest, plan.digest);
    assert_eq!(receipt.event_count, 2);
    assert_eq!(receipt.protected_rows, 4);
    assert!(receipt.integrity_verified && receipt.replay_verified && receipt.source_unchanged);
    assert!(!receipt.source_deleted && !receipt.authority_switched);
    assert_eq!(
        receipt.source_backup,
        SourceBackupDisposition::UnprotectedBackupOutsideProtectedDatabase
    );
    assert_eq!(receipt.source_database_digest, source_before);
    assert_eq!(
        receipt.target_database_digest,
        p::sha256_content_digest(&fs::read(&fixture.target).unwrap())
    );
    let vault = fixture.target.with_extension("content-keys");
    assert!(vault.is_dir());
    assert!(fs::read_dir(&vault).unwrap().any(|entry| entry
        .unwrap()
        .path()
        .extension()
        .is_some_and(|ext| ext == "key")));
    let raw = Connection::open(&fixture.target).unwrap();
    let payload: Vec<u8> = raw
        .query_row("SELECT payload FROM events WHERE stream_seq=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(payload.starts_with(b"FORME-AEAD-1\n"));
    let fts: i64 = raw
        .query_row("SELECT COUNT(*) FROM events_fts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(fts, 0);
    drop(raw);

    for _ in 0..2 {
        let keys = Arc::new(FileContentKeyResolver::new(&vault).unwrap());
        let protection = StoreContentProtection::new(
            &p::VerifiedPrincipal(OWNER.into()),
            plan.target_scope.clone(),
            keys,
        )
        .unwrap();
        let store = SqliteEventStore::open_with_content_protection(
            &fixture.target,
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
            protection,
        )
        .unwrap();
        let events = store
            .read_run(accepted().run_id)
            .collect::<p::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            vec![p::EventKind::RunAccepted, p::EventKind::GoalFramed]
        );
        assert_eq!(
            events
                .iter()
                .map(|event| event.stream_seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(events[0].payload, accepted().payload);
        assert!(store
            .replay(accepted().run_id, snapshot())
            .unwrap()
            .diff_vs_current
            .is_empty());
        assert!(store.load_transcript(accepted().run_id).unwrap().is_some());
        assert_eq!(store.search(PRIVATE, 10).unwrap().len(), 1);
        drop(store);
        for suffix in ["", "-wal", "-shm", "-journal"] {
            if let Ok(bytes) = fs::read(sidecar(&fixture.target, suffix)) {
                assert!(!contains(&bytes, PRIVATE), "target retained plaintext");
            }
        }
        assert_eq!(
            p::sha256_content_digest(&fs::read(&fixture.source).unwrap()),
            source_before
        );
        assert!(contains(&fs::read(&fixture.source).unwrap(), PRIVATE));
    }
}

#[test]
fn c22_operator_requires_environment_owner_bound_to_source_authority() {
    let fixture = Fixture::new(true);
    let (plan, input) = fixture.plan();
    for owner in [None, Some(""), Some("owner:someone-else")] {
        for operation in ["plan", "apply"] {
            let mut command = fixture.command(operation, &fixture.source, &fixture.target);
            command.env_remove("FORME_OWNER_ID");
            if let Some(owner) = owner {
                command.env("FORME_OWNER_ID", owner);
            }
            if operation == "apply" {
                command.arg(&plan.digest.0);
            }
            fixture.rejects_without_writes(command, &input);
        }
    }
    let mut payload = serde_json::to_value(&plan).unwrap();
    payload["owner"] = serde_json::json!(OWNER);
    let mut command = fixture.apply_command(&plan);
    command.env("FORME_OWNER_ID", "owner:someone-else");
    fixture.rejects_without_writes(command, &serde_json::to_vec(&payload).unwrap());
    let unbound = Fixture::new(false);
    unbound.rejects_without_writes(
        unbound.command("plan", &unbound.source, &unbound.target),
        b"",
    );
}

#[test]
fn c22_operator_rejects_changed_digest_bindings_expiry_and_stale_source_without_writes() {
    let fixture = Fixture::new(true);
    let (plan, input) = fixture.plan();
    let mut wrong_digest = fixture.command("apply", &fixture.source, &fixture.target);
    wrong_digest.arg(p::sha256_content_digest(b"unapproved-plan").0);
    fixture.rejects_without_writes(wrong_digest, &input);
    for field in 0..11 {
        let mut changed = plan.clone();
        let different = p::sha256_content_digest(b"different-binding");
        match field {
            0 => changed.owner_digest = different,
            1 => changed.target_scope = p::Scope("workspace:elsewhere".into()),
            2 => changed.source_path_digest = different,
            3 => changed.target_path_digest = different,
            4 => changed.store_identity = different,
            5 => changed.source_database_digest = different,
            6 => changed.schema_digest = different,
            7 => changed.event_count += 1,
            8 => changed.content_rows += 1,
            9 => changed.schema_version = p::SchemaVersion(99),
            10 => changed.expires_at = 1,
            _ => unreachable!(),
        }
        redigest(&mut changed); // Exercise bindings/expiry, not only a bad hash.
        fixture.rejects_without_writes(
            fixture.apply_command(&changed),
            &serde_json::to_vec(&changed).unwrap(),
        );
    }
    let mut changed = plan.clone();
    changed.event_count += 1; // Unchanged digest also must fail.
    fixture.rejects_without_writes(
        fixture.apply_command(&plan),
        &serde_json::to_vec(&changed).unwrap(),
    );
    let mut command = fixture.apply_command(&plan);
    command.env("FORME_WORKSPACE", "workspace:elsewhere");
    fixture.rejects_without_writes(command, &input);
    let store = SqliteEventStore::open(&fixture.source, StoreOptions::default()).unwrap();
    store.append(framed("event:operator:after-plan")).unwrap();
    drop(store);
    fixture.rejects_without_writes(fixture.apply_command(&plan), &input);
}

#[test]
fn c22_operator_rejects_rebound_paths_and_existing_target_or_key_vault() {
    let fixture = Fixture::new(true);
    let (plan, input) = fixture.plan();
    let copy = fixture.root.join("other source.db");
    fs::copy(&fixture.source, &copy).unwrap();
    for (source, target) in [
        (&copy, fixture.target.clone()),
        (&fixture.source, fixture.root.join("other target.db")),
        (&fixture.source, fixture.source.clone()),
        (
            &fixture.source,
            fixture.root.join("missing parent").join("new.db"),
        ),
        (&fixture.source, fixture.root.join("vault.CONTENT-KEYS")),
    ] {
        let mut command = fixture.command("apply", source, &target);
        command.arg(&plan.digest.0);
        fixture.rejects_without_writes(command, &input);
    }
    for collision in 0..4 {
        let fixture = Fixture::new(true);
        let (plan, input) = fixture.plan();
        let vault = fixture.target.with_extension("content-keys");
        match collision {
            0 => fs::write(&fixture.target, b"existing-owner-database").unwrap(),
            1 => fs::write(vault, b"existing-sensitive-vault-file").unwrap(),
            2 => fs::create_dir(vault).unwrap(),
            3 => {
                fs::create_dir(&vault).unwrap();
                fs::write(vault.join("existing.key"), [0x39; 32]).unwrap();
            }
            _ => unreachable!(),
        }
        fixture.rejects_without_writes(
            fixture.command("plan", &fixture.source, &fixture.target),
            b"",
        );
        fixture.rejects_without_writes(fixture.apply_command(&plan), &input);
    }
}

#[test]
fn c22_operator_plan_input_is_required_bounded_and_strict() {
    let fixture = Fixture::new(true);
    let (plan, input) = fixture.plan();
    let mut unknown = serde_json::to_value(&plan).unwrap();
    unknown["owner"] = serde_json::json!(OWNER);
    let mut trailing = input.clone();
    trailing.extend_from_slice(b"{}");
    let mut oversized = input.clone();
    oversized.resize(MAX_PLAN_BYTES + 1, b' ');
    for bad in [
        Vec::new(),
        b" \n".to_vec(),
        PRIVATE.as_bytes().to_vec(),
        serde_json::to_vec(&unknown).unwrap(),
        trailing,
        oversized,
    ] {
        fixture.rejects_without_writes(fixture.apply_command(&plan), &bad);
    }
    for operation in ["plan", "apply", "restore"] {
        let mut command = fixture.command(operation, &fixture.source, &fixture.target);
        command.arg(&plan.digest.0).arg("extra");
        fixture.rejects_without_writes(command, &input);
    }
    fixture.rejects_without_writes(
        fixture.command("apply", &fixture.source, &fixture.target),
        &input,
    );
    for workspace in [
        "",
        "C:\\private\\brain",
        "https://private.invalid",
        "secret:workspace",
    ] {
        let mut command = fixture.command("plan", &fixture.source, &fixture.target);
        command.env("FORME_WORKSPACE", workspace);
        fixture.rejects_without_writes(command, b"");
    }
}

#[test]
fn c22_operator_requires_cold_source_and_stopped_authority() {
    for (target_sidecar, suffix) in [
        (false, "-wal"),
        (false, "-shm"),
        (false, "-journal"),
        (true, "-wal"),
        (true, "-shm"),
        (true, "-journal"),
    ] {
        let fixture = Fixture::new(true);
        let (plan, input) = fixture.plan();
        fs::write(
            sidecar(
                if target_sidecar {
                    &fixture.target
                } else {
                    &fixture.source
                },
                suffix,
            ),
            b"sensitive-hot-database-sidecar",
        )
        .unwrap();
        fixture.rejects_without_writes(
            fixture.command("plan", &fixture.source, &fixture.target),
            b"",
        );
        fixture.rejects_without_writes(fixture.apply_command(&plan), &input);
    }
    let fixture = Fixture::new(true);
    let (plan, input) = fixture.plan();
    let connection = Connection::open(&fixture.source).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    connection
        .execute(
            "UPDATE authority_writer_lease SET expires_at=?1",
            params![now + 3_600_000],
        )
        .unwrap();
    drop(connection);
    fixture.rejects_without_writes(
        fixture.command("plan", &fixture.source, &fixture.target),
        b"",
    );
    fixture.rejects_without_writes(fixture.apply_command(&plan), &input);

    let fixture = Fixture::new(true);
    let (plan, input) = fixture.plan();
    let before = tree(&fixture.root);
    let connection = Connection::open(&fixture.source).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    // Windows byte-range locks also deny the test's raw file reads. Compare
    // hashes after releasing the lock, while the operator must fail during it.
    fixture.rejects(
        fixture.command("plan", &fixture.source, &fixture.target),
        b"",
    );
    fixture.rejects(fixture.apply_command(&plan), &input);
    connection.execute_batch("ROLLBACK").unwrap();
    drop(connection);
    assert_eq!(tree(&fixture.root), before, "rejection changed owner files");
}

#[test]
fn c22_operator_apply_is_one_shot_and_does_not_overwrite_completed_target() {
    let fixture = Fixture::new(true);
    let mut command = fixture.command("plan", &fixture.source, &fixture.target);
    command.env("FORME_WORKSPACE", "workspace:operator");
    let output = invoke(command, b"");
    fixture.assert_safe(&output);
    assert!(output.status.success());
    let plan: LegacyMigrationPlan = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan.target_scope, p::Scope("workspace:operator".into()));
    let mut command = fixture.apply_command(&plan);
    command.env("FORME_WORKSPACE", "workspace:operator");
    let receipt = invoke(command, &output.stdout);
    fixture.assert_safe(&receipt);
    assert!(receipt.status.success());
    let mut command = fixture.apply_command(&plan);
    command.env("FORME_WORKSPACE", "workspace:operator");
    fixture.rejects_without_writes(command, &output.stdout);
    fixture.rejects_without_writes(
        fixture.command("plan", &fixture.source, &fixture.target),
        b"",
    );
}
