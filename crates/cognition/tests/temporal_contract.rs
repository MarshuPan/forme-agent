use std::sync::Arc;

use forme_cognition::{
    AgentSelfModel, CognitiveRuntime, DelegationState, EvolutionGovernor, GovernanceDecision,
    PartnershipModel, PartnershipObservation, PromotionAsymmetry, RetractionEvent, SelfObservation,
    TemporalEvidence, TemporalModels, TrustLevel, TrustObservation, UserModel, UserObservation,
};
use forme_memory::{EventSourcedMemory, EvidencePriority, ImportedHistoricalEvidence, TimeScale};
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore, StoreOptions};

type RuntimeFixture = (
    Arc<SqliteEventStore>,
    Arc<EventSourcedMemory<SqliteEventStore>>,
    Arc<CognitiveRuntime<SqliteEventStore>>,
    TemporalModels<SqliteEventStore>,
    p::RunId,
);

fn provenance(tier: p::TrustTier, actor: p::Actor) -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor,
        trust_tier: tier,
        caused_by: None,
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

fn runtime() -> RuntimeFixture {
    let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    let run = p::RunId("temporal:test".into());
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
    (store, memory, cognition, models, run)
}

#[test]
fn s8_user_model_is_temporal_candidate_first_and_history_is_bootstrap_only() {
    let (store, memory, cognition, models, run) = runtime();
    models
        .import_historical(ImportedHistoricalEvidence {
            schema_version: p::SchemaVersion(1),
            source: p::HistoricalSourceRef("archive:old-notes".into()),
            low_weight: true,
            bootstrap_only: true,
            provenance: provenance(p::TrustTier::ApprovedSource, p::Actor::System),
        })
        .unwrap();
    assert_eq!(memory.imported_history().len(), 1);
    assert!(models
        .propose_trust(TrustObservation {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("workspace:test".into()),
            trust_level: TrustLevel::Established,
            delegation_state: DelegationState::Bounded,
            evidence: vec![evidence("history:1", 1)],
            time_scale: TimeScale::LongTerm,
            provenance: provenance(p::TrustTier::ApprovedSource, p::Actor::System),
            evidence_priority: EvidencePriority::Imported,
        })
        .is_err());
    assert!(models
        .trust_profile(p::Scope("workspace:test".into()))
        .is_none());

    let single = UserModel::observe(
        &models,
        UserObservation {
            schema_version: p::SchemaVersion(1),
            attribute: p::UserAttributeRef("review-style".into()),
            value: p::UserAttributeValueRef("rigorous".into()),
            evidence: vec![evidence("turn:1", 10)],
            confidence: p::Confidence(0.9),
            scope: p::Scope("workspace:test".into()),
            time_scale: TimeScale::LongTerm,
            feedback: Vec::new(),
            provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
            evidence_priority: EvidencePriority::Process,
        },
    )
    .unwrap();
    assert_eq!(
        memory.user_candidate(&single).unwrap().stability,
        p::StabilityTier::Working
    );
    assert!(models
        .query(
            p::UserAttributeRef("review-style".into()),
            p::Scope("workspace:test".into())
        )
        .is_none());

    let repeated = UserModel::observe(
        &models,
        UserObservation {
            schema_version: p::SchemaVersion(1),
            attribute: p::UserAttributeRef("review-style".into()),
            value: p::UserAttributeValueRef("rigorous".into()),
            evidence: vec![evidence("turn:2", 20), evidence("turn:3", 30)],
            confidence: p::Confidence(0.9),
            scope: p::Scope("workspace:test".into()),
            time_scale: TimeScale::Repeated,
            feedback: vec![p::FeedbackRef("owner-confirmed".into())],
            provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
            evidence_priority: EvidencePriority::Process,
        },
    )
    .unwrap();
    assert_eq!(
        models
            .review_user_candidate(repeated.clone(), false)
            .unwrap(),
        GovernanceDecision::Confirm
    );
    assert!(models
        .query(
            p::UserAttributeRef("review-style".into()),
            p::Scope("workspace:test".into())
        )
        .is_none());
    assert_eq!(
        models.review_user_candidate(repeated, true).unwrap(),
        GovernanceDecision::Promote
    );
    assert_eq!(
        models
            .query(
                p::UserAttributeRef("review-style".into()),
                p::Scope("workspace:test".into())
            )
            .unwrap()
            .value,
        p::UserAttributeValueRef("rigorous".into())
    );
    cognition.on_retraction(RetractionEvent {
        schema_version: p::SchemaVersion(1),
        target_object: p::ObjectRef("user-attribute:review-style:workspace:test".into()),
        evidence_lineage: p::LineageRef("lineage:owner-correction".into()),
        provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
    });
    assert!(models
        .query(
            p::UserAttributeRef("review-style".into()),
            p::Scope("workspace:test".into())
        )
        .is_none());

    let kinds = store
        .read_run(run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    let imported = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::ImportedHistoricalEvidenceRecorded)
        .unwrap();
    let first_candidate = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::UserAttributeCandidateCreated)
        .unwrap();
    let promoted = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::CandidatePromoted)
        .unwrap();
    let retracted = kinds
        .iter()
        .position(|kind| *kind == p::EventKind::RetractionEvent)
        .unwrap();
    assert!(imported < first_candidate);
    assert!(first_candidate < promoted);
    assert!(promoted < retracted);
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == p::EventKind::UserAttributeCandidateCreated)
            .count(),
        2
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == p::EventKind::CandidatePromoted)
            .count(),
        1
    );
}

