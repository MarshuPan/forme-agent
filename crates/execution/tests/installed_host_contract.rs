use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use ed25519_dalek::{Signer, SigningKey};
use forme_execution::installed::{
    file_digest, reject_linked_ancestors, HostCredentials, HostManagement, NativeServiceManager,
    ProductHostBroker, ProductJournal, ReleaseKeyring,
};
use forme_protocol as p;

static NEXT: AtomicU64 = AtomicU64::new(0);
const HOST_TOKEN: &str = "host-token-abcdefghijklmnopqrstuvwxyz-0123456789";
const AGENT_TOKEN: &str = "agent-token-abcdefghijklmnopqrstuvwxyz-0123456789";
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "forme-installed-contract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn journal(&self) -> PathBuf {
        self.0.join("product.jsonl")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn command(revision: u64) -> p::MaintenanceCommand {
    p::MaintenanceCommand {
        schema_version: p::SchemaVersion(1),
        operation_id: "operation-1".into(),
        idempotency_key: "command-1".into(),
        expected_revision: revision,
        actor_proof: p::MaintenanceProofRef {
            schema_version: p::SchemaVersion(1),
            proof_ref: "proof-1".into(),
        },
        kind: p::MaintenanceKind::StartCore,
        requested_deadline: 1_000_000,
    }
}
fn actor() -> p::MaintenanceActorClass {
    p::MaintenanceActorClass::LocalAdministrator
}
fn references() -> p::MaintenanceReferences {
    p::MaintenanceReferences {
        schema_version: p::SchemaVersion(1),
        original_release_ref: "0.1.0".into(),
        target_release_ref: "0.1.0".into(),
        data_candidate_ref: None,
        backup_ref: None,
    }
}

#[derive(Clone)]
struct FakeServices {
    state: Arc<std::sync::Mutex<p::NativeServiceState>>,
    actions: Arc<AtomicU64>,
}

impl FakeServices {
    fn new(state: p::NativeServiceState) -> Self {
        Self {
            state: Arc::new(std::sync::Mutex::new(state)),
            actions: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl NativeServiceManager for FakeServices {
    fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service,
            state: *self.state.lock().unwrap(),
        })
    }

    fn apply(&self, change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        self.actions.fetch_add(1, Ordering::Relaxed);
        let mut state = self.state.lock().unwrap();
        *state = match change.action {
            p::NativeServiceAction::Start => p::NativeServiceState::Running,
            p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
        };
        Ok(p::NativeReceipt {
            schema_version: p::SchemaVersion(1),
            service: change.service,
            requested: change.action,
            observed: *state,
        })
    }
}

fn host_request(
    operation_id: &str,
    kind: p::MaintenanceKind,
    proof_ref: &str,
    original_release_ref: &str,
) -> p::HostManagementRequest {
    p::HostManagementRequest {
        schema_version: p::SchemaVersion(1),
        command: p::MaintenanceCommand {
            schema_version: p::SchemaVersion(1),
            operation_id: operation_id.into(),
            idempotency_key: format!("{operation_id}-key"),
            expected_revision: 1,
            actor_proof: p::MaintenanceProofRef {
                schema_version: p::SchemaVersion(1),
                proof_ref: proof_ref.into(),
            },
            kind,
            requested_deadline: 1_000_000,
        },
        references: p::MaintenanceReferences {
            schema_version: p::SchemaVersion(1),
            original_release_ref: original_release_ref.into(),
            target_release_ref: original_release_ref.into(),
            data_candidate_ref: None,
            backup_ref: None,
        },
    }
}

fn test_broker(temp: &Temp, services: FakeServices) -> ProductHostBroker<FakeServices> {
    ProductHostBroker::open_with_service_manager(
        &temp.journal(),
        "installation-1",
        HostCredentials::legacy_maintenance(HOST_TOKEN.into()),
        p::ProductTarget::WindowsX86_64,
        "0.1.0".into(),
        services,
    )
    .unwrap()
}

fn host_proof() -> String {
    p::canonical_digest(&HOST_TOKEN).unwrap().0
}

