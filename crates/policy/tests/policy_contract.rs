use forme_policy::{
    ActionMatcher, ArgMatcher, DefaultPolicyEngine, DelegationGrant, DelegationSubject,
    EnvelopeDecision, PolicyContext, PolicyEngine, PolicyLayer, PolicyLayerSource, PolicyRule,
};
use forme_protocol as p;

#[test]
fn merge_is_ordered_and_later_layers_cannot_relax_a_deny_or_ask() {
    let engine = DefaultPolicyEngine;
    let effective = engine.merge(&[
        layer(PolicyLayerSource::Hook, p::PolicyDecision::Allow),
        layer(PolicyLayerSource::Managed, p::PolicyDecision::Deny),
        layer(PolicyLayerSource::Final, p::PolicyDecision::Allow),
    ]);
    let context = context(effective);

    assert_eq!(
        engine.evaluate(&context, &shell_intent("git", false)),
        p::PolicyDecision::Deny
    );

    let effective = engine.merge(&[
        layer(PolicyLayerSource::Final, p::PolicyDecision::Allow),
        layer(PolicyLayerSource::User, p::PolicyDecision::Ask),
    ]);
    assert_eq!(
        engine.evaluate(
            &context_with_policy(&context, effective),
            &shell_intent("git", false)
        ),
        p::PolicyDecision::Ask
    );
}

#[test]
fn execution_time_recheck_denies_invisible_or_unsafe_parameters_before_rules() {
    let engine = DefaultPolicyEngine;
    let effective = engine.merge(&[layer(PolicyLayerSource::Managed, p::PolicyDecision::Allow)]);
    let mut policy_context = context(effective);
    let safe = shell_intent("git", false);
    assert_eq!(
        engine.evaluate(&policy_context, &safe),
        p::PolicyDecision::Allow
    );

    policy_context.visible_capabilities.clear();
    assert_eq!(
        engine.evaluate(&policy_context, &safe),
        p::PolicyDecision::Deny
    );

    let mut policy_context = context(policy_context.policy.clone());
    policy_context.sandbox_available = false;
    assert_eq!(
        engine.evaluate(&policy_context, &safe),
        p::PolicyDecision::Deny
    );

    let policy_context = context(policy_context.policy.clone());
    assert_eq!(
        engine.evaluate(&policy_context, &shell_intent("unknown-command", false)),
        p::PolicyDecision::Deny
    );
    assert_eq!(
        engine.evaluate(&policy_context, &shell_intent("git", true)),
        p::PolicyDecision::Deny
    );

    let escaped_file = file_intent("C:/outside/secret.txt");
    assert_eq!(
        engine.evaluate(&policy_context, &escaped_file),
        p::PolicyDecision::Deny
    );
    let traversed_file = file_intent("C:/workspace/alpha/../outside.txt");
    assert_eq!(
        engine.evaluate(&policy_context, &traversed_file),
        p::PolicyDecision::Deny
    );
}

#[test]
fn detailed_evaluation_maps_to_the_frozen_policy_event_payload() {
    let engine = DefaultPolicyEngine;
    let effective = engine.merge(&[layer(PolicyLayerSource::Workspace, p::PolicyDecision::Ask)]);
    let evaluation = engine.evaluate_detailed(&context(effective), &shell_intent("git", false));

    assert_eq!(evaluation.decision, p::PolicyDecision::Ask);
    let p::EventPayload::ToolPolicyEvaluated(payload) = evaluation.event_payload() else {
        panic!("policy evaluation must map to ToolPolicyEvaluated");
    };
    assert_eq!(payload.decision, p::PolicyDecision::Ask);
    assert_eq!(payload.rule_source, p::RuleSourceRef("workspace".into()));
    assert!(!payload.reason.0.is_empty());
}

