use forme_protocol as p;
use serde_json::json;

fn envelope(domain: p::StrategyDomain, suffix: &str) -> p::CognitiveStrategyEnvelope {
    p::CognitiveStrategyEnvelope {
        schema_version: p::SchemaVersion(1),
        domain,
        version: p::StrategyVersionRef(format!("{suffix}:v2")),
        scope: p::Scope("workspace:m3-c".into()),
        content_ref: p::ContentRef(format!("spec:{suffix}:v2")),
        content_digest: p::SchemaDigest(format!("digest:{suffix}:v2")),
        compatibility: p::StrategyRuntimeCompatibility {
            schema_version: p::SchemaVersion(1),
            minimum_runtime_schema: p::SchemaVersion(1),
            event_schema: p::SchemaVersion(1),
            model_profile: None,
            tool_schema: Some(p::SchemaDigest("tool-schema:v1".into())),
            backend_schema: Some(p::SchemaDigest("backend-schema:v1".into())),
        },
        evidence_policy: p::CognitiveStrategyEvidencePolicy {
            schema_version: p::SchemaVersion(1),
            minimum_verified_outcomes: 2,
            minimum_distinct_timepoints: 2,
            freshness_window: p::DurationMs(1_000),
            decay_after: p::DurationMs(2_000),
            expire_after: p::DurationMs(4_000),
            require_owner_feedback: false,
        },
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef(format!("{suffix}:v1")),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    }
}

fn specs() -> Vec<p::M3CStrategySpec> {
    vec![
        p::M3CStrategySpec::StrategyMemory(p::StrategyMemorySpec {
            envelope: envelope(p::StrategyDomain::StrategyMemory, "strategy-memory"),
            conflict_policy: p::StrategyConflictPolicy::PreserveAndReevaluate,
            freshness_basis: p::StrategyFreshnessBasis::VerifiedEventTime,
            untrusted_edge_policy: p::UntrustedEdgePolicy::Deny,
            decay_step_basis_points: 1_000,
            maximum_derived_edges: 16,
            additive_schema_only: true,
            rollback_on_active_evidence_loss: true,
        }),
        p::M3CStrategySpec::AgentSelf(p::AgentSelfStrategySpec {
            envelope: envelope(p::StrategyDomain::AgentSelf, "agent-self"),
            aggregation: p::AgentSelfAggregation::ScopedReliabilityAndGap,
            self_assessment_effect: p::SelfAssessmentEffect::LowerCeilingOnly,
            evidence_window: p::DurationMs(4_000),
            verified_success_weight_basis_points: 4_000,
            verified_failure_weight_basis_points: 6_000,
            minimum_scope_evidence: 2,
        }),
        p::M3CStrategySpec::Partnership(p::PartnershipStrategySpec {
            envelope: envelope(p::StrategyDomain::Partnership, "partnership"),
            axes: vec![
                p::PartnershipAxis::Complementarity,
                p::PartnershipAxis::Collaboration,
                p::PartnershipAxis::Correction,
            ],
            owner_correction: p::PartnershipOwnerCorrection::AuthoritativeEvidence,
            external_feedback: p::ExternalFeedbackRole::ContextOnly,
            minimum_collaboration_timepoints: 2,
            short_term_exception_can_stabilize: p::HistoricalFalse,
        }),
        p::M3CStrategySpec::TrustDelegation(p::TrustDelegationStrategySpec {
            envelope: envelope(p::StrategyDomain::TrustDelegation, "trust-delegation"),
            automatic_direction: p::TrustAutomaticDirection::CautionOnly,
            maximum_recommendation: p::DelegationRecommendationCeiling::RequestNarrowGrant,
            verified_successes_before_recommendation: 3,
            failures_before_downgrade: 1,
            owner_review_required_for_expansion: true,
        }),
        p::M3CStrategySpec::Proactivity(p::ProactivityStrategySpec {
            envelope: envelope(p::StrategyDomain::Proactivity, "proactivity"),
            minimum_value_basis_points: 6_000,
            preferred_delivery: p::DeliveryMode::Digest,
            attention: p::AttentionCostProfile {
                schema_version: p::SchemaVersion(1),
                capacity: 100,
                interrupt_cost: 40,
                digest_cost: 10,
                ask_to_learn_cost: 15,
                rejection_cooldown_ticks: 3,
            },
            commitment_handling: p::CommitmentHandling::DeterministicAndPreserved,
            maximum_interruptions_per_tick: 1,
        }),
        p::M3CStrategySpec::Communication(p::CommunicationStrategySpec {
            envelope: envelope(p::StrategyDomain::Communication, "communication"),
            expression: p::CommunicationExpressionProfile::ExplicitUncertainty,
            summary: p::CommunicationSummaryProfile::PerCheckpoint,
            surface: p::CommunicationSurfacePreference::CurrentAuthorizedSurface,
            external_feedback: p::ExternalFeedbackRole::ContextOnly,
            maximum_summary_items: 8,
        }),
    ]
}

#[test]
fn m3_c_six_domain_specs_round_trip_and_validate() {
    for spec in specs() {
        spec.validate().unwrap();
        let encoded = serde_json::to_vec(&spec).unwrap();
        let decoded: p::M3CStrategySpec = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, spec);
    }
    assert_eq!(p::EventKind::ALL.len(), 99);
}

#[test]
fn m3_c_unknown_fixed_authority_and_disclosure_fields_fail_closed() {
    let forbidden = [
        "permission",
        "approval_bypass",
        "delegation_grant",
        "autonomy_envelope",
        "standing_l5",
        "disclosure_policy",
        "recipient",
        "observation_scope",
        "owner_identity",
        "real_consciousness",
        "loyalty_override",
    ];
    for spec in specs() {
        let base = serde_json::to_value(spec).unwrap();
        for field in forbidden {
            let mut value = base.clone();
            value
                .get_mut("strategy")
                .and_then(serde_json::Value::as_object_mut)
                .unwrap()
                .insert(field.into(), json!(true));
            assert!(serde_json::from_value::<p::M3CStrategySpec>(value).is_err());
        }
    }
}

#[test]
fn m3_c_invalid_identity_decay_expiry_and_evidence_are_rejected() {
    let mut spec = match specs().remove(0) {
        p::M3CStrategySpec::StrategyMemory(spec) => spec,
        _ => unreachable!(),
    };
    spec.envelope.scope = p::Scope(String::new());
    assert!(spec.validate().is_err());

    let mut spec = match specs().remove(0) {
        p::M3CStrategySpec::StrategyMemory(spec) => spec,
        _ => unreachable!(),
    };
    spec.envelope.evidence_policy.decay_after = p::DurationMs(999);
    assert!(spec.validate().is_err());

    let mut spec = match specs().remove(1) {
        p::M3CStrategySpec::AgentSelf(spec) => spec,
        _ => unreachable!(),
    };
    spec.verified_failure_weight_basis_points = u16::MAX;
    assert!(spec.validate().is_err());
}

#[test]
fn m3_c_legacy_without_strategy_envelope_cannot_decode_as_active_spec() {
    let legacy = json!({
        "schema_version": 1,
        "scope": "workspace:m3-c",
        "confidence": 0.9,
        "evidence": ["event:legacy"]
    });
    assert!(serde_json::from_value::<p::M3CStrategySpec>(legacy).is_err());
}
