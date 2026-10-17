use forme_protocol as p;

#[test]
fn c16_optional_orchestration_keeps_legacy_decision_bytes_and_taxonomy() {
    let bytes = r#"{"trace_ref":"trace","refs":{"map":null,"user":null,"agent_self":null,"trust":null,"failure":[]},"rationale":"reason","workspace_snapshot":"workspace"}"#;
    let payload: p::DecisionTraceRecordedPayload = serde_json::from_str(bytes).unwrap();
    assert!(payload.orchestration.is_none());
    assert_eq!(serde_json::to_string(&payload).unwrap(), bytes);
    assert_eq!(p::EventKind::ALL.len(), 99);
}

#[test]
fn c16_orchestration_digest_schema_and_bounds_fail_closed() {
    let mut fact = p::OrchestrationFact::seal(
        p::WorkspaceRef("workspace:test".into()),
        p::IdempotencyKey("cancel".into()),
        0,
        p::OrchestrationOperation::RouteCancelled {
            route: p::ExecutionRouteRef("route:test".into()),
        },
    )
    .unwrap();
    fact.validate().unwrap();
    let bytes = serde_json::to_vec(&fact).unwrap();
    let restored: p::OrchestrationFact = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(fact, restored);
    fact.expected_revision = 1;
    assert!(fact.validate().is_err());
    fact.expected_revision = 0;
    fact.schema_version = p::SchemaVersion(0);
    assert!(fact.validate().is_err());
    assert!(p::OrchestrationFact::seal(
        p::WorkspaceRef("workspace:test".into()),
        p::IdempotencyKey("x".repeat(262_145)),
        0,
        p::OrchestrationOperation::RouteCancelled {
            route: p::ExecutionRouteRef("route:test".into())
        }
    )
    .is_err());
}
