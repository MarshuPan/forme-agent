use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use forme_execution::{
    ActionBackend, ActionStatus, CancelToken, EventSink, McpBackend, OutputBudget,
};
use forme_protocol as p;

fn transport_test() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn s3_mcp_backend_emits_call_event_and_completed_action() {
    let _serial = transport_test();
    let backend = McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(1_000)).unwrap();
    let sink = EventSink::default();
    let result = backend
        .execute(
            backend.plan(&mcp_intent("success", 1_000)).unwrap(),
            &sink,
            CancelToken::default(),
        )
        .unwrap();
    assert_eq!(result.status, ActionStatus::Completed);
    let events = sink.events();
    let call = events
        .iter()
        .find_map(|event| match event {
            p::EventPayload::McpCallEvent(payload) => Some(payload),
            _ => None,
        })
        .unwrap();
    assert!(!call.timeout);
    assert!(call.error_class.is_none());
    assert!(events
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionCompleted(_))));
}

#[test]
fn s3_mcp_backend_classifies_timeout_schema_and_server_errors() {
    let _serial = transport_test();
    for (mode, expected, timeout) in [
        ("timeout", "timeout", 150),
        ("schema", "schema_mismatch", 1_000),
        ("server", "server_error", 1_000),
    ] {
        let backend =
            McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(timeout)).unwrap();
        let sink = EventSink::default();
        assert!(backend
            .execute(
                backend.plan(&mcp_intent(mode, timeout)).unwrap(),
                &sink,
                CancelToken::default(),
            )
            .is_err());
        let events = sink.events();
        let call = events
            .iter()
            .find_map(|event| match event {
                p::EventPayload::McpCallEvent(payload) => Some(payload),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            call.error_class.as_ref().unwrap().0,
            expected,
            "mode={mode}"
        );
        assert_eq!(call.timeout, mode == "timeout");
        let unresolved = mode == "schema";
        assert_eq!(
            events
                .iter()
                .any(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_))),
            unresolved
        );
        assert_eq!(
            events
                .iter()
                .any(|event| matches!(event, p::EventPayload::ActionFailed(_))),
            !unresolved
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event, p::EventPayload::ActionCompleted(_))));
    }
}

#[test]
fn mcp_missing_response_after_a_real_effect_is_unknown() {
    assert_effect_without_outcome("effect-no-response", false);
}

#[test]
fn mcp_invalid_response_after_a_real_effect_is_unknown() {
    for mode in [
        "effect-invalid-response",
        "effect-invalid-version",
        "effect-invalid-error",
        "effect-both-result-and-error",
        "effect-missing-content",
        "effect-invalid-content",
        "effect-invalid-is-error",
    ] {
        assert_effect_without_outcome(mode, false);
    }
}

#[test]
fn mcp_cancellation_after_a_real_effect_is_unknown() {
    assert_effect_without_outcome("effect-no-response", true);
}

fn assert_effect_without_outcome(mode: &str, cancel_after_effect: bool) {
    let _serial = transport_test();
    let directory = tempfile::tempdir().unwrap();
    let effect = directory.path().join("actual-effect");
    let timeout = if cancel_after_effect { 2_000 } else { 500 };
    let backend =
        McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(timeout)).unwrap();
    let mut intent = mcp_intent(mode, timeout);
    if let p::ActionParameters::Mcp { stdio, .. } = &mut intent.parameters {
        stdio.env.push((
            "FORME_MCP_EFFECT".into(),
            effect.to_string_lossy().into_owned(),
        ));
    }
    let token = CancelToken::default();
    let cancellation = token.clone();
    let observed = effect.clone();
    let cancel = cancel_after_effect.then(|| {
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !observed.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            cancellation.cancel();
        })
    });
    let sink = EventSink::default();
    let result = backend.execute(backend.plan(&intent).unwrap(), &sink, token);
    if let Some(cancel) = cancel {
        cancel.join().unwrap();
    }
    assert_eq!(std::fs::read(&effect).unwrap(), b"effect\n");
    assert!(
        result.is_err(),
        "an unobserved effect must remain unresolved"
    );
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_)))
            .count(),
        1,
        "killing and joining a process does not undo its real effect"
    );
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        p::EventPayload::ActionCompleted(_)
            | p::EventPayload::ActionFailed(_)
            | p::EventPayload::ActionCancelled(_)
    )));
}

