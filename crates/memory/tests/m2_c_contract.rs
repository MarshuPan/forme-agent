use std::sync::Arc;

use forme_memory::{
    CandidateState, CapabilityGrowthEngine, EventSourcedMemory, HotColdMemoryProjector,
    HotColdProjectionPolicy, IntentionOutcome, IntentionStore, LongTermContinuationDecision,
    LongTermContinuationGuard, SelectiveRecall,
};
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

fn envelope(scope: &str, capability: &str, approval_rule: p::ApprovalRule) -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope(scope.into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef(capability.into())],
            permissions: vec![p::PermissionRef("permission:read".into())],
        },
        action_type: vec![p::ActionType::Analyze],
        risk_limit: p::Risk::Low,
        approval_rule,
        budget: p::Budget("units:4".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 10,
            expires_at: 1_000,
            max_turns: 4,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: true,
            boundary: Some(p::RollbackBoundary("discard prepared result".into())),
        },
    }
}

fn schedule_command(id: &str) -> p::ScheduleCommand {
    let schedule_envelope = envelope(
        "workspace:alpha/goal:release",
        "capability:planner",
        p::ApprovalRule::Ask,
    );
    p::ScheduleCommand {
        schema_version: p::SchemaVersion(1),
        intention: p::ProspectiveIntention {
            schema_version: p::SchemaVersion(1),
            id: p::IntentionId(id.into()),
            source: p::IntentionSource::Commitment,
            trigger: p::IntentionTrigger::At(100),
            state: p::IntentionState::Pending,
            seed: p::SeedRef("continue the release goal".into()),
            provenance: provenance(),
            expires_at: Some(900),
        },
        session: p::SessionId("session:long-term".into()),
        envelope: schedule_envelope,
        budget: p::Budget("units:4".into()),
    }
}

#[test]
fn s49_long_term_goal_lineage_rebuilds_and_yields_replans_or_stops_before_action() {
    let store = Arc::new(
        SqliteEventStore::open_in_memory(StoreOptions::default()).expect("open event store"),
    );
    let aggregate = p::RunId("memory:m2-c-long-term".into());
    let memory = EventSourcedMemory::open(Arc::clone(&store), aggregate.clone()).unwrap();
    let goal_frame = p::GoalFrameRef("goal-frame:release".into());
    let situation = p::SchemaDigest("sha256:situation-v1".into());
    memory
        .record_long_term_goal(
            p::LongTermGoal {
                schema_version: p::SchemaVersion(1),
                goal_frame: goal_frame.clone(),
                scope: p::Scope("workspace:alpha/goal:release".into()),
                situation_digest: situation.clone(),
                budget: p::Budget("units:4".into()),
                expires_at: 1_000,
            },
            provenance(),
        )
        .unwrap();
    let command = schedule_command("intention:release:1");
    memory
        .create_goal_schedule(goal_frame.clone(), command.clone())
        .unwrap();

    let creation_event = store
        .read_run(aggregate.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .into_iter()
        .find_map(|event| match event.payload {
            p::EventPayload::ProspectiveIntentionCreated(payload) => Some(payload),
            _ => None,
        })
        .unwrap();
    assert_eq!(creation_event.goal_frame, Some(goal_frame.clone()));
    assert_eq!(creation_event.schedule, Some(command.binding()));

    memory
        .record_goal_checkpoint(
            p::GoalCheckpoint {
                schema_version: p::SchemaVersion(1),
                reference: p::GoalCheckpointRef("checkpoint:release:1".into()),
                goal_frame: goal_frame.clone(),
                intention: command.intention.id.clone(),
                route: p::ExecutionRouteRef("route:release:1".into()),
                artifact: p::ContentRef("artifact:release-plan:1".into()),
                situation_digest: situation.clone(),
                evidence_refs: vec![p::EventId("verification:release-plan".into())],
                created_at: 120,
            },
            None,
            provenance(),
        )
        .unwrap();

    let lineage = memory.goal_lineage(&goal_frame).unwrap();
    assert_eq!(lineage.intentions, vec![command.intention.id.clone()]);
    assert_eq!(
        lineage.routes,
        vec![p::ExecutionRouteRef("route:release:1".into())]
    );
    assert_eq!(lineage.checkpoints.len(), 1);
    assert!(!lineage.cancelled);

    let reopened = EventSourcedMemory::open(Arc::clone(&store), aggregate.clone()).unwrap();
    assert_eq!(reopened.goal_lineage(&goal_frame).unwrap(), lineage);

    let guard = LongTermContinuationGuard;
    assert_eq!(
        guard
            .decide(&lineage, &situation, 200, true, 4)
            .unwrap()
            .decision,
        LongTermContinuationDecision::YieldToForeground
    );
    assert_eq!(
        guard
            .decide(
                &lineage,
                &p::SchemaDigest("sha256:situation-v2".into()),
                200,
                false,
                4,
            )
            .unwrap()
            .decision,
        LongTermContinuationDecision::Replan
    );
    let continuation = guard.decide(&lineage, &situation, 200, false, 4).unwrap();
    assert_eq!(
        continuation.decision,
        LongTermContinuationDecision::Continue
    );
    assert_eq!(continuation.intention, Some(command.intention.id.clone()));

    IntentionStore::resolve(
        &reopened,
        command.intention.id.clone(),
        IntentionOutcome::Cancelled,
    )
    .unwrap();
    let cancelled = reopened.goal_lineage(&goal_frame).unwrap();
    assert!(cancelled.cancelled);
    assert_eq!(
        guard
            .decide(&cancelled, &situation, 220, false, 4)
            .unwrap()
            .decision,
        LongTermContinuationDecision::Stop
    );
    let events = store
        .read_run(aggregate)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        vec![
            p::EventKind::GoalFramed,
            p::EventKind::ProspectiveIntentionCreated,
            p::EventKind::OrchestrationRouteCreated,
            p::EventKind::ProspectiveIntentionResolved,
        ]
    );
    assert!(!events.iter().any(|event| matches!(
        event.kind,
        p::EventKind::ActionPlanned | p::EventKind::ActionStarted
    )));
}

