use forme_approval::{
    ApprovalAuthorization, ApprovalBroker, ApprovalGrant, ApprovalRequest, ApprovalScope,
    ApprovalTicket, GrantScope, InMemoryApprovalBroker,
};
use forme_policy::{
    ActionMatcher, ArgMatcher, DefaultPolicyEngine, PolicyContext, PolicyEngine, PolicyLayer,
    PolicyLayerSource, PolicyRule,
};
use forme_protocol as p;

#[test]
fn request_resolve_pending_and_events_are_gateway_neutral() {
    let broker = InMemoryApprovalBroker::with_clock(|| 10);
    let request = request("approval-1");
    let ticket = broker.request(request.clone()).unwrap();
    assert_eq!(
        broker.pending(ApprovalScope::Session(p::SessionId("session-1".into()))),
        vec![request]
    );

    broker.resolve(ticket, denied_grant("approval-1")).unwrap();
    assert!(broker.pending(ApprovalScope::All).is_empty());

    let events = broker.take_events();
    assert!(matches!(events[0], p::EventPayload::ApprovalRequested(_)));
    let p::EventPayload::ApprovalResolved(resolved) = &events[1] else {
        panic!("second event must resolve approval");
    };
    assert_eq!(resolved.outcome, p::ApprovalOutcome::Denied);
    assert!(resolved.grant_ref.is_none());
    assert!(events.iter().all(|event| !matches!(
        event,
        p::EventPayload::ActionStarted(_) | p::EventPayload::ActionCompleted(_)
    )));
}

#[test]
fn s2_ask_then_deny_emits_the_governance_sequence_without_action_events() {
    let engine = DefaultPolicyEngine;
    let intent = shell_intent("git");
    let ask = engine.evaluate_detailed(&policy_context(&engine, p::PolicyDecision::Ask), &intent);
    assert_eq!(ask.decision, p::PolicyDecision::Ask);

    let broker = InMemoryApprovalBroker::with_clock(|| 10);
    let ticket = broker.request(request("approval-s2")).unwrap();
    broker.resolve(ticket, denied_grant("approval-s2")).unwrap();

    let mut events = vec![ask.event_payload()];
    events.extend(broker.take_events());
    assert_eq!(
        events.iter().map(p::EventPayload::kind).collect::<Vec<_>>(),
        vec![
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::ApprovalRequested,
            p::EventKind::ApprovalResolved,
        ]
    );
    assert!(events.iter().all(|event| !matches!(
        event,
        p::EventPayload::ActionStarted(_) | p::EventPayload::ActionCompleted(_)
    )));

    let direct_deny =
        engine.evaluate_detailed(&policy_context(&engine, p::PolicyDecision::Deny), &intent);
    assert_eq!(direct_deny.decision, p::PolicyDecision::Deny);
    assert_eq!(
        direct_deny.event_payload().kind(),
        p::EventKind::ToolPolicyEvaluated
    );
    assert!(broker.pending(ApprovalScope::All).is_empty());
}

#[test]
fn grant_binds_approver_digest_policy_schema_nonce_and_expiry() {
    let broker = InMemoryApprovalBroker::with_clock(|| 10);

    for (label, mutate) in [
        ("digest", mutate_digest as fn(&mut ApprovalGrant)),
        ("policy", mutate_policy),
        ("tool-schema", mutate_tool_schema),
        ("approver", mutate_approver),
        ("expiry", mutate_expiry),
    ] {
        let id = format!("approval-{label}");
        let ticket = broker.request(request(&id)).unwrap();
        let mut grant = granted(&id, GrantScope::OneShot);
        mutate(&mut grant);
        assert!(broker.resolve(ticket, grant).is_err(), "{label} must bind");
    }
}

#[test]
fn one_shot_is_consumed_once_and_post_approval_plan_change_is_rejected() {
    let broker = InMemoryApprovalBroker::with_clock(|| 10);
    let ticket = broker.request(request("approval-one-shot")).unwrap();
    broker
        .resolve(ticket, granted("approval-one-shot", GrantScope::OneShot))
        .unwrap();

    let one_shot_authorization = authorization("approval-one-shot");
    broker.authorize(&one_shot_authorization).unwrap();
    assert!(broker.authorize(&one_shot_authorization).is_err());

    let ticket = broker.request(request("approval-mutated-plan")).unwrap();
    broker
        .resolve(
            ticket,
            granted("approval-mutated-plan", GrantScope::OneShot),
        )
        .unwrap();
    let mut changed = authorization("approval-mutated-plan");
    changed.plan_digest = p::PlanDigest("sha256:changed".into());
    assert!(broker.authorize(&changed).is_err());
}

#[test]
fn session_and_param_pattern_grants_remain_bounded() {
    let broker = InMemoryApprovalBroker::with_clock(|| 10);
    let ticket = broker.request(request("approval-session")).unwrap();
    broker
        .resolve(ticket, granted("approval-session", GrantScope::Session))
        .unwrap();
    let session = authorization("approval-session");
    broker.authorize(&session).unwrap();
    broker.authorize(&session).unwrap();
    let mut other_session = session.clone();
    other_session.session = p::SessionId("session-other".into());
    assert!(broker.authorize(&other_session).is_err());

    let ticket = broker.request(request("approval-pattern")).unwrap();
    broker
        .resolve(
            ticket,
            granted(
                "approval-pattern",
                GrantScope::ParamPattern(ArgMatcher::ShellProgram("git".into())),
            ),
        )
        .unwrap();
    let pattern = authorization("approval-pattern");
    broker.authorize(&pattern).unwrap();
    let mut changed = pattern;
    changed.intent = shell_intent("powershell");
    assert!(broker.authorize(&changed).is_err());
}

