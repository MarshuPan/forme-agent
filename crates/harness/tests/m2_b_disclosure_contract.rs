use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use forme_harness::{
    AgentHarness, HarnessActionIngress, HarnessIngress, IngressAuthority, IngressEvent,
    ProjectAppApiRuntimeConfig, ReactiveHarness,
};
use forme_protocol as p;

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn envelope(now: p::Timestamp) -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("communication:disclosure-test".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef("connector:disclosure-test:deliver".into())],
            permissions: vec![p::PermissionRef("connector:disclosure-test:use".into())],
        },
        action_type: vec![p::ActionType::Deliver],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget("units:1".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: now.saturating_sub(1_000),
            expires_at: now.saturating_add(60_000),
            max_turns: 1,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

fn intent(now: p::Timestamp) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("communication-delivery:message-1".into()),
        source: p::Source::Communication,
        goal: p::GoalRef("send one governed response".into()),
        backend_hint: p::BackendKind::AppApi,
        capability_ref: p::CapabilityRef("connector:disclosure-test:deliver".into()),
        action_type: p::ActionType::Deliver,
        scope: p::Scope("communication:disclosure-test".into()),
        risk_hint: p::Risk::Medium,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("external-message-not-retractable".into()),
        parameters: p::ActionParameters::AppApi(p::AppApiActionSpec {
            schema_version: p::SchemaVersion(1),
            connector: p::ProviderId("connector:disclosure-test".into()),
            endpoint: "http://127.0.0.1:9/api/messages".into(),
            schema_digest: p::SchemaDigest("sha256:disclosure-test-v1".into()),
            credential: None,
            operation: p::AppApiOperation::Mutation {
                method: p::AppApiMutationMethod::Post,
                body: Some(p::ExternalInput::Content(p::ContentRef(
                    "content:message-1".into(),
                ))),
                idempotency_key: p::IdempotencyKey("communication:message-1".into()),
            },
            timeout: p::DurationMs(1_000),
            participant: Some(p::ParticipantId("external:person-a".into())),
            representation: Some(p::Representation::Agent),
            disclosure_request: Some(p::DisclosureRequestRef("disclosure:message-1".into())),
        }),
        requested_permissions: vec![p::PermissionRef("connector:disclosure-test:use".into())],
        requested_at: now,
        estimated_output_bytes: 1_024,
        estimated_duration: p::DurationMs(1_000),
    }
}

fn request() -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::Communication,
        session: p::SessionRef("communication:disclosure-test".into()),
        agent_profile: p::AgentProfileRef("agent:forme".into()),
        input: p::RunInput("deliver one approved external message".into()),
        budget: Some(p::Budget("units:1".into())),
        idempotency_key: Some(p::IdempotencyKey("external-delivery:message-1".into())),
    }
}

fn disclosure(
    authority: &IngressAuthority,
    request: &str,
    outcome: p::DisclosureOutcome,
    representation: p::Representation,
    trust: p::TrustTier,
    binding: Option<p::DisclosureBinding>,
) -> IngressEvent {
    authority.stamp(
        p::EventPayload::DisclosurePolicyApplied(p::DisclosurePolicyAppliedPayload {
            request: p::DisclosureRequestRef(request.into()),
            outcome,
            representation,
            binding,
        }),
        p::Provenance {
            source: p::Source::Communication,
            actor: p::Actor::System,
            trust_tier: trust,
            caused_by: None,
        },
    )
}

fn exact_binding() -> p::DisclosureBinding {
    p::DisclosureBinding {
        schema_version: p::SchemaVersion(1),
        session: p::CommunicationSessionId("communication:disclosure-test".into()),
        participant: p::ParticipantId("external:person-a".into()),
        purpose: p::PurposeRef("send one governed response".into()),
        content_ref: p::ContentRef("content:message-1".into()),
        category: "public".into(),
        sensitive: false,
        confirmed: true,
        high_impact: false,
    }
}

