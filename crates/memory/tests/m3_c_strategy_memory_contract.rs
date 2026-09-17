use forme_memory::{
    StrategyMemoryCandidateState, StrategyMemoryProjector, StrategyMemoryRecommendation,
};
use forme_protocol as p;

fn trusted() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn untrusted() -> p::Provenance {
    p::Provenance {
        source: p::Source::Communication,
        actor: p::Actor::External(p::ParticipantId("participant:web".into())),
        trust_tier: p::TrustTier::Untrusted,
        caused_by: None,
    }
}

fn spec() -> p::StrategyMemorySpec {
    p::StrategyMemorySpec {
        envelope: p::CognitiveStrategyEnvelope {
            schema_version: p::SchemaVersion(1),
            domain: p::StrategyDomain::StrategyMemory,
            version: p::StrategyVersionRef("strategy-memory:v2".into()),
            scope: p::Scope("workspace:m3-c".into()),
            content_ref: p::ContentRef("spec:strategy-memory:v2".into()),
            content_digest: p::SchemaDigest("digest:strategy-memory:v2".into()),
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
                decay_after: p::DurationMs(2_000),
                expire_after: p::DurationMs(4_000),
                require_owner_feedback: false,
            },
            rollback_policy: p::StrategyRollbackPolicy {
                schema_version: p::SchemaVersion(1),
                known_good: p::StrategyVersionRef("strategy-memory:v1".into()),
                rollback_on_hard_regression: true,
                owner_on_unverifiable: true,
            },
        },
        conflict_policy: p::StrategyConflictPolicy::PreserveAndReevaluate,
        freshness_basis: p::StrategyFreshnessBasis::VerifiedEventTime,
        untrusted_edge_policy: p::UntrustedEdgePolicy::Deny,
        decay_step_basis_points: 1_000,
        maximum_derived_edges: 16,
        additive_schema_only: true,
        rollback_on_active_evidence_loss: true,
    }
}

fn strategy_candidate(id: &str, version: &str, evidence: &str) -> p::StrategyCandidate {
    p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId(id.into()),
        domain: p::StrategyDomain::StrategyMemory,
        scope: p::Scope("workspace:m3-c".into()),
        target_tier: p::StabilityTier::Stable,
        proposed_version: p::StrategyVersionRef(version.into()),
        baseline: p::StrategyVersionRef("strategy-memory:v1".into()),
        spec_ref: p::ContentRef(format!("spec:{version}")),
        spec_digest: p::SchemaDigest(format!("digest:{version}")),
        evidence: vec![p::EvidenceRef(evidence.into())],
        provenance: trusted(),
        impact: p::EvolutionImpact::Bounded,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef("strategy-memory:v1".into()),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    }
}

fn candidate_payload(candidate: p::StrategyCandidate) -> p::EventPayload {
    p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
        candidate_id: candidate.candidate.clone(),
        target: p::CandidateTargetRef(format!("strategy:{}", candidate.candidate.0)),
        evidence_refs: candidate.evidence.clone(),
        confidence: p::Confidence(0.8),
        provenance: candidate.provenance.clone(),
        target_tier: p::StabilityTier::Stable,
        capability_update: None,
        strategy_candidate: Some(candidate),
    })
}

fn event(
    sequence: u64,
    ts: p::Timestamp,
    provenance: p::Provenance,
    payload: p::EventPayload,
) -> p::Event {
    let mut event = p::Event::new(
        p::EventId(format!("event:{sequence}")),
        p::RunId("run:m3-c-strategy-memory".into()),
        None,
        payload,
        p::SchemaVersion(1),
        ts,
        provenance,
    );
    event.stream_seq = sequence;
    event
}

fn activation() -> p::StrategyActivation {
    let aggregate = p::EvolutionAggregateRef("evolution:m3-c".into());
    p::StrategyActivation {
        schema_version: p::SchemaVersion(1),
        aggregate: aggregate.clone(),
        domain: p::StrategyDomain::StrategyMemory,
        scope: p::Scope("workspace:m3-c".into()),
        from: Some(p::StrategyVersionRef("strategy-memory:v1".into())),
        to: p::StrategyVersionRef("strategy-memory:v2-left".into()),
        spec_ref: p::ContentRef("spec:strategy-memory:v2-left".into()),
        spec_digest: p::SchemaDigest("digest:strategy-memory:v2-left".into()),
        evaluation: p::EvolutionEvaluationRef("evaluation:strategy-memory".into()),
        promotion: p::EventId("event:promotion".into()),
        owner_confirmation: Some(p::OwnerControlRef("owner:activation".into())),
        impact: p::EvolutionImpact::Bounded,
        expected_version: p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: aggregate.clone(),
            value: 0,
        },
        committed_version: p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate,
            value: 1,
        },
    }
}

