#[path = "support/core_brain.rs"]
mod support;
use forme_cognition::*;
use forme_harness::ProductionRuntime;
use forme_protocol as p;
use forme_store::EventStore;
use std::sync::atomic::Ordering;
use support::*;

fn query(section: p::WorkbenchSection) -> p::WorkbenchQuery {
    p::WorkbenchQuery {
        schema_version: p::SchemaVersion(1),
        section,
        scope: None,
        offset: 0,
        limit: 100,
        snapshot: None,
    }
}
fn answered(runtime: &ProductionRuntime, f: &Fixture) -> p::CandidateId {
    runtime
        .register_learning_goal(
            &f.owner(),
            p::IdempotencyKey("goal".into()),
            p::GoalFrameRef("goal:owner-choice".into()),
            knowledge("risk-choice", p::Impact::High),
        )
        .unwrap();
    runtime.cognitive_tick_at(now()).unwrap();
    runtime
        .answer_learning(
            &f.owner(),
            p::IdempotencyKey("answer".into()),
            p::LearningReplyRecord {
                schema_version: p::SchemaVersion(1),
                proposal: ask(runtime).0,
                value: p::UserAttributeValueRef("content:owner-answer".into()),
            },
        )
        .unwrap()
}
fn command(
    runtime: &ProductionRuntime,
    f: &Fixture,
    id: &p::CandidateId,
    decision: p::CandidateReviewDecision,
) -> p::WorkbenchReviewCommand {
    let snapshot = runtime
        .workbench_snapshot(&f.owner(), query(p::WorkbenchSection::Cognition))
        .unwrap();
    let item = snapshot
        .items
        .iter()
        .find(|item| item.kind == p::WorkbenchItemKind::Candidate && item.id == id.0)
        .unwrap();
    p::WorkbenchReviewCommand {
        schema_version: p::SchemaVersion(1),
        command: p::IdempotencyKey(format!("review:{}:{decision:?}", id.0)),
        run: item.run.clone(),
        candidate: id.clone(),
        expected_state: serde_json::from_value(serde_json::json!(item.state)).unwrap(),
        decision,
    }
}

#[test]
fn projections_are_owner_only_readonly_and_paginated_against_the_real_snapshot() {
    let f = Fixture::new();
    let runtime = f.build();
    let id = answered(&runtime, &f);
    let before = events(&runtime);
    let inbox = runtime
        .workbench_snapshot(&f.owner(), query(p::WorkbenchSection::Inbox))
        .unwrap();
    assert!(inbox
        .items
        .iter()
        .any(|item| item.kind == p::WorkbenchItemKind::AskToLearn && item.state == "Answered"));
    let snapshot = runtime
        .workbench_snapshot(&f.owner(), query(p::WorkbenchSection::Cognition))
        .unwrap();
    assert!(snapshot
        .items
        .iter()
        .any(|item| item.id == id.0 && item.state == "Candidate" && !item.evidence.is_empty()));
    assert_eq!(events(&runtime), before);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(runtime
        .workbench_snapshot(
            &p::VerifiedPrincipal("other-owner".into()),
            query(p::WorkbenchSection::Cognition)
        )
        .is_err());
    let mut page = query(p::WorkbenchSection::Cognition);
    page.offset = 1;
    assert!(runtime
        .workbench_snapshot(&f.owner(), page.clone())
        .is_err());
    page.snapshot = Some(snapshot.digest);
    assert!(runtime.workbench_snapshot(&f.owner(), page.clone()).is_ok());
    runtime
        .review_workbench_candidate(
            &f.owner(),
            command(&runtime, &f, &id, p::CandidateReviewDecision::Reject),
        )
        .unwrap();
    assert!(runtime
        .workbench_snapshot(&f.owner(), page)
        .unwrap_err()
        .0
        .contains("snapshot changed"));
}

#[test]
fn review_is_durable_idempotent_and_conflicting_command_keys_do_not_write() {
    let f = Fixture::new();
    let runtime = f.build();
    let id = answered(&runtime, &f);
    let cmd = command(&runtime, &f, &id, p::CandidateReviewDecision::Reject);
    let receipt = runtime
        .review_workbench_candidate(&f.owner(), cmd.clone())
        .unwrap();
    assert_eq!(receipt.status, p::WorkbenchCommandStatus::Confirmed);
    let before = events(&runtime);
    assert_eq!(
        runtime
            .review_workbench_candidate(&f.owner(), cmd.clone())
            .unwrap(),
        receipt
    );
    assert_eq!(events(&runtime), before);
    let mut conflict = cmd.clone();
    conflict.decision = p::CandidateReviewDecision::Promote;
    assert!(runtime
        .review_workbench_candidate(&f.owner(), conflict)
        .is_err());
    assert!(runtime
        .review_workbench_candidate(&p::VerifiedPrincipal("other".into()), cmd.clone())
        .is_err());
    assert_eq!(events(&runtime), before);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(
        runtime.review_workbench_candidate(&f.owner(), cmd).unwrap(),
        receipt
    );
    assert_eq!(events(&runtime).iter().filter(|e| matches!(&e.payload, p::EventPayload::CandidateRejected(p) if p.candidate_id == id)).count(), 1);
}

