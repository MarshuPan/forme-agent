//! A catalog binding cannot turn model-supplied arguments into owner discovery.
use super::*;
use crate::{GovernanceConfig, HarnessConfig};
use forme_models::{
    Cost, ModelCapability, ModelProfile, ModelStrength, RateLimit, ScriptedModelProvider, Url,
};
use std::sync::Arc;

#[test]
fn model_catalog_binding_never_resolves_managed_discovery() {
    let profile = ModelProfile {
        schema_version: p::SchemaVersion(1),
        provider: p::ProviderId("fixture:discovery".into()),
        model: "scripted".into(),
        base_url: Url::parse("http://127.0.0.1:9").unwrap(),
        capability: ModelCapability {
            schema_version: p::SchemaVersion(1),
            context_window: 4096,
            tool_use: true,
            strength: ModelStrength::Standard,
        },
        cost: Cost {
            schema_version: p::SchemaVersion(1),
            input_microunits_per_million: 0,
            output_microunits_per_million: 0,
        },
        rate_limit: RateLimit {
            schema_version: p::SchemaVersion(1),
            requests_per_minute: 10,
            tokens_per_minute: 4096,
        },
        credential_ref: p::CredentialRef("credential:fixture".into()),
    };
    let capability = p::CapabilityRef("mcp-discovery:server:test".into());
    let mut harness = ReactiveHarness::new(
        forme_store::SqliteEventStore::open_in_memory(Default::default()).unwrap(),
        Arc::new(ScriptedModelProvider::new(profile.clone(), vec![]).unwrap()),
        Arc::new(forme_execution::ExecutionBackendRegistry::default()),
        GovernanceConfig {
            visible_capabilities: vec![capability.clone()],
            ..Default::default()
        },
        HarnessConfig::for_model(&profile),
    )
    .unwrap();
    let tool_ref = p::ToolRef("catalog:fixture".into());
    harness.formal_tool_bindings.insert(
        tool_ref.clone(),
        FormalToolBinding {
            capability,
            backend: p::BackendKind::Mcp,
            scope: p::Scope("workspace:default".into()),
            permissions: vec![],
            risk: p::Risk::High,
        },
    );
    let mut call = ModelToolCall {
        schema_version: p::SchemaVersion(1),
        call_id: p::ToolCallId("call:fixture".into()),
        tool: tool_ref,
        intent: None,
        arguments: serde_json::to_value(p::ActionParameters::Mcp {
            server: p::McpServerRef("server:test".into()),
            tool: p::ToolRef("echo".into()),
            arguments: serde_json::json!({}),
            schema_digest: None,
            transport: p::McpTransport::Stdio,
            stdio: p::McpStdioSpec {
                schema_version: p::SchemaVersion(1),
                command: "fixture".into(),
                args: vec![],
                env: vec![],
            },
            timeout: p::DurationMs(1000),
        })
        .unwrap(),
    };
    let run = p::RunId("run:fixture".into());
    let workspace = p::WorkspaceRef("workspace:default".into());
    assert!(harness
        .resolve_formal_tool(&run, &workspace, &call, None)
        .is_some());
    call.arguments = serde_json::to_value(p::ActionParameters::McpDiscovery(
        p::ManagedMcpDiscoverySpec {
            schema_version: p::SchemaVersion(1),
            server: p::McpServerRef("server:test".into()),
            program_ref: "program:test".into(),
            registration_digest: p::canonical_digest(&"registration").unwrap(),
            settings_digest: p::canonical_digest(&"settings").unwrap(),
            timeout: p::DurationMs(1000),
            output_budget_bytes: 4096,
        },
    ))
    .unwrap();
    assert!(harness
        .resolve_formal_tool(&run, &workspace, &call, None)
        .is_none());
}