fn result_evidence(count: usize, owner_confirmed: bool) -> Vec<p::CapabilityResultEvidence> {
    (0..count)
        .map(|index| p::CapabilityResultEvidence {
            schema_version: p::SchemaVersion(1),
            evidence_ref: p::EvidenceRef(format!("verification:lint:{index}")),
            outcome: p::ResourceEvidenceOutcome::Pass,
            observed_at: 100 + index as i64,
            owner_feedback: (index + 1 == count).then_some(owner_confirmed),
        })
        .collect()
}

#[test]
fn s50_capability_growth_is_result_led_candidate_only_and_owner_grants_narrowly() {
    let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    let aggregate = p::RunId("memory:m2-c-capability".into());
    let memory = EventSourcedMemory::open(Arc::clone(&store), aggregate.clone()).unwrap();
    let engine = CapabilityGrowthEngine;
    let scope = p::Scope("workspace:alpha/resource:lint".into());
    let capability = p::CapabilityRef("capability:lint".into());

    let strong = engine
        .assess(
            p::CapabilityGapRef("gap:lint:strong".into()),
            capability.clone(),
            scope.clone(),
            result_evidence(3, true),
            Some(p::Confidence(0.9)),
        )
        .unwrap();
    assert_eq!(strong.ceiling, p::InterventionLevel::L4ActAutonomously);
    let self_limited = engine
        .assess(
            p::CapabilityGapRef("gap:lint:self-limited".into()),
            capability.clone(),
            scope.clone(),
            result_evidence(3, true),
            Some(p::Confidence(0.2)),
        )
        .unwrap();
    assert_eq!(self_limited.ceiling, p::InterventionLevel::L2Prepare);
    let one_success = engine
        .assess(
            p::CapabilityGapRef("gap:lint:one".into()),
            capability.clone(),
            scope.clone(),
            result_evidence(1, true),
            Some(p::Confidence(1.0)),
        )
        .unwrap();
    assert_eq!(one_success.ceiling, p::InterventionLevel::L2Prepare);

    let proposal = p::CapabilityUpdateProposal {
        schema_version: p::SchemaVersion(1),
        reference: p::CapabilityUpdateProposalRef("proposal:lint:narrow".into()),
        candidate_id: p::CandidateId("candidate:lint:narrow".into()),
        evidence_refs: strong
            .result_evidence
            .iter()
            .map(|evidence| evidence.evidence_ref.clone())
            .collect(),
        gap: strong,
        requested_envelope: envelope(&scope.0, &capability.0, p::ApprovalRule::Allow),
    };
    memory
        .propose_capability_update(proposal.clone(), provenance())
        .unwrap();
    assert_eq!(
        memory
            .candidate_record(&proposal.candidate_id)
            .unwrap()
            .state,
        CandidateState::Candidate
    );
    assert!(memory
        .review_capability_update(
            proposal.candidate_id.clone(),
            p::CandidateReviewDecision::Promote,
            p::VerifiedPrincipal(String::new()),
            200,
        )
        .is_err());
    let before_review = store
        .read_run(aggregate.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        before_review
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![p::EventKind::CandidateCreated]
    );

    let grant = memory
        .review_capability_update(
            proposal.candidate_id.clone(),
            p::CandidateReviewDecision::Promote,
            p::VerifiedPrincipal("owner:local".into()),
            200,
        )
        .unwrap()
        .unwrap();
    assert_eq!(grant.proposal, proposal.reference);
    assert_eq!(grant.envelope.scope, scope);
    assert_eq!(grant.envelope.capability.capabilities, vec![capability]);
    assert_eq!(grant.envelope.risk_limit, p::Risk::Low);
    assert_eq!(grant.envelope.timebox.max_turns, 4);

    let events = store
        .read_run(aggregate.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        vec![
            p::EventKind::CandidateCreated,
            p::EventKind::CandidatePromoted,
        ]
    );
    assert!(!events.iter().any(|event| matches!(
        event.kind,
        p::EventKind::AutonomyEnvelopeSet
            | p::EventKind::PluginToggled
            | p::EventKind::ToolsetResolved
    )));

    let reopened = EventSourcedMemory::open(store, aggregate).unwrap();
    assert_eq!(
        reopened
            .candidate_record(&proposal.candidate_id)
            .unwrap()
            .state,
        CandidateState::Promoted
    );
    assert_eq!(
        reopened
            .capability_update_proposal(&proposal.candidate_id)
            .unwrap(),
        proposal
    );
}

