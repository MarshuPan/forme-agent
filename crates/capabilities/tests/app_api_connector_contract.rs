use forme_capabilities::{
    AppApiConnectorRegistry, AppApiRequest, CapabilityRegistry, ExecutionCapabilityRechecker,
    InMemoryCapabilityRegistry, ProjectAppApiConnector,
};
use forme_policy::{DefaultPolicyEngine, ExternalPolicyLimits, PolicyContext, PolicyEngine};
use forme_protocol as p;

fn connector() -> ProjectAppApiConnector {
    ProjectAppApiConnector::new(
        p::ProviderId("connector:project-loopback".into()),
        "http://127.0.0.1:38191/api/".into(),
        p::SchemaDigest("sha256:project-schema-v1".into()),
        Some(p::SecretRef("secret:project-loopback".into())),
        p::Scope("communication:project".into()),
        p::CapabilityRef("connector:project-loopback:request".into()),
        p::PermissionRef("connector:project-loopback:use".into()),
        vec![p::AppApiMutationMethod::Post],
        2,
        p::DurationMs(2_000),
    )
    .unwrap()
}

fn envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("communication:project".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef(
                "connector:project-loopback:request".into(),
            )],
            permissions: vec![p::PermissionRef("connector:project-loopback:use".into())],
        },
        action_type: vec![
            p::ActionType::Observe,
            p::ActionType::Execute,
            p::ActionType::Deliver,
            p::ActionType::ExternalCommit,
        ],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget("tool_calls=4".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 0,
            expires_at: 10_000,
            max_turns: 4,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

fn resolve_context() -> p::ResolveContext {
    p::ResolveContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session:connector".into()),
        toolset: p::ToolsetRef("toolset:connector".into()),
        envelope: envelope(),
        policy_allowed_providers: vec![p::ProviderId("connector:project-loopback".into())],
        policy_allowed_capabilities: vec![p::CapabilityRef(
            "connector:project-loopback:request".into(),
        )],
    }
}

fn policy_context() -> PolicyContext {
    let engine = DefaultPolicyEngine;
    PolicyContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session:connector".into()),
        toolset: p::ToolsetRef("toolset:connector".into()),
        policy: engine.merge(&[]),
        visible_capabilities: vec![p::CapabilityRef(
            "connector:project-loopback:request".into(),
        )],
        granted_permissions: vec![p::PermissionRef("connector:project-loopback:use".into())],
        allowed_scopes: vec![p::Scope("communication:project".into())],
        shell_allowlist: Vec::new(),
        file_roots: Vec::new(),
        mcp_allowlist: Vec::new(),
        external: ExternalPolicyLimits::default(),
        network_allowed: true,
        sandbox_available: true,
        delegation: None,
        envelope: Some(envelope()),
    }
}

fn request(operation: p::AppApiOperation, action_type: p::ActionType) -> AppApiRequest {
    let disclosure_request = (action_type == p::ActionType::Deliver)
        .then(|| p::DisclosureRequestRef("disclosure:message-1".into()));
    AppApiRequest {
        schema_version: p::SchemaVersion(1),
        resource: "items".into(),
        operation,
        source: p::Source::Communication,
        goal: p::GoalRef("exchange one bounded project message".into()),
        action_type,
        risk: p::Risk::Medium,
        participant: Some(p::ParticipantId("external:person-a".into())),
        representation: Some(p::Representation::Agent),
        disclosure_request,
        requested_at: 100,
        estimated_output_bytes: 4_096,
    }
}

