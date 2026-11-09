#![cfg(windows)]
#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use forme_execution::{
    ActionBackend, ActionStatus, CancelToken, EventSink, InMemoryContentResolver,
    InMemorySecretResolver, OutputBudget, PtyBackend,
};
use forme_protocol as p;
use serde_json::json;
use win_custody::{Job, JobLimits, Launch, Process, ProcessAccess, WaitOutcome};

#[test]
fn blocked_terminal_input_obeys_the_original_timeout() {
    run_isolated_case("input-timeout");
}

#[test]
fn blocked_terminal_input_can_be_cancelled() {
    run_isolated_case("input-cancel");
}

#[test]
fn continuously_ready_output_cannot_starve_the_deadline() {
    run_isolated_case("output-timeout");
}

#[test]
fn strict_output_failure_drains_and_reaps_the_original_terminal() {
    run_isolated_case("output-strict");
}

#[test]
fn rejected_output_append_drains_and_reaps_the_original_terminal() {
    run_isolated_case("output-rejected");
}

#[test]
fn normal_terminal_exit_preserves_the_entire_bounded_output_tail() {
    run_isolated_case("output-burst");
}

#[test]
fn truncated_output_has_one_marker_and_preserves_a_trusted_exit() {
    run_isolated_case("output-truncate");
}

#[test]
fn nonzero_terminal_exit_has_failure_evidence_without_completion() {
    run_isolated_case("exit-error");
}

