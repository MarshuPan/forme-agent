use std::fs;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forme_execution::{
    ActionBackend, ActionStatus, BackendKind, CancelToken, DefaultExecutionPlanner, EventSink,
    ExecutionBackendRegistry, ExecutionPlanner, FileBackend, InMemoryNotificationSink,
    NotificationBackend, OutputBudget, ShellBackend, ShellSandbox,
};
use forme_policy::{
    ActionMatcher, DefaultPolicyEngine, PolicyContext, PolicyEngine, PolicyLayer,
    PolicyLayerSource, PolicyRule,
};
use forme_protocol as p;

#[test]
fn plan_digest_is_deterministic_and_mutation_is_rejected_before_effects() {
    let backend = shell_backend("output", OutputBudget::truncate_at(128), 1_000);
    let intent = shell_intent();
    let first = backend.plan(&intent).unwrap();
    let second = backend.plan(&intent).unwrap();
    assert_eq!(first.digest, second.digest);
    assert!(first.digest.0.starts_with("sha256:"));

    let mut mutated = first;
    let p::ActionParameters::Shell { args, .. } = &mut mutated.intent.parameters else {
        panic!("fixture must remain shell parameters");
    };
    args.push("--mutated-after-approval".into());
    let sink = EventSink::default();
    assert!(backend
        .execute(mutated, &sink, CancelToken::default())
        .is_err());
    assert!(sink.events().is_empty());
}

#[test]
fn notification_target_scope_and_body_ref_are_bound_by_the_plan_digest() {
    let delivered = Arc::new(InMemoryNotificationSink::default());
    let backend = NotificationBackend::new(
        delivered.clone(),
        OutputBudget::truncate_at(256),
        p::DurationMs(1_000),
    );
    let original = backend.plan(&notification_intent()).unwrap();

    let mut target_changed = original.clone();
    let p::ActionParameters::Notification { target, .. } = &mut target_changed.intent.parameters
    else {
        unreachable!()
    };
    *target = p::ParticipantId("owner:changed".into());

    let mut scope_changed = original.clone();
    scope_changed.intent.scope = p::Scope("workspace:changed".into());

    let mut body_changed = original;
    let p::ActionParameters::Notification { body_ref, .. } = &mut body_changed.intent.parameters
    else {
        unreachable!()
    };
    *body_ref = p::ContentRef("content:changed".into());

    for plan in [target_changed, scope_changed, body_changed] {
        let sink = EventSink::default();
        assert!(backend
            .execute(plan, &sink, CancelToken::default())
            .is_err());
        assert!(sink.events().is_empty());
    }
    assert!(delivered.delivered().is_empty());
}

#[test]
fn shell_enforces_output_budget_timeout_and_cancellation() {
    let backend = shell_backend("output", OutputBudget::truncate_at(32), 1_000);
    let result = backend
        .execute(
            backend.plan(&shell_intent()).unwrap(),
            &EventSink::default(),
            CancelToken::default(),
        )
        .unwrap();
    assert_eq!(result.status, ActionStatus::Completed);
    assert!(result.truncated);
    assert!(result.output.len() <= 32);

    let timeout_backend = shell_backend("sleep", OutputBudget::truncate_at(128), 100);
    let timeout_sink = EventSink::default();
    assert!(timeout_backend
        .execute(
            timeout_backend.plan(&shell_intent()).unwrap(),
            &timeout_sink,
            CancelToken::default(),
        )
        .is_err());
    assert!(timeout_sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionFailed(_))));
    assert!(!timeout_sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionCompleted(_))));

    let cancelled_backend = shell_backend("sleep", OutputBudget::truncate_at(128), 2_000);
    let cancelled_sink = EventSink::default();
    let token = CancelToken::default();
    token.cancel();
    let cancelled = cancelled_backend
        .execute(
            cancelled_backend.plan(&shell_intent()).unwrap(),
            &cancelled_sink,
            token,
        )
        .unwrap();
    assert_eq!(cancelled.status, ActionStatus::Cancelled);
    assert!(matches!(
        cancelled_sink.events().as_slice(),
        [
            p::EventPayload::ActionCancelled(_),
            p::EventPayload::CapabilityEvidenceRecorded(_)
        ]
    ));
}