#[test]
fn mcp_timeout_covers_a_request_to_a_nonreading_server() {
    let _serial = transport_test();
    let directory = tempfile::tempdir().unwrap();
    let ready = directory.path().join("server-ready");
    let backend = McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(400)).unwrap();
    let mut intent = mcp_intent("blocked-input", 400);
    if let p::ActionParameters::Mcp {
        arguments, stdio, ..
    } = &mut intent.parameters
    {
        *arguments = serde_json::json!({"payload": "x".repeat(256 * 1_024)});
        stdio.env.push((
            "FORME_MCP_READY".into(),
            ready.to_string_lossy().into_owned(),
        ));
    }
    let sink = EventSink::default();
    let started = Instant::now();
    let result = backend.execute(
        backend.plan(&intent).unwrap(),
        &sink,
        CancelToken::default(),
    );
    assert!(result.is_err());
    assert!(
        ready.exists(),
        "the fixture must complete initialize before blocking"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    println!(
        "MCP blocked-input timeout: {} ms",
        started.elapsed().as_millis()
    );
    assert!(sink.events().iter().any(|event| matches!(
        event,
        p::EventPayload::McpCallEvent(call)
            if call.timeout && call.error_class == Some(p::McpErrorClass("timeout".into()))
    )));
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        p::EventPayload::ActionCompleted(_) | p::EventPayload::ActionFailed(_)
    )));
    assert!(sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_))));
}

#[test]
fn mcp_cancel_covers_a_request_to_a_nonreading_server() {
    let _serial = transport_test();
    let directory = tempfile::tempdir().unwrap();
    let ready = directory.path().join("server-ready");
    let backend = McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(10_000)).unwrap();
    let mut intent = mcp_intent("blocked-input", 10_000);
    if let p::ActionParameters::Mcp {
        arguments, stdio, ..
    } = &mut intent.parameters
    {
        *arguments = serde_json::json!({"payload": "x".repeat(256 * 1_024)});
        stdio.env.push((
            "FORME_MCP_READY".into(),
            ready.to_string_lossy().into_owned(),
        ));
    }
    let token = CancelToken::default();
    let cancellation = token.clone();
    let observed = ready.clone();
    let cancel = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !observed.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(100));
        cancellation.cancel();
    });
    let sink = EventSink::default();
    let started = Instant::now();
    let result = backend.execute(backend.plan(&intent).unwrap(), &sink, token);
    cancel.join().unwrap();
    assert!(ready.exists(), "cancel must follow successful initialize");
    assert!(started.elapsed() < Duration::from_secs(2));
    println!(
        "MCP blocked-input cancellation: {} ms",
        started.elapsed().as_millis()
    );
    assert!(result.is_err());
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_)))
            .count(),
        1
    );
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        p::EventPayload::ActionCompleted(_)
            | p::EventPayload::ActionFailed(_)
            | p::EventPayload::ActionCancelled(_)
    )));
}

#[test]
fn mcp_cancel_before_dispatch_has_no_tool_effect_and_is_cancelled() {
    let _serial = transport_test();
    let backend = McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(1_000)).unwrap();
    let sink = EventSink::default();
    let token = CancelToken::default();
    token.cancel();
    let result = backend
        .execute(
            backend.plan(&mcp_intent("success", 1_000)).unwrap(),
            &sink,
            token,
        )
        .unwrap();
    assert_eq!(result.status, ActionStatus::Cancelled);
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        p::EventPayload::ActionStarted(_) | p::EventPayload::ActionOutcomeUnknown(_)
    )));
}