#[test]
#[ignore = "invoked only by the bounded, job-owned outer fault tests"]
fn isolated_terminal_io_probe() {
    let case = std::env::var("FORME_PTY_IO_CASE").expect("explicit isolated case");
    let root = PathBuf::from(std::env::var_os("FORME_PTY_IO_ROOT").unwrap());
    let program = PathBuf::from(env!("CARGO_BIN_EXE_forme-pty-fixture"));
    let secrets = Arc::new(InMemorySecretResolver::default());
    secrets
        .insert(p::SecretRef("secret:io-mode".into()), "1")
        .unwrap();
    let backend = PtyBackend::new(
        vec![program.clone()],
        vec![root.clone()],
        secrets,
        Arc::new(InMemoryContentResolver::default()),
        if case == "output-strict" {
            OutputBudget::strict(4096)
        } else if case == "output-truncate" {
            OutputBudget::truncate_at(4096)
        } else {
            OutputBudget::truncate_at(128 * 1024 * 1024)
        },
        p::DurationMs(if case == "input-cancel" { 5000 } else { 750 }),
    )
    .unwrap();
    let log = Arc::new(Mutex::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("events.jsonl"))
            .unwrap(),
    ));
    let observed_log = log.clone();
    let slow_output = case == "output-timeout";
    let reject_output = case == "output-rejected";
    let sink = EventSink::with_observer(move |event| {
        if matches!(event, p::EventPayload::ActionOutputDelta(_)) {
            if reject_output {
                return Err(p::Error("self-created output journal rejection".into()));
            }
            if slow_output {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let mut log = observed_log.lock().unwrap();
        writeln!(log, "{}", serde_json::to_string(&event.kind()).unwrap()).unwrap();
        log.sync_all().unwrap();
        Ok(())
    });
    let intent = intent(&case, &root, &program);
    let cancel = CancelToken::default();
    let canceller = if case == "input-cancel" {
        let token = cancel.clone();
        let ready = root.join("fixture.pid");
        Some(std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !ready.is_file() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(
                ready.is_file(),
                "cancellation must observe the original program first"
            );
            token.cancel();
        }))
    } else {
        None
    };
    let started = Instant::now();
    let result = backend.execute(backend.plan(&intent).unwrap(), &sink, cancel);
    if let Some(canceller) = canceller {
        canceller.join().unwrap();
    }
    let events = sink.events();
    let kinds: Vec<_> = events.iter().map(p::EventPayload::kind).collect();
    fs::write(
        root.join("returned.json"),
        serde_json::to_vec(&json!({
            "schema_version": 1, "elapsed_ms": started.elapsed().as_millis(),
            "result_is_error": result.is_err(), "event_kinds": kinds,
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "terminal I/O escaped its runtime bound"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == p::EventKind::ActionStarted)
            .count(),
        1
    );
    assert!(!kinds.contains(&p::EventKind::ActionOutcomeUnknown));
    assert!(kinds.contains(&p::EventKind::CapabilityEvidenceRecorded));
    if case == "output-burst" || case == "output-truncate" {
        let result = result.unwrap();
        assert_eq!(result.status, ActionStatus::Completed);
        assert!(kinds.contains(&p::EventKind::ActionCompleted));
        if case == "output-burst" {
            assert!(
                result.output.contains("FORME-PTY-IO-END"),
                "final terminal marker was lost"
            );
            assert!(result.output.len() > 4096);
            assert!(!result.truncated);
        } else {
            assert_eq!(result.output.len(), 4096);
            assert!(result.truncated);
            assert_eq!(events.iter().filter(|event| matches!(event, p::EventPayload::ActionOutputDelta(delta) if delta.truncated)).count(), 1);
        }
    } else if case == "input-cancel" {
        assert!(!kinds.contains(&p::EventKind::ActionCompleted));
        assert_eq!(result.unwrap().status, ActionStatus::Cancelled);
        assert!(kinds.contains(&p::EventKind::ActionCancelled));
    } else {
        assert!(!kinds.contains(&p::EventKind::ActionCompleted));
        assert!(result.is_err());
        assert!(kinds.contains(&p::EventKind::ActionFailed));
    }
    if let Ok(pid) = fs::read_to_string(root.join("fixture.pid")) {
        if let Ok(process) = Process::open(
            pid.trim().parse().unwrap(),
            ProcessAccess::QueryLimitedAndSynchronize,
        ) {
            assert_eq!(
                process.wait(Some(Duration::from_secs(1))).unwrap(),
                WaitOutcome::Signaled,
                "the original terminal program outlived execute"
            );
        }
    }
}

fn run_isolated_case(case: &str) {
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/closure-inventory-evidence");
    fs::create_dir_all(&evidence).unwrap();
    let root = tempfile::Builder::new()
        .prefix(&format!("pty-io-{case}-"))
        .tempdir_in(evidence)
        .unwrap()
        .keep();
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("probe.log"))
        .unwrap();
    let null = fs::File::open("NUL").unwrap();
    let job = Job::create(&JobLimits::new().kill_on_close(true)).unwrap();
    let mut launch = Launch::new(std::env::current_exe().unwrap());
    launch
        .args([
            "--ignored",
            "--exact",
            "isolated_terminal_io_probe",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("FORME_PTY_IO_CASE", case)
        .env("FORME_PTY_IO_ROOT", &root)
        .stdin(&null)
        .stdout(&log)
        .stderr(&log)
        .create_no_window(true);
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        launch.env("SystemRoot", system_root);
    }
    let process = launch.spawn_in_job(&job).unwrap();
    let observed = process.wait(Some(Duration::from_secs(8))).unwrap();
    if observed != WaitOutcome::Signaled {
        job.terminate(99).unwrap();
        assert_eq!(
            process.wait(Some(Duration::from_secs(3))).unwrap(),
            WaitOutcome::Signaled
        );
    }
    fs::write(root.join("outer-receipt.json"), serde_json::to_vec(&json!({
        "schema_version": 1, "case": case, "completed_in_bound": observed == WaitOutcome::Signaled,
        "exit_code": process.exit_code().unwrap(), "pid": process.pid().unwrap(),
        "forced_original_job_cleanup": observed != WaitOutcome::Signaled,
    })).unwrap()).unwrap();
    println!("terminal I/O evidence: {}", root.display());
    assert_eq!(
        observed,
        WaitOutcome::Signaled,
        "PTY {case} blocked beyond its deadline; original Job terminated, evidence={}",
        root.display()
    );
    assert_eq!(
        process.exit_code().unwrap(),
        Some(0),
        "PTY {case} failed; evidence={}; {}",
        root.display(),
        fs::read_to_string(root.join("probe.log")).unwrap()
    );
}

fn intent(case: &str, root: &Path, program: &Path) -> p::ActionIntent {
    let unread = case.starts_with("input-");
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId(format!("action:pty-io:{case}")),
        source: p::Source::UserTurn,
        goal: p::GoalRef("verify bounded terminal lifecycle".into()),
        backend_hint: p::BackendKind::Pty,
        capability_ref: p::CapabilityRef("capability:pty".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:pty-io".into()),
        risk_hint: p::Risk::Medium,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("not-retractable".into()),
        parameters: p::ActionParameters::Pty(p::PtyActionSpec {
            schema_version: p::SchemaVersion(1),
            program: program.to_string_lossy().into_owned(),
            args: vec![
                if unread {
                    "--io-unread"
                } else if case == "output-burst" || case == "output-truncate" {
                    "--io-burst"
                } else if case == "exit-error" {
                    "--io-error"
                } else {
                    "--io-flood"
                }
                .into(),
                root.to_string_lossy().into_owned(),
            ],
            cwd: root.to_string_lossy().into_owned(),
            cols: 80,
            rows: 24,
            input: unread.then(|| p::ExternalInput::Literal("x".repeat(4 * 1024 * 1024))),
            environment: vec![p::SecretBinding {
                schema_version: p::SchemaVersion(1),
                name: "FORME_PTY_FIXTURE_CHILD".into(),
                value: p::SecretRef("secret:io-mode".into()),
            }],
        }),
        requested_permissions: vec![p::PermissionRef("execute:external".into())],
        requested_at: 1,
        estimated_output_bytes: 4096,
        estimated_duration: p::DurationMs(750),
    }
}
