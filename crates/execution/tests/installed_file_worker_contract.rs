#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_protocol as p;

struct Fixture {
    _directory: tempfile::TempDir,
    original: PathBuf,
    run: PathBuf,
    request: p::WorkerDispatchRequest,
    target: p::WorkerFileTarget,
}

impl Fixture {
    fn new(operation: p::FileOperation, content: Option<Vec<u8>>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        let run = directory.path().join("job");
        fs::create_dir(&original).unwrap();
        fs::create_dir(&run).unwrap();
        fs::create_dir(run.join("workspace")).unwrap();
        fs::create_dir(run.join("results")).unwrap();
        fs::write(original.join("sample.txt"), b"original\n").unwrap();
        fs::write(run.join("workspace/sample.txt"), b"original\n").unwrap();
        let original = fs::canonicalize(original).unwrap();
        let request = request(&original, operation, content);
        let target = p::WorkerFileTarget {
            schema_version: p::SchemaVersion(1),
            job_id: request.job.job_id.clone(),
            plan_digest: request.job.plan_digest.clone(),
            normalized_action_digest: request
                .execution
                .as_ref()
                .unwrap()
                .normalized_action_digest
                .clone(),
            relative_path: "sample.txt".into(),
        };
        Self {
            _directory: directory,
            original,
            run,
            request,
            target,
        }
    }

