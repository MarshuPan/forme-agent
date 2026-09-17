use std::sync::Arc;

use forme_cognition::*;
use forme_memory as memory;
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore, StoreOptions};

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn competence_with_results() -> CompetenceInputs {
    CompetenceInputs {
        schema_version: p::SchemaVersion(1),
        map_confidence: Some(MapConfidenceInput {
            schema_version: p::SchemaVersion(1),
            reference: p::MapConfidenceRef("map:scope".into()),
            value: p::Confidence(0.9),
        }),
        self_model: Some(AgentSelfInput {
            schema_version: p::SchemaVersion(1),
            reference: p::AgentSelfModelRef("self:scope".into()),
            confidence: p::Confidence(0.9),
        }),
        capability_evidence: vec![
            CapabilityEvidenceInput {
                schema_version: p::SchemaVersion(1),
                reference: p::CapabilityEvidenceRef("capability:result-1".into()),
                verified_success: true,
                reliability: p::Confidence(0.95),
            },
            CapabilityEvidenceInput {
                schema_version: p::SchemaVersion(1),
                reference: p::CapabilityEvidenceRef("capability:result-2".into()),
                verified_success: true,
                reliability: p::Confidence(0.92),
            },
        ],
        trust: Some(TrustInput {
            schema_version: p::SchemaVersion(1),
            reference: p::TrustProfileRef("trust:scope".into()),
            ceiling: InterventionLevel::L4Autonomous,
        }),
        failure: Vec::new(),
        verification: vec![VerificationEvidenceInput {
            schema_version: p::SchemaVersion(1),
            reference: p::EvidenceRef("verification:pass".into()),
            passed: true,
        }],
    }
}

#[test]
fn s18_competence_is_raised_by_outcomes_and_self_assessment_can_only_lower_it() {
    let gate = EvidenceCompetenceGate::default();
    let mut self_only = competence_with_results();
    self_only.capability_evidence.clear();
    self_only.verification.clear();
    assert_eq!(
        gate.ceiling(p::Scope("workspace:test".into()), p::Risk::Low, &self_only),
        InterventionLevel::L1Suggest
    );

    let grounded = competence_with_results();
    assert_eq!(
        gate.ceiling(p::Scope("workspace:test".into()), p::Risk::Low, &grounded),
        InterventionLevel::L4Autonomous
    );

    let mut low_self = grounded.clone();
    low_self.self_model.as_mut().unwrap().confidence = p::Confidence(0.2);
    assert_eq!(
        gate.ceiling(p::Scope("workspace:test".into()), p::Risk::Low, &low_self),
        InterventionLevel::L0Observe
    );

    let mut failed = grounded;
    failed.failure.push(FailureEvidenceInput {
        schema_version: p::SchemaVersion(1),
        reference: p::FailureEvidenceRef("failure:recent".into()),
        impact: p::Impact::High,
    });
    assert_eq!(
        gate.ceiling(p::Scope("workspace:test".into()), p::Risk::Low, &failed),
        InterventionLevel::L2Prepare
    );
    assert_eq!(
        failed.protocol_reads().failure_evidence,
        vec![p::FailureEvidenceRef("failure:recent".into())]
    );
    assert_eq!(
        failed.protocol_reads().verification_evidence,
        vec![p::EvidenceRef("verification:pass".into())]
    );
}

fn observation(signal: ImpulseSource, seed: &str) -> Observation {
    Observation {
        schema_version: p::SchemaVersion(1),
        source: p::Source::Internal,
        scope: p::Scope("workspace:test".into()),
        grant_ref: Some(p::GrantRef("observation-grant".into())),
        authorized: true,
        seed: vec![p::NodeId(seed.into())],
        signal,
        estimated_value: 80,
        urgency: 50,
        requested_level: InterventionLevel::L3ActWithApproval,
        delivery: DeliveryMode::Interrupt,
        proposal_intent: if signal == ImpulseSource::Gap {
            ProposalIntent::Communication(CommunicationPurpose::AskToLearn)
        } else {
            ProposalIntent::Learning
        },
    }
}

