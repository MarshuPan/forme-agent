use std::sync::Arc;

use forme_cognition::{
    ChangeDirection, CognitiveMapStore, CognitiveMapUpdateProposal, CognitiveRuntime,
    EvolutionGovernor, GovernanceCandidate, GovernanceDecision, GovernanceEvidence, MapScope,
    MapUpdateKind, PromotionAsymmetry, RetractionEvent, StableCognition, StableCognitionKind, Tick,
};
use forme_memory::{CandidateSpec, CandidateState, CandidateStore, EventSourcedMemory};
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore, StoreOptions};

fn provenance(tier: p::TrustTier, actor: p::Actor) -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor,
        trust_tier: tier,
        caused_by: None,
    }
}

fn scope() -> MapScope {
    MapScope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:test".into()),
    }
}

fn runtime() -> (
    Arc<SqliteEventStore>,
    Arc<EventSourcedMemory<SqliteEventStore>>,
    CognitiveRuntime<SqliteEventStore>,
    p::RunId,
) {
    let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    let run = p::RunId("cognition:test".into());
    let memory =
        Arc::new(EventSourcedMemory::with_clock(store.clone(), run.clone(), || 100).unwrap());
    let cognition = CognitiveRuntime::with_clock(
        store.clone(),
        memory.clone(),
        run.clone(),
        PromotionAsymmetry::default(),
        || 100,
    )
    .unwrap();
    (store, memory, cognition, run)
}

fn install_stable(
    memory: &EventSourcedMemory<SqliteEventStore>,
    cognition: &CognitiveRuntime<SqliteEventStore>,
    object: p::ObjectRef,
    kind: StableCognitionKind,
    last_reproduced_at: i64,
) {
    let id = memory.reserve_candidate_id();
    memory
        .create_candidate_spec(CandidateSpec {
            schema_version: p::SchemaVersion(1),
            id: id.clone(),
            target: p::CandidateTargetRef(format!("stable:{}", object.0)),
            evidence_refs: vec![p::EvidenceRef("evidence:stable".into())],
            confidence: p::Confidence(0.9),
            provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
            target_tier: p::StabilityTier::Stable,
        })
        .unwrap();
    assert_eq!(
        cognition
            .govern_and_apply(GovernanceCandidate {
                schema_version: p::SchemaVersion(1),
                candidate_id: id,
                direction: ChangeDirection::CautionIncreasing,
                confidence: p::Confidence(0.9),
                evidence: vec![GovernanceEvidence {
                    schema_version: p::SchemaVersion(1),
                    reference: p::EvidenceRef("evidence:stable".into()),
                    observed_at: 10,
                    verified_process: true,
                }],
                impact: p::Impact::Low,
                provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
                conflicts: Vec::new(),
                owner_confirmed: true,
                target: StableCognition {
                    schema_version: p::SchemaVersion(1),
                    object: object.clone(),
                    scope: scope(),
                    kind,
                    statement: format!("stable statement for {}", object.0),
                    tier: p::StabilityTier::Stable,
                    confidence: p::Confidence(0.9),
                    evidence: vec![GovernanceEvidence {
                        schema_version: p::SchemaVersion(1),
                        reference: p::EvidenceRef("evidence:stable".into()),
                        observed_at: 10,
                        verified_process: true,
                    }],
                    last_reproduced_at,
                    active: false,
                },
            })
            .unwrap(),
        GovernanceDecision::Promote
    );
}

#[test]
fn s9_reflection_creates_low_confidence_candidate_without_mutating_stable_map() {
    let (store, memory, cognition, run) = runtime();
    let candidate_id = cognition
        .propose_update(CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: scope(),
            kind: MapUpdateKind::Frame,
            confidence: p::Confidence(0.3),
            evidence: vec![p::EvidenceRef("trace:1".into())],
            frame: Some(p::JudgmentFrameRef("frame:verify-first".into())),
            quality: None,
            blindspot: None,
            resource: None,
            reflection_inputs: vec![p::EvidenceRef("feedback:1".into())],
            statement: "Verify externally before declaring completion".into(),
            provenance: provenance(p::TrustTier::VerifiedProcess, p::Actor::System),
        })
        .unwrap();
    assert!(cognition.read(scope()).frames.is_empty());
    memory
        .transition(candidate_id, CandidateState::Rejected, p::Actor::Owner)
        .unwrap();
    assert!(cognition.read(scope()).frames.is_empty());

    let kinds = store
        .read_run(run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            p::EventKind::ReflectionProduced,
            p::EventKind::CognitiveMapUpdateProposed,
            p::EventKind::CandidateCreated,
            p::EventKind::CandidateRejected,
        ]
    );
}

