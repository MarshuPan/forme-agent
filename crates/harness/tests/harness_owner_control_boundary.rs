//! Assembly and owner-control moves must retain the durable approval boundary.
#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod support;

use forme_harness::{AgentHarness, GatewayControl, ProductionRuntime};
use forme_protocol as p;
use std::path::Path;
use std::sync::atomic::Ordering;
use support::Fixture;

fn assert_read_only(
    runtime: &ProductionRuntime,
    fixture: &Fixture,
    expected: &p::PendingApproval,
    target: &Path,
) {
    let harness = runtime.harness();
    let heads = runtime.services().store.run_heads().unwrap();
    let calls = fixture.calls.load(Ordering::SeqCst);
    for _ in 0..3 {
        let projected =
            GatewayControl::pending_approvals(harness.as_ref(), expected.session.clone()).unwrap();
        assert_eq!(projected, vec![expected.clone()]);
        let direct = harness.pending_approvals(expected.session.clone()).unwrap();
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].approval_id, expected.approval_id);
        assert_eq!(direct[0].plan_digest, expected.plan_digest);
        assert_eq!(
            direct[0].normalized_action_digest,
            expected.normalized_action_digest
        );
        assert_eq!(direct[0].affected_resources, expected.affected_resources);
        let other = p::SessionId("session:other-owner-conversation".into());
        assert!(harness.pending_approvals(other.clone()).unwrap().is_empty());
        assert!(GatewayControl::pending_approvals(harness.as_ref(), other)
            .unwrap()
            .is_empty());
        assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), calls);
        assert!(!target.exists());
    }
}

#[test]
fn production_approval_reads_survive_restart_without_cross_session_or_execution_bypass() {
    let mut fixture = Fixture::new();
    let workspace = fixture
        .config
        .store_path
        .parent()
        .unwrap()
        .join("file-workspace");
    std::fs::create_dir(&workspace).unwrap();
    fixture.config.capabilities.file.enabled = true;
    fixture.config.capabilities.file.roots = vec![workspace.to_string_lossy().into_owned()];
    fixture.config.models[0].tool_use = true;
    fixture.config.models[0].tool_schema_versions = vec![1];
    let target = workspace.join("owner-approved-only.txt");
    let parameters = p::ActionParameters::File {
        operation: p::FileOperation::Write,
        path: target.to_string_lossy().into_owned(),
        content: Some(b"must not be written by a query".to_vec()),
    };
    fixture
        .responses
        .lock()
        .unwrap()
        .push_back(serde_json::json!({
            "choices": [{"message": {"tool_calls": [{
                "id": "owner-control-fixture", "type": "function",
                "function": {
                    "name": forme_models::ToolDefinition::catalog_alias("capability:file").unwrap(),
                    "arguments": serde_json::to_string(&parameters).unwrap()
                }
            }]}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 20, "completion_tokens": 4}
        }));
    let runtime = fixture.build();
    let run = runtime
        .harness()
        .submit_run(p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::UserTurn,
            session: p::SessionRef("session:owner-control".into()),
            agent_profile: fixture.config.agent_profile.clone(),
            input: p::RunInput("Write the fixture file only after exact owner approval".into()),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey("owner-control-fixture".into())),
        })
        .unwrap();
    let pending = GatewayControl::pending_approvals(
        runtime.harness().as_ref(),
        p::SessionId("session:owner-control".into()),
    )
    .unwrap();
    assert_eq!(pending.len(), 1);
    let expected = &pending[0];
    assert_eq!(expected.run, run);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    assert_read_only(&runtime, &fixture, expected, &target);
    let heads = runtime.services().store.run_heads().unwrap();
    drop(runtime);

    let runtime = fixture.build();
    assert_eq!(runtime.services().store.run_heads().unwrap(), heads);
    assert_read_only(&runtime, &fixture, expected, &target);
}
