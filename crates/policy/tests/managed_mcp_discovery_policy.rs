use forme_policy::{
    ActionMatcher, ArgMatcher, DefaultPolicyEngine, PolicyContext, PolicyEngine, PolicyLayer,
    PolicyLayerSource, PolicyRule,
};
use forme_protocol as p;

fn context(intent: &p::ActionIntent, explicit: bool) -> PolicyContext {
    let mut layers = vec![PolicyLayer {
        schema_version: p::SchemaVersion(1),
        source: PolicyLayerSource::User,
        rules: vec![PolicyRule {
            schema_version: p::SchemaVersion(1),
            matcher: ActionMatcher::default(),
            effect: p::PolicyDecision::Allow,
            scope: p::Scope("*".into()),
        }],
    }];
    if explicit {
        layers.push(PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::Session,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                scope: intent.scope.clone(),
                effect: p::PolicyDecision::Ask,
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::Mcp),
                    capability: Some(intent.capability_ref.clone()),
                    action_type: Some(p::ActionType::Execute),
                    parameters: ArgMatcher::McpDiscoveryTarget {
                        server: p::McpServerRef("fixture:catalog".into()),
                        program_ref: "program:fixture".into(),
                        action_digest: p::normalized_action_digest(intent).unwrap(),
                    },
                },
            }],
        });
    }
    PolicyContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session:test".into()),
        toolset: p::ToolsetRef("toolset:test".into()),
        policy: DefaultPolicyEngine.merge(&layers),
        visible_capabilities: vec![intent.capability_ref.clone()],
        granted_permissions: intent.requested_permissions.clone(),
        allowed_scopes: vec![intent.scope.clone()],
        shell_allowlist: vec![],
        file_roots: vec![],
        mcp_allowlist: vec!["fixture:catalog".into()],
        external: Default::default(),
        network_allowed: false,
        sandbox_available: true,
        delegation: None,
        envelope: None,
    }
}
#[test]
fn discovery_needs_an_exact_owner_request_and_always_asks_for_approval() {
    let i = intent();
    let ctx = context(&i, true);
    assert_eq!(
        DefaultPolicyEngine.evaluate(&ctx, &i),
        p::PolicyDecision::Ask
    );
    let evaluation = DefaultPolicyEngine.evaluate_detailed(&ctx, &i);
    assert!(
        matches!(evaluation.event_payload(), p::EventPayload::ToolPolicyEvaluated(v) if v.decision == p::PolicyDecision::Ask)
    );
    assert_eq!(
        DefaultPolicyEngine.evaluate(&context(&i, false), &i),
        p::PolicyDecision::Deny
    );
    let mut denied = ctx.clone();
    denied.sandbox_available = false;
    assert_eq!(
        DefaultPolicyEngine.evaluate(&denied, &i),
        p::PolicyDecision::Deny
    );
    denied = ctx.clone();
    denied.granted_permissions.clear();
    assert_eq!(
        DefaultPolicyEngine.evaluate(&denied, &i),
        p::PolicyDecision::Deny
    );
    denied = ctx.clone();
    denied.allowed_scopes.clear();
    assert_eq!(
        DefaultPolicyEngine.evaluate(&denied, &i),
        p::PolicyDecision::Deny
    );
    let mut layers = ctx.policy.layers().to_vec();
    layers[0].rules[0].effect = p::PolicyDecision::Deny;
    denied = ctx;
    denied.policy = DefaultPolicyEngine.merge(&layers);
    assert_eq!(
        DefaultPolicyEngine.evaluate(&denied, &i),
        p::PolicyDecision::Deny
    );
}
#[test]
fn generic_or_tool_rules_cannot_be_promoted_to_a_discovery_request() {
    let i = intent();
    let valid = context(&i, true);
    let tool = ArgMatcher::McpTarget {
        server: p::McpServerRef("fixture:catalog".into()),
        tool: p::ToolRef("echo".into()),
    };
    assert!(!tool.matches(&i));
    for matcher in [ArgMatcher::Any, tool] {
        let mut ctx = valid.clone();
        let mut layers = ctx.policy.layers().to_vec();
        layers[1].rules[0].matcher.parameters = matcher;
        ctx.policy = DefaultPolicyEngine.merge(&layers);
        assert_eq!(
            DefaultPolicyEngine.evaluate(&ctx, &i),
            p::PolicyDecision::Deny
        );
    }
    for source in [
        PolicyLayerSource::User,
        PolicyLayerSource::Agent,
        PolicyLayerSource::SkillToolPlugin,
        PolicyLayerSource::Hook,
    ] {
        let mut ctx = valid.clone();
        let mut layers = ctx.policy.layers().to_vec();
        layers[1].source = source;
        ctx.policy = DefaultPolicyEngine.merge(&layers);
        assert_eq!(
            DefaultPolicyEngine.evaluate(&ctx, &i),
            p::PolicyDecision::Deny
        );
    }
    let mut ctx = valid;
    let mut layers = ctx.policy.layers().to_vec();
    layers[1].rules[0].effect = p::PolicyDecision::Allow;
    ctx.policy = DefaultPolicyEngine.merge(&layers);
    assert_eq!(
        DefaultPolicyEngine.evaluate(&ctx, &i),
        p::PolicyDecision::Deny
    );
}
#[test]
fn request_binding_rejects_mutation_replay_and_model_sources() {
    let original = intent();
    let ctx = context(&original, true);
    let mut changed = original.clone();
    changed.intent_id.0 = "action:another".into();
    assert_eq!(
        DefaultPolicyEngine.evaluate(&ctx, &changed),
        p::PolicyDecision::Deny
    );
    for source in [
        p::Source::UserTurn,
        p::Source::Internal,
        p::Source::Subagent,
        p::Source::ProactiveJob,
        p::Source::Replay,
    ] {
        let mut changed = original.clone();
        changed.source = source;
        assert_eq!(
            DefaultPolicyEngine.evaluate(&context(&changed, true), &changed),
            p::PolicyDecision::Deny
        );
    }
    for n in 0..4 {
        let mut changed = original.clone();
        let p::ActionParameters::McpDiscovery(spec) = &mut changed.parameters else {
            unreachable!()
        };
        match n {
            0 => spec.registration_digest = p::canonical_digest(&"replaced").unwrap(),
            1 => spec.settings_digest = p::canonical_digest(&"replaced").unwrap(),
            2 => spec.timeout.0 += 1,
            _ => spec.output_budget_bytes += 1,
        }
        assert_eq!(
            DefaultPolicyEngine.evaluate(&ctx, &changed),
            p::PolicyDecision::Deny
        );
    }
}

