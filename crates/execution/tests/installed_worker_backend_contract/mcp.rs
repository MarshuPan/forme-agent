//! Managed call facts remain brain events; discovery is not a tool call.
use super::*;
use serde_json::json;

fn mcp(
    mode: FailureMode,
    discovery: bool,
) -> (
    WorkerActionBackend,
    Arc<FakeTransport>,
    forme_execution::ExecutionPlan,
) {
    let (shell, transport) = backend(mode);
    let worker = WorkerActionBackend::new(
        BackendKind::Mcp,
        transport.clone(),
        p::VerifiedPrincipal("owner:worker-backend".into()),
        p::AuthorityEpoch(1),
        p::MANAGED_MCP_PROGRAM_PROFILE,
        OutputBudget::strict(8192),
        p::DurationMs(1000),
    )
    .unwrap();
    let mut intent = plan(&shell, "mcp-fact").intent;
    intent.backend_hint = BackendKind::Mcp;
    intent.source = p::Source::OwnerControl;
    intent.risk_hint = p::Risk::High;
    intent.expected_effect = p::ExpectedEffect::Outward;
    intent.capability_ref = p::CapabilityRef(
        if discovery {
            "mcp-discovery:fixture"
        } else {
            "mcp:fixture:inspect"
        }
        .into(),
    );
    intent.parameters = if discovery {
        p::ActionParameters::McpDiscovery(p::ManagedMcpDiscoverySpec {
            schema_version: p::SchemaVersion(1),
            server: p::McpServerRef("fixture".into()),
            program_ref: "program:fixture".into(),
            registration_digest: p::canonical_digest(&"registration").unwrap(),
            settings_digest: p::canonical_digest(&"settings").unwrap(),
            timeout: p::DurationMs(100),
            output_budget_bytes: 4096,
        })
    } else {
        p::ActionParameters::Mcp {
            server: p::McpServerRef("fixture".into()),
            tool: p::ToolRef("inspect".into()),
            arguments: json!({}),
            schema_digest: Some(p::canonical_digest(&json!({"type":"object"})).unwrap()),
            transport: p::McpTransport::Stdio,
            stdio: p::McpStdioSpec {
                schema_version: p::SchemaVersion(1),
                command: "program:fixture".into(),
                args: vec![],
                env: vec![],
            },
            timeout: p::DurationMs(100),
        }
    };
    let plan = worker.plan(&intent).unwrap();
    (worker, transport, plan)
}

#[test]
fn managed_mcp_call_emits_original_call_fact_for_success_and_unverifiable_result() {
    for mode in [
        FailureMode::None,
        FailureMode::Unknown,
        FailureMode::Manifest,
        FailureMode::Dispatch,
    ] {
        let (worker, transport, plan) = mcp(mode, false);
        let sink = EventSink::default();
        let result = worker.execute(plan, &sink, CancelToken::default()).unwrap();
        let events = sink.events();
        let calls: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                p::EventPayload::McpCallEvent(call) => Some(call),
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].server.0, "fixture");
        assert_eq!(calls[0].tool.0, "inspect");
        assert!(!calls[0].timeout);
        assert_eq!(calls[0].error_class.is_some(), mode != FailureMode::None);
        if mode != FailureMode::None {
            assert_eq!(result.status, ActionStatus::Unknown);
            assert_unknown(&sink);
        }
        assert_eq!(transport.snapshot().0, 1);
    }
}

#[test]
fn managed_mcp_discovery_never_fabricates_a_tool_call_fact() {
    let (worker, _, plan) = mcp(FailureMode::None, true);
    let sink = EventSink::default();
    worker.execute(plan, &sink, CancelToken::default()).unwrap();
    assert!(!sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::McpCallEvent(_))));
}

#[test]
fn ordinary_worker_journal_rejects_unrecognized_execution_binding_version() {
    let (worker, transport) = backend(FailureMode::None);
    worker
        .execute(
            plan(&worker, "ordinary-version"),
            &EventSink::default(),
            CancelToken::default(),
        )
        .unwrap();
    let mut request = transport.state.lock().unwrap().request.clone().unwrap();
    request.execution.as_mut().unwrap().schema_version = p::SchemaVersion(2);
    let temp = tempfile::tempdir().unwrap();
    let journal =
        forme_execution::installed::WorkerJournal::open(&temp.path().join("worker.jsonl")).unwrap();
    assert!(journal
        .admit_with_execution(
            request.job.clone(),
            request.permit.clone(),
            request.execution.as_ref(),
            request.permit.issued_at
        )
        .is_err());
    assert!(journal
        .existing_observation(&request.job.job_id, request.permit.issued_at)
        .unwrap()
        .is_none());
}
