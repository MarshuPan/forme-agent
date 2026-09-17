use std::io::{BufRead, Write};
use std::time::Duration;

use forme_execution::{
    ActionBackend, ActionStatus, CancelToken, EventSink, McpBackend, OutputBudget,
};
use forme_protocol as p;

#[test]
fn s3_mcp_backend_emits_call_event_and_completed_action() {
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
        assert!(events
            .iter()
            .any(|event| matches!(event, p::EventPayload::ActionFailed(_))));
        assert!(!events
            .iter()
            .any(|event| matches!(event, p::EventPayload::ActionCompleted(_))));
    }
}

#[test]
fn execution_mcp_fixture_child() {
    let Ok(mode) = std::env::var("FORME_MCP_EXECUTION_FIXTURE") else {
        return;
    };
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
        if mode == "timeout" {
            std::thread::sleep(Duration::from_secs(2));
            return;
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
