//! B0: a stable-cognition promotion replays exactly as it was applied
//! (canonical §28.9 item 3, architecture/09 §12.2.1).

use std::sync::Arc;

use forme_cognition::{
    AgentSelfModel, ChangeDirection, CognitiveMapStore, CognitiveMapUpdateProposal,
    CognitiveRuntime, DelegationState, EvolutionGovernor, GovernanceCandidate, GovernanceDecision,
    GovernanceEvidence, MapScope, MapUpdateKind, PartnershipObservation, PromotionAsymmetry,
    SelfObservation, StableCognition, StableCognitionKind, TemporalEvidence, TemporalModels, Tick,
    TrustLevel, TrustObservation, UserModel, UserObservation,
};
use forme_memory::{
    CandidateSpec, CandidateState, EventSourcedMemory, EvidencePriority, TimeScale,
};
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore, StoreOptions};

struct Brain {
    memory: Arc<EventSourcedMemory<SqliteEventStore>>,
    cognition: Arc<CognitiveRuntime<SqliteEventStore>>,
    models: TemporalModels<SqliteEventStore>,
}

fn open(store: &Arc<SqliteEventStore>, run: &p::RunId) -> Brain {
    let memory =
        Arc::new(EventSourcedMemory::with_clock(store.clone(), run.clone(), || 100).unwrap());
    let cognition = Arc::new(
        CognitiveRuntime::with_clock(
            store.clone(),
            memory.clone(),
            run.clone(),
            PromotionAsymmetry::default(),
            || 100,
        )
        .unwrap(),
    );
    let models = TemporalModels::new(memory.clone(), cognition.clone());
    Brain {
        memory,
        cognition,
        models,
    }
}

fn fresh() -> (Arc<SqliteEventStore>, p::RunId, Brain) {
    let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    let run = p::RunId("b0:brain".into());
    let brain = open(&store, &run);
    (store, run, brain)
}

fn owner() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}

fn scope() -> MapScope {
    MapScope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:b0".into()),
    }
}

fn evidence(reference: &str, observed_at: i64) -> TemporalEvidence {
    TemporalEvidence {
        schema_version: p::SchemaVersion(1),
        reference: p::EvidenceRef(reference.into()),
        observed_at,
        verified_process: true,
    }
}

fn governed(
    candidate_id: p::CandidateId,
    object: &str,
    kind: StableCognitionKind,
    direction: ChangeDirection,
) -> GovernanceCandidate {
    let evidence = vec![
        GovernanceEvidence {
            schema_version: p::SchemaVersion(1),
            reference: p::EvidenceRef("evidence:b0:first".into()),
            observed_at: 10,
            verified_process: true,
        },
        GovernanceEvidence {
            schema_version: p::SchemaVersion(1),
            reference: p::EvidenceRef("evidence:b0:second".into()),
            observed_at: 20,
            verified_process: true,
        },
    ];
    GovernanceCandidate {
        schema_version: p::SchemaVersion(1),
        candidate_id,
        direction,
        confidence: p::Confidence(0.9),
        evidence: evidence.clone(),
        impact: p::Impact::Low,
        provenance: owner(),
        conflicts: Vec::new(),
        owner_confirmed: true,
        target: StableCognition {
            schema_version: p::SchemaVersion(1),
            object: p::ObjectRef(object.into()),
            scope: scope(),
            kind,
            statement: format!("stable statement for {object}"),
            tier: p::StabilityTier::Stable,
            confidence: p::Confidence(0.9),
            evidence,
            last_reproduced_at: 30,
            active: false,
        },
    }
}

fn frame_proposal(brain: &Brain, frame: &str) -> p::CandidateId {
    brain
        .cognition
        .propose_update(CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: scope(),
            kind: MapUpdateKind::Frame,
            confidence: p::Confidence(0.3),
            evidence: vec![p::EvidenceRef("trace:b0".into())],
            frame: Some(p::JudgmentFrameRef(frame.into())),
            quality: None,
            blindspot: None,
            resource: None,
            reflection_inputs: vec![p::EvidenceRef("feedback:b0".into())],
            statement: format!("stable statement for {frame}"),
            provenance: owner(),
        })
        .unwrap()
}

