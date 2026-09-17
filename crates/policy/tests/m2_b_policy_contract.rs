use forme_policy::{
    ActionMatcher, AppApiConnectorLimit, ArgMatcher, DefaultPolicyEngine, ExternalPolicyLimits,
    PolicyContext, PolicyEngine, PolicyLayer, PolicyLayerSource, PolicyRule,
};
use forme_protocol as p;

fn intent() -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("app-api:policy-test".into()),
        source: p::Source::Communication,
        goal: p::GoalRef("send one bounded response".into()),
        backend_hint: p::BackendKind::AppApi,
        capability_ref: p::CapabilityRef("connector:loopback:request".into()),
        action_type: p::ActionType::Deliver,
        scope: p::Scope("communication:loopback".into()),
        risk_hint: p::Risk::Medium,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("api-mutation-not-retractable".into()),
        parameters: p::ActionParameters::AppApi(p::AppApiActionSpec {
            schema_version: p::SchemaVersion(1),
            connector: p::ProviderId("connector:loopback".into()),
            endpoint: "http://127.0.0.1:38192/api/messages".into(),
            schema_digest: p::SchemaDigest("sha256:loopback-v1".into()),
            credential: Some(p::SecretRef("secret:loopback".into())),
            operation: p::AppApiOperation::Mutation {
                method: p::AppApiMutationMethod::Post,
                body: Some(p::ExternalInput::Content(p::ContentRef(
                    "content:message-1".into(),
                ))),
                idempotency_key: p::IdempotencyKey("message-1".into()),
            },
            timeout: p::DurationMs(2_000),
            participant: Some(p::ParticipantId("external:person-a".into())),
            representation: Some(p::Representation::Agent),
            disclosure_request: Some(p::DisclosureRequestRef("disclosure:message-1".into())),
        }),
        requested_permissions: vec![p::PermissionRef("connector:loopback:use".into())],
        requested_at: 100,
        estimated_output_bytes: 4_096,
        estimated_duration: p::DurationMs(2_000),
    }
}

fn context() -> PolicyContext {
    let engine = DefaultPolicyEngine;
    let policy = engine.merge(&[PolicyLayer {
        schema_version: p::SchemaVersion(1),
        source: PolicyLayerSource::User,
        rules: vec![PolicyRule {
            schema_version: p::SchemaVersion(1),
            matcher: ActionMatcher {
                backend: Some(p::BackendKind::AppApi),
                capability: None,
                action_type: None,
                parameters: ArgMatcher::Any,
            },
            effect: p::PolicyDecision::Ask,
            scope: p::Scope("communication:loopback".into()),
        }],
    }]);
    PolicyContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session:loopback".into()),
        toolset: p::ToolsetRef("toolset:loopback".into()),
        policy,
        visible_capabilities: vec![p::CapabilityRef("connector:loopback:request".into())],
        granted_permissions: vec![p::PermissionRef("connector:loopback:use".into())],
        allowed_scopes: vec![p::Scope("communication:loopback".into())],
        shell_allowlist: Vec::new(),
        file_roots: Vec::new(),
        mcp_allowlist: Vec::new(),
        external: ExternalPolicyLimits {
            schema_version: p::SchemaVersion(1),
            browser_origins: Vec::new(),
            computer_surfaces: Vec::new(),
            pty_programs: Vec::new(),
            pty_roots: Vec::new(),
            app_api_connectors: vec![AppApiConnectorLimit {
                schema_version: p::SchemaVersion(1),
                connector: p::ProviderId("connector:loopback".into()),
                base_url: "http://127.0.0.1:38192/api/".into(),
                schema_digest: p::SchemaDigest("sha256:loopback-v1".into()),
                credential_ref: Some(p::SecretRef("secret:loopback".into())),
                allowed_mutations: vec![p::AppApiMutationMethod::Post],
                max_timeout: p::DurationMs(2_000),
            }],
        },
        network_allowed: true,
        sandbox_available: true,
        delegation: None,
        envelope: None,
    }
}

#[test]
fn s43_app_api_execution_recheck_is_total_and_fails_closed() {
    let engine = DefaultPolicyEngine;
    let valid = intent();
    assert_eq!(
        engine.evaluate_detailed(&context(), &valid).decision,
        p::PolicyDecision::Ask
    );

    let mut cases = Vec::new();
    let mut schema_drift = valid.clone();
    let p::ActionParameters::AppApi(spec) = &mut schema_drift.parameters else {
        panic!("wrong parameters");
    };
    spec.schema_digest = p::SchemaDigest("sha256:changed".into());
    cases.push(schema_drift);

    let mut endpoint_escape = valid.clone();
    let p::ActionParameters::AppApi(spec) = &mut endpoint_escape.parameters else {
        panic!("wrong parameters");
    };
    spec.endpoint = "http://127.0.0.1:38192/other/messages".into();
    cases.push(endpoint_escape);

    let mut credential_drift = valid.clone();
    let p::ActionParameters::AppApi(spec) = &mut credential_drift.parameters else {
        panic!("wrong parameters");
    };
    spec.credential = Some(p::SecretRef("secret:other".into()));
    cases.push(credential_drift);

    let mut impersonation_gap = valid.clone();
    let p::ActionParameters::AppApi(spec) = &mut impersonation_gap.parameters else {
        panic!("wrong parameters");
    };
    spec.representation = None;
    cases.push(impersonation_gap);

    let mut disclosure_gap = valid.clone();
    let p::ActionParameters::AppApi(spec) = &mut disclosure_gap.parameters else {
        panic!("wrong parameters");
    };
    spec.disclosure_request = None;
    cases.push(disclosure_gap);

    let mut unsafe_idempotency = valid;
    let p::ActionParameters::AppApi(spec) = &mut unsafe_idempotency.parameters else {
        panic!("wrong parameters");
    };
    let p::AppApiOperation::Mutation {
        idempotency_key, ..
    } = &mut spec.operation
    else {
        panic!("wrong operation");
    };
    *idempotency_key = p::IdempotencyKey("message-1\r\nforged: true".into());
    cases.push(unsafe_idempotency);

    assert!(cases.into_iter().all(|candidate| {
        engine.evaluate_detailed(&context(), &candidate).decision == p::PolicyDecision::Deny
    }));
}
