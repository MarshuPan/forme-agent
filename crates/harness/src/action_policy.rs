//! Action classification and policy-floor checks owned by the harness boundary.
use super::{cognition, p, scope_within, IngressAuthority, IngressEvent};
use forme_execution::ExecutionPlan;
use forme_policy::{PolicyContext, PolicyEvaluation};

pub(super) fn requires_competence_gate(intent: &p::ActionIntent) -> bool {
    matches!(intent.source, p::Source::ProactiveJob | p::Source::Schedule)
        || intent.expected_effect == p::ExpectedEffect::Outward
}

pub(super) fn required_intervention_level(
    intent: &p::ActionIntent,
    rollback_boundary: &p::RollbackBoundary,
) -> cognition::InterventionLevel {
    if requires_one_shot_approval(intent, rollback_boundary) {
        cognition::InterventionLevel::L5HighImpact
    } else if intent.backend_hint == p::BackendKind::Notification
        && intent.risk_hint == p::Risk::Low
    {
        cognition::InterventionLevel::L1Suggest
    } else {
        cognition::InterventionLevel::L3ActWithApproval
    }
}

pub(super) fn is_external_action(intent: &p::ActionIntent) -> bool {
    matches!(
        intent.backend_hint,
        p::BackendKind::Browser
            | p::BackendKind::Computer
            | p::BackendKind::Pty
            | p::BackendKind::AppApi
    ) || (intent.expected_effect == p::ExpectedEffect::Outward
        && !is_owner_local_notification(intent))
}

pub(super) fn delivery_disclosure_is_bound(
    request: &p::RunRequest,
    intent: &p::ActionIntent,
    prelude: &[IngressEvent],
    authority: &IngressAuthority,
) -> bool {
    let p::ActionParameters::AppApi(spec) = &intent.parameters else {
        return false;
    };
    let (Some(expected_request), Some(expected_representation), Some(expected_participant)) = (
        &spec.disclosure_request,
        spec.representation,
        &spec.participant,
    ) else {
        return false;
    };
    let p::AppApiOperation::Mutation {
        body: Some(p::ExternalInput::Content(expected_content)),
        ..
    } = &spec.operation
    else {
        return false;
    };
    prelude.iter().any(|event| {
        event.is_authorized_by(authority)
            && event.provenance().source == p::Source::Communication
            && event.provenance().actor == p::Actor::System
            && event.provenance().trust_tier == p::TrustTier::VerifiedProcess
            && matches!(
                event.payload(),
                p::EventPayload::DisclosurePolicyApplied(disclosure)
                    if &disclosure.request == expected_request
                        && disclosure.representation == expected_representation
                        && matches!(
                            disclosure.outcome,
                            p::DisclosureOutcome::Answer | p::DisclosureOutcome::Approve
                        )
                        && disclosure.binding.as_ref().is_some_and(|binding| {
                            binding.schema_version.0 > 0
                                && binding.session.0 == request.session.0
                                && &binding.participant == expected_participant
                                && binding.purpose.0 == intent.goal.0
                                && &binding.content_ref == expected_content
                                && !binding.category.trim().is_empty()
                                && (!(binding.sensitive
                                    || !binding.confirmed
                                    || binding.high_impact)
                                    || intent.risk_hint == p::Risk::High)
                        })
            )
    })
}

fn is_owner_local_notification(intent: &p::ActionIntent) -> bool {
    matches!(
        &intent.parameters,
        p::ActionParameters::Notification {
            surface,
            target,
            ..
        } if intent.backend_hint == p::BackendKind::Notification
            && surface.0.starts_with("surface:local")
            && target.0 == "owner"
    )
}

pub(super) fn requires_one_shot_approval(
    intent: &p::ActionIntent,
    rollback_boundary: &p::RollbackBoundary,
) -> bool {
    let rollback = rollback_boundary.0.trim().to_ascii_lowercase();
    is_external_action(intent)
        && (intent.risk_hint == p::Risk::High
            || intent.action_type == p::ActionType::ExternalCommit
            || rollback.is_empty()
            || rollback == "none"
            || rollback.contains("not-retractable")
            || rollback.contains("unknown"))
}

fn external_policy_floor(
    intent: &p::ActionIntent,
    rollback_boundary: Option<&p::RollbackBoundary>,
    policy: p::PolicyDecision,
    envelope: Option<&p::AutonomyEnvelope>,
    inputs: &cognition::CompetenceInputs,
    competence: &(dyn cognition::CompetenceGate + Send + Sync),
) -> p::PolicyDecision {
    if policy == p::PolicyDecision::Deny || !is_external_action(intent) {
        return policy;
    }
    let Some(rollback_boundary) = rollback_boundary else {
        return p::PolicyDecision::Ask;
    };
    if requires_one_shot_approval(intent, rollback_boundary) {
        return p::PolicyDecision::Ask;
    }
    let evidence_present =
        !inputs.capability_evidence.is_empty() || !inputs.verification.is_empty();
    let narrow_scope = !intent.scope.0.trim().is_empty()
        && intent.scope.0 != "*"
        && !intent.scope.0.eq_ignore_ascii_case("all");
    let explicit_l4 = envelope.is_some_and(|envelope| {
        envelope.approval_rule == p::ApprovalRule::Allow
            && envelope.risk_limit == p::Risk::Low
            && envelope.rollback.required
            && envelope.rollback.boundary.as_ref() == Some(rollback_boundary)
            && scope_within(&intent.scope, &envelope.scope)
            && envelope
                .capability
                .capabilities
                .contains(&intent.capability_ref)
            && intent
                .requested_permissions
                .iter()
                .all(|permission| envelope.capability.permissions.contains(permission))
    });
    let ceiling = competence.ceiling(intent.scope.clone(), intent.risk_hint, inputs);
    if policy == p::PolicyDecision::Allow
        && intent.risk_hint == p::Risk::Low
        && evidence_present
        && narrow_scope
        && explicit_l4
        && ceiling >= cognition::InterventionLevel::L4Autonomous
    {
        p::PolicyDecision::Allow
    } else {
        p::PolicyDecision::Ask
    }
}