fn spec_candidate(brain: &Brain, target: &str) -> p::CandidateId {
    let id = brain.memory.reserve_candidate_id();
    brain
        .memory
        .create_candidate_spec(CandidateSpec {
            schema_version: p::SchemaVersion(1),
            id: id.clone(),
            target: p::CandidateTargetRef(target.into()),
            evidence_refs: vec![p::EvidenceRef("evidence:b0:first".into())],
            confidence: p::Confidence(0.9),
            provenance: owner(),
            target_tier: p::StabilityTier::Stable,
        })
        .unwrap();
    id
}

fn sorted_stable(brain: &Brain) -> Vec<StableCognition> {
    let mut stable = brain.cognition.stable_objects();
    stable.sort_by(|left, right| left.object.cmp(&right.object));
    stable
}

fn event_count(store: &SqliteEventStore, run: &p::RunId) -> usize {
    store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .len()
}

#[test]
fn promotions_replay_field_for_field_across_every_stable_family() {
    let (store, run, brain) = fresh();

    let frame = frame_proposal(&brain, "frame:verify-first");
    assert_eq!(
        brain
            .cognition
            .govern_and_apply(governed(
                frame,
                "frame:verify-first",
                StableCognitionKind::Frame,
                ChangeDirection::CautionIncreasing,
            ))
            .unwrap(),
        GovernanceDecision::Promote
    );

    let attribute = UserModel::observe(
        &brain.models,
        UserObservation {
            schema_version: p::SchemaVersion(1),
            attribute: p::UserAttributeRef("review-style".into()),
            value: p::UserAttributeValueRef("rigorous".into()),
            evidence: vec![evidence("turn:b0:1", 20), evidence("turn:b0:2", 30)],
            confidence: p::Confidence(0.9),
            scope: p::Scope("workspace:b0".into()),
            time_scale: TimeScale::Repeated,
            feedback: vec![p::FeedbackRef("owner-confirmed".into())],
            provenance: owner(),
            evidence_priority: EvidencePriority::Process,
        },
    )
    .unwrap();
    assert_eq!(
        brain.models.review_user_candidate(attribute, true).unwrap(),
        GovernanceDecision::Promote
    );

    let capability = AgentSelfModel::observe(
        &brain.models,
        SelfObservation {
            schema_version: p::SchemaVersion(1),
            capability: p::CapabilityRef("tool:file".into()),
            reliability: p::Reliability("verified".into()),
            gap: Some("large binary edits".into()),
            evidence: vec![evidence("run:b0:1", 10), evidence("run:b0:2", 20)],
            confidence: p::Confidence(0.9),
            scope: p::Scope("workspace:b0".into()),
            time_scale: TimeScale::Repeated,
            provenance: owner(),
            initial_profile: false,
        },
    )
    .unwrap();
    assert_eq!(
        brain
            .models
            .review_agent_self_candidate(capability, true)
            .unwrap(),
        GovernanceDecision::Promote
    );

    let partnership = brain
        .models
        .propose_partnership(PartnershipObservation {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("workspace:b0".into()),
            complement: vec!["owner sets direction".into(), "agent verifies".into()],
            delegation_state: DelegationState::Bounded,
            evidence: vec![evidence("collab:b0:1", 10), evidence("collab:b0:2", 20)],
            time_scale: TimeScale::Repeated,
            provenance: owner(),
        })
        .unwrap();
    assert_eq!(
        brain
            .models
            .review_partnership_candidate(partnership, true)
            .unwrap(),
        GovernanceDecision::Promote
    );

    let trust = brain
        .models
        .propose_trust(TrustObservation {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("workspace:b0".into()),
            trust_level: TrustLevel::Established,
            delegation_state: DelegationState::Bounded,
            evidence: vec![evidence("verify:b0:1", 10), evidence("verify:b0:2", 20)],
            time_scale: TimeScale::Repeated,
            provenance: owner(),
            evidence_priority: EvidencePriority::Process,
        })
        .unwrap();
    assert_eq!(
        brain.models.review_trust_candidate(trust, true).unwrap(),
        GovernanceDecision::Promote
    );

    let online = sorted_stable(&brain);
    let online_map = brain.cognition.read(scope());
    let mut kinds = Vec::new();
    for stable in &online {
        if !kinds.contains(&stable.kind) {
            kinds.push(stable.kind);
        }
    }
    assert_eq!(
        kinds.len(),
        5,
        "every promoted family is present online: {online:?}"
    );
    drop(brain);

    let replayed = open(&store, &run);
    assert_eq!(
        sorted_stable(&replayed),
        online,
        "replay reproduces every stable object field for field"
    );
    assert_eq!(replayed.cognition.read(scope()), online_map);
    for stable in &online {
        assert!(replayed.cognition.is_object_active(&stable.object));
    }
}