#[test]
fn s7_timer_floor_maps_five_sources_and_ask_to_learn_respects_attention_budget() {
    let config = ProactivityConfig {
        schema_version: p::SchemaVersion(1),
        minimum_value: 40,
        workspace_capacity: 10,
        intention_lease_ms: 100,
        attention: AttentionBudget {
            schema_version: p::SchemaVersion(1),
            quiet_hours: vec![TimeWindow {
                schema_version: p::SchemaVersion(1),
                starts_minute_utc: 0,
                ends_minute_utc: 1_440,
            }],
            interrupt_rate: RatePolicy {
                schema_version: p::SchemaVersion(1),
                max_interrupts: 1,
                window_ms: 1_000,
            },
            urgent_interrupt_threshold: 90,
        },
    };
    let engine = M0ProactivityEngine::new(config);
    let mut observations = vec![
        observation(ImpulseSource::Gap, "gap"),
        observation(ImpulseSource::Change, "change"),
        observation(ImpulseSource::Tension, "tension"),
        observation(ImpulseSource::Association, "association"),
        observation(ImpulseSource::Pressure, "pressure"),
    ];
    let mut unauthorized = observation(ImpulseSource::Gap, "unauthorized");
    unauthorized.authorized = false;
    observations.push(unauthorized);
    let snapshot = CognitionSnapshot {
        schema_version: p::SchemaVersion(1),
        now: 60_000,
        observations,
        competence: competence_with_results(),
    };
    let impulses = engine.tick(TickTrigger::PostTurn, &snapshot);
    assert_eq!(impulses.len(), 5);
    assert!(impulses
        .iter()
        .all(|impulse| impulse.activation_shape.is_some()));
    assert!(impulses
        .iter()
        .all(|impulse| impulse.delivery == DeliveryMode::Hitchhike));
    assert_eq!(engine.interruptions_used(), 0);
    assert!(!impulses
        .iter()
        .flat_map(|impulse| &impulse.seed)
        .any(|seed| seed.0 == "unauthorized"));

    let gap = impulses
        .into_iter()
        .find(|impulse| impulse.source == ImpulseSource::Gap)
        .unwrap();
    let proposal = engine
        .emit(
            gap,
            EmissionGuard {
                schema_version: p::SchemaVersion(1),
                value: ValueDecision::Worth(Value(80)),
                competence: InterventionLevel::L1Suggest,
            },
        )
        .unwrap();
    assert!(matches!(
        proposal,
        Proposal::Communication(CommunicationProposal {
            purpose: CommunicationPurpose::AskToLearn,
            ..
        })
    ));
    assert_eq!(proposal.level(), InterventionLevel::L1Suggest);
    engine
        .resolve(&proposal, p::ProposalOutcome::Reject, None)
        .unwrap();
    assert_eq!(
        engine
            .tick(TickTrigger::PostTurn, &snapshot)
            .into_iter()
            .filter(|impulse| impulse.source == ImpulseSource::Gap)
            .count(),
        0
    );
}

#[test]
fn attention_rate_counts_interrupts_but_never_counts_hitchhike_delivery() {
    let engine = M0ProactivityEngine::new(ProactivityConfig {
        attention: AttentionBudget {
            schema_version: p::SchemaVersion(1),
            quiet_hours: Vec::new(),
            interrupt_rate: RatePolicy {
                schema_version: p::SchemaVersion(1),
                max_interrupts: 1,
                window_ms: 10_000,
            },
            urgent_interrupt_threshold: 90,
        },
        ..ProactivityConfig::default()
    });
    let first = observation(ImpulseSource::Change, "first");
    let second = observation(ImpulseSource::Change, "second");
    let mut hitchhike = observation(ImpulseSource::Gap, "hitchhike");
    hitchhike.delivery = DeliveryMode::Hitchhike;
    let impulses = engine.tick(
        TickTrigger::PostTurn,
        &CognitionSnapshot {
            schema_version: p::SchemaVersion(1),
            now: 100,
            observations: vec![first, second, hitchhike],
            competence: CompetenceInputs::default(),
        },
    );
    assert_eq!(engine.interruptions_used(), 1);
    assert_eq!(
        impulses
            .iter()
            .filter(|impulse| impulse.delivery == DeliveryMode::Interrupt)
            .count(),
        1
    );
    assert_eq!(
        impulses
            .iter()
            .filter(|impulse| impulse.delivery == DeliveryMode::Digest)
            .count(),
        1
    );
}

#[test]
fn commitment_is_claimed_once_without_activation_and_defer_creates_an_agenda_item() {
    let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    let aggregate = p::RunId("memory:proactivity-intentions".into());
    let memory = Arc::new(
        memory::EventSourcedMemory::with_clock(store.clone(), aggregate.clone(), || 100).unwrap(),
    );
    memory::IntentionStore::create(
        memory.as_ref(),
        memory::ProspectiveIntention {
            schema_version: p::SchemaVersion(1),
            id: p::IntentionId("commitment:review".into()),
            source: p::IntentionSource::Commitment,
            trigger: memory::IntentionTrigger::At(100),
            state: memory::IntentionState::Pending,
            seed: p::SeedRef("review-m0".into()),
            provenance: provenance(),
            expires_at: None,
        },
    )
    .unwrap();
    let engine = M0ProactivityEngine::new(ProactivityConfig {
        intention_lease_ms: 1_000,
        ..ProactivityConfig::default()
    })
    .with_intention_store(memory.clone());
    let snapshot = CognitionSnapshot {
        schema_version: p::SchemaVersion(1),
        now: 100,
        observations: Vec::new(),
        competence: competence_with_results(),
    };
    let impulses = engine.tick(TickTrigger::Schedule, &snapshot);
    assert_eq!(impulses.len(), 1);
    assert_eq!(impulses[0].source, ImpulseSource::Commitment);
    assert_eq!(impulses[0].activation_shape, None);
    assert!(engine.tick(TickTrigger::Schedule, &snapshot).is_empty());
    let proposal = engine
        .emit(
            impulses[0].clone(),
            EmissionGuard {
                schema_version: p::SchemaVersion(1),
                value: impulses[0].value.clone(),
                competence: InterventionLevel::L1Suggest,
            },
        )
        .unwrap();
    engine
        .resolve(&proposal, p::ProposalOutcome::Adopt, None)
        .unwrap();
    assert!(engine
        .tick(
            TickTrigger::Schedule,
            &CognitionSnapshot {
                now: 10_000,
                ..snapshot.clone()
            }
        )
        .is_empty());

    let deferred = engine
        .resolve(&proposal, p::ProposalOutcome::Defer, Some(20_000))
        .unwrap()
        .unwrap();
    assert!(deferred.0.starts_with("deferred:"));
    let kinds = store
        .read_run(aggregate)
        .map(|event| event.unwrap().kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| **kind == p::EventKind::ProspectiveIntentionResolved)
            .count(),
        2
    );
    assert_eq!(
        kinds.last(),
        Some(&p::EventKind::ProspectiveIntentionCreated)
    );
}