fn rollback() -> p::StrategyRollback {
    let aggregate = p::EvolutionAggregateRef("evolution:m3-c".into());
    p::StrategyRollback {
        schema_version: p::SchemaVersion(1),
        aggregate: aggregate.clone(),
        domain: p::StrategyDomain::StrategyMemory,
        scope: p::Scope("workspace:m3-c".into()),
        failed: p::StrategyVersionRef("strategy-memory:v2-left".into()),
        restored: p::StrategyVersionRef("strategy-memory:v1".into()),
        restored_spec_ref: p::ContentRef("spec:strategy-memory:v1".into()),
        restored_spec_digest: p::SchemaDigest("digest:strategy-memory:v1".into()),
        triggers: vec![p::EvidenceRef("evidence:left".into())],
        expected_version: p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: aggregate.clone(),
            value: 1,
        },
        committed_version: p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate,
            value: 2,
        },
        in_flight: p::InFlightDisposition::KeepPinned,
        external_effects_reverted: p::HistoricalFalse,
    }
}

#[test]
fn s63_conflict_retraction_and_decay_produce_governed_recommendations() {
    let left = strategy_candidate("candidate:left", "strategy-memory:v2-left", "evidence:left");
    let right = strategy_candidate(
        "candidate:right",
        "strategy-memory:v2-right",
        "evidence:right",
    );
    let events = vec![
        event(1, 100, trusted(), candidate_payload(left)),
        event(2, 110, trusted(), candidate_payload(right)),
        event(
            3,
            120,
            trusted(),
            p::EventPayload::CandidateConflictDetected(p::CandidateConflictDetectedPayload {
                candidate_id: p::CandidateId("candidate:left".into()),
                conflict_with: p::CandidateId("candidate:right".into()),
                kind: p::ConflictKind("opposite-outcome".into()),
            }),
        ),
        event(
            4,
            130,
            trusted(),
            p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                candidate_id: p::CandidateId("candidate:left".into()),
                by: p::DecisionActor::User,
                reason: p::ReasonRef("owner-confirmed".into()),
            }),
        ),
        event(
            5,
            140,
            trusted(),
            p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
                activation: activation(),
                active_snapshot: p::EvolutionSnapshotRef("snapshot:v2".into()),
            }),
        ),
        event(
            6,
            150,
            trusted(),
            p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                target_object: p::ObjectRef("evidence:left".into()),
                evidence_lineage: p::LineageRef("lineage:left".into()),
            }),
        ),
    ];

    let snapshot = StrategyMemoryProjector
        .rebuild(&events, &spec(), 151)
        .unwrap();
    assert_eq!(snapshot.candidates.len(), 2);
    assert_eq!(snapshot.lineage.len(), 2);
    let left = snapshot
        .candidates
        .iter()
        .find(|candidate| candidate.candidate.0 == "candidate:left")
        .unwrap();
    assert_eq!(left.state, StrategyMemoryCandidateState::Stable);
    assert!(left.active);
    assert_eq!(
        left.conflicts,
        vec![p::CandidateId("candidate:right".into())]
    );
    assert!(snapshot
        .recommendations
        .iter()
        .any(|recommendation| matches!(
            recommendation,
            StrategyMemoryRecommendation::Reevaluate { candidate, .. }
                if candidate.0 == "candidate:left"
        )));
    assert!(snapshot
        .recommendations
        .iter()
        .any(|recommendation| matches!(
            recommendation,
            StrategyMemoryRecommendation::Downgrade { candidate, .. }
                if candidate.0 == "candidate:left"
        )));
    assert!(snapshot
        .recommendations
        .iter()
        .any(|recommendation| matches!(
            recommendation,
            StrategyMemoryRecommendation::Rollback { failed, restored, .. }
                if failed.0 == "strategy-memory:v2-left" && restored.0 == "strategy-memory:v1"
        )));
}

#[test]
fn s63_untrusted_content_never_creates_lineage_or_maintenance_actions() {
    let mut candidate = strategy_candidate(
        "candidate:external",
        "strategy-memory:v2-external",
        "evidence:external",
    );
    candidate.provenance = untrusted();
    let events = vec![event(1, 100, untrusted(), candidate_payload(candidate))];
    let snapshot = StrategyMemoryProjector
        .rebuild(&events, &spec(), 10_000)
        .unwrap();
    assert_eq!(snapshot.candidates.len(), 1);
    assert!(!snapshot.candidates[0].trusted_lineage);
    assert!(snapshot.lineage.is_empty());
    assert!(snapshot.recommendations.is_empty());
}

