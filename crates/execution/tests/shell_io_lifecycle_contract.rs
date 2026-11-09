use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_execution::{
    ActionBackend, ActionStatus, CancelToken, EventSink, OutputBudget, ShellBackend, ShellSandbox,
};
use forme_protocol as p;

const ROOT_ENV: &str = "FORME_SHELL_IO_FIXTURE_ROOT";
const MODE_ENV: &str = "FORME_SHELL_IO_FIXTURE_MODE";
const FIXTURE: &str = "shell_io_fixture_child";

fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn shell_timeout_with_inherited_pipes_is_bounded_and_unknown() {
    let _serial = serial();
    inherited_pipes("timeout");
}

#[test]
fn shell_cancel_with_inherited_pipes_is_bounded_and_unknown() {
    let _serial = serial();
    inherited_pipes("cancel");
}

#[test]
fn shell_strict_budget_with_inherited_pipes_is_bounded_and_unknown() {
    let _serial = serial();
    inherited_pipes("budget");
}

#[test]
fn shell_parent_exit_does_not_confirm_inherited_pipe_cleanup() {
    let _serial = serial();
    inherited_pipes("parent-exit");
}

fn inherited_pipes(mode: &str) {
    let root = TestRoot::new();
    let backend = Arc::new(make_backend(&root.path, mode, 500, mode != "budget"));
    let plan = backend.plan(&intent(&root.path, "original")).unwrap();
    let sink = Arc::new(EventSink::default());
    let token = CancelToken::default();
    let worker_token = token.clone();
    let worker_sink = sink.clone();
    let worker_backend = backend.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    let started = Instant::now();
    let worker = std::thread::spawn(move || {
        sender
            .send(worker_backend.execute(plan, &worker_sink, worker_token))
            .unwrap();
    });
    wait_bytes(
        &root.path.join("holder-ready"),
        b"ready",
        Duration::from_secs(2),
    );
    if mode == "cancel" {
        token.cancel();
    }
    let result = receiver.recv_timeout(Duration::from_secs(3));
    let within_bound = result.is_ok();
    let elapsed = started.elapsed();
    if !within_bound {
        // Every descendant is finite. Release before reporting an assertion so
        // an old unbounded implementation cannot strand this test executable.
        fs::write(root.path.join("release"), b"release").unwrap();
    }
    let result = result.unwrap_or_else(|_| {
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("finite fixture did not release the original execution")
    });
    worker.join().unwrap();
    if !within_bound {
        wait_bytes(
            &root.path.join("holder-done"),
            b"done",
            Duration::from_secs(2),
        );
    }
    assert!(within_bound, "shell exceeded cleanup bound: {elapsed:?}");
    let result = result.expect("unconfirmed cleanup must have a structured Unknown result");
    assert_eq!(result.status, ActionStatus::Unknown);
    let original_events = sink.events();
    assert_eq!(terminal_counts(&original_events), (1, 0));
    assert_eq!(fs::read(root.path.join("effects")).unwrap(), b"original\n");

    // A separate backend instance must not bypass custody of the original I/O.
    let replacement = make_backend(&root.path, "success", 1_000, true);
    let blocked_sink = EventSink::default();
    assert!(replacement
        .execute(
            replacement.plan(&intent(&root.path, "blocked")).unwrap(),
            &blocked_sink,
            CancelToken::default(),
        )
        .is_err());
    assert!(blocked_sink.events().is_empty());
    assert_eq!(fs::read(root.path.join("effects")).unwrap(), b"original\n");

    fs::write(root.path.join("release"), b"release").unwrap();
    wait_bytes(
        &root.path.join("holder-done"),
        b"done",
        Duration::from_secs(2),
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let next_sink = EventSink::default();
        let next = replacement.execute(
            replacement.plan(&intent(&root.path, "new-action")).unwrap(),
            &next_sink,
            CancelToken::default(),
        );
        match next {
            Ok(next) => {
                assert_eq!(next.status, ActionStatus::Completed);
                assert_eq!(terminal_counts(&next_sink.events()), (0, 1));
                break;
            }
            Err(_) => {
                assert!(next_sink.events().is_empty());
                assert!(
                    Instant::now() < deadline,
                    "original cleanup never released admission"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    assert_eq!(
        fs::read(root.path.join("effects")).unwrap(),
        b"original\nnew-action\n"
    );
    assert_eq!(
        sink.events(),
        original_events,
        "late cleanup rewrote original Unknown"
    );
}

#[test]
fn shell_confirmed_exit_and_running_cancel_keep_existing_outcomes() {
    let _serial = serial();
    let root = TestRoot::new();
    for (mode, expected) in [
        ("success", Some(ActionStatus::Completed)),
        ("failure", None),
        ("budget-clean", None),
    ] {
        let backend = make_backend(&root.path, mode, 1_000, mode != "budget-clean");
        let sink = EventSink::default();
        let result = backend.execute(
            backend.plan(&intent(&root.path, mode)).unwrap(),
            &sink,
            CancelToken::default(),
        );
        match expected {
            Some(status) => assert_eq!(result.unwrap().status, status),
            None => assert!(result.is_err()),
        }
        assert_eq!(terminal_counts(&sink.events()), (0, 1));
    }

    let backend = Arc::new(make_backend(&root.path, "sleep", 2_000, true));
    let sink = Arc::new(EventSink::default());
    let worker_backend = backend.clone();
    let worker_sink = sink.clone();
    let plan = backend.plan(&intent(&root.path, "cancel-running")).unwrap();
    let worker = std::thread::spawn(move || {
        worker_backend.execute(plan, &worker_sink, CancelToken::default())
    });
    wait_bytes(&root.path.join("running"), b"ready", Duration::from_secs(2));
    backend
        .cancel(p::ActionId("cancel-running".into()))
        .unwrap();
    assert_eq!(
        worker.join().unwrap().unwrap().status,
        ActionStatus::Cancelled
    );
    assert_eq!(terminal_counts(&sink.events()), (0, 1));
}

#[test]
fn shell_process_capacity_is_enforced_before_dispatch_and_released_after_join() {
    let _serial = serial();
    let roots: Vec<_> = (0..9).map(|_| TestRoot::new()).collect();
    let mut workers = Vec::new();
    let mut tokens = Vec::new();
    for (index, root) in roots.iter().take(8).enumerate() {
        let backend = make_backend(&root.path, "capacity", 30_000, true);
        let plan = backend
            .plan(&intent(&root.path, &format!("slot-{index}")))
            .unwrap();
        let token = CancelToken::default();
        tokens.push(token.clone());
        workers.push(std::thread::spawn(move || {
            backend.execute(plan, &EventSink::default(), token)
        }));
        wait_bytes(&root.path.join("running"), b"ready", Duration::from_secs(2));
    }
    let backend = make_backend(&roots[8].path, "success", 1_000, true);
    let sink = EventSink::default();
    let blocked = backend.execute(
        backend.plan(&intent(&roots[8].path, "ninth")).unwrap(),
        &sink,
        CancelToken::default(),
    );
    for token in tokens {
        token.cancel();
    }
    for worker in workers {
        assert_eq!(
            worker.join().unwrap().unwrap().status,
            ActionStatus::Cancelled
        );
    }
    assert!(blocked.is_err());
    assert!(sink.events().is_empty());
    assert!(!roots[8].path.join("effects").exists());
    let result = backend
        .execute(
            backend.plan(&intent(&roots[8].path, "after-join")).unwrap(),
            &EventSink::default(),
            CancelToken::default(),
        )
        .unwrap();
    assert_eq!(result.status, ActionStatus::Completed);
    assert_eq!(
        fs::read(roots[8].path.join("effects")).unwrap(),
        b"after-join\n"
    );
}

#[test]
fn shell_cancel_before_dispatch_has_no_process_effect() {
    let _serial = serial();
    let root = TestRoot::new();
    let backend = make_backend(&root.path, "success", 1_000, true);
    let token = CancelToken::default();
    token.cancel();
    let sink = EventSink::default();
    let result = backend
        .execute(
            backend.plan(&intent(&root.path, "pre-cancel")).unwrap(),
            &sink,
            token,
        )
        .unwrap();
    assert_eq!(result.status, ActionStatus::Cancelled);
    assert!(!root.path.join("effects").exists());
    assert!(!sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionStarted(_))));
}

fn terminal_counts(events: &[p::EventPayload]) -> (usize, usize) {
    let unknown = events
        .iter()
        .filter(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_)))
        .count();
    let ordinary = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                p::EventPayload::ActionCompleted(_)
                    | p::EventPayload::ActionFailed(_)
                    | p::EventPayload::ActionCancelled(_)
            )
        })
        .count();
    (unknown, ordinary)
}

