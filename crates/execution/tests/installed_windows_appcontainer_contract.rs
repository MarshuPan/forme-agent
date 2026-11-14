#![cfg(windows)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_execution::installed::{
    WindowsAppContainerWorkerIsolation, WorkerIsolation, WorkerLaunchRegistry, WorkerLaunchSpec,
};
use forme_protocol as p;

fn request(root: &Path, now: p::Timestamp) -> p::WorkerDispatchRequest {
    let path = root
        .to_str()
        .unwrap()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("appcontainer-action".into()),
        source: p::Source::Internal,
        goal: p::GoalRef("appcontainer-goal".into()),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("shell:appcontainer-fixture".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:appcontainer".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: "registered-fixture".into(),
            args: vec![],
            cwd: Some(path),
            network: false,
        },
        requested_permissions: vec![],
        requested_at: now,
        estimated_output_bytes: 4096,
        estimated_duration: p::DurationMs(30_000),
    };
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: "native-overlay-job".into(),
        owner_ref: p::VerifiedPrincipal("owner:appcontainer".into()),
        action_ref: intent.intent_id.clone(),
        affected_resources: p::affected_resources_for(&intent),
        execution_plan_ref: "native-overlay-plan".into(),
        plan_digest: p::PlanDigest(p::canonical_digest(&intent).unwrap().0),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: "native-overlay-permit".into(),
        sandbox_profile_ref: "sandbox:appcontainer".into(),
        workspace_scope: intent.scope.clone(),
        deadline: now + 60_000,
        nonce: p::Nonce("native-overlay-nonce".into()),
        cancel_ref: "native-overlay-cancel".into(),
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
        issued_at: now,
    };
    let execution = p::WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: job.plan_digest.clone(),
        intent: intent.clone(),
        backend: intent.backend_hint,
        scope: intent.scope.clone(),
        affected_resources: job.affected_resources.clone(),
        normalized_action_digest: p::normalized_action_digest(&intent).unwrap(),
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

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
#[ignore = "launches a native Low AppContainer worker and requires an external TCP positive control"]
fn native_low_worker_uses_a_private_copy_and_host_verifies_writeback() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("original");
    let sibling = temporary.path().join("sibling");
    let runtime = temporary.path().join("runtime");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&sibling).unwrap();
    fs::write(root.join("existing"), b"before").unwrap();
    fs::write(sibling.join("denied"), b"private sibling").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    // Positive control: this is a live, reachable listener before sandboxing.
    TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let external_tcp: std::net::SocketAddr = "1.1.1.1:443".parse().unwrap();
    TcpStream::connect_timeout(&external_tcp, Duration::from_secs(3))
        .expect("external TCP control must be reachable outside the sandbox");
    let profile = "sandbox:appcontainer";
    let program = Path::new(env!("CARGO_BIN_EXE_forme-worker-fixture")).to_path_buf();
    assert!(std::process::Command::new(&program)
        .arg("--noop")
        .status()
        .unwrap()
        .success());
    let registry = WorkerLaunchRegistry::from_specs([WorkerLaunchSpec::new(
        profile,
        program,
        vec!["--isolation-probe".into()],
    )
    .unwrap()])
    .unwrap();
    let maps = BTreeMap::from([("workspace:appcontainer".into(), root.clone())]);
    let isolation =
        WindowsAppContainerWorkerIsolation::open(runtime.clone(), registry.clone(), maps.clone())
            .unwrap();
    let request = request(&root, now_ms());
    fs::write(
        root.join("isolation-probe-input.json"),
        serde_json::to_vec(&serde_json::json!({
            "deny_paths": [root.join("existing"), sibling.join("denied")], "loopback_port": port,
            "external_tcp": external_tcp.to_string(),
        }))
        .unwrap(),
    )
    .unwrap();
    let result = (|| -> p::Result<()> {
        isolation.admit_with_execution(
            &request.job,
            &request.permit,
            request.execution.as_ref(),
            now_ms(),
        )?;
        assert_eq!(fs::read(root.join("existing")).unwrap(), b"before");
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut diagnostic = None;
        let receipt = loop {
            for directory in fs::read_dir(&runtime).unwrap().flatten() {
                let copy = directory.path().join("workspace");
                if let Ok(bytes) = fs::read(copy.join("isolation-probe-result.json")) {
                    diagnostic = Some(String::from_utf8_lossy(&bytes).to_string());
                } else if let Ok(bytes) = fs::read(copy.join("probe-stage")) {
                    diagnostic = Some(String::from_utf8_lossy(&bytes).to_string());
                }
            }
            if let Some(receipt) = isolation.observe(&request.job, now_ms())? {
                break receipt;
            }
            if Instant::now() >= deadline {
                return Err(p::Error("native worker timed out".into()));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        if receipt.observed_outcome != p::WorkerOutcome::Completed {
            return Err(p::Error(format!(
                "native worker did not commit: {:?}; {:?}; {:?}",
                receipt.observed_outcome, receipt.ground_truth_refs, diagnostic
            )));
        }
        assert_eq!(fs::read(root.join("existing")).unwrap(), b"sandbox result");
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("isolation-probe-result.json")).unwrap())
                .unwrap();
        for field in [
            "denied_paths",
            "inputs_readonly",
            "host_records_denied",
            "child_denied",
            "writable_program_denied",
            "network_denied",
            "external_network_denied",
            "environment_closed",
            "copy_read_write",
        ] {
            assert_eq!(report[field], true, "native probe {field}");
        }
        assert_eq!(
            fs::read(sibling.join("denied")).unwrap(),
            b"private sibling"
        );
        let source = isolation.result_path(&request.job)?.unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"worker fixture result\n");
        // Standalone adapter test explicitly acknowledges its inspected bytes.
        isolation.result_sealed(&request.job, now_ms())?;
        assert!(!source.exists());
        Ok(())
    })();
    drop(isolation);
    let recovery = WindowsAppContainerWorkerIsolation::open(runtime, registry, maps);
    if recovery.is_err() {
        let retained = temporary.keep();
        panic!(
            "native test recovery failed; retained bounded fixture at {}: {:?}; run: {:?}",
            retained.display(),
            recovery.err(),
            result
        );
    }
    drop(recovery);
    result.unwrap();
}