fn sequenced_event(
    sequence: u64,
    occurred_at: p::Timestamp,
    payload: p::EventPayload,
    trust: p::TrustTier,
) -> p::Event {
    let mut event = p::Event::new(
        p::EventId(format!("memory-tier-event:{sequence}")),
        p::RunId("memory:m2-c-hot-cold".into()),
        None,
        payload,
        p::SchemaVersion(1),
        occurred_at,
        p::Provenance {
            source: if trust == p::TrustTier::Untrusted {
                p::Source::Communication
            } else {
                p::Source::Internal
            },
            actor: if trust == p::TrustTier::Untrusted {
                p::Actor::External(p::ParticipantId("external:web".into()))
            } else {
                p::Actor::System
            },
            trust_tier: trust,
            caused_by: None,
        },
    );
    event.stream_seq = sequence;
    event
}

fn memory_node(sequence: u64, occurred_at: p::Timestamp, scope: &str, content: &str) -> p::Event {
    sequenced_event(
        sequence,
        occurred_at,
        p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
            node_id: p::NodeId(format!("memory-tier-node:{sequence}")),
            kind: p::MemoryNodeType("episode".into()),
            content_ref: p::ContentRef(content.into()),
            tier: p::StabilityTier::Working,
            confidence: p::Confidence(0.8),
            scope: p::Scope(scope.into()),
            resting_activation: p::RestingActivation(0.1),
            recency: p::Recency(occurred_at),
        }),
        p::TrustTier::VerifiedProcess,
    )
}