#[test]
fn host_broker_rejects_wrong_proof_and_stale_release_without_writing() {
    let temp = Temp::new();
    let broker = test_broker(&temp, FakeServices::new(p::NativeServiceState::Stopped));
    assert!(broker.authenticate("wrong-token").is_err());
    let auth = broker.authenticate(HOST_TOKEN).unwrap();
    let wrong_proof = host_request(
        "operation-proof",
        p::MaintenanceKind::Inspect,
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "0.1.0",
    );
    assert!(broker.plan(&auth, wrong_proof, 2).is_err());
    let stale_release = host_request(
        "operation-stale",
        p::MaintenanceKind::Inspect,
        &host_proof(),
        "0.0.9",
    );
    assert!(broker.plan(&auth, stale_release, 1).is_err());
    assert_eq!(
        broker.status(&auth).unwrap().product_state,
        p::ProductState::Stopped
    );
}

#[test]
fn maintenance_and_agent_credentials_are_distinct_and_non_interchangeable() {
    let temp = Temp::new();
    let broker = ProductHostBroker::open_with_service_manager(
        &temp.journal(),
        "installation-1",
        HostCredentials::new(HOST_TOKEN.into(), Some(AGENT_TOKEN.into())).unwrap(),
        p::ProductTarget::WindowsX86_64,
        "0.1.0".into(),
        FakeServices::new(p::NativeServiceState::Stopped),
    )
    .unwrap();
    let maintenance = broker.authenticate(HOST_TOKEN).unwrap();
    let agent = broker.authenticate_agent(AGENT_TOKEN).unwrap();
    assert_eq!(
        maintenance.domain(),
        forme_execution::installed::HostCredentialDomain::OwnerMaintenance
    );
    assert_eq!(
        agent.domain(),
        forme_execution::installed::HostCredentialDomain::AgentExecution
    );
    assert!(broker.status(&agent).is_err());
    assert!(broker.authenticate(AGENT_TOKEN).is_err());
    assert!(broker.authenticate_agent(HOST_TOKEN).is_err());
    assert!(HostCredentials::new(HOST_TOKEN.into(), Some(HOST_TOKEN.into())).is_err());
}

#[test]
fn host_broker_plans_and_executes_a_native_action_exactly_once() {
    let temp = Temp::new();
    let services = FakeServices::new(p::NativeServiceState::Stopped);
    let actions = services.actions.clone();
    let broker = test_broker(&temp, services);
    let auth = broker.authenticate(HOST_TOKEN).unwrap();
    let request = host_request(
        "operation-start",
        p::MaintenanceKind::StartCore,
        &host_proof(),
        "0.1.0",
    );
    let planned = broker.plan(&auth, request.clone(), 1).unwrap();
    assert_eq!(planned.stage, p::MaintenanceStage::Planned);
    assert_eq!(broker.plan(&auth, request, 2).unwrap(), planned);
    let committed = broker.execute(&auth, "operation-start", 3).unwrap();
    assert_eq!(committed.stage, p::MaintenanceStage::Committed);
    assert_eq!(actions.load(Ordering::Relaxed), 1);
    assert_eq!(
        broker.execute(&auth, "operation-start", 4).unwrap(),
        committed
    );
    assert_eq!(actions.load(Ordering::Relaxed), 1);
}

#[test]
fn host_broker_fails_closed_for_unimplemented_maintenance_workers() {
    let temp = Temp::new();
    let broker = test_broker(&temp, FakeServices::new(p::NativeServiceState::Stopped));
    let auth = broker.authenticate(HOST_TOKEN).unwrap();
    let request = host_request(
        "operation-config",
        p::MaintenanceKind::ValidateConfig { config_revision: 1 },
        &host_proof(),
        "0.1.0",
    );
    assert!(broker.plan(&auth, request, 1).is_err());
    assert!(broker.operation(&auth, "operation-config").is_err());
}