#[test]
fn mcp_rejects_an_oversized_unterminated_response() {
    let _serial = transport_test();
    let backend = McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(5_000)).unwrap();
    let sink = EventSink::default();
    let error = backend
        .execute(
            backend.plan(&mcp_intent("oversized-frame", 5_000)).unwrap(),
            &sink,
            CancelToken::default(),
        )
        .unwrap_err();
    assert!(error.0.contains("wire frame bound"), "{error}");
    assert!(sink.events().iter().any(|event| matches!(event,
        p::EventPayload::McpCallEvent(call)
            if call.error_class == Some(p::McpErrorClass("schema_mismatch".into()))
    )));
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_)))
            .count(),
        1
    );
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        p::EventPayload::ActionOutputDelta(_)
            | p::EventPayload::ActionCompleted(_)
            | p::EventPayload::ActionFailed(_)
    )));
}

#[test]
fn mcp_rejects_an_oversized_request_before_tool_dispatch() {
    let _serial = transport_test();
    let directory = tempfile::tempdir().unwrap();
    let called = directory.path().join("tool-called");
    let backend = McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(5_000)).unwrap();
    let mut intent = mcp_intent("success", 5_000);
    if let p::ActionParameters::Mcp {
        arguments, stdio, ..
    } = &mut intent.parameters
    {
        *arguments = serde_json::json!({"payload": "x".repeat(8 * 1_024 * 1_024)});
        stdio.env.push((
            "FORME_MCP_CALLED".into(),
            called.to_string_lossy().into_owned(),
        ));
    }
    let sink = EventSink::default();
    let error = backend
        .execute(
            backend.plan(&intent).unwrap(),
            &sink,
            CancelToken::default(),
        )
        .unwrap_err();
    assert!(
        error.0.contains("request exceeds its wire frame bound"),
        "{error}"
    );
    assert!(
        !called.exists(),
        "an oversized tool request must not be dispatched"
    );
    assert!(sink.events().iter().any(|event| matches!(event,
        p::EventPayload::McpCallEvent(call)
            if call.error_class == Some(p::McpErrorClass("schema_mismatch".into()))
    )));
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        p::EventPayload::ActionCompleted(_) | p::EventPayload::ActionOutcomeUnknown(_)
    )));
}

#[test]
fn mcp_wire_framing_preserves_output_budget_and_ignores_unmatched_frames() {
    let _serial = transport_test();
    for truncate in [true, false] {
        let backend = McpBackend::new(
            OutputBudget {
                schema_version: p::SchemaVersion(1),
                max_bytes: 16,
                truncate,
            },
            p::DurationMs(2_000),
        )
        .unwrap();
        let sink = EventSink::default();
        let result = backend.execute(
            backend.plan(&mcp_intent("noisy-frames", 2_000)).unwrap(),
            &sink,
            CancelToken::default(),
        );
        if truncate {
            let result = result.unwrap();
            assert_eq!(result.status, ActionStatus::Completed);
            assert!(result.truncated);
        } else {
            assert!(result.unwrap_err().0.contains("strict byte budget"));
        }
        assert!(!sink
            .events()
            .iter()
            .any(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_))));
    }
}