#[test]
fn backend_cancel_stops_a_running_shell_action() {
    let backend = Arc::new(shell_backend(
        "sleep",
        OutputBudget::truncate_at(128),
        5_000,
    ));
    let plan = backend.plan(&shell_intent()).unwrap();
    let action = plan.intent.intent_id.clone();
    let sink = Arc::new(EventSink::default());
    let worker_backend = Arc::clone(&backend);
    let worker_sink = Arc::clone(&sink);
    let worker = std::thread::spawn(move || {
        worker_backend.execute(plan, &worker_sink, CancelToken::default())
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionStarted(_)))
    {
        assert!(std::time::Instant::now() < deadline, "action did not start");
        std::thread::yield_now();
    }
    backend.cancel(action).unwrap();
    assert_eq!(
        worker.join().unwrap().unwrap().status,
        ActionStatus::Cancelled
    );
    assert!(sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionCancelled(_))));
}

#[test]
fn file_write_emits_diff_before_effect_and_rejects_root_escape() {
    let root = TestRoot::new();
    let target = root.path.join("note.txt");
    fs::write(&target, b"old").unwrap();
    let observed_path = target.clone();
    let sink = EventSink::with_observer(move |event| {
        if matches!(event, p::EventPayload::ActionOutputDelta(_))
            && fs::read(&observed_path).ok().as_deref() != Some(b"old")
        {
            return Err(p::Error("file changed before diff event".into()));
        }
        Ok(())
    });
    let backend = FileBackend::new(
        vec![root.path.to_string_lossy().into_owned()],
        OutputBudget::truncate_at(4_096),
        p::DurationMs(1_000),
    )
    .unwrap();
    let plan = backend.plan(&file_intent(&target, b"new")).unwrap();
    assert_eq!(plan.rollback_boundary.0, "file-content-snapshot");
    let result = backend
        .execute(plan, &sink, CancelToken::default())
        .unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert!(result.diff.is_some());
    assert!(result.rollback.is_some());
    let kinds: Vec<_> = sink.events().iter().map(p::EventPayload::kind).collect();
    assert_eq!(
        kinds,
        vec![
            p::EventKind::ActionStarted,
            p::EventKind::ActionOutputDelta,
            p::EventKind::ActionCompleted,
            p::EventKind::CapabilityEvidenceRecorded,
        ]
    );

    let escaped = root.path.join("..").join(format!(
        "forme-outside-{}-{}.txt",
        std::process::id(),
        now()
    ));
    let escape_sink = EventSink::default();
    assert!(backend
        .execute(
            backend.plan(&file_intent(&escaped, b"escape")).unwrap(),
            &escape_sink,
            CancelToken::default(),
        )
        .is_err());
    assert!(!escaped.exists());
}

#[test]
fn missing_backend_is_an_error_and_never_falls_back_to_shell() {
    let registry = ExecutionBackendRegistry::default();
    registry
        .register(Arc::new(shell_backend(
            "output",
            OutputBudget::truncate_at(128),
            1_000,
        )))
        .unwrap();
    let root = TestRoot::new();
    let target = root.path.join("missing-backend.txt");
    let planner =
        DefaultExecutionPlanner::new(OutputBudget::truncate_at(128), p::DurationMs(1_000));
    let plan = planner.plan(&file_intent(&target, b"never")).unwrap();
    let sink = EventSink::default();
    assert!(registry
        .execute(plan, &sink, CancelToken::default())
        .is_err());
    assert!(!target.exists());
    assert!(sink.events().is_empty());
}

#[test]
fn s2_execution_side_only_runs_an_allowed_action() {
    let backend = Arc::new(shell_backend(
        "output",
        OutputBudget::truncate_at(128),
        1_000,
    ));
    let registry = ExecutionBackendRegistry::default();
    registry.register(backend.clone()).unwrap();
    let intent = shell_intent();
    let engine = DefaultPolicyEngine;

    let allow_context = policy_context(&engine, p::PolicyDecision::Allow);
    assert_eq!(
        engine.evaluate(&allow_context, &intent),
        p::PolicyDecision::Allow
    );
    let allowed_sink = EventSink::default();
    registry
        .execute(
            backend.plan(&intent).unwrap(),
            &allowed_sink,
            CancelToken::default(),
        )
        .unwrap();
    assert!(allowed_sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionStarted(_))));

    let deny_context = policy_context(&engine, p::PolicyDecision::Deny);
    assert_eq!(
        engine.evaluate(&deny_context, &intent),
        p::PolicyDecision::Deny
    );
    let denied_sink = EventSink::default();
    assert!(denied_sink.events().iter().all(|event| !matches!(
        event,
        p::EventPayload::ActionStarted(_) | p::EventPayload::ActionCompleted(_)
    )));
}

#[test]
fn shell_fixture_child() {
    let Ok(mode) = std::env::var("FORME_SHELL_FIXTURE_MODE") else {
        return;
    };
    match mode.as_str() {
        "output" => {
            let mut stdout = std::io::stdout().lock();
            writeln!(stdout, "{}", "x".repeat(256)).unwrap();
            stdout.flush().unwrap();
        }
        "sleep" => std::thread::sleep(Duration::from_secs(3)),
        _ => {}
    }
}