#[test]
fn concurrent_host_execution_cannot_duplicate_the_native_call() {
    let temp = Temp::new();
    let services = FakeServices::new(p::NativeServiceState::Stopped);
    let actions = services.actions.clone();
    let broker = Arc::new(test_broker(&temp, services));
    let auth = broker.authenticate(HOST_TOKEN).unwrap();
    broker
        .plan(
            &auth,
            host_request(
                "start-once",
                p::MaintenanceKind::StartCore,
                &host_proof(),
                "0.1.0",
            ),
            1,
        )
        .unwrap();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let broker = broker.clone();
            let auth = auth.clone();
            std::thread::spawn(move || broker.execute(&auth, "start-once", 2))
        })
        .collect();
    for thread in threads {
        let _ = thread.join().unwrap();
    }
    assert_eq!(actions.load(Ordering::Relaxed), 1);
    assert_eq!(
        broker.operation(&auth, "start-once").unwrap().stage,
        p::MaintenanceStage::Committed
    );
    assert_eq!(
        broker.status(&auth).unwrap().core_health,
        p::CoreHealth::Starting
    );
    assert_ne!(
        broker.status(&auth).unwrap().product_state,
        p::ProductState::Ready
    );
}

#[test]
fn expired_host_plan_has_zero_native_calls_and_zero_revision_change() {
    let temp = Temp::new();
    let services = FakeServices::new(p::NativeServiceState::Stopped);
    let actions = services.actions.clone();
    let broker = test_broker(&temp, services);
    let auth = broker.authenticate(HOST_TOKEN).unwrap();
    let planned = broker
        .plan(
            &auth,
            host_request(
                "expired",
                p::MaintenanceKind::StartCore,
                &host_proof(),
                "0.1.0",
            ),
            1,
        )
        .unwrap();
    assert!(broker.execute(&auth, "expired", 1_000_000).is_err());
    assert_eq!(actions.load(Ordering::Relaxed), 0);
    assert_eq!(broker.operation(&auth, "expired").unwrap(), planned);
}

#[test]
fn idempotency_cannot_replace_server_bound_release_references() {
    let temp = Temp::new();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    journal.plan(command(1), actor(), references(), 1).unwrap();
    let mut changed = references();
    changed.original_release_ref = "0.0.9".into();
    changed.target_release_ref = "0.0.9".into();
    assert!(journal.plan(command(1), actor(), changed, 1).is_err());
    assert_eq!(journal.revision().unwrap(), 2);
}

#[test]
fn broker_crash_after_native_effect_preserves_intent_and_forbids_reexecution() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let temp = Temp::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_forme-host-storage-fixture"))
        .arg(temp.journal())
        .arg("--broker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = send.send(line);
    });
    let marker = receive.recv_timeout(std::time::Duration::from_secs(10));
    let _ = child.kill();
    let _ = child.wait();
    reader.join().unwrap();
    assert_eq!(marker.unwrap().trim(), "host-intent-durable");
    let services = FakeServices::new(p::NativeServiceState::Running);
    let actions = services.actions.clone();
    let broker = test_broker(&temp, services);
    let auth = broker.authenticate(HOST_TOKEN).unwrap();
    assert_eq!(
        broker
            .operation(&auth, "operation-1")
            .unwrap()
            .pending_stage,
        Some(p::MaintenanceStage::Checking)
    );
    assert_eq!(
        broker.status(&auth).unwrap().product_state,
        p::ProductState::RecoveryRequired
    );
    assert!(broker.execute(&auth, "operation-1", 3).is_err());
    assert_eq!(actions.load(Ordering::Relaxed), 0);
    assert_eq!(
        fs::read(temp.journal().with_extension("effect-count")).unwrap(),
        b"1"
    );
}
fn receipt() -> Vec<p::SchemaDigest> {
    vec![p::canonical_digest(&"independently-observed").unwrap()]
}
fn stage(journal: &ProductJournal, next: p::MaintenanceStage) {
    journal
        .begin_stage("operation-1", journal.revision().unwrap(), next, actor(), 2)
        .unwrap();
    journal
        .finish_stage(
            "operation-1",
            journal.revision().unwrap(),
            next,
            receipt(),
            3,
        )
        .unwrap();
}