#[test]
fn mcp_retains_inherited_pipe_cleanup_and_blocks_new_processes_until_joined() {
    let _serial = transport_test();
    let directory = tempfile::tempdir().unwrap();
    let ready = directory.path().join("server-ready");
    let done = directory.path().join("holder-done");
    let backend = McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(1_000)).unwrap();
    let mut original = mcp_intent("held-pipe", 500);
    if let p::ActionParameters::Mcp { stdio, .. } = &mut original.parameters {
        stdio.env.push((
            "FORME_MCP_READY".into(),
            ready.to_string_lossy().into_owned(),
        ));
        stdio.env.push((
            "FORME_MCP_HOLDER_DONE".into(),
            done.to_string_lossy().into_owned(),
        ));
    }
    let sink = EventSink::default();
    let started = Instant::now();
    let error = backend
        .execute(
            backend.plan(&original).unwrap(),
            &sink,
            CancelToken::default(),
        )
        .unwrap_err();
    assert!(ready.exists());
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(error.0.contains("cleanup is unverified"), "{error}");
    println!(
        "MCP retained original cleanup: {} ms",
        started.elapsed().as_millis()
    );
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_)))
            .count(),
        1
    );
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        p::EventPayload::ActionCompleted(_)
            | p::EventPayload::ActionCancelled(_)
            | p::EventPayload::ActionFailed(_)
    )));

    // This is a different action, never a redispatch of the Unknown original.
    let mut next = mcp_intent("success", 1_000);
    let blocked = backend
        .execute(
            backend.plan(&next).unwrap(),
            &EventSink::default(),
            CancelToken::default(),
        )
        .unwrap_err();
    assert!(blocked.0.contains("original cleanup"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(done.exists(), "the finite pipe holder must stop");
    loop {
        next.intent_id.0.push('x');
        match backend.execute(
            backend.plan(&next).unwrap(),
            &EventSink::default(),
            CancelToken::default(),
        ) {
            Ok(result) => {
                assert_eq!(result.status, ActionStatus::Completed);
                break;
            }
            Err(error) if error.0.contains("original cleanup") && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("new admission after original cleanup: {error}"),
        }
    }
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_)))
            .count(),
        1,
        "cleanup must not rewrite the original terminal event"
    );
}