#[test]
fn agent_self_partnership_and_trust_require_process_evidence_and_rebuild() {
    let (store, memory, cognition, models, run) = runtime();
    let initial = AgentSelfModel::observe(
        &models,
        SelfObservation {
            schema_version: p::SchemaVersion(1),
            capability: p::CapabilityRef("tool:file".into()),
            reliability: p::Reliability("declared".into()),
            gap: None,
            evidence: vec![evidence("profile:1", 1)],
            confidence: p::Confidence(0.9),
            scope: p::Scope("workspace:test".into()),
            time_scale: TimeScale::LongTerm,
            provenance: provenance(p::TrustTier::ApprovedSource, p::Actor::System),
            initial_profile: true,
        },
    )
    .unwrap();
    assert_eq!(
        models.review_agent_self_candidate(initial, true).unwrap(),
        GovernanceDecision::Confirm
    );
    assert!(models
        .capability(p::CapabilityRef("tool:file".into()))
        .is_none());

    let capability = AgentSelfModel::observe(
        &models,
        SelfObservation {
            schema_version: p::SchemaVersion(1),
            capability: p::CapabilityRef("tool:file".into()),
            reliability: p::Reliability("verified".into()),
            gap: Some("large binary edits".into()),
            evidence: vec![evidence("run:1", 10), evidence("run:2", 20)],
            confidence: p::Confidence(0.9),
            scope: p::Scope("workspace:test".into()),
            time_scale: TimeScale::Repeated,
            provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
            initial_profile: false,
        },
    )
    .unwrap();
    assert_eq!(
        models
            .review_agent_self_candidate(capability, true)
            .unwrap(),
        GovernanceDecision::Promote
    );

    let partnership = models
        .propose_partnership(PartnershipObservation {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("workspace:test".into()),
            complement: vec![
                "owner sets product judgment".into(),
                "agent verifies changes".into(),
            ],
            delegation_state: DelegationState::Bounded,
            evidence: vec![evidence("collab:1", 10), evidence("collab:2", 20)],
            time_scale: TimeScale::Repeated,
            provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
        })
        .unwrap();
    assert_eq!(
        models
            .review_partnership_candidate(partnership, true)
            .unwrap(),
        GovernanceDecision::Promote
    );

    let trust = models
        .propose_trust(TrustObservation {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("workspace:test".into()),
            trust_level: TrustLevel::Established,
            delegation_state: DelegationState::Bounded,
            evidence: vec![evidence("verify:1", 10), evidence("verify:2", 20)],
            time_scale: TimeScale::Repeated,
            provenance: provenance(p::TrustTier::OwnerInput, p::Actor::Owner),
            evidence_priority: EvidencePriority::Process,
        })
        .unwrap();
    assert_eq!(
        models.review_trust_candidate(trust, true).unwrap(),
        GovernanceDecision::Promote
    );
    assert_eq!(
        models
            .capability(p::CapabilityRef("tool:file".into()))
            .unwrap()
            .reliability,
        p::Reliability("verified".into())
    );
    assert_eq!(
        models
            .state(p::Scope("workspace:test".into()))
            .delegation_state,
        DelegationState::Bounded
    );
    assert_eq!(
        models
            .trust_profile(p::Scope("workspace:test".into()))
            .unwrap()
            .trust_level,
        TrustLevel::Established
    );

    drop(models);
    drop(cognition);
    drop(memory);
    let rebuilt_memory =
        Arc::new(EventSourcedMemory::with_clock(store.clone(), run.clone(), || 200).unwrap());
    let rebuilt_cognition = Arc::new(
        CognitiveRuntime::with_clock(
            store,
            rebuilt_memory.clone(),
            run,
            PromotionAsymmetry::default(),
            || 200,
        )
        .unwrap(),
    );
    let rebuilt = TemporalModels::new(rebuilt_memory, rebuilt_cognition);
    assert!(rebuilt
        .capability(p::CapabilityRef("tool:file".into()))
        .is_some());
    assert_eq!(
        rebuilt
            .state(p::Scope("workspace:test".into()))
            .delegation_state,
        DelegationState::Bounded
    );
    assert!(rebuilt
        .trust_profile(p::Scope("workspace:test".into()))
        .is_some());
}