#[test]
fn s63_append_only_resolution_keeps_history_and_removes_active_v2() {
    let candidate =
        strategy_candidate("candidate:left", "strategy-memory:v2-left", "evidence:left");
    let events = vec![
        event(1, 100, trusted(), candidate_payload(candidate)),
        event(
            2,
            110,
            trusted(),
            p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                candidate_id: p::CandidateId("candidate:left".into()),
                by: p::DecisionActor::User,
                reason: p::ReasonRef("owner-confirmed".into()),
            }),
        ),
        event(
            3,
            120,
            trusted(),
            p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
                activation: activation(),
                active_snapshot: p::EvolutionSnapshotRef("snapshot:v2".into()),
            }),
        ),
        event(
            4,
            130,
            trusted(),
            p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                target_object: p::ObjectRef("evidence:left".into()),
                evidence_lineage: p::LineageRef("lineage:left".into()),
            }),
        ),
        event(
            5,
            140,
            trusted(),
            p::EventPayload::ReevaluationTaskCreated(p::ReevaluationTaskCreatedPayload {
                derived_refs: vec![p::ObjectRef("candidate:left".into())],
                trigger: p::ReevaluationTriggerRef("retraction:evidence:left".into()),
            }),
        ),
        event(
            6,
            150,
            trusted(),
            p::EventPayload::CandidateDowngraded(p::CandidateDowngradedPayload {
                candidate_id: p::CandidateId("candidate:left".into()),
                by: p::DecisionActor::Auto,
                reason: p::ReasonRef("evidence-retracted".into()),
            }),
        ),
        event(
            7,
            160,
            trusted(),
            p::EventPayload::StrategyRolledBack(p::StrategyRolledBackPayload {
                rollback: rollback(),
                active_snapshot: p::EvolutionSnapshotRef("snapshot:v1-restored".into()),
            }),
        ),
    ];
    let snapshot = StrategyMemoryProjector
        .rebuild(&events, &spec(), 161)
        .unwrap();
    assert_eq!(snapshot.candidates.len(), 1);
    assert_eq!(
        snapshot.candidates[0].state,
        StrategyMemoryCandidateState::Downgraded
    );
    assert!(!snapshot.candidates[0].active);
    assert!(snapshot.recommendations.is_empty());
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        vec![
            p::EventKind::CandidateCreated,
            p::EventKind::CandidatePromoted,
            p::EventKind::StrategyActivated,
            p::EventKind::RetractionEvent,
            p::EventKind::ReevaluationTaskCreated,
            p::EventKind::CandidateDowngraded,
            p::EventKind::StrategyRolledBack,
        ]
    );
}

#[test]
fn s63_malformed_control_payloads_and_candidate_envelopes_fail_closed() {
    let candidate =
        strategy_candidate("candidate:left", "strategy-memory:v2-left", "evidence:left");
    let mut mismatched = match candidate_payload(candidate.clone()) {
        p::EventPayload::CandidateCreated(payload) => payload,
        _ => unreachable!(),
    };
    mismatched.candidate_id = p::CandidateId("candidate:other".into());
    assert!(StrategyMemoryProjector
        .rebuild(
            &[event(
                1,
                100,
                trusted(),
                p::EventPayload::CandidateCreated(mismatched),
            )],
            &spec(),
            101,
        )
        .is_err());

    let mut invalid_activation = activation();
    invalid_activation.committed_version.value = 4;
    let events = vec![
        event(1, 100, trusted(), candidate_payload(candidate)),
        event(
            2,
            110,
            trusted(),
            p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
                activation: invalid_activation,
                active_snapshot: p::EvolutionSnapshotRef("snapshot:invalid".into()),
            }),
        ),
    ];
    assert!(StrategyMemoryProjector
        .rebuild(&events, &spec(), 111)
        .is_err());

    let mut invalid_rollback = rollback();
    invalid_rollback.committed_version.value = 9;
    let events = vec![event(
        1,
        100,
        trusted(),
        p::EventPayload::StrategyRolledBack(p::StrategyRolledBackPayload {
            rollback: invalid_rollback,
            active_snapshot: p::EvolutionSnapshotRef("snapshot:invalid".into()),
        }),
    )];
    assert!(StrategyMemoryProjector
        .rebuild(&events, &spec(), 101)
        .is_err());
}