#[test]
fn s43_connector_lifecycle_indexes_only_active_identity_and_prepares_typed_actions() {
    let id = p::ProviderId("connector:project-loopback".into());
    let registry = AppApiConnectorRegistry::with_connectors(vec![connector()]).unwrap();
    assert!(registry
        .prepare(
            id.clone(),
            request(p::AppApiOperation::Read, p::ActionType::Observe)
        )
        .is_err());

    registry.configure(id.clone()).unwrap();
    registry.enable(id.clone()).unwrap();
    registry
        .bind_trust(id.clone(), p::TrustTier::ApprovedSource, p::Actor::Owner)
        .unwrap();
    registry.grant(id.clone(), envelope()).unwrap();

    let capabilities = InMemoryCapabilityRegistry::default();
    let context = resolve_context();
    registry.index_active(&capabilities, &context).unwrap();
    let toolset = capabilities.resolve_toolset(&context).unwrap();
    assert_eq!(toolset.items.len(), 1);
    assert_eq!(
        capabilities
            .take_events()
            .iter()
            .map(p::EventPayload::kind)
            .collect::<Vec<_>>(),
        vec![
            p::EventKind::CapabilityIndexed,
            p::EventKind::ToolsetResolved
        ]
    );

    let read = registry
        .prepare(
            id.clone(),
            request(p::AppApiOperation::Read, p::ActionType::Observe),
        )
        .unwrap();
    assert_eq!(read.backend_hint, p::BackendKind::AppApi);
    assert_eq!(read.expected_effect, p::ExpectedEffect::Outward);
    let mutation = registry
        .prepare(
            id,
            request(
                p::AppApiOperation::Mutation {
                    method: p::AppApiMutationMethod::Post,
                    body: Some(p::ExternalInput::Content(p::ContentRef(
                        "content:message-1".into(),
                    ))),
                    idempotency_key: p::IdempotencyKey("message-1".into()),
                },
                p::ActionType::Deliver,
            ),
        )
        .unwrap();
    let p::ActionParameters::AppApi(spec) = mutation.parameters else {
        panic!("connector prepared the wrong backend parameters");
    };
    assert_eq!(spec.credential.unwrap().0, "secret:project-loopback");
    assert_eq!(spec.endpoint, "http://127.0.0.1:38191/api/items");

    assert_eq!(
        registry
            .take_provider_transitions()
            .iter()
            .map(|transition| transition.to.clone())
            .collect::<Vec<_>>(),
        vec![
            p::ProviderLifecycle::Discovered,
            p::ProviderLifecycle::Configured,
            p::ProviderLifecycle::Enabled,
            p::ProviderLifecycle::Trusted(p::TrustTier::ApprovedSource),
        ]
    );
}

#[test]
fn s43_schema_drift_and_revocation_fail_the_execution_time_recheck() {
    let id = p::ProviderId("connector:project-loopback".into());
    let registry = AppApiConnectorRegistry::with_connectors(vec![connector()]).unwrap();
    registry.configure(id.clone()).unwrap();
    registry.enable(id.clone()).unwrap();
    registry
        .bind_trust(id.clone(), p::TrustTier::ApprovedSource, p::Actor::Owner)
        .unwrap();
    registry.grant(id.clone(), envelope()).unwrap();
    let intent = registry
        .prepare(
            id.clone(),
            request(p::AppApiOperation::Read, p::ActionType::Observe),
        )
        .unwrap();
    registry.recheck(&intent, &policy_context()).unwrap();

    let mut drifted = intent.clone();
    let p::ActionParameters::AppApi(spec) = &mut drifted.parameters else {
        panic!("connector prepared the wrong backend parameters");
    };
    spec.schema_digest = p::SchemaDigest("sha256:changed".into());
    assert!(registry.recheck(&drifted, &policy_context()).is_err());

    registry
        .revoke(id.clone(), p::LineageRef("owner:revoke-connector".into()))
        .unwrap();
    assert!(registry.recheck(&intent, &policy_context()).is_err());
    assert!(registry
        .prepare(
            id,
            request(p::AppApiOperation::Read, p::ActionType::Observe)
        )
        .is_err());
    assert_eq!(
        registry
            .take_events()
            .iter()
            .map(p::EventPayload::kind)
            .collect::<Vec<_>>(),
        vec![p::EventKind::RevocationEvent]
    );
}
