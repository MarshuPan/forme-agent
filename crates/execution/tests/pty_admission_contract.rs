use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(not(windows))]
use std::time::{Duration, Instant};

use forme_execution::{
    ActionBackend, ActionStatus, CancelToken, EventSink, InMemoryContentResolver,
    InMemorySecretResolver, OutputBudget, PtyBackend,
};
use forme_protocol as p;

#[test]
fn failed_started_append_prevents_pty_process_launch() {
    let (root, backend, intent) = fixture();
    let marker = root.path().join("effect.txt");
    let observed_marker = marker.clone();
    let launched = Arc::new(AtomicBool::new(false));
    let observed_launch = launched.clone();
    let sink = EventSink::with_observer(move |event| {
        if matches!(event, p::EventPayload::ActionStarted(_)) {
            observed_launch.store(fixture_has_started(&observed_marker), Ordering::SeqCst);
            return Err(p::Error("test journal rejected the start event".into()));
        }
        Ok(())
    });

    let result = backend.execute(
        backend.plan(&intent).unwrap(),
        &sink,
        CancelToken::default(),
    );
    assert!(result.is_err());
    assert!(
        !launched.load(Ordering::SeqCst),
        "PTY process existed before start acceptance"
    );
    assert!(
        !marker.exists(),
        "the PTY program ran before its start event could be accepted"
    );
    assert_no_execution_events(&sink, false);
    assert_eq!(count(&sink, p::EventKind::ActionFailed), 1);
    assert_eq!(count(&sink, p::EventKind::CapabilityEvidenceRecorded), 1);
}

#[test]
fn cancellation_before_execute_starts_no_pty_program() {
    let (root, backend, intent) = fixture();
    let cancel = CancelToken::default();
    cancel.cancel();
    let sink = EventSink::default();
    let result = backend
        .execute(backend.plan(&intent).unwrap(), &sink, cancel)
        .unwrap();

    assert_eq!(result.status, ActionStatus::Cancelled);
    assert!(!root.path().join("effect.txt").exists());
    assert_no_execution_events(&sink, false);
    assert_eq!(count(&sink, p::EventKind::ActionCancelled), 1);
}

#[test]
fn cancellation_accepted_during_start_append_prevents_pty_launch() {
    let (root, backend, intent) = fixture();
    let marker = root.path().join("effect.txt");
    let observed_marker = marker.clone();
    let launched = Arc::new(AtomicBool::new(false));
    let observed_launch = launched.clone();
    let cancel = CancelToken::default();
    let observer_cancel = cancel.clone();
    let sink = EventSink::with_observer(move |event| {
        if matches!(event, p::EventPayload::ActionStarted(_)) {
            observer_cancel.cancel();
            observed_launch.store(fixture_has_started(&observed_marker), Ordering::SeqCst);
        }
        Ok(())
    });
    let result = backend
        .execute(backend.plan(&intent).unwrap(), &sink, cancel)
        .unwrap();

    assert_eq!(result.status, ActionStatus::Cancelled);
    assert!(
        !launched.load(Ordering::SeqCst),
        "PTY process existed before cancellation was accepted"
    );
    assert!(
        !marker.exists(),
        "the PTY program ran despite cancellation at the start boundary"
    );
    assert_no_execution_events(&sink, true);
    assert_eq!(count(&sink, p::EventKind::ActionCancelled), 1);
}

#[test]
fn accepted_start_executes_the_original_program_once() {
    let (root, backend, intent) = fixture();
    let sink = EventSink::default();
    let result = backend
        .execute(
            backend.plan(&intent).unwrap(),
            &sink,
            CancelToken::default(),
        )
        .unwrap();

    assert_eq!(result.status, ActionStatus::Completed);
    assert_eq!(
        std::fs::read(root.path().join("effect.txt")).unwrap(),
        b"original PTY fixture executed"
    );
    assert!(result.output.contains("admission-fixture-input"));
    let kinds: Vec<_> = sink.events().iter().map(p::EventPayload::kind).collect();
    assert_eq!(count(&sink, p::EventKind::ActionStarted), 1);
    assert_eq!(count(&sink, p::EventKind::ActionCompleted), 1);
    let started = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::ActionStarted)
        .unwrap();
    let completed = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::ActionCompleted)
        .unwrap();
    assert!(started < completed);
}

