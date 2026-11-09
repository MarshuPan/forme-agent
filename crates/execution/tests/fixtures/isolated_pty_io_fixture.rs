use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use forme_execution::{
    ActionBackend, ActionStatus, CancelToken, EventSink, InMemoryContentResolver,
    InMemorySecretResolver, OutputBudget, PtyBackend,
};
use forme_protocol as p;
use serde_json::json;

const CASES: &[&str] = &[
    "input-timeout",
    "input-cancel",
    "output-timeout",
    "output-strict",
    "output-rejected",
    "output-burst",
    "output-truncate",
    "exit-error",
];

fn selected_case() -> Result<String, String> {
    std::env::args()
        .nth(2)
        .filter(|case| CASES.contains(&case.as_str()))
        .ok_or_else(|| "fixed terminal I/O case required".into())
}

pub(super) fn runner() -> Result<(), String> {
    let case = selected_case()?;
    super::record(
        "runner-started.json",
        json!({"pid": std::process::id(), "boundary": super::boundary_observations()}),
    );
    let cwd = std::env::current_dir().map_err(|_| "fixture directory")?;
    let program = std::env::current_exe().map_err(|_| "fixture image")?;
    let secrets = Arc::new(InMemorySecretResolver::default());
    let input_path = std::env::var_os("FORME_WORKER_JOB_PATH").ok_or("probe input binding")?;
    secrets
        .insert(
            p::SecretRef("secret:probe-input-path".into()),
            input_path.to_string_lossy().as_ref(),
        )
        .map_err(|_| "probe input resolver")?;
    let backend = PtyBackend::new(
        vec![program.clone()],
        vec![cwd.clone()],
        secrets,
        Arc::new(InMemoryContentResolver::default()),
        if case == "output-strict" {
            OutputBudget::strict(4096)
        } else if case == "output-truncate" {
            OutputBudget::truncate_at(4096)
        } else {
            OutputBudget::truncate_at(1024 * 1024)
        },
        p::DurationMs(3500),
    )
    .map_err(|_| "terminal backend setup")?;
    let body = cwd.join("io-body-started.json");
    let reject_output = case == "output-rejected";
    let slow_output = case == "output-timeout";
    let sink = EventSink::with_observer(move |event| {
        if matches!(event, p::EventPayload::ActionOutputDelta(_)) {
            if reject_output && body.is_file() {
                return Err(p::Error("fixed terminal output append refusal".into()));
            }
            if slow_output {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Ok(())
    });
    let cancel = CancelToken::default();
    let finished = Arc::new(AtomicBool::new(false));
    let watcher = if case == "input-cancel" {
        let signal = cancel.clone();
        let done = finished.clone();
        let cancellation = cwd.join("cancel-io");
        Some(std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(6);
            while !done.load(Ordering::SeqCst) && Instant::now() < deadline {
                if cancellation.is_file() {
                    signal.cancel();
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }))
    } else {
        None
    };
    let intent = intent(&case, &program, &cwd);
    let plan = backend.plan(&intent).map_err(|_| "terminal plan")?;
    let started = Instant::now();
    let result = backend.execute(plan, &sink, cancel);
    finished.store(true, Ordering::SeqCst);
    if let Some(watcher) = watcher {
        watcher.join().map_err(|_| "terminal cancel observer")?;
    }
    let events = sink.events();
    let kinds: Vec<_> = events.iter().map(p::EventPayload::kind).collect();
    let failure_kind = match result.as_ref().err().map(|error| error.0.as_str()) {
        None => None,
        Some("PTY action exceeded its timeout") => Some("timeout"),
        Some("action output exceeded its strict byte budget") => Some("output-budget"),
        Some("fixed terminal output append refusal") => Some("append-rejected"),
        Some("PTY child exited with code 7") => Some("exit-7"),
        Some(_) => Some("unexpected"),
    };
    let expected = if case == "input-cancel" {
        p::EventKind::ActionCancelled
    } else if case == "output-burst" || case == "output-truncate" {
        p::EventKind::ActionCompleted
    } else {
        p::EventKind::ActionFailed
    };
    let status_ok = match &result {
        Ok(result) if case == "input-cancel" => result.status == ActionStatus::Cancelled,
        Ok(result) if case == "output-burst" => {
            result.status == ActionStatus::Completed
                && result.output.contains("FORME-ISOLATED-IO-END")
                && !result.truncated
        }
        Ok(result) if case == "output-truncate" => {
            result.status == ActionStatus::Completed
                && result.output.len() == 4096
                && result.truncated
                && events.iter().filter(|event| matches!(event, p::EventPayload::ActionOutputDelta(delta) if delta.truncated)).count() == 1
        }
        Err(_) => match case.as_str() {
            "input-timeout" | "output-timeout" => failure_kind == Some("timeout"),
            "output-strict" => failure_kind == Some("output-budget"),
            "output-rejected" => failure_kind == Some("append-rejected"),
            "exit-error" => failure_kind == Some("exit-7"),
            _ => false,
        },
        _ => false,
    };
    let passed = status_ok
        && kinds.iter().filter(|kind| **kind == expected).count() == 1
        && kinds
            .iter()
            .filter(|kind| **kind == p::EventKind::ActionStarted)
            .count()
            == 1
        && !kinds.contains(&p::EventKind::ActionOutcomeUnknown)
        && kinds.contains(&p::EventKind::CapabilityEvidenceRecorded);
    super::record(
        "io-result.json",
        json!({
            "schema_version": 1, "case": case, "passed": passed,
            "elapsed_ms": started.elapsed().as_millis(), "event_kinds": kinds,
            "result_is_error": result.is_err(),
            "failure_kind": failure_kind,
            "output_bytes": result.as_ref().ok().map(|result| result.output.len()),
            "truncated": result.as_ref().ok().map(|result| result.truncated),
            "tail_marker": result.as_ref().ok().map(|result| result.output.contains("FORME-ISOLATED-IO-END")),
        }),
    );
    if passed {
        Ok(())
    } else {
        Err("isolated terminal lifecycle assertion".into())
    }
}

pub(super) fn child() -> Result<(), String> {
    let case = selected_case()?;
    super::record(
        "child-ready.json",
        json!({
            "pid": std::process::id(), "stdin_terminal": std::io::stdin().is_terminal(),
            "stdout_terminal": std::io::stdout().is_terminal(),
            "boundary": super::boundary_observations(),
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(6);
    while !std::path::Path::new("release-io").is_file() {
        if Instant::now() >= deadline {
            return Err("independent terminal observation gate timed out".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    super::record("io-body-started.json", json!({"case": case}));
    if case.starts_with("input-") {
        std::thread::sleep(Duration::from_secs(10));
        return Ok(());
    }
    if case == "exit-error" {
        std::process::exit(7);
    }
    let burst = case == "output-burst" || case == "output-truncate";
    let mut stdout = std::io::stdout().lock();
    let mut bytes = 0;
    while Instant::now() < deadline {
        if stdout
            .write_all(&[b'x'; 4096])
            .and_then(|()| stdout.flush())
            .is_err()
        {
            return Ok(());
        }
        bytes += 4096;
        if burst && bytes >= 128 * 1024 {
            stdout
                .write_all(b"\r\nFORME-ISOLATED-IO-END\r\n")
                .map_err(|_| "terminal tail")?;
            stdout.flush().map_err(|_| "terminal tail flush")?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

fn intent(case: &str, program: &std::path::Path, cwd: &std::path::Path) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId(format!("action:isolated-pty:{case}")),
        source: p::Source::UserTurn,
        goal: p::GoalRef("verify isolated terminal I/O".into()),
        backend_hint: p::BackendKind::Pty,
        capability_ref: p::CapabilityRef("capability:pty".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:pty-mechanism-test".into()),
        risk_hint: p::Risk::Medium,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("not-retractable".into()),
        parameters: p::ActionParameters::Pty(p::PtyActionSpec {
            schema_version: p::SchemaVersion(1),
            program: program.to_string_lossy().into_owned(),
            args: vec!["--io-child".into(), case.into()],
            cwd: cwd.to_string_lossy().into_owned(),
            cols: 80,
            rows: 24,
            input: case
                .starts_with("input-")
                .then(|| p::ExternalInput::Literal("x".repeat(4 * 1024 * 1024))),
            environment: vec![p::SecretBinding {
                schema_version: p::SchemaVersion(1),
                name: "FORME_PTY_PROBE_INPUT".into(),
                value: p::SecretRef("secret:probe-input-path".into()),
            }],
        }),
        requested_permissions: vec![p::PermissionRef("execute:external".into())],
        requested_at: 1,
        estimated_output_bytes: 4096,
        estimated_duration: p::DurationMs(3500),
    }
}
