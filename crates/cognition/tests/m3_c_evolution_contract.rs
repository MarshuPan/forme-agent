use forme_cognition::{
    apply_communication_strategy, apply_proactivity_strategy, compare_proactivity_strategy,
    AgentSelfEvolutionEngine, CognitiveOutcomeEvidence, CognitiveOutcomeKind,
    CognitiveStrategyDecision, CognitiveStrategyRegistry, CommunicationRuntimeBoundary,
    CommunicationRuntimeDecision, InMemoryCognitiveStrategyRegistry, PartnershipEvolutionEngine,
    ProactivityFitnessSample, ProactivityOpportunity, ProactivityRuntimeDecision,
    TrustDelegationEvolutionEngine,
};
use forme_protocol as p;

fn system() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn owner() -> p::Provenance {
    p::Provenance {
        source: p::Source::UserTurn,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}

fn agent() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::Agent,
        trust_tier: p::TrustTier::ApprovedSource,
        caused_by: None,
    }
}

fn external() -> p::Provenance {
    p::Provenance {
        source: p::Source::Communication,
        actor: p::Actor::External(p::ParticipantId("participant:external".into())),
        trust_tier: p::TrustTier::Untrusted,
        caused_by: None,
    }
}

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
            tool_schema: Some(p::SchemaDigest("tool:v1".into())),
            backend_schema: Some(p::SchemaDigest("backend:v1".into())),
        },
        evidence_policy: p::CognitiveStrategyEvidencePolicy {
            schema_version: p::SchemaVersion(1),
            minimum_verified_outcomes: 2,
            minimum_distinct_timepoints: 2,
            freshness_window: p::DurationMs(1_000),
            decay_after: p::DurationMs(5_000),
            expire_after: p::DurationMs(10_000),
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

fn memory_spec() -> p::StrategyMemorySpec {
    p::StrategyMemorySpec {
        envelope: envelope(p::StrategyDomain::StrategyMemory, "strategy-memory"),
        conflict_policy: p::StrategyConflictPolicy::PreserveAndReevaluate,
        freshness_basis: p::StrategyFreshnessBasis::VerifiedEventTime,
        untrusted_edge_policy: p::UntrustedEdgePolicy::Deny,
        decay_step_basis_points: 1_000,
        maximum_derived_edges: 16,
        additive_schema_only: true,
        rollback_on_active_evidence_loss: true,
    }
}

fn self_spec() -> p::AgentSelfStrategySpec {
    p::AgentSelfStrategySpec {
        envelope: envelope(p::StrategyDomain::AgentSelf, "agent-self"),
        aggregation: p::AgentSelfAggregation::ScopedReliabilityAndGap,
        self_assessment_effect: p::SelfAssessmentEffect::LowerCeilingOnly,
        evidence_window: p::DurationMs(10_000),
        verified_success_weight_basis_points: 4_000,
        verified_failure_weight_basis_points: 6_000,
        minimum_scope_evidence: 2,
    }
}

fn partnership_spec() -> p::PartnershipStrategySpec {
    let mut envelope = envelope(p::StrategyDomain::Partnership, "partnership");
    envelope.evidence_policy.require_owner_feedback = true;
    p::PartnershipStrategySpec {
        envelope,
        axes: vec![
            p::PartnershipAxis::Complementarity,
            p::PartnershipAxis::Collaboration,
            p::PartnershipAxis::Correction,
        ],
        owner_correction: p::PartnershipOwnerCorrection::AuthoritativeEvidence,
        external_feedback: p::ExternalFeedbackRole::ContextOnly,
        minimum_collaboration_timepoints: 2,
        short_term_exception_can_stabilize: p::HistoricalFalse,
    }
}

fn trust_spec() -> p::TrustDelegationStrategySpec {
    p::TrustDelegationStrategySpec {
        envelope: envelope(p::StrategyDomain::TrustDelegation, "trust-delegation"),
        automatic_direction: p::TrustAutomaticDirection::CautionOnly,
        maximum_recommendation: p::DelegationRecommendationCeiling::RequestNarrowGrant,
        verified_successes_before_recommendation: 3,
        failures_before_downgrade: 1,
        owner_review_required_for_expansion: true,
    }
}

fn proactive_spec() -> p::ProactivityStrategySpec {
    p::ProactivityStrategySpec {
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
    }
}

fn communication_spec() -> p::CommunicationStrategySpec {
    p::CommunicationStrategySpec {
        envelope: envelope(p::StrategyDomain::Communication, "communication"),
        expression: p::CommunicationExpressionProfile::ExplicitUncertainty,
        summary: p::CommunicationSummaryProfile::PerCheckpoint,
        surface: p::CommunicationSurfacePreference::CurrentAuthorizedSurface,
        external_feedback: p::ExternalFeedbackRole::ContextOnly,
        maximum_summary_items: 8,
    }
}

fn all_specs() -> Vec<p::M3CStrategySpec> {
    vec![
        p::M3CStrategySpec::StrategyMemory(memory_spec()),
        p::M3CStrategySpec::AgentSelf(self_spec()),
        p::M3CStrategySpec::Partnership(partnership_spec()),
        p::M3CStrategySpec::TrustDelegation(trust_spec()),
        p::M3CStrategySpec::Proactivity(proactive_spec()),
        p::M3CStrategySpec::Communication(communication_spec()),
    ]
}

fn evidence(
    suffix: &str,
    observed_at: p::Timestamp,
    kind: CognitiveOutcomeKind,
    confidence_basis_points: u16,
    provenance: p::Provenance,
) -> CognitiveOutcomeEvidence {
    CognitiveOutcomeEvidence {
        schema_version: p::SchemaVersion(1),
        reference: p::EvidenceRef(format!("evidence:{suffix}")),
        scope: p::Scope("workspace:m3-c".into()),
        observed_at,
        kind,
        confidence_basis_points,
        provenance,
    }
}

#[test]
fn m3_c_registry_requires_all_seeds_and_keeps_version_content_immutable() {
    assert!(InMemoryCognitiveStrategyRegistry::with_seeds(all_specs()[..5].to_vec()).is_err());
    let registry = InMemoryCognitiveStrategyRegistry::with_seeds(all_specs()).unwrap();
    assert_eq!(
        registry
            .seed_version(p::StrategyDomain::AgentSelf)
            .unwrap()
            .0,
        "agent-self:v2"
    );
    let resolved = registry
        .resolve(
            p::StrategyDomain::Communication,
            &p::StrategyVersionRef("communication:v2".into()),
        )
        .unwrap();
    assert!(matches!(resolved, p::M3CStrategySpec::Communication(_)));

    let mut collision = self_spec();
    collision.envelope.content_digest = p::SchemaDigest("digest:collision".into());
    assert!(registry
        .register(p::M3CStrategySpec::AgentSelf(collision))
        .is_err());
}

#[test]
fn s64_agent_self_uses_result_evidence_and_self_assessment_only_lowers() {
    let engine = AgentSelfEvolutionEngine;
    let mut outcomes = vec![
        evidence(
            "pass-1",
            100,
            CognitiveOutcomeKind::VerifiedPass,
            10_000,
            system(),
        ),
        evidence(
            "pass-2",
            200,
            CognitiveOutcomeKind::VerifiedPass,
            10_000,
            system(),
        ),
        evidence(
            "self-high",
            210,
            CognitiveOutcomeKind::SelfAssessment,
            10_000,
            agent(),
        ),
    ];
    let high = engine.assess(&self_spec(), &outcomes, 300).unwrap();
    assert_eq!(high.decision, CognitiveStrategyDecision::PromoteCandidate);
    assert_eq!(high.result_ceiling_basis_points, 10_000);
    assert_eq!(high.effective_ceiling_basis_points, 10_000);

    outcomes.pop();
    outcomes.push(evidence(
        "self-low",
        210,
        CognitiveOutcomeKind::SelfAssessment,
        3_000,
        agent(),
    ));
    let low = engine.assess(&self_spec(), &outcomes, 300).unwrap();
    assert_eq!(low.result_ceiling_basis_points, 10_000);
    assert_eq!(low.effective_ceiling_basis_points, 3_000);

    let mixed = vec![
        evidence(
            "pass",
            100,
            CognitiveOutcomeKind::VerifiedPass,
            10_000,
            system(),
        ),
        evidence(
            "fail",
            200,
            CognitiveOutcomeKind::VerifiedFailure,
            0,
            system(),
        ),
        evidence(
            "self-max",
            210,
            CognitiveOutcomeKind::SelfAssessment,
            10_000,
            agent(),
        ),
    ];
    let mixed = engine.assess(&self_spec(), &mixed, 300).unwrap();
    assert_eq!(mixed.result_ceiling_basis_points, 4_000);
    assert_eq!(mixed.effective_ceiling_basis_points, 4_000);

    let one_failure = vec![evidence(
        "single-fail",
        100,
        CognitiveOutcomeKind::VerifiedFailure,
        0,
        system(),
    )];
    assert_eq!(
        engine
            .assess(&self_spec(), &one_failure, 200)
            .unwrap()
            .decision,
        CognitiveStrategyDecision::NeedMoreEvidence
    );
}

#[test]
fn s65_partnership_requires_process_timepoints_and_authenticated_owner_correction() {
    let engine = PartnershipEvolutionEngine;
    let valid = vec![
        evidence(
            "collaboration",
            100,
            CognitiveOutcomeKind::VerifiedPass,
            10_000,
            system(),
        ),
        evidence(
            "correction",
            200,
            CognitiveOutcomeKind::OwnerCorrection,
            10_000,
            owner(),
        ),
    ];
    let assessment = engine.assess(&partnership_spec(), &valid, 300).unwrap();
    assert_eq!(
        assessment.decision,
        CognitiveStrategyDecision::PromoteCandidate
    );
    assert_eq!(assessment.owner_corrections.len(), 1);

    let transient = vec![
        evidence(
            "short-term",
            100,
            CognitiveOutcomeKind::ShortTermException,
            10_000,
            owner(),
        ),
        evidence(
            "external",
            200,
            CognitiveOutcomeKind::ExternalInput,
            10_000,
            external(),
        ),
    ];
    assert_eq!(
        engine
            .assess(&partnership_spec(), &transient, 300)
            .unwrap()
            .decision,
        CognitiveStrategyDecision::NeedMoreEvidence
    );

    let spoof = vec![evidence(
        "spoofed-owner",
        100,
        CognitiveOutcomeKind::OwnerCorrection,
        10_000,
        external(),
    )];
    assert!(engine.assess(&partnership_spec(), &spoof, 300).is_err());
}

#[test]
fn s66_trust_success_stops_at_owner_proposal_and_failure_downgrades() {
    let engine = TrustDelegationEvolutionEngine;
    let successes = vec![
        evidence(
            "success-1",
            100,
            CognitiveOutcomeKind::VerifiedPass,
            10_000,
            system(),
        ),
        evidence(
            "success-2",
            200,
            CognitiveOutcomeKind::VerifiedPass,
            10_000,
            system(),
        ),
        evidence(
            "success-3",
            300,
            CognitiveOutcomeKind::VerifiedPass,
            10_000,
            system(),
        ),
    ];
    let assessment = engine.assess(&trust_spec(), &successes, 400).unwrap();
    assert_eq!(
        assessment.decision,
        CognitiveStrategyDecision::OwnerReviewProposal
    );
    assert!(engine
        .owner_review(&assessment, p::VerifiedPrincipal(String::new()))
        .is_err());
    let reviewed = engine
        .owner_review(&assessment, p::VerifiedPrincipal("owner:local".into()))
        .unwrap();
    assert_eq!(
        reviewed.ceiling,
        p::DelegationRecommendationCeiling::RequestNarrowGrant
    );

    let failure = vec![evidence(
        "failure",
        350,
        CognitiveOutcomeKind::VerifiedFailure,
        0,
        system(),
    )];
    assert_eq!(
        engine
            .assess(&trust_spec(), &failure, 400)
            .unwrap()
            .decision,
        CognitiveStrategyDecision::DowngradeAutomatically
    );
}

#[test]
fn s67_proactivity_reduces_regret_without_missing_commitment_or_bypassing_attention() {
    let baseline = ProactivityFitnessSample {
        schema_version: p::SchemaVersion(1),
        adopted: 5,
        rejected: 3,
        deferred: 2,
        interrupt_regrets: 2,
        ask_to_learn_helpful: 2,
        missed_commitments: 0,
        attention_cost: 80,
        ground_truth: vec![p::EvidenceRef("ground-truth:baseline".into())],
    };
    let mut candidate = baseline.clone();
    candidate.rejected = 1;
    candidate.interrupt_regrets = 0;
    candidate.ask_to_learn_helpful = 3;
    candidate.attention_cost = 40;
    candidate.ground_truth = vec![p::EvidenceRef("ground-truth:candidate".into())];
    assert_eq!(
        compare_proactivity_strategy(&proactive_spec(), &baseline, &candidate).unwrap(),
        CognitiveStrategyDecision::PromoteCandidate
    );
    candidate.missed_commitments = 1;
    assert_eq!(
        compare_proactivity_strategy(&proactive_spec(), &baseline, &candidate).unwrap(),
        CognitiveStrategyDecision::DowngradeAutomatically
    );

    let commitment = ProactivityOpportunity {
        schema_version: p::SchemaVersion(1),
        value_basis_points: 100,
        commitment: true,
        requested_delivery: p::DeliveryMode::Digest,
        attention_remaining: 10,
        interruptions_used: 0,
        value_gate_passed: true,
        competence_gate_passed: true,
        policy_and_envelope_passed: true,
    };
    assert_eq!(
        apply_proactivity_strategy(&proactive_spec(), &commitment).unwrap(),
        ProactivityRuntimeDecision::Emit(p::DeliveryMode::Digest)
    );
    let mut exhausted = commitment.clone();
    exhausted.attention_remaining = 9;
    assert_eq!(
        apply_proactivity_strategy(&proactive_spec(), &exhausted).unwrap(),
        ProactivityRuntimeDecision::Emit(p::DeliveryMode::Internal)
    );

    let interruption_cap = ProactivityOpportunity {
        requested_delivery: p::DeliveryMode::Interrupt,
        attention_remaining: 100,
        interruptions_used: 1,
        ..commitment
    };
    assert_eq!(
        apply_proactivity_strategy(&proactive_spec(), &interruption_cap).unwrap(),
        ProactivityRuntimeDecision::Emit(p::DeliveryMode::Digest)
    );
}

#[test]
fn s67_communication_strategy_never_expands_recipient_disclosure_or_outward_authority() {
    let mut boundary = CommunicationRuntimeBoundary {
        schema_version: p::SchemaVersion(1),
        current_surface_authorized: false,
        recipient_bound: true,
        disclosure: p::DisclosureOutcome::Answer,
        outward: true,
        risk: p::Risk::Medium,
        approval_granted: false,
    };
    assert_eq!(
        apply_communication_strategy(&communication_spec(), &boundary).unwrap(),
        CommunicationRuntimeDecision::Refuse
    );
    boundary.current_surface_authorized = true;
    assert_eq!(
        apply_communication_strategy(&communication_spec(), &boundary).unwrap(),
        CommunicationRuntimeDecision::NeedActionApproval
    );
    boundary.approval_granted = true;
    assert_eq!(
        apply_communication_strategy(&communication_spec(), &boundary).unwrap(),
        CommunicationRuntimeDecision::PrepareOnAuthorizedSurface
    );
    boundary.disclosure = p::DisclosureOutcome::Refuse;
    assert_eq!(
        apply_communication_strategy(&communication_spec(), &boundary).unwrap(),
        CommunicationRuntimeDecision::Refuse
    );
}