fn count(sink: &EventSink, kind: p::EventKind) -> usize {
    sink.events()
        .iter()
        .filter(|event| event.kind() == kind)
        .count()
}

fn assert_no_execution_events(sink: &EventSink, started: bool) {
    assert_eq!(
        count(sink, p::EventKind::ActionStarted),
        usize::from(started)
    );
    for kind in [
        p::EventKind::ActionOutputDelta,
        p::EventKind::ActionCompleted,
    ] {
        assert_eq!(count(sink, kind), 0);
    }
}

#[cfg(not(windows))]
fn fixture_has_started(marker: &std::path::Path) -> bool {
    let started = Instant::now();
    while !marker.exists() && started.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(5));
    }
    marker.exists()
}

#[cfg(windows)]
fn fixture_has_started(marker: &std::path::Path) -> bool {
    // ConPTY may pause the child on a terminal query before Rust main runs.
    // Inspect the exact temporary marker in its command line as well as effects.
    let powershell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = std::process::Command::new(powershell)
        .args([
            "-NoProfile", "-NonInteractive", "-Command",
            "$ErrorActionPreference='Stop'; Get-CimInstance Win32_Process -Filter \"Name='forme-pty-fixture.exe'\" | Where-Object { $_.CommandLine -and $_.CommandLine.Contains($env:FORME_PTY_ADMISSION_MARKER) } | Select-Object -ExpandProperty ProcessId",
        ])
        .env("FORME_PTY_ADMISSION_MARKER", marker)
        .output()
        .unwrap();
    assert!(output.status.success(), "fixture process inspection failed");
    marker.exists()
        || String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .any(|line| line.trim().parse::<u32>().is_ok())
}

fn fixture() -> (tempfile::TempDir, PtyBackend, p::ActionIntent) {
    let root = tempfile::tempdir().unwrap();
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_forme-pty-fixture"));
    let secrets = Arc::new(InMemorySecretResolver::default());
    secrets
        .insert(p::SecretRef("secret:admission-mode".into()), "1")
        .unwrap();
    let backend = PtyBackend::new(
        vec![executable.clone()],
        vec![root.path().to_path_buf()],
        secrets,
        Arc::new(InMemoryContentResolver::default()),
        OutputBudget::truncate_at(4_096),
        p::DurationMs(5_000),
    )
    .unwrap();
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:pty-admission".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("verify PTY admission before effects".into()),
        backend_hint: p::BackendKind::Pty,
        capability_ref: p::CapabilityRef("capability:pty".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:pty-admission".into()),
        risk_hint: p::Risk::Medium,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("not-retractable".into()),
        parameters: p::ActionParameters::Pty(p::PtyActionSpec {
            schema_version: p::SchemaVersion(1),
            program: executable.to_string_lossy().into_owned(),
            args: vec![
                "--effect-marker".into(),
                root.path()
                    .join("effect.txt")
                    .to_string_lossy()
                    .into_owned(),
            ],
            cwd: root.path().to_string_lossy().into_owned(),
            cols: 80,
            rows: 24,
            input: Some(p::ExternalInput::Literal(
                "admission-fixture-input\r\n".into(),
            )),
            environment: vec![p::SecretBinding {
                schema_version: p::SchemaVersion(1),
                name: "FORME_PTY_FIXTURE_CHILD".into(),
                value: p::SecretRef("secret:admission-mode".into()),
            }],
        }),
        requested_permissions: vec![p::PermissionRef("execute:external".into())],
        requested_at: 1,
        estimated_output_bytes: 4_096,
        estimated_duration: p::DurationMs(1_000),
    };
    (root, backend, intent)
}
