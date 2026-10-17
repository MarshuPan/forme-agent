#[path = "support/core_brain.rs"]
mod support;
use forme_cognition::*;
use forme_harness::*;
use forme_protocol as p;
use support::*;

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::OwnerControl,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}
fn promote(runtime: &ProductionRuntime, object: &str, evidence_at: i64, reproduced: i64) {
    let c = runtime.services().cognition.clone();
    let scope = MapScope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:core-b".into()),
    };
    let candidate = c
        .propose_update(CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: scope.clone(),
            kind: MapUpdateKind::Frame,
            confidence: p::Confidence(0.3),
            evidence: vec![p::EvidenceRef(format!("evidence:{object}"))],
            frame: Some(p::JudgmentFrameRef(object.into())),
            quality: None,
            blindspot: None,
            resource: None,
            reflection_inputs: vec![],
            statement: format!("checked statement {object}"),
            provenance: provenance(),
        })
        .unwrap();
    let evidence = vec![GovernanceEvidence {
        schema_version: p::SchemaVersion(1),
        reference: p::EvidenceRef(format!("evidence:{object}")),
        observed_at: evidence_at,
        verified_process: true,
    }];
    assert_eq!(
        c.govern_and_apply(GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: candidate,
            direction: ChangeDirection::CautionIncreasing,
            confidence: p::Confidence(0.9),
            evidence: evidence.clone(),
            impact: p::Impact::Low,
            provenance: provenance(),
            conflicts: vec![],
            owner_confirmed: true,
            target: StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef(object.into()),
                scope,
                kind: StableCognitionKind::Frame,
                statement: format!("checked statement {object}"),
                tier: p::StabilityTier::Stable,
                confidence: p::Confidence(0.9),
                evidence,
                last_reproduced_at: reproduced,
                active: false
            }
        })
        .unwrap(),
        GovernanceDecision::Promote
    );
}
fn scope() -> MemoryScope {
    MemoryScope {
        schema_version: p::SchemaVersion(1),
        slice: TimeSlice::LongTerm,
        workspace: Some(p::WorkspaceRef("workspace:core-b".into())),
        scope: p::Scope("workspace:core-b".into()),
    }
}
fn node(id: &str, kind: NodeKind, content: &str, confidence: f32, recency: i64) -> MemoryNode {
    MemoryNode {
        schema_version: p::SchemaVersion(1),
        id: p::NodeId(id.into()),
        kind,
        content_ref: p::ContentRef(content.into()),
        tier: p::StabilityTier::Working,
        confidence: p::Confidence(confidence),
        scope: scope(),
        resting_activation: p::RestingActivation(0.0),
        recency,
        provenance: provenance(),
    }
}

#[test]
fn c12_formal_maintenance_covers_age_unseen_conflict_pruning_and_transitive_replay() {
    let mut f = Fixture::new();
    f.config.cognition.relevance_mode = p::RelevanceMode::Diffusion;
    let runtime = f.build();
    let at = now();
    promote(&runtime, "frame:aged", 1, at);
    promote(&runtime, "frame:unseen", at, 1);
    for id in ["frame:left", "frame:right", "frame:derived"] {
        promote(&runtime, id, at, at);
    }
    runtime
        .services()
        .cognition
        .record_lineage(
            p::ObjectRef("frame:left".into()),
            p::ObjectRef("frame:derived".into()),
        )
        .unwrap();
    for (id, content) in [("left", "frame:left"), ("right", "frame:right")] {
        runtime
            .services()
            .memory
            .add_node(node(id, NodeKind::CognitiveObjectRef, content, 0.9, at))
            .unwrap();
    }
    runtime
        .services()
        .memory
        .add_node(node(
            "obsolete",
            NodeKind::Fact,
            "content:obsolete",
            0.05,
            1,
        ))
        .unwrap();
    runtime
        .services()
        .memory
        .add_edge(MemoryEdge {
            schema_version: p::SchemaVersion(1),
            id: p::EdgeId("contradiction".into()),
            from: p::NodeId("left".into()),
            to: p::NodeId("right".into()),
            kind: EdgeKind::Contradicts,
            weight: p::Weight(0.8),
            provenance: provenance(),
        })
        .unwrap();
    let before = events(&runtime).len();
    let CognitiveTickOutcome::Ran(report) = runtime.cognitive_tick_at(at).unwrap() else {
        panic!()
    };
    assert!(!report.failed(), "{report:?}");
    let all = events(&runtime);
    let facts = &all[before..];
    assert!(facts
        .iter()
        .any(|event| event.kind == p::EventKind::FailureEvidenceRecorded));
    assert!(all.iter().any(|event|matches!(&event.payload,p::EventPayload::ReevaluationTaskCreated(p) if p.trigger.0.contains("EvidenceAge"))));
    assert!(all.iter().any(|event|matches!(&event.payload,p::EventPayload::ReevaluationTaskCreated(p) if p.trigger.0=="stale:frame:unseen")));
    assert!(all.iter().any(|event|matches!(&event.payload,p::EventPayload::ReevaluationTaskCreated(p) if p.trigger.0.contains("Conflict")&&p.derived_refs.contains(&p::ObjectRef("frame:derived".into())))));
    let stable = runtime.services().cognition.stable_objects();
    assert_eq!(stable.len(), 5);
    assert!(stable
        .iter()
        .all(|s| !s.active && s.confidence == p::Confidence(0.9)));
    let graph = runtime.services().memory.graph_snapshot_at(None).unwrap();
    assert!(!graph.nodes.contains_key(&p::NodeId("obsolete".into())));
    assert!(all.iter().any(|event|matches!(&event.payload,p::EventPayload::MemoryMaintenanceApplied(p) if p.deltas_ref.0.starts_with("graph-deltas:v1:"))));
    assert!(!all
        .iter()
        .any(|event| event.kind == p::EventKind::ActionStarted));
    let candidates = all
        .iter()
        .filter(|e| e.kind == p::EventKind::CandidateCreated)
        .count();
    drop(runtime);
    let runtime = f.build();
    assert_eq!(stable, runtime.services().cognition.stable_objects());
    assert_eq!(
        graph,
        runtime.services().memory.graph_snapshot_at(None).unwrap()
    );
    runtime.cognitive_tick_at(at + 60_001).unwrap();
    assert_eq!(
        events(&runtime)
            .iter()
            .filter(|e| e.kind == p::EventKind::CandidateCreated)
            .count(),
        candidates
    );
}