/// Native execution is opt-in; each case retains its bounded directory if
/// original-lease recovery cannot be verified, rather than deleting evidence.
fn negative_native_case(mode: &str) {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("original");
    let runtime = temporary.path().join("runtime");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("existing"), b"before").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let arguments = match mode {
        "failed" => vec!["--fail".into()],
        "cancel" | "drop" => vec!["--sleep-ms=10000".into()],
        "conflict" => vec![],
        _ => panic!("unknown native test case"),
    };
    let registry = WorkerLaunchRegistry::from_specs([WorkerLaunchSpec::new(
        "sandbox:appcontainer",
        Path::new(env!("CARGO_BIN_EXE_forme-worker-fixture")).to_path_buf(),
        arguments,
    )
    .unwrap()])
    .unwrap();
    let maps = BTreeMap::from([("workspace:appcontainer".into(), root.clone())]);
    let isolation =
        WindowsAppContainerWorkerIsolation::open(runtime.clone(), registry.clone(), maps.clone())
            .unwrap();
    let request = request(&root, now_ms());
    let result = (|| -> p::Result<()> {
        isolation.admit_with_execution(
            &request.job,
            &request.permit,
            request.execution.as_ref(),
            now_ms(),
        )?;
        let deadline = Instant::now() + Duration::from_secs(5);
        // Prove the real worker ran and wrote the private copy before testing
        // cancellation/recovery; a suspended or failed loader is insufficient.
        loop {
            let ran = fs::read_dir(&runtime).unwrap().flatten().any(|entry| {
                entry.path().join("workspace/worker-fixture-ran").is_file()
                    && fs::read(entry.path().join("results/result.bin"))
                        .is_ok_and(|bytes| bytes == b"worker fixture result\n")
            });
            if ran {
                break;
            }
            if Instant::now() >= deadline {
                return Err(p::Error(
                    "native negative fixture never entered main".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!root.join("worker-fixture-ran").exists());
        if mode == "conflict" {
            fs::write(root.join("existing"), b"owner update").unwrap();
        }
        if mode == "cancel" {
            assert_eq!(
                isolation.cancel_at(&request.job, now_ms())?.outcome,
                p::WorkerOutcome::Cancelled
            );
        } else if mode != "drop" {
            loop {
                if let Some(receipt) = isolation.observe(&request.job, now_ms())? {
                    assert_eq!(receipt.observed_outcome, p::WorkerOutcome::Failed);
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(p::Error("native negative fixture did not terminate".into()));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        if mode != "drop" {
            let source = isolation.result_path(&request.job)?.unwrap();
            assert_eq!(fs::read(source).unwrap(), b"worker fixture result\n");
            isolation.result_sealed(&request.job, now_ms())?;
        }
        Ok(())
    })();
    drop(isolation);
    let recovery = WindowsAppContainerWorkerIsolation::open(runtime.clone(), registry, maps);
    if mode == "drop" {
        result.unwrap();
        let recovered = recovery.expect("query/maintenance composition remains available");
        assert!(recovered
            .admit_with_execution(
                &request.job,
                &request.permit,
                request.execution.as_ref(),
                now_ms()
            )
            .is_err());
        drop(recovered);
        assert_retained_result(&runtime);
        assert_eq!(fs::read(root.join("existing")).unwrap(), b"before");
        assert!(!root.join("worker-fixture-ran").exists());
        return;
    }
    if recovery.is_err() {
        let retained = temporary.keep();
        panic!(
            "native {mode} recovery failed; retained {}: {:?}; run: {:?}",
            retained.display(),
            recovery.err(),
            result
        );
    }
    drop(recovery);
    result.unwrap();
    assert_eq!(
        fs::read(root.join("existing")).unwrap(),
        if mode == "conflict" {
            b"owner update".as_slice()
        } else {
            b"before".as_slice()
        }
    );
    assert!(!root.join("worker-fixture-ran").exists());
    let journal = forme_execution::installed::SandboxLeaseJournal::open(
        &runtime.join("sandbox-leases.jsonl"),
    )
    .unwrap();
    journal.verify_startup_recovery_complete().unwrap();
    let entries = fs::read_dir(runtime).unwrap().flatten().count();
    assert_eq!(entries, 1, "only the durable lease journal remains");
}

#[test]
#[ignore = "creates real AppContainer profiles and executes native failure/cancellation/conflict paths"]
fn native_failure_cancel_and_owner_conflict_never_write_back() {
    for mode in ["failed", "cancel", "conflict", "drop"] {
        negative_native_case(mode);
    }
}

#[test]
#[ignore = "kills a real adapter process with a running native AppContainer worker"]
fn native_host_death_recovers_original_lease_without_replaying_worker() {
    const ROOT: &str = "FORME_NATIVE_CRASH_FIXTURE";
    let make_registry = || {
        WorkerLaunchRegistry::from_specs([WorkerLaunchSpec::new(
            "sandbox:appcontainer",
            Path::new(env!("CARGO_BIN_EXE_forme-worker-fixture")).to_path_buf(),
            vec!["--sleep-ms=10000".into()],
        )
        .unwrap()])
        .unwrap()
    };
    if let Some(directory) = std::env::var_os(ROOT) {
        let directory = std::path::PathBuf::from(directory);
        let root = fs::canonicalize(directory.join("original")).unwrap();
        let maps = BTreeMap::from([("workspace:appcontainer".into(), root.clone())]);
        let isolation = WindowsAppContainerWorkerIsolation::open(
            directory.join("runtime"),
            make_registry(),
            maps,
        )
        .unwrap();
        let request = request(&root, now_ms());
        isolation
            .admit_with_execution(
                &request.job,
                &request.permit,
                request.execution.as_ref(),
                now_ms(),
            )
            .unwrap();
        fs::write(directory.join("adapter-ready"), b"ready").unwrap();
        // Parent kills this process. Destructors must not supply the recovery
        // evidence, and no second child/worker may be launched on reopening.
        std::thread::sleep(Duration::from_secs(30));
        panic!("native crash fixture was not killed by its parent");
    }
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("original");
    let runtime = temporary.path().join("runtime");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("existing"), b"before").unwrap();
    let root = fs::canonicalize(root).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "native_host_death_recovers_original_lease_without_replaying_worker",
        ])
        .env(ROOT, temporary.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let ready = loop {
        let entered = fs::read_dir(&runtime).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry.path().join("workspace/worker-fixture-ran").is_file()
                    && fs::read(entry.path().join("results/result.bin"))
                        .is_ok_and(|bytes| bytes == b"worker fixture result\n")
            })
        });
        if temporary.path().join("adapter-ready").exists() && entered {
            break true;
        }
        if child.try_wait().unwrap().is_some() || Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = child.kill();
    child.wait().unwrap();
    let maps = BTreeMap::from([("workspace:appcontainer".into(), root.clone())]);
    let recovery = WindowsAppContainerWorkerIsolation::open(runtime.clone(), make_registry(), maps);
    let recovered = recovery.expect("query/maintenance composition remains available");
    let request = request(&root, now_ms());
    assert!(recovered
        .admit_with_execution(
            &request.job,
            &request.permit,
            request.execution.as_ref(),
            now_ms()
        )
        .is_err());
    drop(recovered);
    assert!(
        ready,
        "real worker did not enter main before the host was killed"
    );
    assert_eq!(fs::read(root.join("existing")).unwrap(), b"before");
    assert!(!root.join("worker-fixture-ran").exists());
    let journal = forme_execution::installed::SandboxLeaseJournal::open(
        &runtime.join("sandbox-leases.jsonl"),
    )
    .unwrap();
    assert!(journal.verify_startup_recovery_complete().is_err());
    for lease in journal.pending_cleanup().unwrap() {
        assert!(lease.process_tree_termination.is_some());
        assert!(lease.appcontainer_removed);
        assert!(!lease.job_directory_removed);
    }
    assert_retained_result(&runtime);
}