fn make_backend(root: &Path, mode: &str, timeout: u64, truncate: bool) -> ShellBackend {
    let executable = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    ShellBackend::new(
        ShellSandbox {
            schema_version: p::SchemaVersion(1),
            available: true,
            allowed_programs: vec![executable],
            allowed_roots: vec![root.to_string_lossy().into_owned()],
            environment: vec![
                (ROOT_ENV.into(), root.to_string_lossy().into_owned()),
                (MODE_ENV.into(), mode.into()),
            ],
            network_allowed: false,
        },
        OutputBudget {
            schema_version: p::SchemaVersion(1),
            max_bytes: 512,
            truncate,
        },
        p::DurationMs(timeout),
    )
    .unwrap()
}

fn intent(root: &Path, action: &str) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId(action.into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("bounded shell I/O".into()),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("tool:shell-io-fixture".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:shell-io-fixture".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            args: vec![
                "--exact".into(),
                FIXTURE.into(),
                "--nocapture".into(),
                "--test-threads=1".into(),
                action.into(),
            ],
            cwd: Some(root.to_string_lossy().into_owned()),
            network: false,
        },
        requested_permissions: vec![p::PermissionRef("execute".into())],
        requested_at: 1,
        estimated_output_bytes: 128,
        estimated_duration: p::DurationMs(100),
    }
}