#[test]
fn delegation_proposal_is_evidence_bound_and_has_no_action_side_effect() {
    let engine = M0ProactivityEngine::default();
    let impulse = Impulse {
        schema_version: p::SchemaVersion(1),
        source: ImpulseSource::Gap,
        observation_source: p::Source::Internal,
        reach: Reach::Collaborate,
        seed: vec![p::NodeId("capability-gap".into())],
        activation_shape: Some(p::ActivationShape::Gap),
        scope: p::Scope("workspace:test".into()),
        grant_ref: None,
        value: ValueDecision::Worth(Value(90)),
        urgency: 80,
        requested_level: InterventionLevel::L5HighImpact,
        delivery: DeliveryMode::Hitchhike,
        proposal_intent: ProposalIntent::Delegation,
        intention_id: None,
        capability_evidence: vec![p::CapabilityEvidenceRef("capability:evidence".into())],
    };
    let proposal = engine
        .emit(
            impulse,
            EmissionGuard {
                schema_version: p::SchemaVersion(1),
                value: ValueDecision::Worth(Value(90)),
                competence: InterventionLevel::L2Prepare,
            },
        )
        .unwrap();
    let Proposal::Delegation(delegation) = proposal else {
        panic!("expected a delegation proposal");
    };
    assert_eq!(
        delegation.capability_evidence,
        vec![p::CapabilityEvidenceRef("capability:evidence".into())]
    );
    assert!(delegation.core.confirmation_required);
    assert_eq!(delegation.core.level, InterventionLevel::L2Prepare);
}

fn append(store: &SqliteEventStore, run: &p::RunId, id: &str, payload: p::EventPayload) {
    store
        .append(p::Event::new(
            p::EventId(id.into()),
            run.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            100,
            provenance(),
        ))
        .unwrap();
}

#[test]
fn agent_workspace_is_a_bounded_rebuildable_event_projection() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let run = p::RunId("run:workspace".into());
    append(
        &store,
        &run,
        "accepted",
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId("session:workspace".into()),
            input_ref: p::InputRef("input".into()),
            idempotency_key: None,
        }),
    );
    append(
        &store,
        &run,
        "goal",
        p::EventPayload::GoalFramed(p::GoalFramedPayload {
            goal_frame: p::GoalFrameRef("goal:workspace".into()),
            long_term: None,
        }),
    );
    for index in 0..4 {
        append(
            &store,
            &run,
            &format!("impulse-{index}"),
            p::EventPayload::ImpulseRaised(p::ImpulseRaisedPayload {
                source: p::ImpulseSource::Gap,
                reach: p::Reach(2),
                seed: vec![p::NodeId(format!("seed-{index}"))],
            }),
        );
    }
    let events = store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let first = AgentWorkspaceProjection::rebuild(&events, 3);
    let second = AgentWorkspaceProjection::rebuild(&events, 3);
    assert_eq!(first, second);
    assert_eq!(first.items.len(), 3);
    assert_eq!(first.capacity, 3);
    assert!(first
        .items
        .iter()
        .any(|item| item.kind == AgentWorkspaceItemKind::Run));

    append(
        &store,
        &run,
        "complete",
        p::EventPayload::RunComplete(p::RunCompletePayload {
            stop_reason: p::StopReason("final_output".into()),
            result_ref: Some(p::EventId("result".into())),
        }),
    );
    let events = store.read_run(run).collect::<p::Result<Vec<_>>>().unwrap();
    let completed = AgentWorkspaceProjection::rebuild(&events, 10);
    assert!(!completed
        .items
        .iter()
        .any(|item| item.kind == AgentWorkspaceItemKind::Run));
}