#[test]
fn expired_request_or_grant_never_authorizes_and_nonce_cannot_be_replayed() {
    let broker = InMemoryApprovalBroker::with_clock(|| 10);
    let mut expired_request = request("approval-expired-request");
    expired_request.expires_at = 9;
    assert!(broker.request(expired_request).is_err());

    let ticket = broker.request(request("approval-expired-use")).unwrap();
    let mut expired_grant = granted("approval-expired-use", GrantScope::OneShot);
    expired_grant.use_by = 9;
    assert!(broker.resolve(ticket, expired_grant).is_err());

    let ticket = broker.request(request("approval-nonce-a")).unwrap();
    broker
        .resolve(ticket, granted("approval-nonce-a", GrantScope::OneShot))
        .unwrap();
    let ticket = broker.request(request("approval-nonce-b")).unwrap();
    let mut replay = granted("approval-nonce-b", GrantScope::OneShot);
    replay.nonce = p::Nonce("nonce-approval-nonce-a".into());
    assert!(broker.resolve(ticket, replay).is_err());
}

fn request(id: &str) -> ApprovalRequest {
    ApprovalRequest {
        schema_version: p::SchemaVersion(1),
        approval_id: p::ApprovalId(id.into()),
        session: p::SessionId("session-1".into()),
        action_summary: "run git status".into(),
        risk_level: p::Risk::High,
        scope: p::Scope("workspace:alpha".into()),
        requested_permissions: vec![p::PermissionRef("execute".into())],
        affected_resources: vec![p::ResourceRef("repository".into())],
        rollback_boundary: p::RollbackBoundary("none".into()),
        expires_at: 100,
        choices: vec![p::ApprovalChoice("approve-once".into())],
        plan_digest: p::PlanDigest("sha256:approved".into()),
        policy_version: p::Version(7),
        tool_schema_version: p::Version(11),
    }
}

fn granted(id: &str, granted_scope: GrantScope) -> ApprovalGrant {
    ApprovalGrant {
        schema_version: p::SchemaVersion(1),
        approval_id: p::ApprovalId(id.into()),
        outcome: p::ApprovalOutcome::Granted,
        granted_scope,
        approver: p::VerifiedPrincipal("owner".into()),
        bound_plan_digest: p::PlanDigest("sha256:approved".into()),
        policy_version: p::Version(7),
        tool_schema_version: p::Version(11),
        nonce: p::Nonce(format!("nonce-{id}")),
        use_by: 100,
    }
}

fn denied_grant(id: &str) -> ApprovalGrant {
    ApprovalGrant {
        outcome: p::ApprovalOutcome::Denied,
        ..granted(id, GrantScope::OneShot)
    }
}

fn authorization(id: &str) -> ApprovalAuthorization {
    ApprovalAuthorization {
        approval_id: p::ApprovalId(id.into()),
        session: p::SessionId("session-1".into()),
        scope: p::Scope("workspace:alpha".into()),
        plan_digest: p::PlanDigest("sha256:approved".into()),
        policy_version: p::Version(7),
        tool_schema_version: p::Version(11),
        now: 10,
        intent: shell_intent("git"),
    }
}

fn shell_intent(program: &str) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId(format!("intent-{program}")),
        source: p::Source::UserTurn,
        goal: p::GoalRef("inspect repository".into()),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("tool:shell".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:alpha".into()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: program.into(),
            args: vec!["status".into()],
            cwd: Some("C:/workspace/alpha".into()),
            network: false,
        },
        requested_permissions: vec![p::PermissionRef("execute".into())],
        requested_at: 10,
        estimated_output_bytes: 512,
        estimated_duration: p::DurationMs(100),
    }
}

fn policy_context(engine: &DefaultPolicyEngine, decision: p::PolicyDecision) -> PolicyContext {
    PolicyContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session-1".into()),
        toolset: p::ToolsetRef("toolset-1".into()),
        policy: engine.merge(&[PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::Managed,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                matcher: ActionMatcher::default(),
                effect: decision,
                scope: p::Scope("workspace:alpha".into()),
            }],
        }]),
        visible_capabilities: vec![p::CapabilityRef("tool:shell".into())],
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

fn mutate_digest(grant: &mut ApprovalGrant) {
    grant.bound_plan_digest = p::PlanDigest("sha256:other".into());
}

fn mutate_policy(grant: &mut ApprovalGrant) {
    grant.policy_version = p::Version(8);
}

fn mutate_tool_schema(grant: &mut ApprovalGrant) {
    grant.tool_schema_version = p::Version(12);
}

fn mutate_approver(grant: &mut ApprovalGrant) {
    grant.approver = p::VerifiedPrincipal(String::new());
}

fn mutate_expiry(grant: &mut ApprovalGrant) {
    grant.use_by = 101;
}

#[allow(dead_code)]
fn _ticket_shape(ticket: ApprovalTicket) -> p::ApprovalId {
    ticket.0
}

#[allow(dead_code)]
fn _source_shape(source: PolicyLayerSource) -> PolicyLayerSource {
    source
}
