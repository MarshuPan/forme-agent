//! Captured legacy wire and digest before extending the action vocabulary.
use forme_protocol as p;

const LEGACY_PARAMETERS: &str = r#"{"Mcp":{"server":"server:legacy","tool":"tool:echo","arguments":{"text":"hello"},"schema_digest":"sha256:1111111111111111111111111111111111111111111111111111111111111111","transport":"Stdio","stdio":{"schema_version":1,"command":"fixture-echo","args":["--stdio"],"env":[]},"timeout":1000}}"#;
const LEGACY_INTENT: &str = r#"{"schema_version":1,"intent_id":"action:legacy","source":"UserTurn","goal":"goal:legacy","backend_hint":"Mcp","capability_ref":"mcp:echo","action_type":"Execute","scope":"workspace:test","risk_hint":"Medium","expected_effect":"Internal","rollback_expectation":"none","parameters":{"Mcp":{"server":"server:legacy","tool":"tool:echo","arguments":{"text":"hello"},"schema_digest":"sha256:1111111111111111111111111111111111111111111111111111111111111111","transport":"Stdio","stdio":{"schema_version":1,"command":"fixture-echo","args":["--stdio"],"env":[]},"timeout":1000}},"requested_permissions":[],"requested_at":42,"estimated_output_bytes":1024,"estimated_duration":500}"#;

#[test]
fn legacy_mcp_keeps_exact_wire_digest_and_resource_identity() {
    let parameters: p::ActionParameters = serde_json::from_str(LEGACY_PARAMETERS).unwrap();
    assert_eq!(
        serde_json::to_string(&parameters).unwrap(),
        LEGACY_PARAMETERS
    );
    let intent: p::ActionIntent = serde_json::from_str(LEGACY_INTENT).unwrap();
    assert_eq!(intent.parameters, parameters);
    assert_eq!(
        p::normalized_action_digest(&intent).unwrap().0,
        "sha256:a9450563592e7883a49c1cd655ffa5561753f5b7e1a6f9fd40542da30edc587d"
    );
    assert_eq!(
        p::affected_resources_for(&intent),
        vec![p::ResourceRef("mcp:server:legacy:tool:echo".into())]
    );
    assert!(serde_json::from_str::<p::ActionParameters>(r#"{"UnknownAction":{}}"#).is_err());
}