pub(super) fn apply_policy_floors(
    mut evaluation: PolicyEvaluation,
    intent: &p::ActionIntent,
    plan: Option<&ExecutionPlan>,
    context: &PolicyContext,
    inputs: &cognition::CompetenceInputs,
    competence: &(dyn cognition::CompetenceGate + Send + Sync),
) -> PolicyEvaluation {
    let external_floor = external_policy_floor(
        intent,
        plan.map(|plan| &plan.rollback_boundary),
        evaluation.decision,
        context.envelope.as_ref(),
        inputs,
        competence,
    );
    let effective_decision = untrusted_ingress_policy_floor(intent, external_floor);
    if effective_decision != evaluation.decision {
        let external_floor_changed = external_floor != evaluation.decision;
        evaluation.decision = effective_decision;
        if external_floor_changed {
            evaluation.rule_source = p::RuleSourceRef("external-action-floor".into());
            evaluation.reason = p::ReasonRef(
                "external action requires plan-bound approval under canonical section 24".into(),
            );
        } else {
            evaluation.rule_source = p::RuleSourceRef("untrusted-ingress-floor".into());
            evaluation.reason = p::ReasonRef(
                "an action derived from untrusted communication requires owner approval".into(),
            );
        }
    }
    evaluation
}

fn untrusted_ingress_policy_floor(
    intent: &p::ActionIntent,
    policy: p::PolicyDecision,
) -> p::PolicyDecision {
    if intent.source == p::Source::Communication && policy == p::PolicyDecision::Allow {
        p::PolicyDecision::Ask
    } else {
        policy
    }
}

pub(super) fn validate_competence_snapshot(
    snapshot: &cognition::CompetenceInputs,
) -> p::Result<()> {
    let references_are_present =
        snapshot
            .capability_evidence
            .iter()
            .all(|item| item.schema_version.0 > 0 && !item.reference.0.trim().is_empty())
            && snapshot
                .verification
                .iter()
                .all(|item| item.schema_version.0 > 0 && !item.reference.0.trim().is_empty())
            && snapshot
                .failure
                .iter()
                .all(|item| item.schema_version.0 > 0 && !item.reference.0.trim().is_empty())
            && snapshot.map_confidence.as_ref().is_none_or(|item| {
                item.schema_version.0 > 0 && !item.reference.0.trim().is_empty()
            })
            && snapshot.self_model.as_ref().is_none_or(|item| {
                item.schema_version.0 > 0 && !item.reference.0.trim().is_empty()
            })
            && snapshot.trust.as_ref().is_none_or(|item| {
                item.schema_version.0 > 0 && !item.reference.0.trim().is_empty()
            });
    if snapshot.schema_version.0 == 0 || !references_are_present {
        return Err(p::Error(
            "competence evidence snapshot is incomplete".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(source: p::Source) -> p::ActionIntent {
        p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId("action:action-policy-test".into()),
            source,
            goal: p::GoalRef("goal:action-policy-test".into()),
            backend_hint: p::BackendKind::Shell,
            capability_ref: p::CapabilityRef("capability:shell".into()),
            action_type: p::ActionType::Analyze,
            scope: p::Scope("workspace:test".into()),
            risk_hint: p::Risk::Low,
            expected_effect: p::ExpectedEffect::Internal,
            rollback_expectation: p::RollbackBoundary("none".into()),
            parameters: p::ActionParameters::Shell {
                program: "true".into(),
                args: Vec::new(),
                cwd: None,
                network: false,
            },
            requested_permissions: Vec::new(),
            requested_at: 0,
            estimated_output_bytes: 0,
            estimated_duration: p::DurationMs(0),
        }
    }

    #[test]
    fn communication_action_cannot_keep_an_allow_decision() {
        assert_eq!(
            untrusted_ingress_policy_floor(
                &intent(p::Source::Communication),
                p::PolicyDecision::Allow
            ),
            p::PolicyDecision::Ask
        );
        assert_eq!(
            untrusted_ingress_policy_floor(
                &intent(p::Source::Communication),
                p::PolicyDecision::Deny
            ),
            p::PolicyDecision::Deny
        );
        assert_eq!(
            untrusted_ingress_policy_floor(&intent(p::Source::UserTurn), p::PolicyDecision::Allow),
            p::PolicyDecision::Allow
        );
    }
}