#[test]
fn confirmation_does_not_promote_an_under_evidenced_answer() {
    let f = Fixture::new();
    let runtime = f.build();
    let id = answered(&runtime, &f);
    let cmd = command(&runtime, &f, &id, p::CandidateReviewDecision::Promote);
    let receipt = runtime
        .review_workbench_candidate(&f.owner(), cmd.clone())
        .unwrap();
    assert_ne!(receipt.status, p::WorkbenchCommandStatus::Confirmed);
    assert!(runtime.services().cognition.stable_objects().is_empty());
    let count = events(&runtime).len();
    assert_eq!(
        runtime.review_workbench_candidate(&f.owner(), cmd).unwrap(),
        receipt
    );
    assert_eq!(events(&runtime).len(), count);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn interrupted_review_returns_original_unknown_receipt_without_executing() {
    let f = Fixture::new();
    let runtime = f.build();
    let id = answered(&runtime, &f);
    let cmd = command(&runtime, &f, &id, p::CandidateReviewDecision::Reject);
    let key = p::canonical_digest(&(&f.owner(), &cmd.command)).unwrap();
    let binding = p::canonical_digest(&(&f.owner(), &cmd)).unwrap();
    let target = p::canonical_digest(&(&cmd.run, &cmd.candidate)).unwrap();
    let event = p::Event::new(
        p::EventId(format!("workbench-review-v1:{}:started", key.0)),
        runtime.services().aggregates.memory_cognition.clone(),
        None,
        p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
            deltas_ref: p::MemoryDeltasRef(format!(
                "workbench-review-v1|{}|{}|started|{}",
                key.0, binding.0, target.0
            )),
        }),
        p::SchemaVersion(1),
        now(),
        p::Provenance {
            source: p::Source::OwnerControl,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    );
    runtime.services().store.append(event).unwrap();
    drop(runtime);
    let runtime = f.build();
    let before = events(&runtime);
    assert_eq!(
        runtime
            .review_workbench_candidate(&f.owner(), cmd.clone())
            .unwrap()
            .status,
        p::WorkbenchCommandStatus::Unknown
    );
    let mut fresh = cmd;
    fresh.command = p::IdempotencyKey("attempt-with-new-key".into());
    assert!(runtime
        .review_workbench_candidate(&f.owner(), fresh)
        .unwrap_err()
        .0
        .contains("unresolved review"));
    assert_eq!(events(&runtime), before);
    assert!(!before.iter().any(
        |e| matches!(&e.payload, p::EventPayload::CandidateRejected(p) if p.candidate_id == id)
    ));
}

fn promote(runtime: &ProductionRuntime, object: &str) -> p::CandidateId {
    let c = runtime.services().cognition.clone();
    let at = now();
    let scope = MapScope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:core-b".into()),
    };
    let provenance = p::Provenance {
        source: p::Source::OwnerControl,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    };
    let id = c
        .propose_update(CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: scope.clone(),
            kind: MapUpdateKind::Frame,
            confidence: p::Confidence(0.3),
            evidence: vec![p::EvidenceRef(format!("checked:{object}"))],
            frame: Some(p::JudgmentFrameRef(object.into())),
            quality: None,
            blindspot: None,
            resource: None,
            reflection_inputs: vec![],
            statement: format!("Caution about {object}"),
            provenance: provenance.clone(),
        })
        .unwrap();
    let evidence = vec![GovernanceEvidence {
        schema_version: p::SchemaVersion(1),
        reference: p::EvidenceRef(format!("checked:{object}")),
        observed_at: at,
        verified_process: true,
    }];
    let result = c
        .govern_and_apply(GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: id.clone(),
            direction: ChangeDirection::CautionIncreasing,
            confidence: p::Confidence(0.9),
            evidence: evidence.clone(),
            impact: p::Impact::Low,
            provenance,
            conflicts: vec![],
            owner_confirmed: true,
            target: StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef(object.into()),
                scope,
                kind: StableCognitionKind::Frame,
                statement: format!("Caution about {object}"),
                tier: p::StabilityTier::Stable,
                confidence: p::Confidence(0.9),
                evidence,
                last_reproduced_at: at,
                active: false,
            },
        })
        .unwrap();
    assert_eq!(result, GovernanceDecision::Promote);
    id
}

#[test]
fn retract_computes_transitive_lineage_in_the_brain_and_replays_inactive() {
    let f = Fixture::new();
    let runtime = f.build();
    let id = promote(&runtime, "frame:root");
    promote(&runtime, "frame:child");
    promote(&runtime, "frame:grandchild");
    let c = runtime.services().cognition.clone();
    c.record_lineage(
        p::ObjectRef("frame:root".into()),
        p::ObjectRef("frame:child".into()),
    )
    .unwrap();
    c.record_lineage(
        p::ObjectRef("frame:child".into()),
        p::ObjectRef("frame:grandchild".into()),
    )
    .unwrap();
    let cmd = command(&runtime, &f, &id, p::CandidateReviewDecision::Retract);
    assert_eq!(
        runtime
            .review_workbench_candidate(&f.owner(), cmd)
            .unwrap()
            .status,
        p::WorkbenchCommandStatus::Confirmed
    );
    let recorded = events(&runtime);
    assert!(recorded.iter().any(|event| matches!(&event.payload, p::EventPayload::ReevaluationTaskCreated(p) if p.derived_refs.contains(&p::ObjectRef("frame:grandchild".into())))));
    for object in ["frame:root", "frame:child", "frame:grandchild"] {
        assert!(!c.is_object_usable(&p::ObjectRef(object.into())));
    }
    drop(c);
    drop(runtime);
    let runtime = f.build();
    let view = runtime
        .workbench_snapshot(&f.owner(), query(p::WorkbenchSection::Cognition))
        .unwrap();
    assert_eq!(
        view.items
            .iter()
            .filter(|item| item.kind == p::WorkbenchItemKind::Stable && item.state == "Inactive")
            .count(),
        3
    );
    assert_eq!(
        view.items
            .iter()
            .filter(|item| item.id == id.0 && item.state == "Retracted")
            .count(),
        1
    );
}