#[test]
fn s45_harness_requires_exact_allowed_disclosure_bound_into_the_action_plan() {
    let now = now_ms();
    let envelope = envelope(now);
    let harness = Arc::new(
        ReactiveHarness::project_owned_app_api(ProjectAppApiRuntimeConfig {
            schema_version: p::SchemaVersion(1),
            connector: p::ProviderId("connector:disclosure-test".into()),
            base_url: "http://127.0.0.1:9/api/".into(),
            schema_digest: p::SchemaDigest("sha256:disclosure-test-v1".into()),
            credential_ref: None,
            scope: p::Scope("communication:disclosure-test".into()),
            capability: p::CapabilityRef("connector:disclosure-test:deliver".into()),
            permission: p::PermissionRef("connector:disclosure-test:use".into()),
            allowed_mutations: vec![p::AppApiMutationMethod::Post],
            requests_per_minute: 1,
            timeout: p::DurationMs(1_000),
            max_response_bytes: 1_024,
            envelope: envelope.clone(),
            contents: vec![(
                p::ContentRef("content:message-1".into()),
                b"approved exact body".to_vec(),
            )],
        })
        .unwrap(),
    );
    let intent = intent(now);
    let authority = harness.ingress_authority();

    let mut wrong_session = exact_binding();
    wrong_session.session = p::CommunicationSessionId("communication:other".into());
    let mut wrong_participant = exact_binding();
    wrong_participant.participant = p::ParticipantId("external:person-b".into());
    let mut wrong_purpose = exact_binding();
    wrong_purpose.purpose = p::PurposeRef("different purpose".into());
    let mut wrong_content = exact_binding();
    wrong_content.content_ref = p::ContentRef("content:other".into());

    for unbound in [
        disclosure(
            &authority,
            "disclosure:other-message",
            p::DisclosureOutcome::Answer,
            p::Representation::Agent,
            p::TrustTier::VerifiedProcess,
            Some(exact_binding()),
        ),
        disclosure(
            &authority,
            "disclosure:message-1",
            p::DisclosureOutcome::Refuse,
            p::Representation::Agent,
            p::TrustTier::VerifiedProcess,
            Some(exact_binding()),
        ),
        disclosure(
            &authority,
            "disclosure:message-1",
            p::DisclosureOutcome::Answer,
            p::Representation::AgentRepresentingOwner,
            p::TrustTier::VerifiedProcess,
            Some(exact_binding()),
        ),
        disclosure(
            &authority,
            "disclosure:message-1",
            p::DisclosureOutcome::Answer,
            p::Representation::Agent,
            p::TrustTier::Untrusted,
            Some(exact_binding()),
        ),
        disclosure(
            &authority,
            "disclosure:message-1",
            p::DisclosureOutcome::Answer,
            p::Representation::Agent,
            p::TrustTier::VerifiedProcess,
            None,
        ),
        disclosure(
            &IngressAuthority::default(),
            "disclosure:message-1",
            p::DisclosureOutcome::Answer,
            p::Representation::Agent,
            p::TrustTier::VerifiedProcess,
            Some(exact_binding()),
        ),
        disclosure(
            &authority,
            "disclosure:message-1",
            p::DisclosureOutcome::Answer,
            p::Representation::Agent,
            p::TrustTier::VerifiedProcess,
            Some(wrong_session),
        ),
        disclosure(
            &authority,
            "disclosure:message-1",
            p::DisclosureOutcome::Answer,
            p::Representation::Agent,
            p::TrustTier::VerifiedProcess,
            Some(wrong_participant),
        ),
        disclosure(
            &authority,
            "disclosure:message-1",
            p::DisclosureOutcome::Answer,
            p::Representation::Agent,
            p::TrustTier::VerifiedProcess,
            Some(wrong_purpose),
        ),
        disclosure(
            &authority,
            "disclosure:message-1",
            p::DisclosureOutcome::Answer,
            p::Representation::Agent,
            p::TrustTier::VerifiedProcess,
            Some(wrong_content),
        ),
    ] {
        assert!(harness
            .submit_action(request(), intent.clone(), envelope.clone(), vec![unbound],)
            .is_err());
    }

    let run = harness
        .submit_action(
            request(),
            intent,
            envelope,
            vec![disclosure(
                &authority,
                "disclosure:message-1",
                p::DisclosureOutcome::Answer,
                p::Representation::Agent,
                p::TrustTier::VerifiedProcess,
                Some(exact_binding()),
            )],
        )
        .unwrap();
    let kinds = harness
        .stream_events(run)
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            p::EventKind::DisclosurePolicyApplied,
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::ToolCallProposed,
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::ApprovalRequested,
            p::EventKind::RunWaiting,
        ]
    );
}