    fn write_inputs(&self) {
        for (name, value) in [
            ("job.json", serde_json::to_value(&self.request.job).unwrap()),
            (
                "permit.json",
                serde_json::to_value(&self.request.permit).unwrap(),
            ),
            (
                "execution.json",
                serde_json::to_value(&self.request.execution).unwrap(),
            ),
            (
                "file-target.json",
                serde_json::to_value(&self.target).unwrap(),
            ),
        ] {
            fs::write(self.run.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
        }
    }

    fn execute(&self) -> Output {
        self.execute_image(env!("CARGO_BIN_EXE_forme-file-worker"))
    }

    fn execute_image(&self, program: &str) -> Output {
        self.write_inputs();
        Command::new(program)
            .arg("--installed-file-worker")
            .env_clear()
            .env("FORME_WORKER_PROTOCOL_VERSION", "1")
            .env("FORME_WORKER_JOB_PATH", self.run.join("job.json"))
            .env("FORME_WORKER_PERMIT_PATH", self.run.join("permit.json"))
            .env(
                "FORME_WORKER_EXECUTION_PATH",
                self.run.join("execution.json"),
            )
            .env(
                "FORME_WORKER_FILE_TARGET_PATH",
                self.run.join("file-target.json"),
            )
            .env("FORME_WORKER_WORKSPACE_PATH", self.run.join("workspace"))
            .env(
                "FORME_WORKER_RESULT_PATH",
                self.run.join("results/result.bin"),
            )
            .env(
                "FORME_WORKER_WORKSPACE_SCOPE",
                &self.request.job.workspace_scope.0,
            )
            .env(
                "FORME_WORKER_DEADLINE",
                self.request.job.deadline.to_string(),
            )
            .output()
            .unwrap()
    }

    fn assert_rejected(&self) {
        let output = self.execute();
        assert!(!output.status.success());
        assert!(!self.run.join("results/result.bin").exists());
        assert_eq!(
            fs::read(self.original.join("sample.txt")).unwrap(),
            b"original\n"
        );
        assert_eq!(
            fs::read(self.run.join("workspace/sample.txt")).unwrap(),
            b"original\n"
        );
    }
}

fn now() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn request(
    root: &Path,
    operation: p::FileOperation,
    content: Option<Vec<u8>>,
) -> p::WorkerDispatchRequest {
    let issued = now();
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("file-worker-action".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("file-worker-goal".into()),
        backend_hint: p::BackendKind::File,
        capability_ref: p::CapabilityRef("file-worker".into()),
        action_type: if operation == p::FileOperation::Read {
            p::ActionType::Observe
        } else {
            p::ActionType::Write
        },
        scope: p::Scope("workspace:file-worker".into()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::File {
            operation,
            path: root
                .join("sample.txt")
                .to_str()
                .unwrap()
                .trim_start_matches(r"\\?\")
                .replace('\\', "/"),
            content,
        },
        requested_permissions: vec![],
        requested_at: issued,
        estimated_output_bytes: 4096,
        estimated_duration: p::DurationMs(30_000),
    };
    let digest = p::PlanDigest(
        p::canonical_digest(&("file-worker-test-plan", &intent))
            .unwrap()
            .0,
    );
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: "file-worker-job".into(),
        owner_ref: p::VerifiedPrincipal("owner:file-worker".into()),
        action_ref: intent.intent_id.clone(),
        affected_resources: p::affected_resources_for(&intent),
        execution_plan_ref: "file-worker-plan".into(),
        plan_digest: digest,
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: "file-worker-permit".into(),
        sandbox_profile_ref: "sandbox:file-worker".into(),
        workspace_scope: intent.scope.clone(),
        deadline: issued + 60_000,
        nonce: p::Nonce("file-worker-nonce".into()),
        cancel_ref: "file-worker-cancel".into(),
    };
    let permit = p::WorkerPermit {
        schema_version: p::SchemaVersion(1),
        permit_ref: job.permit_ref.clone(),
        job_id: job.job_id.clone(),
        owner_ref: job.owner_ref.clone(),
        action_ref: job.action_ref.clone(),
        affected_resources: job.affected_resources.clone(),
        plan_digest: job.plan_digest.clone(),
        authority_epoch: job.authority_epoch,
        workspace_scope: job.workspace_scope.clone(),
        sandbox_profile_ref: job.sandbox_profile_ref.clone(),
        deadline: job.deadline,
        nonce: job.nonce.clone(),
        issued_at: issued,
    };
    let execution = p::WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: job.plan_digest.clone(),
        backend: p::BackendKind::File,
        scope: intent.scope.clone(),
        affected_resources: job.affected_resources.clone(),
        normalized_action_digest: p::normalized_action_digest(&intent).unwrap(),
        intent,
        timeout: p::DurationMs(30_000),
        output_budget_bytes: 4096,
        desktop_session: None,
        input_grants: Vec::new(),
    };
    p::WorkerDispatchRequest {
        schema_version: p::SchemaVersion(1),
        job,
        permit,
        execution: Some(execution),
    }
}

#[test]
fn legacy_executor_file_mode_remains_compatible_and_new_entry_rejects_other_modes() {
    let fixture = Fixture::new(p::FileOperation::Read, None);
    assert!(fixture
        .execute_image(env!("CARGO_BIN_EXE_forme-executord"))
        .status
        .success());
    assert_eq!(
        fs::read(fixture.run.join("results/result.bin")).unwrap(),
        b"original\n"
    );
    for arguments in [
        vec![],
        vec!["--desktop-session-helper"],
        vec!["--installed-file-worker", "extra"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_forme-file-worker"))
            .args(arguments)
            .env_clear()
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn installed_file_entry_reads_writes_and_deletes_only_the_private_copy() {
    for (operation, content, expected) in [
        (p::FileOperation::Read, None, b"original\n".as_slice()),
        (
            p::FileOperation::Write,
            Some(b"updated\n".to_vec()),
            b"file write complete\n".as_slice(),
        ),
        (
            p::FileOperation::Delete,
            None,
            b"file delete complete\n".as_slice(),
        ),
    ] {
        let fixture = Fixture::new(operation, content);
        let output = fixture.execute();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert_eq!(
            fs::read(fixture.run.join("results/result.bin")).unwrap(),
            expected
        );
        assert_eq!(
            fs::read(fixture.original.join("sample.txt")).unwrap(),
            b"original\n"
        );
        let copied = fixture.run.join("workspace/sample.txt");
        match operation {
            p::FileOperation::Write => assert_eq!(fs::read(copied).unwrap(), b"updated\n"),
            p::FileOperation::Delete => assert!(!copied.exists()),
            p::FileOperation::Read => assert_eq!(fs::read(copied).unwrap(), b"original\n"),
        }
    }
}

#[test]
fn installed_file_entry_rejects_mapping_permit_and_scope_substitutions_before_writes() {
    for case in [
        "job",
        "plan",
        "action",
        "schema",
        "permit",
        "scope",
        "expired",
        "backend",
        "traversal",
        "absolute",
        "device",
        "ads",
    ] {
        let mut fixture = Fixture::new(p::FileOperation::Write, Some(b"forbidden".to_vec()));
        match case {
            "job" => fixture.target.job_id = "foreign-job".into(),
            "plan" => {
                fixture.target.plan_digest.0 = p::canonical_digest(&"foreign-plan").unwrap().0
            }
            "action" => {
                fixture.target.normalized_action_digest =
                    p::canonical_digest(&"foreign-action").unwrap()
            }
            "schema" => fixture.target.schema_version = p::SchemaVersion(99),
            "permit" => fixture.request.permit.nonce = p::Nonce("foreign-nonce".into()),
            "scope" => fixture.request.job.workspace_scope = p::Scope("workspace:foreign".into()),
            "expired" => {
                fixture.request.job.deadline = now() - 1;
                fixture.request.permit.deadline = fixture.request.job.deadline;
            }
            "backend" => {
                fixture.request.execution.as_mut().unwrap().backend = p::BackendKind::Shell
            }
            "traversal" => fixture.target.relative_path = "../original/sample.txt".into(),
            "absolute" => fixture.target.relative_path = "/sample.txt".into(),
            "device" => fixture.target.relative_path = "NUL".into(),
            "ads" => fixture.target.relative_path = "sample.txt:stream".into(),
            _ => unreachable!(),
        }
        fixture.assert_rejected();
    }
}

#[test]
fn installed_file_entry_checks_budget_encoding_and_link_custody_before_mutations() {
    let mut too_small = Fixture::new(p::FileOperation::Write, Some(b"forbidden".to_vec()));
    too_small
        .request
        .execution
        .as_mut()
        .unwrap()
        .output_budget_bytes = 1;
    too_small.assert_rejected();
    for bytes in [vec![b'x'; 4097], vec![0xff]] {
        let fixture = Fixture::new(p::FileOperation::Read, None);
        fs::write(fixture.run.join("workspace/sample.txt"), bytes).unwrap();
        assert!(!fixture.execute().status.success());
        assert!(!fixture.run.join("results/result.bin").exists());
    }
    for operation in [
        p::FileOperation::Read,
        p::FileOperation::Write,
        p::FileOperation::Delete,
    ] {
        let fixture = Fixture::new(
            operation,
            (operation == p::FileOperation::Write).then(|| b"forbidden".to_vec()),
        );
        fs::remove_file(fixture.run.join("workspace/sample.txt")).unwrap();
        fs::hard_link(
            fixture.original.join("sample.txt"),
            fixture.run.join("workspace/sample.txt"),
        )
        .unwrap();
        fixture.assert_rejected();
    }
}

#[test]
fn installed_file_entry_result_creation_failure_never_modifies_the_original() {
    let fixture = Fixture::new(p::FileOperation::Write, Some(b"private update\n".to_vec()));
    // The entry may validate a missing result parent, but publication must
    // fail after the private write. It must not claim success or touch origin.
    fs::remove_dir(fixture.run.join("results")).unwrap();
    let output = fixture.execute();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!fixture.run.join("results/result.bin").exists());
    assert_eq!(
        fs::read(fixture.run.join("workspace/sample.txt")).unwrap(),
        b"private update\n"
    );
    assert_eq!(
        fs::read(fixture.original.join("sample.txt")).unwrap(),
        b"original\n"
    );
}

#[cfg(windows)]
#[test]
fn installed_file_entry_rejects_a_junction_inside_the_job_root() {
    let mut fixture = Fixture::new(p::FileOperation::Write, Some(b"forbidden\n".to_vec()));
    fixture.target.relative_path = "linked/sample.txt".into();
    let junction = fixture.run.join("workspace").join("linked");
    let output = Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(
            fixture
                .original
                .to_string_lossy()
                .trim_start_matches(r"\\?\"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.assert_rejected();
    fs::remove_dir(junction).unwrap();
}

#[cfg(unix)]
#[test]
fn installed_file_entry_rejects_target_and_parent_symlinks() {
    for parent in [false, true] {
        let mut fixture = Fixture::new(p::FileOperation::Write, Some(b"forbidden".to_vec()));
        if parent {
            fixture.target.relative_path = "linked/sample.txt".into();
            std::os::unix::fs::symlink(&fixture.original, fixture.run.join("workspace/linked"))
                .unwrap();
        } else {
            fs::remove_file(fixture.run.join("workspace/sample.txt")).unwrap();
            std::os::unix::fs::symlink(
                fixture.original.join("sample.txt"),
                fixture.run.join("workspace/sample.txt"),
            )
            .unwrap();
        }
        fixture.assert_rejected();
    }
}

#[cfg(windows)]
#[test]
#[ignore = "requires Windows symbolic-link creation permission; no elevation performed"]
fn native_file_entry_rejects_a_symbolic_link_target() {
    let fixture = Fixture::new(p::FileOperation::Write, Some(b"forbidden".to_vec()));
    fs::remove_file(fixture.run.join("workspace/sample.txt")).unwrap();
    std::os::windows::fs::symlink_file(
        fixture.original.join("sample.txt"),
        fixture.run.join("workspace/sample.txt"),
    )
    .unwrap();
    fixture.assert_rejected();
}

#[cfg(windows)]
#[test]
#[ignore = "runs the real file entry in native AppContainers; no installed services"]
fn native_file_entry_cancel_failure_and_post_execution_replacement_never_write_back() {
    use forme_execution::installed::{
        WindowsAppContainerWorkerIsolation, WorkerIsolation, WorkerLaunchRegistry, WorkerLaunchSpec,
    };
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};
    for case in ["cancel", "replacement", "failed"] {
        let temporary = tempfile::tempdir().unwrap();
        let original = temporary.path().join("original");
        fs::create_dir(&original).unwrap();
        let before = if case == "failed" {
            vec![0xff]
        } else {
            b"original\n".to_vec()
        };
        fs::write(original.join("sample.txt"), &before).unwrap();
        let original = fs::canonicalize(original).unwrap();
        let runtime = temporary.path().join("runtime");
        let registry = WorkerLaunchRegistry::from_specs([WorkerLaunchSpec::new(
            "sandbox:file-worker",
            PathBuf::from(env!("CARGO_BIN_EXE_forme-file-worker")),
            vec!["--installed-file-worker".into()],
        )
        .unwrap()])
        .unwrap();
        let roots = BTreeMap::from([("workspace:file-worker".into(), original.clone())]);
        let isolation = WindowsAppContainerWorkerIsolation::open(
            runtime.clone(),
            registry.clone(),
            roots.clone(),
        )
        .unwrap();
        let operation = if case == "failed" {
            p::FileOperation::Read
        } else {
            p::FileOperation::Write
        };
        let request = request(
            &original,
            operation,
            (operation == p::FileOperation::Write).then(|| b"updated\n".to_vec()),
        );
        isolation
            .admit_with_execution(
                &request.job,
                &request.permit,
                request.execution.as_ref(),
                now(),
            )
            .unwrap();
        if case == "cancel" {
            // If the short-lived worker has already exited, the adapter must
            // retain Unknown rather than inventing an in-flight cancellation.
            let receipt = isolation.cancel_at(&request.job, now()).unwrap();
            assert!(matches!(
                receipt.outcome,
                p::WorkerOutcome::Cancelled | p::WorkerOutcome::Unknown
            ));
        } else {
            let deadline = Instant::now() + Duration::from_secs(10);
            if case == "replacement" {
                let root = loop {
                    let found = fs::read_dir(&runtime)
                        .unwrap()
                        .flatten()
                        .find(|entry| entry.path().join("results/result.bin").is_file());
                    if let Some(entry) = found {
                        break entry.path();
                    }
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(5));
                };
                let outside = temporary.path().join("outside.txt");
                fs::write(&outside, b"outside custody\n").unwrap();
                fs::remove_file(root.join("workspace/sample.txt")).unwrap();
                fs::hard_link(&outside, root.join("workspace/sample.txt")).unwrap();
            }
            loop {
                if let Some(receipt) = isolation.observe(&request.job, now()).unwrap() {
                    assert_eq!(receipt.observed_outcome, p::WorkerOutcome::Failed);
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        assert_eq!(fs::read(original.join("sample.txt")).unwrap(), before);
        // Standalone adapter evidence: explicitly acknowledge inspected source
        // custody. AEAD sealing is covered by the separate real Host test.
        isolation.result_sealed(&request.job, now()).unwrap();
        drop(isolation);
        let reopened =
            WindowsAppContainerWorkerIsolation::open(runtime.clone(), registry, roots).unwrap();
        assert_eq!(fs::read(original.join("sample.txt")).unwrap(), before);
        assert_eq!(fs::read_dir(&runtime).unwrap().flatten().count(), 1);
        drop(reopened);
    }
}

#[cfg(windows)]
#[test]
#[ignore = "executes the actual installed file binary in a native Low AppContainer; no services are installed"]
fn native_installed_file_entry_uses_exact_mapping_host_writeback_and_encrypted_results() {
    use forme_execution::installed::{
        HostCredentials, IsolatedExecutorHost, ProductHostBroker, SystemServiceManager,
        WindowsAppContainerWorkerIsolation, WorkerExecutorHost, WorkerLaunchRegistry,
        WorkerLaunchSpec,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::{Duration, Instant};
    for operation in [
        p::FileOperation::Read,
        p::FileOperation::Write,
        p::FileOperation::Delete,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        fs::create_dir(&original).unwrap();
        fs::write(original.join("sample.txt"), b"original\n").unwrap();
        let original = fs::canonicalize(original).unwrap();
        let runtime = directory.path().join("runtime");
        let mut request = request(
            &original,
            operation,
            (operation == p::FileOperation::Write).then(|| b"updated\n".to_vec()),
        );
        request.job.sandbox_profile_ref = "sandbox:file-worker".into();
        let registry = WorkerLaunchRegistry::from_specs([WorkerLaunchSpec::new(
            "sandbox:file-worker",
            PathBuf::from(env!("CARGO_BIN_EXE_forme-file-worker")),
            vec!["--installed-file-worker".into()],
        )
        .unwrap()])
        .unwrap();
        let isolation = WindowsAppContainerWorkerIsolation::open(
            runtime.clone(),
            registry,
            BTreeMap::from([("workspace:file-worker".into(), original.clone())]),
        )
        .unwrap();
        let token = "file-worker-native-agent-abcdefghijklmnopqrstuvwxyz";
        let broker = ProductHostBroker::open_with_service_manager(
            &directory.path().join("host.journal"),
            "file-worker-install",
            HostCredentials::new(
                "file-worker-maintenance-abcdefghijklmnopqrstuvwxyz".into(),
                Some(token.into()),
            )
            .unwrap(),
            p::ProductTarget::WindowsX86_64,
            "0.1.0".into(),
            SystemServiceManager,
        )
        .unwrap();
        let auth = broker.authenticate_agent(token).unwrap();
        let host = WorkerExecutorHost::open_with_isolation(
            &directory.path().join("worker.journal"),
            p::canonical_digest(&token).unwrap(),
            p::AuthorityEpoch(1),
            BTreeSet::from([p::Scope("workspace:file-worker".into())]),
            isolation,
        )
        .unwrap();
        host.dispatch_with_execution(
            &auth,
            request.job.clone(),
            request.permit.clone(),
            request.execution.clone(),
            now(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let finished_output = fs::read_dir(&runtime)
                .unwrap()
                .flatten()
                .any(|entry| entry.path().join("results/result.bin").is_file());
            if finished_output {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "real file worker did not produce its result"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            fs::read(original.join("sample.txt")).unwrap(),
            b"original\n",
            "only Host may write back"
        );
        loop {
            let observation = host.probe(&auth, &request.job.job_id, now()).unwrap();
            if observation.state == p::WorkerState::Completed {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Host did not settle the original file job: {observation:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let manifest = host
            .result_manifest(&auth, &request.job.job_id, now())
            .unwrap();
        let chunk = host
            .read_result(
                &auth,
                p::WorkerResultRead {
                    schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
                    binding: manifest.binding.clone(),
                    manifest_digest: manifest.manifest_digest.clone(),
                    offset: 0,
                    max_bytes: 4096,
                },
                now(),
            )
            .unwrap();
        assert_eq!(manifest.outcome, Some(p::WorkerOutcome::Completed));
        assert!(chunk.finished);
        match operation {
            p::FileOperation::Read => assert_eq!(chunk.bytes, b"original\n"),
            p::FileOperation::Write => {
                assert_eq!(chunk.bytes, b"file write complete\n");
                assert_eq!(fs::read(original.join("sample.txt")).unwrap(), b"updated\n");
            }
            p::FileOperation::Delete => {
                assert_eq!(chunk.bytes, b"file delete complete\n");
                assert!(!original.join("sample.txt").exists());
            }
        }
        assert_eq!(fs::read_dir(&runtime).unwrap().flatten().count(), 1);
    }
}