#[test]
fn owner_envelope_asking_does_not_mask_a_discovery_deny_and_cannot_be_delegated() {
    let i = intent();
    let mut ctx = context(&i, true);
    let envelope = p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: i.scope.clone(),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![i.capability_ref.clone()],
            permissions: i.requested_permissions.clone(),
        },
        action_type: vec![p::ActionType::Execute],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget("units:1".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 9_000,
            expires_at: 20_000,
            max_turns: 1,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    };
    ctx.envelope = Some(envelope.clone());
    ctx.delegation = Some(forme_policy::DelegationGrant {
        schema_version: p::SchemaVersion(1),
        subject: forme_policy::DelegationSubject::Owner,
        envelope,
        granted_by: p::Actor::Owner,
        audit_ref: p::EventId("event:grant".into()),
    });
    assert_eq!(
        DefaultPolicyEngine.evaluate(&ctx, &i),
        p::PolicyDecision::Ask
    );
    let mut delegated = ctx.clone();
    delegated.delegation.as_mut().unwrap().subject = forme_policy::DelegationSubject::Agent;
    assert_eq!(
        DefaultPolicyEngine.evaluate(&delegated, &i),
        p::PolicyDecision::Deny
    );
    let mut layers = ctx.policy.layers().to_vec();
    layers[0].rules[0].effect = p::PolicyDecision::Deny;
    ctx.policy = DefaultPolicyEngine.merge(&layers);
    assert_eq!(
        DefaultPolicyEngine.evaluate(&ctx, &i),
        p::PolicyDecision::Deny
    );
}

fn intent() -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:discover".into()),
        source: p::Source::OwnerControl,
        goal: p::GoalRef("goal:discover".into()),
        backend_hint: p::BackendKind::Mcp,
        capability_ref: p::CapabilityRef("mcp-discovery:fixture:catalog".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:test".into()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::McpDiscovery(p::ManagedMcpDiscoverySpec {
            schema_version: p::SchemaVersion(1),
            server: p::McpServerRef("fixture:catalog".into()),
            program_ref: "program:fixture".into(),
            registration_digest: p::canonical_digest(&"registration").unwrap(),
            settings_digest: p::canonical_digest(&"settings").unwrap(),
            timeout: p::DurationMs(1000),
            output_budget_bytes: 4096,
        }),
        requested_permissions: vec![p::PermissionRef("mcp:discovery".into())],
        requested_at: 10_000,
        estimated_output_bytes: 4096,
        estimated_duration: p::DurationMs(1000),
    }
}