#[test]
fn promotion_requires_owner_confirmation_and_uses_asymmetric_thresholds() {
    let (_store, memory, cognition, _run) = runtime();
    let cautious_id = memory.reserve_candidate_id();
    memory
        .create_candidate_spec(CandidateSpec {
            schema_version: p::SchemaVersion(1),
            id: cautious_id.clone(),
            target: p::CandidateTargetRef("caution:test".into()),
            evidence_refs: vec![p::EvidenceRef("evidence:1".into())],
            confidence: p::Confidence(0.5),
            provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
            target_tier: p::StabilityTier::Stable,
        })
        .unwrap();
    let cautious = GovernanceCandidate {
        schema_version: p::SchemaVersion(1),
        candidate_id: cautious_id,
        direction: ChangeDirection::CautionIncreasing,
        confidence: p::Confidence(0.5),
        evidence: vec![GovernanceEvidence {
            schema_version: p::SchemaVersion(1),
            reference: p::EvidenceRef("evidence:1".into()),
            observed_at: 10,
            verified_process: true,
        }],
        impact: p::Impact::Low,
        provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
        conflicts: Vec::new(),
        owner_confirmed: true,
        target: StableCognition {
            schema_version: p::SchemaVersion(1),
            object: p::ObjectRef("blindspot:caution".into()),
            scope: scope(),
            kind: StableCognitionKind::BlindSpot,
            statement: "Ask before assuming an irreversible choice".into(),
            tier: p::StabilityTier::Stable,
            confidence: p::Confidence(0.5),
            evidence: Vec::new(),
            last_reproduced_at: 10,
            active: false,
        },
    };
    assert_eq!(
        cognition.govern_and_apply(cautious).unwrap(),
        GovernanceDecision::Promote
    );

    let assertive = GovernanceCandidate {
        schema_version: p::SchemaVersion(1),
        candidate_id: memory.reserve_candidate_id(),
        direction: ChangeDirection::ConfidenceOrAutonomyIncreasing,
        confidence: p::Confidence(0.9),
        evidence: vec![GovernanceEvidence {
            schema_version: p::SchemaVersion(1),
            reference: p::EvidenceRef("evidence:2".into()),
            observed_at: 10,
            verified_process: true,
        }],
        impact: p::Impact::Low,
        provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
        conflicts: Vec::new(),
        owner_confirmed: true,
        target: StableCognition {
            schema_version: p::SchemaVersion(1),
            object: p::ObjectRef("frame:assertive".into()),
            scope: scope(),
            kind: StableCognitionKind::Frame,
            statement: "Act without confirmation".into(),
            tier: p::StabilityTier::Stable,
            confidence: p::Confidence(0.9),
            evidence: Vec::new(),
            last_reproduced_at: 10,
            active: false,
        },
    };
    assert_eq!(cognition.govern(&assertive), GovernanceDecision::Confirm);
    let mut untrusted = assertive;
    untrusted.direction = ChangeDirection::CautionIncreasing;
    untrusted.provenance = provenance(
        p::TrustTier::Untrusted,
        p::Actor::External(p::ParticipantId("outside".into())),
    );
    assert_eq!(cognition.govern(&untrusted), GovernanceDecision::Confirm);
}