#[test]
fn envelope_enforces_permission_scope_capability_timebox_and_approval() {
    let engine = DefaultPolicyEngine;
    let runtime_envelope = envelope(p::ApprovalRule::Allow);
    let grant = DelegationGrant {
        schema_version: p::SchemaVersion(1),
        subject: DelegationSubject::Subagent(p::RunId("child-run".into())),
        envelope: runtime_envelope.clone(),
        granted_by: p::Actor::Owner,
        audit_ref: p::EventId("grant-event".into()),
    };
    let intent = shell_intent("git", false);

    assert_eq!(
        engine.enforce_envelope(&grant, &runtime_envelope, &intent),
        EnvelopeDecision::Within
    );

    let mut missing_permission = runtime_envelope.clone();
    missing_permission.capability.permissions.clear();
    assert_eq!(
        engine.enforce_envelope(&grant, &missing_permission, &intent),
        EnvelopeDecision::OutOfScope
    );

    let mut wrong_capability = intent.clone();
    wrong_capability.capability_ref = p::CapabilityRef("tool:delete-all".into());
    assert_eq!(
        engine.enforce_envelope(&grant, &runtime_envelope, &wrong_capability),
        EnvelopeDecision::OutOfScope
    );

    let mut expired = runtime_envelope.clone();
    expired.timebox.expires_at = 9;
    assert_eq!(
        engine.enforce_envelope(&grant, &expired, &intent),
        EnvelopeDecision::OutOfScope
    );

    let approval_envelope = envelope(p::ApprovalRule::Ask);
    let approval_grant = DelegationGrant {
        envelope: approval_envelope.clone(),
        ..grant
    };
    assert_eq!(
        engine.enforce_envelope(&approval_grant, &approval_envelope, &intent),
        EnvelopeDecision::NeedsApproval
    );
}

#[test]
fn s11_subagent_cannot_escape_its_envelope() {
    let engine = DefaultPolicyEngine;
    let envelope = envelope(p::ApprovalRule::Allow);
    let grant = DelegationGrant {
        schema_version: p::SchemaVersion(1),
        subject: DelegationSubject::Subagent(p::RunId("child-run".into())),
        envelope: envelope.clone(),
        granted_by: p::Actor::Owner,
        audit_ref: p::EventId("grant-event".into()),
    };
    let mut intent = shell_intent("git", false);
    intent.scope = p::Scope("workspace:other".into());

    assert_eq!(
        engine.enforce_envelope(&grant, &envelope, &intent),
        EnvelopeDecision::OutOfScope
    );
}

fn layer(source: PolicyLayerSource, effect: p::PolicyDecision) -> PolicyLayer {
    PolicyLayer {
        schema_version: p::SchemaVersion(1),
        source,
        rules: vec![PolicyRule {
            schema_version: p::SchemaVersion(1),
            matcher: ActionMatcher {
                parameters: ArgMatcher::Any,
                ..ActionMatcher::default()
            },
            effect,
            scope: p::Scope("workspace:alpha".into()),
        }],
    }
}

fn context(policy: forme_policy::EffectivePolicy) -> PolicyContext {
    PolicyContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session-1".into()),
        toolset: p::ToolsetRef("toolset-1".into()),
        policy,
        visible_capabilities: vec![
            p::CapabilityRef("tool:shell".into()),
            p::CapabilityRef("tool:file".into()),
        ],
        granted_permissions: vec![p::PermissionRef("execute".into())],
        allowed_scopes: vec![p::Scope("workspace:alpha".into())],
        shell_allowlist: vec!["git".into()],
        file_roots: vec!["C:/workspace/alpha".into()],
        mcp_allowlist: Vec::new(),
        external: forme_policy::ExternalPolicyLimits::default(),
        network_allowed: false,
        sandbox_available: true,
        delegation: None,
        envelope: None,
    }
}

fn context_with_policy(
    context: &PolicyContext,
    policy: forme_policy::EffectivePolicy,
) -> PolicyContext {
    PolicyContext {
        policy,
        ..context.clone()
    }
}