#[test]
fn c12_old_graph_cannot_decay_new_knowledge_and_window_receipts_are_idempotent() {
    let f = Fixture::new();
    let runtime = f.build();
    let memory = runtime.services().memory.clone();
    memory
        .add_node(node("weak", NodeKind::Fact, "content:weak", 0.2, 1))
        .unwrap();
    let snapshot = memory.graph_snapshot_at(None).unwrap();
    let at = now();
    memory
        .add_node(node("late", NodeKind::Fact, "content:late", 0.9, at))
        .unwrap();
    assert_eq!(
        memory
            .maintain_low_value_branches(
                &snapshot,
                at,
                100,
                p::IdempotencyKey("stale-window".into())
            )
            .unwrap(),
        None
    );
    let fresh = memory.graph_snapshot_at(None).unwrap();
    runtime
        .services()
        .cognition
        .record_lineage(p::ObjectRef("a".into()), p::ObjectRef("b".into()))
        .unwrap();
    let first = memory
        .maintain_low_value_branches(&fresh, at, 100, p::IdempotencyKey("current-window".into()))
        .unwrap()
        .unwrap();
    let graph = memory.graph_snapshot_at(None).unwrap();
    assert_eq!(
        graph.nodes[&p::NodeId("weak".into())].confidence,
        p::Confidence(0.1)
    );
    assert_eq!(
        memory
            .maintain_low_value_branches(
                &fresh,
                at,
                100,
                p::IdempotencyKey("current-window".into())
            )
            .unwrap(),
        Some(first)
    );
    assert_eq!(memory.graph_snapshot_at(None).unwrap(), graph);
    assert!(memory
        .maintain_low_value_branches(
            &fresh,
            at + 1,
            100,
            p::IdempotencyKey("current-window".into())
        )
        .is_err());
    drop(memory);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(
        runtime.services().memory.graph_snapshot_at(None).unwrap(),
        graph
    );
}

#[test]
fn c12_healthy_prefix_cannot_hide_later_maintenance_issues() {
    let f = Fixture::new();
    let runtime = f.build();
    let at = now();
    for index in 0..65 {
        promote(&runtime, &format!("frame:healthy:{index:02}"), at, at);
    }
    promote(&runtime, "frame:z-aged", 1, at);
    let cognition = runtime.services().cognition.clone();
    let snapshot = cognition.stable_objects();
    assert_eq!(snapshot.last().unwrap().object.0, "frame:z-aged");
    let graph = runtime.services().memory.graph_snapshot_at(None).unwrap();
    let candidates = cognition
        .audit_maintenance_snapshot(
            Tick {
                schema_version: p::SchemaVersion(1),
                now: at,
                stale_after_ms: 100,
            },
            &snapshot,
            &graph,
        )
        .unwrap();
    assert_eq!(candidates.len(), 1);
    let stable = cognition.stable_objects();
    assert!(stable[..65].iter().all(|object| object.active));
    assert!(!stable[65].active);
    assert!(stable
        .iter()
        .all(|object| object.confidence == p::Confidence(0.9)));
    assert!(events(&runtime).iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::ReevaluationTaskCreated(payload)
            if payload.trigger.0 == "maintenance:EvidenceAge:frame:z-aged"
    )));
}