#[test]
fn s20_retraction_traverses_all_derived_objects_without_deleting_history() {
    let (store, memory, cognition, run) = runtime();
    let target = p::ObjectRef("user-attribute:review-style".into());
    let derived_a = p::ObjectRef("frame:review-plan".into());
    let derived_b = p::ObjectRef("trust:review-scope".into());
    for object in [&target, &derived_a, &derived_b] {
        install_stable(
            &memory,
            &cognition,
            object.clone(),
            StableCognitionKind::Frame,
            10,
        );
    }
    cognition
        .record_lineage(target.clone(), derived_a.clone())
        .unwrap();
    cognition
        .record_lineage(derived_a.clone(), derived_b.clone())
        .unwrap();
    let tasks = cognition.on_retraction(RetractionEvent {
        schema_version: p::SchemaVersion(1),
        target_object: target.clone(),
        evidence_lineage: p::LineageRef("lineage:user-correction".into()),
        provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
    });
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.derived.clone())
            .collect::<Vec<_>>(),
        vec![derived_a.clone(), derived_b.clone()]
    );
    assert!(!cognition.is_object_active(&target));
    assert!(!cognition.is_object_active(&derived_a));
    assert!(!cognition.is_object_active(&derived_b));
    assert_eq!(cognition.stable_objects().len(), 3);

    let kinds = store
        .read_run(run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    let retraction = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::RetractionEvent)
        .unwrap();
    let reevaluation = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::ReevaluationTaskCreated)
        .unwrap();
    assert!(retraction < reevaluation);
    assert_eq!(
        kinds[reevaluation + 1..]
            .iter()
            .filter(|kind| **kind == p::EventKind::CandidateCreated)
            .count(),
        2
    );
}

#[test]
fn stale_stable_cognition_emits_memory_misevolution_and_downgrade_candidate() {
    let (store, memory, cognition, run) = runtime();
    install_stable(
        &memory,
        &cognition,
        p::ObjectRef("frame:stale".into()),
        StableCognitionKind::Frame,
        10,
    );
    let candidates = cognition.decay(Tick {
        schema_version: p::SchemaVersion(1),
        now: 1_000,
        stale_after_ms: 100,
    });
    assert_eq!(candidates.len(), 1);
    assert!(!cognition.is_object_active(&p::ObjectRef("frame:stale".into())));
    let events = store.read_run(run).collect::<p::Result<Vec<_>>>().unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::FailureEvidenceRecorded(payload)
            if payload.class == p::FailureClass::MemoryMisevolution
    )));
    assert!(events
        .iter()
        .any(|event| event.kind == p::EventKind::ReevaluationTaskCreated));
}

#[test]
fn s21_promoted_map_projection_rebuilds_from_authoritative_event_order() {
    let (store, _memory, cognition, run) = runtime();
    let candidate_id = cognition
        .propose_update(CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: scope(),
            kind: MapUpdateKind::Frame,
            confidence: p::Confidence(0.4),
            evidence: vec![p::EvidenceRef("trace:replay".into())],
            frame: Some(p::JudgmentFrameRef("frame:replay-safe".into())),
            quality: None,
            blindspot: None,
            resource: None,
            reflection_inputs: vec![p::EvidenceRef("reflection:replay".into())],
            statement: "Keep replay semantics explicit".into(),
            provenance: provenance(p::TrustTier::VerifiedProcess, p::Actor::System),
        })
        .unwrap();
    cognition
        .govern_and_apply(GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id,
            direction: ChangeDirection::CautionIncreasing,
            confidence: p::Confidence(0.6),
            evidence: vec![GovernanceEvidence {
                schema_version: p::SchemaVersion(1),
                reference: p::EvidenceRef("trace:replay".into()),
                observed_at: 100,
                verified_process: true,
            }],
            impact: p::Impact::Low,
            provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
            conflicts: Vec::new(),
            owner_confirmed: true,
            target: StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef("frame:replay-safe".into()),
                scope: scope(),
                kind: StableCognitionKind::Frame,
                statement: "Keep replay semantics explicit".into(),
                tier: p::StabilityTier::Stable,
                confidence: p::Confidence(0.6),
                evidence: Vec::new(),
                last_reproduced_at: 100,
                active: false,
            },
        })
        .unwrap();
    assert_eq!(cognition.read(scope()).frames.len(), 1);

    let rebuilt_memory =
        Arc::new(EventSourcedMemory::with_clock(store.clone(), run.clone(), || 200).unwrap());
    let rebuilt = CognitiveRuntime::with_clock(
        store.clone(),
        rebuilt_memory,
        run.clone(),
        PromotionAsymmetry::default(),
        || 200,
    )
    .unwrap();
    let view = rebuilt.read(scope());
    assert_eq!(view.frames.len(), 1);
    assert_eq!(view.frames[0].statement, "Keep replay semantics explicit");
    let events = store.read_run(run).collect::<p::Result<Vec<_>>>().unwrap();
    assert!(events
        .windows(2)
        .all(|pair| pair[0].stream_seq < pair[1].stream_seq));
}