#[test]
fn os_lock_blocks_a_second_writer_and_binding_survives_restart() {
    let temp = Temp::new();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    assert!(ProductJournal::open(&temp.journal(), "installation-1").is_err());
    assert_eq!(journal.revision().unwrap(), 1);
    drop(journal);
    assert!(ProductJournal::open(&temp.journal(), "different-installation").is_err());
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    assert_eq!(journal.revision().unwrap(), 1);
}

#[test]
fn killed_writer_releases_os_lock_and_restart_never_repeats_the_observed_effect() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let temp = Temp::new();
    let mut child = Command::new(env!("CARGO_BIN_EXE_forme-host-storage-fixture"))
        .arg(temp.journal())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line);
        let _ = send.send((result.is_ok(), line));
    });
    let marker = receive.recv_timeout(std::time::Duration::from_secs(10));
    if !marker
        .as_ref()
        .is_ok_and(|(ok, line)| *ok && line.trim() == "host-intent-durable")
    {
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        panic!("host fixture did not publish its durable marker");
    }
    assert!(ProductJournal::open(&temp.journal(), "installation-1").is_err());
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();
    let baseline = fs::read(temp.journal()).unwrap();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    assert_eq!(
        journal.operation("operation-1").unwrap().pending_stage,
        Some(p::MaintenanceStage::Validated)
    );
    assert_eq!(
        fs::read(temp.journal().with_extension("effect-count")).unwrap(),
        b"1"
    );
    drop(journal);
    assert_eq!(fs::read(temp.journal()).unwrap(), baseline);
}

#[test]
fn durable_idempotency_and_cas_reject_conflicts_without_writes() {
    let temp = Temp::new();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    let request = command(journal.revision().unwrap());
    let planned = journal
        .plan(request.clone(), actor(), references(), 1)
        .unwrap();
    assert_eq!(
        journal
            .plan(request.clone(), actor(), references(), 1)
            .unwrap(),
        planned
    );
    drop(journal);
    let baseline = fs::read(temp.journal()).unwrap();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    let mut conflict = request.clone();
    conflict.kind = p::MaintenanceKind::StopCore;
    assert!(journal.plan(conflict, actor(), references(), 1).is_err());
    let mut stale = request.clone();
    stale.idempotency_key = "other-key".into();
    stale.operation_id = "operation-2".into();
    assert!(journal.plan(stale, actor(), references(), 1).is_err());
    drop(journal);
    assert_eq!(fs::read(temp.journal()).unwrap(), baseline);
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    assert_eq!(
        journal
            .plan(request, actor(), references(), 2_000_000)
            .unwrap(),
        planned
    );
    drop(journal);
    assert_eq!(fs::read(temp.journal()).unwrap(), baseline);
}

