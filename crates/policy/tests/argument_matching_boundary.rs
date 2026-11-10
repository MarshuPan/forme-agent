use forme_policy::ArgMatcher;
use forme_protocol as p;

#[test]
fn mcp_tool_matcher_cannot_authorize_another_tool_or_file() {
    let matcher = ArgMatcher::McpTarget {
        server: p::McpServerRef("server:test".into()),
        tool: p::ToolRef("tool:read".into()),
    };
    let mut intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:test".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("goal:test".into()),
        backend_hint: p::BackendKind::Mcp,
        capability_ref: p::CapabilityRef("mcp:read".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:test".into()),
        risk_hint: p::Risk::Medium,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Mcp {
            server: p::McpServerRef("server:test".into()),
            tool: p::ToolRef("tool:read".into()),
            arguments: Default::default(),
            schema_digest: None,
            transport: p::McpTransport::Stdio,
            stdio: p::McpStdioSpec {
                schema_version: p::SchemaVersion(1),
                command: "fixture".into(),
                args: vec![],
                env: vec![],
            },
            timeout: p::DurationMs(1000),
        },
        requested_permissions: vec![],
        requested_at: 42,
        estimated_output_bytes: 1024,
        estimated_duration: p::DurationMs(500),
    };
    assert!(matcher.matches(&intent));
    if let p::ActionParameters::Mcp { tool, .. } = &mut intent.parameters {
        tool.0 = "tool:write".into();
    }
    assert!(!matcher.matches(&intent));
    intent.parameters = p::ActionParameters::File {
        operation: p::FileOperation::Read,
        path: "server:test".into(),
        content: None,
    };
    assert!(!matcher.matches(&intent));
}