#[test]
fn s52_hot_cold_projection_is_rebuildable_scoped_retained_and_secret_free() {
    let mut events = vec![
        memory_node(1, 700, "workspace:alpha", "artifact:cold-alpha"),
        memory_node(
            2,
            940,
            "workspace:alpha/project:release",
            "artifact:hot-alpha",
        ),
        memory_node(3, 950, "workspace:alpha", "secret:provider-api-key"),
        memory_node(4, 960, "workspace:beta", "artifact:hot-beta"),
        memory_node(5, 300, "workspace:alpha", "artifact:expired-alpha"),
        memory_node(6, 850, "workspace:alpha", "artifact:retracted-alpha"),
        sequenced_event(
            7,
            970,
            p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                target_object: p::ObjectRef("memory-tier-node:6".into()),
                evidence_lineage: p::LineageRef("lineage:owner-retraction".into()),
            }),
            p::TrustTier::OwnerInput,
        ),
        sequenced_event(
            8,
            980,
            p::EventPayload::ActionOutputDelta(p::ActionOutputDeltaPayload {
                intent_id: p::ActionId("action:external-output".into()),
                backend: p::BackendKind::Browser,
                scope: p::Scope("workspace:alpha".into()),
                delta: "raw-sensitive-marker-must-not-enter-the-projection".into(),
                truncated: false,
                trust: p::TrustTier::Untrusted,
                content_ref: Some(p::ContentRef("artifact:external-output".into())),
                remote_lease: None,
            }),
            p::TrustTier::Untrusted,
        ),
    ];
    let policy = HotColdProjectionPolicy {
        schema_version: p::SchemaVersion(1),
        hot_window: p::DurationMs(100),
        retention_window: p::DurationMs(600),
        redact_untrusted_content: true,
    };
    let projector = HotColdMemoryProjector;
    let snapshot = projector.rebuild(&events, &policy, 1_000).unwrap();
    events.reverse();
    let rebuilt = projector.rebuild(&events, &policy, 1_000).unwrap();
    assert_eq!(snapshot, rebuilt);
    assert_eq!(snapshot.aggregate_versions[0].value, 8);

    let entry = |sequence| {
        snapshot
            .entries
            .iter()
            .find(|entry| entry.stream_seq == sequence)
            .unwrap()
    };
    assert_eq!(entry(1).temperature, p::MemoryTemperature::Cold);
    assert_eq!(entry(1).retention, p::MemoryRetentionState::Active);
    assert_eq!(entry(2).temperature, p::MemoryTemperature::Hot);
    assert_eq!(entry(3).retention, p::MemoryRetentionState::Redacted);
    assert!(entry(3).content_ref.is_none());
    assert_eq!(entry(5).retention, p::MemoryRetentionState::Expired);
    assert!(entry(5).content_ref.is_none());
    assert_eq!(entry(6).retention, p::MemoryRetentionState::Tombstoned);
    assert!(entry(6).content_ref.is_none());
    assert_eq!(entry(8).retention, p::MemoryRetentionState::Redacted);

    let serialized = format!("{snapshot:?}");
    assert!(!serialized.contains("secret:provider-api-key"));
    assert!(!serialized.contains("raw-sensitive-marker"));

    let alpha_hot = projector
        .recall(
            &snapshot,
            &SelectiveRecall {
                schema_version: p::SchemaVersion(1),
                scope: p::Scope("workspace:alpha".into()),
                include_cold: false,
                limit: 10,
            },
        )
        .unwrap();
    assert_eq!(
        alpha_hot
            .iter()
            .map(|entry| entry.stream_seq)
            .collect::<Vec<_>>(),
        vec![2]
    );
    let alpha_all = projector
        .recall(
            &snapshot,
            &SelectiveRecall {
                schema_version: p::SchemaVersion(1),
                scope: p::Scope("workspace:alpha".into()),
                include_cold: true,
                limit: 10,
            },
        )
        .unwrap();
    assert_eq!(
        alpha_all
            .iter()
            .map(|entry| entry.stream_seq)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(alpha_all.iter().all(|entry| entry
        .scope
        .as_ref()
        .is_some_and(|scope| scope.0 == "workspace:alpha"
            || scope.0.starts_with("workspace:alpha/")
            || scope.0.starts_with("workspace:alpha:"))));
    let beta = projector
        .recall(
            &snapshot,
            &SelectiveRecall {
                schema_version: p::SchemaVersion(1),
                scope: p::Scope("workspace:beta".into()),
                include_cold: true,
                limit: 10,
            },
        )
        .unwrap();
    assert_eq!(beta.len(), 1);
    assert_eq!(beta[0].stream_seq, 4);
}