#[test]
fn concurrent_commands_cannot_both_advance_the_installation() {
    let temp = Temp::new();
    let journal = Arc::new(ProductJournal::open(&temp.journal(), "installation-1").unwrap());
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let journal = journal.clone();
            std::thread::spawn(move || {
                let mut c = command(1);
                c.idempotency_key = format!("key-{i}");
                c.operation_id = format!("op-{i}");
                journal.plan(c, actor(), references(), 1).is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter(|won| *won)
            .count(),
        1
    );
    assert_eq!(journal.revision().unwrap(), 2);
}

#[test]
fn crash_after_intent_preserves_ambiguity_and_never_reexecutes_a_stage() {
    let temp = Temp::new();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    journal.plan(command(1), actor(), references(), 1).unwrap();
    assert!(journal
        .plan(command(1), p::MaintenanceActorClass::Setup, references(), 2)
        .is_err());
    journal
        .begin_stage("operation-1", 2, p::MaintenanceStage::Validated, actor(), 2)
        .unwrap();
    drop(journal);
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    let state = journal.operation("operation-1").unwrap();
    assert_eq!(state.stage, p::MaintenanceStage::Planned);
    assert_eq!(state.pending_stage, Some(p::MaintenanceStage::Validated));
    assert!(journal
        .begin_stage("operation-1", 3, p::MaintenanceStage::Validated, actor(), 3)
        .is_err());
    let mut new = command(3);
    new.operation_id = "new-attempt".into();
    new.idempotency_key = "new-key".into();
    assert!(journal.plan(new, actor(), references(), 3).is_err());
    // Only a receipt for the original intent can close the pending stage.
    journal
        .finish_stage(
            "operation-1",
            3,
            p::MaintenanceStage::Validated,
            receipt(),
            4,
        )
        .unwrap();
    assert_eq!(
        journal.operation("operation-1").unwrap().pending_stage,
        None
    );
}

#[test]
fn setup_cannot_start_core_and_failed_stage_cannot_be_overwritten_as_success() {
    let temp = Temp::new();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    assert!(journal
        .plan(command(1), p::MaintenanceActorClass::Setup, references(), 1)
        .is_err());
    assert_eq!(journal.revision().unwrap(), 1);
    journal.plan(command(1), actor(), references(), 1).unwrap();
    journal
        .require_recovery(
            "operation-1",
            2,
            p::canonical_digest(&"probe-required").unwrap(),
            2,
        )
        .unwrap();
    assert!(journal
        .begin_stage("operation-1", 3, p::MaintenanceStage::Validated, actor(), 3)
        .is_err());
    assert!(journal
        .finish_stage(
            "operation-1",
            3,
            p::MaintenanceStage::Committed,
            receipt(),
            3
        )
        .is_err());
}

#[test]
fn recovery_required_retains_the_pending_stage_but_rejects_an_unreviewed_receipt() {
    let temp = Temp::new();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    journal.plan(command(1), actor(), references(), 1).unwrap();
    journal
        .begin_stage("operation-1", 2, p::MaintenanceStage::Validated, actor(), 2)
        .unwrap();
    journal
        .require_recovery(
            "operation-1",
            3,
            p::canonical_digest(&"unresolved-native-outcome").unwrap(),
            3,
        )
        .unwrap();
    assert!(journal
        .finish_stage(
            "operation-1",
            4,
            p::MaintenanceStage::Validated,
            receipt(),
            4
        )
        .is_err());
    assert_eq!(
        journal.operation("operation-1").unwrap().pending_stage,
        Some(p::MaintenanceStage::Validated)
    );
    assert_eq!(journal.revision().unwrap(), 4);
}

#[test]
fn successful_stage_chain_is_rebuilt_exactly_without_new_facts() {
    let temp = Temp::new();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    journal.plan(command(1), actor(), references(), 1).unwrap();
    for next in [
        p::MaintenanceStage::Validated,
        p::MaintenanceStage::Checking,
        p::MaintenanceStage::Committed,
    ] {
        stage(&journal, next);
    }
    let expected = journal.operation("operation-1").unwrap();
    drop(journal);
    let bytes = fs::read(temp.journal()).unwrap();
    let journal = ProductJournal::open(&temp.journal(), "installation-1").unwrap();
    assert_eq!(journal.operation("operation-1").unwrap(), expected);
    drop(journal);
    assert_eq!(fs::read(temp.journal()).unwrap(), bytes);
}

#[test]
fn truncated_or_tampered_journal_fails_closed_and_is_never_repaired_silently() {
    for corruption in [b"{\"partial\":".as_slice(), b"{}\n".as_slice()] {
        let temp = Temp::new();
        drop(ProductJournal::open(&temp.journal(), "installation-1").unwrap());
        fs::OpenOptions::new()
            .append(true)
            .open(temp.journal())
            .unwrap()
            .write_all(corruption)
            .unwrap();
        let baseline = fs::read(temp.journal()).unwrap();
        assert!(ProductJournal::open(&temp.journal(), "installation-1").is_err());
        assert_eq!(fs::read(temp.journal()).unwrap(), baseline);
    }
}

fn release(root: &Path) -> p::ReleaseManifest {
    let mut files = Vec::new();
    for path in [
        "bin/forme-hostd.exe",
        "bin/forme-webd.exe",
        "bin/forme-gatewayd.exe",
        "bin/forme-cli.exe",
        "web/index.html",
    ] {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, format!("repository-owned release fixture: {path}")).unwrap();
        files.push(p::ReleaseFile {
            schema_version: p::SchemaVersion(1),
            relative_path: path.into(),
            digest: file_digest(&file).unwrap(),
            size: fs::metadata(file).unwrap().len(),
        });
    }
    let ui_files: Vec<_> = files
        .iter()
        .filter(|f| f.relative_path.starts_with("web/"))
        .cloned()
        .collect();
    let range = p::SchemaReadWriteRange {
        schema_version: p::SchemaVersion(1),
        read_min: 1,
        read_max: 1,
        write_version: 1,
    };
    p::ReleaseManifest {
        schema_version: p::SchemaVersion(1),
        release_id: "test-release-1".into(),
        target: p::ProductTarget::WindowsX86_64,
        ui_digest: p::canonical_digest(&ui_files).unwrap(),
        files,
        managed_dependencies: vec![],
        config_range: range.clone(),
        data_range: range,
        minimum_upgrader: 1,
        signature_ref: "test-distribution-key".into(),
    }
}