fn assert_retained_result(runtime: &Path) {
    let retained = fs::read_dir(runtime)
        .unwrap()
        .flatten()
        .find(|entry| entry.path().is_dir())
        .expect("original result source remains")
        .path();
    assert!(retained.join("result-seal-pending.json").is_file());
    assert!(!retained.join("result-sealed.json").exists());
    assert_eq!(
        fs::read(retained.join("results/result.bin")).unwrap(),
        b"worker fixture result\n"
    );
}

#[test]
#[ignore = "runs native AppContainer jobs through the Host encrypted result and cleanup lifecycle"]
fn native_host_seals_results_before_cleanup_and_retains_unsealed_failures() {
    use forme_execution::installed::{
        HostCredentials, IsolatedExecutorHost, ProductHostBroker, SystemServiceManager,
        WorkerExecutorHost,
    };
    for budget in [4096, 1] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("original");
        let runtime = temporary.path().join("runtime");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("existing"), b"before").unwrap();
        let root = fs::canonicalize(root).unwrap();
        let registry = WorkerLaunchRegistry::from_specs([WorkerLaunchSpec::new(
            "sandbox:appcontainer",
            Path::new(env!("CARGO_BIN_EXE_forme-worker-fixture")).to_path_buf(),
            vec![],
        )
        .unwrap()])
        .unwrap();
        let maps = BTreeMap::from([("workspace:appcontainer".into(), root.clone())]);
        let isolation = WindowsAppContainerWorkerIsolation::open(
            runtime.clone(),
            registry.clone(),
            maps.clone(),
        )
        .unwrap();
        let token = "native-result-agent-abcdefghijklmnopqrstuvwxyz";
        let broker = ProductHostBroker::open_with_service_manager(
            &temporary.path().join("host.jsonl"),
            "native-result-install",
            HostCredentials::new(
                "native-result-maintenance-abcdefghijklmnopqrstuvwxyz".into(),
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
            &temporary.path().join("worker.jsonl"),
            p::canonical_digest(&token).unwrap(),
            p::AuthorityEpoch(1),
            [p::Scope("workspace:appcontainer".into())].into(),
            isolation,
        )
        .unwrap();
        let mut request = request(&root, now_ms());
        request.execution.as_mut().unwrap().output_budget_bytes = budget;
        host.dispatch_with_execution(
            &auth,
            request.job.clone(),
            request.permit.clone(),
            request.execution.clone(),
            now_ms(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match host.probe(&auth, &request.job.job_id, now_ms()) {
                Ok(observation) if observation.state == p::WorkerState::Completed => {
                    assert_eq!(budget, 4096);
                    break;
                }
                Err(_) => {
                    assert_eq!(budget, 1);
                    assert_eq!(
                        host.probe(&auth, &request.job.job_id, now_ms())
                            .unwrap()
                            .state,
                        p::WorkerState::Unknown
                    );
                    assert_retained_result(&runtime);
                    break;
                }
                _ => assert!(Instant::now() < deadline, "native Host did not finish"),
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if budget == 4096 {
            let manifest = host
                .result_manifest(&auth, &request.job.job_id, now_ms())
                .unwrap();
            assert_eq!(manifest.status, p::WorkerResultStatus::Available);
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
                    now_ms(),
                )
                .unwrap();
            assert_eq!(chunk.bytes, b"worker fixture result\n");
            assert_eq!(fs::read_dir(&runtime).unwrap().flatten().count(), 1);
            assert!(!root.join("result.bin").exists());
        }
        drop(host);
        let reopened = WindowsAppContainerWorkerIsolation::open(runtime.clone(), registry, maps);
        let reopened =
            reopened.expect("worker recovery must not prevent query/maintenance composition");
        if budget == 1 {
            assert!(reopened
                .admit_with_execution(
                    &request.job,
                    &request.permit,
                    request.execution.as_ref(),
                    now_ms()
                )
                .is_err());
        }
        let reopened_host = WorkerExecutorHost::open_with_isolation(
            &temporary.path().join("worker.jsonl"),
            p::canonical_digest(&token).unwrap(),
            p::AuthorityEpoch(1),
            [p::Scope("workspace:appcontainer".into())].into(),
            reopened,
        )
        .unwrap();
        assert_eq!(
            reopened_host
                .probe(&auth, &request.job.job_id, now_ms())
                .unwrap()
                .state,
            if budget == 4096 {
                p::WorkerState::Completed
            } else {
                p::WorkerState::Unknown
            }
        );
        drop(reopened_host);
        if budget == 1 {
            assert_retained_result(&runtime);
        }
    }
}