#[test]
fn shell_io_fixture_child() {
    let Ok(root) = std::env::var(ROOT_ENV) else {
        return;
    };
    let root = PathBuf::from(root);
    let mode = std::env::var(MODE_ENV).unwrap();
    if mode == "holder" {
        fs::write(root.join("holder-ready"), b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(7);
        while !root.join("release").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        fs::write(root.join("holder-done"), b"done").unwrap();
        return;
    }
    let action = std::env::args().next_back().unwrap();
    let mut effects = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("effects"))
        .unwrap();
    writeln!(effects, "{action}").unwrap();
    effects.sync_all().unwrap();
    match mode.as_str() {
        "success" => return,
        "failure" => std::process::exit(7),
        "budget-clean" => {
            println!("{}", "x".repeat(4_096));
            return;
        }
        "sleep" | "capacity" => {
            fs::write(root.join("running"), b"ready").unwrap();
            std::thread::sleep(Duration::from_secs(if mode == "capacity" { 30 } else { 3 }));
            return;
        }
        _ => {}
    }
    let mut holder = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", FIXTURE, "--nocapture", "--test-threads=1"])
        .env_clear()
        .env(ROOT_ENV, &root)
        .env(MODE_ENV, "holder")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    wait_bytes(&root.join("holder-ready"), b"ready", Duration::from_secs(2));
    if mode == "budget" {
        println!("{}", "x".repeat(4_096));
        std::io::stdout().flush().unwrap();
    }
    if mode == "parent-exit" {
        // This fixture deliberately exits before its finite descendant so
        // the backend must observe the inherited pipe, not just this parent.
        std::process::exit(0);
    }
    holder.wait().unwrap();
}

fn wait_bytes(path: &Path, expected: &[u8], timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while fs::read(path).ok().as_deref() != Some(expected) {
        assert!(
            Instant::now() < deadline,
            "fixture signal did not arrive: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("forme-shell-io-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self { path }
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::write(self.path.join("release"), b"release");
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