#[test]
fn pinned_signature_binds_every_binary_ui_target_and_closed_file_set() {
    let temp = Temp::new();
    let manifest = release(&temp.0);
    let signing = SigningKey::from_bytes(&[0x17; 32]);
    let ring = ReleaseKeyring::from_pinned_keys(BTreeMap::from([(
        "test-distribution-key".into(),
        signing.verifying_key().to_bytes(),
    )]))
    .unwrap();
    let signature = signing.sign(manifest.digest().unwrap().0.as_bytes());
    let mut reordered = manifest.clone();
    reordered.files.reverse();
    assert_eq!(reordered.digest().unwrap(), manifest.digest().unwrap());
    let verified = ring
        .verify(&manifest, &signature.to_bytes(), &temp.0, manifest.target)
        .unwrap();
    assert_eq!(verified.digest(), &manifest.digest().unwrap());
    assert!(ring
        .verify(&manifest, &[0; 64], &temp.0, manifest.target)
        .is_err());
    assert!(ring
        .verify(
            &manifest,
            &signature.to_bytes(),
            &temp.0,
            p::ProductTarget::MacosArm64
        )
        .is_err());
    let mut changed = manifest.clone();
    changed.minimum_upgrader = 2;
    assert!(ring
        .verify(&changed, &signature.to_bytes(), &temp.0, manifest.target)
        .is_err());
    fs::write(temp.0.join("web/index.html"), "tampered").unwrap();
    assert!(verified.recheck(&temp.0).is_err());
}

#[test]
fn unsigned_extra_files_and_schema_incompatible_rollback_are_rejected() {
    let temp = Temp::new();
    let manifest = release(&temp.0);
    let signing = SigningKey::from_bytes(&[0x18; 32]);
    let ring = ReleaseKeyring::from_pinned_keys(BTreeMap::from([(
        "test-distribution-key".into(),
        signing.verifying_key().to_bytes(),
    )]))
    .unwrap();
    let signature = signing.sign(manifest.digest().unwrap().0.as_bytes());
    fs::write(temp.0.join("bin/extra.exe"), "undeclared").unwrap();
    assert!(ring
        .verify(&manifest, &signature.to_bytes(), &temp.0, manifest.target)
        .is_err());
    let mut old = manifest.clone();
    old.data_range.read_max = 1;
    let mut current = manifest;
    current.data_range.write_version = 2;
    current.data_range.read_max = 2;
    assert!(!old.compatible_with(&current, 1));
}

#[test]
fn relative_host_paths_are_rejected_before_access() {
    assert!(reject_linked_ancestors(Path::new("relative/product.jsonl")).is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_host_paths_are_rejected_before_open() {
    let temp = Temp::new();
    let real = temp.0.join("real");
    fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, temp.0.join("alias")).unwrap();
    assert!(ProductJournal::open(&temp.0.join("alias/product.jsonl"), "installation-1").is_err());
    assert!(!real.join("product.jsonl").exists());
}