#[test]
fn restart_keeps_reproduction_time_so_nothing_turns_stale() {
    let (store, run, brain) = fresh();
    let frame = frame_proposal(&brain, "frame:fresh");
    brain
        .cognition
        .govern_and_apply(governed(
            frame,
            "frame:fresh",
            StableCognitionKind::Frame,
            ChangeDirection::CautionIncreasing,
        ))
        .unwrap();
    drop(brain);

    let replayed = open(&store, &run);
    let stale = replayed.cognition.decay(Tick {
        schema_version: p::SchemaVersion(1),
        now: 40,
        stale_after_ms: 1_000,
    });
    assert!(
        stale.is_empty(),
        "a cognition reproduced at 30 is not stale at 40 after a restart"
    );
    assert!(replayed
        .cognition
        .is_object_active(&p::ObjectRef("frame:fresh".into())));
}

#[test]
fn a_target_that_is_not_the_recorded_object_writes_nothing() {
    let (store, run, brain) = fresh();
    let candidate = spec_candidate(&brain, "stable:frame:recorded");
    let events = event_count(&store, &run);
    let error = brain
        .cognition
        .govern_and_apply(governed(
            candidate.clone(),
            "frame:substituted",
            StableCognitionKind::Frame,
            ChangeDirection::CautionIncreasing,
        ))
        .unwrap_err();
    assert!(error.0.contains("recorded object"), "{error:?}");
    assert_eq!(event_count(&store, &run), events, "zero events");
    assert!(brain.cognition.stable_objects().is_empty(), "zero state");
    assert_eq!(
        brain.memory.candidate_record(&candidate).unwrap().state,
        CandidateState::Candidate
    );

    let unrecorded = spec_candidate(&brain, "caution:no-object");
    assert!(brain
        .cognition
        .govern_and_apply(governed(
            unrecorded,
            "frame:anything",
            StableCognitionKind::Frame,
            ChangeDirection::CautionIncreasing,
        ))
        .is_err());
    assert_eq!(
        event_count(&store, &run),
        events + 1,
        "only the spec was written"
    );
}

#[test]
fn an_owner_downgrade_replays_on_the_recorded_object() {
    let (store, run, brain) = fresh();
    let promoted = spec_candidate(&brain, "stable:frame:downgraded");
    brain
        .cognition
        .govern_and_apply(governed(
            promoted,
            "frame:downgraded",
            StableCognitionKind::Frame,
            ChangeDirection::CautionIncreasing,
        ))
        .unwrap();
    let correction = spec_candidate(&brain, "downgrade:frame:downgraded");
    assert_eq!(
        brain
            .cognition
            .govern_and_apply(governed(
                correction,
                "frame:downgraded",
                StableCognitionKind::Frame,
                ChangeDirection::CorrectionDowngrade,
            ))
            .unwrap(),
        GovernanceDecision::Downgrade
    );
    let object = p::ObjectRef("frame:downgraded".into());
    assert!(!brain.cognition.is_object_active(&object));
    let online = sorted_stable(&brain);
    drop(brain);

    let replayed = open(&store, &run);
    assert!(!replayed.cognition.is_object_active(&object));
    assert_eq!(sorted_stable(&replayed), online);
}

#[test]
fn a_historical_promotion_without_a_record_keeps_its_original_replay() {
    let (store, run, brain) = fresh();
    let legacy = frame_proposal(&brain, "frame:legacy");
    drop(brain);
    store
        .append(p::Event::new(
            p::EventId("b0:legacy-promotion".into()),
            run.clone(),
            None,
            p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                candidate_id: legacy,
                by: p::DecisionActor::User,
                reason: p::ReasonRef("recorded before B0".into()),
                stable: None,
            }),
            p::SchemaVersion(1),
            50,
            owner(),
        ))
        .unwrap();

    let replayed = open(&store, &run);
    let stable = replayed.cognition.stable_objects();
    assert_eq!(stable.len(), 1);
    assert_eq!(stable[0].object, p::ObjectRef("frame:legacy".into()));
    assert_eq!(stable[0].confidence, p::Confidence(0.3));
}