#[test]
fn execution_mcp_fixture_child() {
    let Ok(mode) = std::env::var("FORME_MCP_EXECUTION_FIXTURE") else {
        return;
    };
    if mode == "pipe-holder" {
        std::thread::sleep(Duration::from_secs(4));
        std::fs::write(std::env::var_os("FORME_MCP_HOLDER_DONE").unwrap(), b"done").unwrap();
        return;
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout).unwrap();
    stdout.flush().unwrap();
    for line in stdin.lock().lines() {
        let request: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let method = request["method"].as_str().unwrap();
        if method == "notifications/initialized" {
            continue;
        }
        if method == "tools/call" {
            if let Some(called) = std::env::var_os("FORME_MCP_CALLED") {
                std::fs::write(called, b"called").unwrap();
            }
        }
        if method == "tools/call" && mode.starts_with("effect-") {
            let mut effect = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(std::env::var_os("FORME_MCP_EFFECT").unwrap())
                .unwrap();
            effect.write_all(b"effect\n").unwrap();
            effect.sync_all().unwrap();
            if mode != "effect-no-response" {
                let response = match mode.as_str() {
                    "effect-invalid-version" => serde_json::json!({
                        "jsonrpc":"1.0", "id":request["id"], "result":{}
                    }),
                    "effect-invalid-error" => serde_json::json!({
                        "jsonrpc":"2.0", "id":request["id"], "error":null
                    }),
                    "effect-both-result-and-error" => serde_json::json!({
                        "jsonrpc":"2.0", "id":request["id"], "result":{},
                        "error":{"code":-32000,"message":"fixture"}
                    }),
                    "effect-missing-content" => serde_json::json!({
                        "jsonrpc":"2.0", "id":request["id"], "result":{}
                    }),
                    "effect-invalid-content" => serde_json::json!({
                        "jsonrpc":"2.0", "id":request["id"], "result":{"content":null}
                    }),
                    "effect-invalid-is-error" => serde_json::json!({
                        "jsonrpc":"2.0", "id":request["id"],
                        "result":{"content":[], "isError":"false"}
                    }),
                    _ => serde_json::json!({"jsonrpc":"2.0", "id":request["id"]}),
                };
                writeln!(stdout, "{response}").unwrap();
                stdout.flush().unwrap();
            } else {
                std::thread::sleep(Duration::from_secs(4));
            }
            return;
        }
        if mode == "timeout" {
            std::thread::sleep(Duration::from_secs(2));
            return;
        }
        if method == "tools/call" && mode == "oversized-frame" {
            let _ = stdout.write_all(&vec![b'x'; 8 * 1_024 * 1_024 + 1]);
            let _ = stdout.flush();
            std::thread::sleep(Duration::from_secs(4));
            return;
        }
        if method == "tools/call" && mode == "noisy-frames" {
            stdout.write_all(b"\xff\xfe\n{broken\n").unwrap();
            for id in 100..120 {
                writeln!(
                    stdout,
                    "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{}}}}"
                )
                .unwrap();
            }
        }
        let id = request["id"].clone();
        let response = match method {
            "initialize" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "protocolVersion": "2025-06-18", "capabilities": {} }
            }),
            "tools/call" if mode == "schema" => serde_json::json!({
                "jsonrpc": "2.0", "id": id, "result": []
            }),
            "tools/call" if mode == "server" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32002, "message": "fixture call failure" }
            }),
            "tools/call" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "content": [{ "type": "text", "text": "fixture result" }] }
            }),
            _ => continue,
        };
        writeln!(stdout, "{response}").unwrap();
        stdout.flush().unwrap();
        if method == "initialize" && (mode == "blocked-input" || mode == "held-pipe") {
            let holder = if mode == "held-pipe" {
                Some(
                    std::process::Command::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "execution_mcp_fixture_child",
                            "--nocapture",
                            "--test-threads=1",
                        ])
                        .env_clear()
                        .env("FORME_MCP_EXECUTION_FIXTURE", "pipe-holder")
                        .env(
                            "FORME_MCP_HOLDER_DONE",
                            std::env::var_os("FORME_MCP_HOLDER_DONE").unwrap(),
                        )
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::inherit())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .unwrap(),
                )
            } else {
                None
            };
            let ready = std::env::var_os("FORME_MCP_READY").unwrap();
            std::fs::write(ready, b"initialized").unwrap();
            if let Some(mut holder) = holder {
                holder.wait().unwrap();
            } else {
                std::thread::sleep(Duration::from_secs(4));
            }
            return;
        }
        if method == "tools/call" {
            return;
        }
    }
}

fn mcp_intent(mode: &str, timeout_ms: u64) -> p::ActionIntent {
    let executable = std::env::current_exe().unwrap();
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId(format!("mcp-execution-{mode}")),
        source: p::Source::UserTurn,
        goal: p::GoalRef("exercise MCP backend".into()),
        backend_hint: p::BackendKind::Mcp,
        capability_ref: p::CapabilityRef("mcp:fixture:read".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:alpha".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Mcp {
            server: p::McpServerRef("mcp-execution-fixture".into()),
            tool: p::ToolRef("read".into()),
            arguments: serde_json::json!({ "key": "value" }),
            schema_digest: Some(p::SchemaDigest("sha256:fixture".into())),
            transport: p::McpTransport::Stdio,
            stdio: p::McpStdioSpec {
                schema_version: p::SchemaVersion(1),
                command: executable.to_string_lossy().into_owned(),
                args: vec![
                    "--exact".into(),
                    "execution_mcp_fixture_child".into(),
                    "--nocapture".into(),
                    "--test-threads=1".into(),
                ],
                env: vec![("FORME_MCP_EXECUTION_FIXTURE".into(), mode.into())],
            },
            timeout: p::DurationMs(timeout_ms),
        },
        requested_permissions: vec![p::PermissionRef("execute".into())],
        requested_at: 10,
        estimated_output_bytes: 1_024,
        estimated_duration: p::DurationMs(timeout_ms),
    }
}