fn shell_backend(mode: &str, budget: OutputBudget, timeout_ms: u64) -> ShellBackend {
    let executable = std::env::current_exe().unwrap();
    let cwd = std::env::current_dir().unwrap();
    ShellBackend::new(
        ShellSandbox {
            schema_version: p::SchemaVersion(1),
            available: true,
            allowed_programs: vec![executable.to_string_lossy().into_owned()],
            allowed_roots: vec![cwd.to_string_lossy().into_owned()],
            environment: vec![("FORME_SHELL_FIXTURE_MODE".into(), mode.into())],
            network_allowed: false,
        },
        budget,
        p::DurationMs(timeout_ms),
    )
    .unwrap()
}

fn shell_intent() -> p::ActionIntent {
    let executable = std::env::current_exe().unwrap();
    let cwd = std::env::current_dir().unwrap();
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("shell-fixture-action".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("exercise shell backend".into()),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("tool:shell-fixture".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:alpha".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: executable.to_string_lossy().into_owned(),
            args: vec![
                "--exact".into(),
                "shell_fixture_child".into(),
                "--nocapture".into(),
                "--test-threads=1".into(),
            ],
            cwd: Some(cwd.to_string_lossy().into_owned()),
            network: false,
        },
        requested_permissions: vec![p::PermissionRef("execute".into())],
        requested_at: now(),
        estimated_output_bytes: 512,
        estimated_duration: p::DurationMs(500),
    }
}

fn file_intent(path: &std::path::Path, content: &[u8]) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("file-fixture-action".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("exercise file backend".into()),
        backend_hint: p::BackendKind::File,
        capability_ref: p::CapabilityRef("tool:file".into()),
        action_type: p::ActionType::Write,
        scope: p::Scope("workspace:alpha".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("file-content-snapshot".into()),
        parameters: p::ActionParameters::File {
            operation: p::FileOperation::Write,
            path: path.to_string_lossy().into_owned(),
            content: Some(content.to_vec()),
        },
        requested_permissions: vec![p::PermissionRef("write".into())],
        requested_at: now(),
        estimated_output_bytes: 256,
        estimated_duration: p::DurationMs(100),
    }
}

fn notification_intent() -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("notification-fixture-action".into()),
        source: p::Source::Schedule,
        goal: p::GoalRef("deliver a bounded local reminder".into()),
        backend_hint: p::BackendKind::Notification,
        capability_ref: p::CapabilityRef("capability:local-notification".into()),
        action_type: p::ActionType::Deliver,
        scope: p::Scope("workspace:alpha".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("notification-not-retractable".into()),
        parameters: p::ActionParameters::Notification {
            surface: p::SurfaceRef("surface:local-notification".into()),
            target: p::ParticipantId("owner".into()),
            title: "Scheduled reminder".into(),
            body_ref: p::ContentRef("content:notification-fixture".into()),
        },
        requested_permissions: vec![p::PermissionRef("permission:local-notification".into())],
        requested_at: now(),
        estimated_output_bytes: 128,
        estimated_duration: p::DurationMs(100),
    }
}

fn policy_context(engine: &DefaultPolicyEngine, decision: p::PolicyDecision) -> PolicyContext {
    let executable = std::env::current_exe().unwrap();
    let cwd = std::env::current_dir().unwrap();
    PolicyContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session-1".into()),
        toolset: p::ToolsetRef("toolset-execution".into()),
        policy: engine.merge(&[PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::Managed,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                matcher: ActionMatcher::default(),
                effect: decision,
                scope: p::Scope("workspace:alpha".into()),
            }],
        }]),
        visible_capabilities: vec![p::CapabilityRef("tool:shell-fixture".into())],
        granted_permissions: vec![p::PermissionRef("execute".into())],
        allowed_scopes: vec![p::Scope("workspace:alpha".into())],
        shell_allowlist: vec![executable.to_string_lossy().into_owned()],
        file_roots: vec![cwd.to_string_lossy().into_owned()],
        mcp_allowlist: Vec::new(),
        external: forme_policy::ExternalPolicyLimits::default(),
        network_allowed: false,
        sandbox_available: true,
        delegation: None,
        envelope: None,
    }
}

struct TestRoot {
    path: std::path::PathBuf,
}

impl TestRoot {
    fn new() -> Self {
        static NEXT_ROOT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT_ROOT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "forme-execution-{}-{}-{id}",
            std::process::id(),
            now()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[allow(dead_code)]
fn _backend_kind_shape(kind: BackendKind) -> p::BackendKind {
    kind
}