#[test]
fn s38_s40_external_parameter_recheck_is_fail_closed() {
    let engine = DefaultPolicyEngine;
    let browser = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("policy:browser".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("read page".into()),
        backend_hint: p::BackendKind::Browser,
        capability_ref: p::CapabilityRef("tool:browser".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:alpha".into()),
        risk_hint: p::Risk::Medium,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Browser(p::BrowserActionSpec {
            schema_version: p::SchemaVersion(1),
            driver: p::ProviderId("driver:browser".into()),
            target_url: "http://127.0.0.1:34001/task".into(),
            allowed_origins: vec!["http://127.0.0.1:34001".into()],
            operation: p::BrowserOperation::ReadText { selector: None },
            artifact_scope: p::Scope("workspace:alpha".into()),
        }),
        requested_permissions: vec![p::PermissionRef("execute".into())],
        requested_at: 10,
        estimated_output_bytes: 512,
        estimated_duration: p::DurationMs(100),
    };
    let mut allowed =
        context(engine.merge(&[layer(PolicyLayerSource::User, p::PolicyDecision::Allow)]));
    allowed
        .visible_capabilities
        .push(browser.capability_ref.clone());
    allowed.external.browser_origins = vec!["http://127.0.0.1:34001".into()];
    assert_eq!(
        engine.evaluate(&allowed, &browser),
        p::PolicyDecision::Allow
    );

    let mut bad_scheme = browser.clone();
    let p::ActionParameters::Browser(spec) = &mut bad_scheme.parameters else {
        unreachable!()
    };
    spec.target_url = "javascript:alert(1)".into();
    assert_eq!(
        engine.evaluate(&allowed, &bad_scheme),
        p::PolicyDecision::Deny
    );

    let mut wrong_origin = browser;
    let p::ActionParameters::Browser(spec) = &mut wrong_origin.parameters else {
        unreachable!()
    };
    spec.target_url = "http://127.0.0.1:34002/task".into();
    assert_eq!(
        engine.evaluate(&allowed, &wrong_origin),
        p::PolicyDecision::Deny
    );

    let mut computer = wrong_origin;
    computer.backend_hint = p::BackendKind::Computer;
    computer.capability_ref = p::CapabilityRef("tool:computer".into());
    computer.parameters = p::ActionParameters::Computer(p::ComputerActionSpec {
        schema_version: p::SchemaVersion(1),
        driver: p::ProviderId("driver:computer".into()),
        surface: p::SurfaceRef("surface:isolated".into()),
        bounds: p::CoordinateBounds {
            schema_version: p::SchemaVersion(1),
            min_x: 0,
            min_y: 0,
            max_x_exclusive: 100,
            max_y_exclusive: 100,
        },
        operation: p::ComputerOperation::Click {
            x: 101,
            y: 20,
            button: p::PointerButton::Primary,
        },
        artifact_scope: p::Scope("workspace:alpha".into()),
    });
    allowed
        .visible_capabilities
        .push(computer.capability_ref.clone());
    allowed.external.computer_surfaces = vec![forme_policy::ComputerSurfaceLimit {
        schema_version: p::SchemaVersion(1),
        surface: p::SurfaceRef("surface:isolated".into()),
        bounds: p::CoordinateBounds {
            schema_version: p::SchemaVersion(1),
            min_x: 0,
            min_y: 0,
            max_x_exclusive: 100,
            max_y_exclusive: 100,
        },
    }];
    assert_eq!(
        engine.evaluate(&allowed, &computer),
        p::PolicyDecision::Deny
    );

    let mut pty = computer;
    pty.backend_hint = p::BackendKind::Pty;
    pty.capability_ref = p::CapabilityRef("tool:pty".into());
    pty.parameters = p::ActionParameters::Pty(p::PtyActionSpec {
        schema_version: p::SchemaVersion(1),
        program: "C:/fixture/pty.exe".into(),
        args: Vec::new(),
        cwd: "C:/fixture".into(),
        cols: 80,
        rows: 24,
        input: None,
        environment: vec![p::SecretBinding {
            schema_version: p::SchemaVersion(1),
            name: "BAD-NAME".into(),
            value: p::SecretRef("secret:fixture".into()),
        }],
    });
    allowed
        .visible_capabilities
        .push(pty.capability_ref.clone());
    allowed.external.pty_programs = vec!["C:/fixture/pty.exe".into()];
    allowed.external.pty_roots = vec!["C:/fixture".into()];
    assert_eq!(engine.evaluate(&allowed, &pty), p::PolicyDecision::Deny);
}

fn shell_intent(program: &str, network: bool) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId(format!("intent-{program}")),
        source: p::Source::UserTurn,
        goal: p::GoalRef("inspect repository".into()),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("tool:shell".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:alpha".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: program.into(),
            args: vec!["status".into()],
            cwd: Some("C:/workspace/alpha".into()),
            network,
        },
        requested_permissions: vec![p::PermissionRef("execute".into())],
        requested_at: 10,
        estimated_output_bytes: 512,
        estimated_duration: p::DurationMs(100),
    }
}

fn file_intent(path: &str) -> p::ActionIntent {
    p::ActionIntent {
        intent_id: p::ActionId("intent-file".into()),
        goal: p::GoalRef("write file".into()),
        backend_hint: p::BackendKind::File,
        capability_ref: p::CapabilityRef("tool:file".into()),
        action_type: p::ActionType::Write,
        parameters: p::ActionParameters::File {
            operation: p::FileOperation::Write,
            path: path.into(),
            content: Some(b"content".to_vec()),
        },
        ..shell_intent("git", false)
    }
}

fn envelope(approval_rule: p::ApprovalRule) -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:alpha".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef("tool:shell".into())],
            permissions: vec![p::PermissionRef("execute".into())],
        },
        action_type: vec![p::ActionType::Execute],
        risk_limit: p::Risk::Low,
        approval_rule,
        budget: p::Budget("m0-bounded".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 0,
            expires_at: 100,
            max_turns: 3,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}
