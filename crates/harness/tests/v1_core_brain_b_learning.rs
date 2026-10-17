#[path = "support/core_brain.rs"]
mod support;
use forme_harness::{CognitiveTickOutcome, CognitiveTickPhaseStatus};
use forme_protocol as p;
use std::sync::atomic::Ordering;
use support::*;

#[test]
fn c8_goal_gap_questions_and_owner_answers_are_durable_candidates_only() {
    let f = Fixture::new();
    let runtime = f.build();
    let now = now();
    let key = p::IdempotencyKey("declare-risk".into());
    let event = runtime
        .register_learning_goal(
            &f.owner(),
            key.clone(),
            p::GoalFrameRef("goal:choose-investment".into()),
            knowledge("risk-preference", p::Impact::High),
        )
        .unwrap();
    assert_eq!(
        event,
        runtime
            .register_learning_goal(
                &f.owner(),
                key,
                p::GoalFrameRef("goal:choose-investment".into()),
                knowledge("risk-preference", p::Impact::High)
            )
            .unwrap()
    );
    let CognitiveTickOutcome::Ran(report) = runtime.cognitive_tick_at(now).unwrap() else {
        panic!()
    };
    assert_eq!(report.phases[0].status, CognitiveTickPhaseStatus::Ran);
    let (proposal, record) = ask(&runtime);
    assert_eq!(record.goal.0, "goal:choose-investment");
    let all = events(&runtime);
    assert!(all.iter().any(|e|matches!(&e.payload,p::EventPayload::ImpulseRaised(p) if p.source==p::ImpulseSource::Gap)));
    assert!(all.iter().any(|e|matches!(&e.payload,p::EventPayload::ProactiveProposalEmitted(p) if p.level==p::InterventionLevel::L1Suggest && p.delivery==p::DeliveryMode::Hitchhike && p.guard.policy_and_envelope_passed)));
    let reply = p::LearningReplyRecord {
        schema_version: p::SchemaVersion(1),
        proposal,
        value: p::UserAttributeValueRef("content:owner-risk-answer".into()),
    };
    let candidate = runtime
        .answer_learning(
            &f.owner(),
            p::IdempotencyKey("risk-answer".into()),
            reply.clone(),
        )
        .unwrap();
    assert_eq!(
        candidate,
        runtime
            .answer_learning(&f.owner(), p::IdempotencyKey("risk-answer".into()), reply)
            .unwrap()
    );
    assert_eq!(events(&runtime).iter().filter(|e|matches!(&e.payload,p::EventPayload::CandidateCreated(p) if p.candidate_id==candidate)).count(),1);
    assert!(runtime.services().cognition.stable_objects().is_empty());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    drop(runtime);
    let runtime = f.build();
    runtime.cognitive_tick_at(now + 60_001).unwrap();
    assert_eq!(events(&runtime).iter().filter(|e|matches!(&e.payload,p::EventPayload::ProactiveProposalEmitted(p) if p.learning.is_some())).count(),1);
    assert!(runtime
        .services()
        .memory
        .user_candidate(&candidate)
        .is_some());
}

#[test]
fn c8_rejection_and_defer_survive_restart_and_never_repeat_the_origin() {
    let f = Fixture::new();
    let runtime = f.build();
    let start = now();
    runtime
        .register_learning_goal(
            &f.owner(),
            p::IdempotencyKey("goal".into()),
            p::GoalFrameRef("goal:risk".into()),
            knowledge("risk", p::Impact::Medium),
        )
        .unwrap();
    runtime.cognitive_tick_at(start).unwrap();
    let (proposal, _) = ask(&runtime);
    runtime
        .resolve_learning(
            &f.owner(),
            p::IdempotencyKey("reject".into()),
            proposal,
            p::ProposalOutcome::Reject,
            None,
        )
        .unwrap();
    drop(runtime);
    let runtime = f.build();
    runtime.cognitive_tick_at(start + 60_001).unwrap();
    assert_eq!(events(&runtime).iter().filter(|e|matches!(&e.payload,p::EventPayload::ProactiveProposalEmitted(p) if p.learning.is_some())).count(),1);
    runtime
        .register_learning_goal(
            &f.owner(),
            p::IdempotencyKey("goal-2".into()),
            p::GoalFrameRef("goal:time".into()),
            knowledge("time", p::Impact::Medium),
        )
        .unwrap();
    runtime.cognitive_tick_at(start + 120_002).unwrap();
    let (proposal, _) = events(&runtime)
        .into_iter()
        .filter_map(|e| match e.payload {
            p::EventPayload::ProactiveProposalEmitted(p) => p.learning.map(|l| (p.proposal_ref, l)),
            _ => None,
        })
        .find(|(_, l)| l.target.key() == "time")
        .unwrap();
    runtime
        .resolve_learning(
            &f.owner(),
            p::IdempotencyKey("defer".into()),
            proposal,
            p::ProposalOutcome::Defer,
            Some(start + 240_000),
        )
        .unwrap();
    assert!(events(&runtime).iter().any(|e|matches!(&e.payload,p::EventPayload::ProspectiveIntentionCreated(p) if p.source==p::IntentionSource::DeferredProposal)));
    drop(runtime);
    let runtime = f.build();
    runtime.cognitive_tick_at(start + 180_003).unwrap();
    assert_eq!(events(&runtime).iter().filter(|e|matches!(&e.payload,p::EventPayload::ProactiveProposalEmitted(p) if p.learning.is_some())).count(),2);
    runtime.cognitive_tick_at(start + 240_004).unwrap();
    assert!(events(&runtime).iter().any(|e|matches!(&e.payload,p::EventPayload::ProspectiveIntentionResolved(p) if p.outcome==p::IntentionOutcome::Done)));
    assert!(!events(&runtime)
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
}

#[test]
fn c8_low_value_quiet_unrelated_and_unauthenticated_learning_are_zero_emit_or_zero_write() {
    let f = Fixture::new();
    let runtime = f.build();
    let before = events(&runtime).len();
    assert!(runtime
        .register_learning_goal(
            &p::VerifiedPrincipal("intruder".into()),
            p::IdempotencyKey("bad".into()),
            p::GoalFrameRef("goal".into()),
            knowledge("risk", p::Impact::High)
        )
        .is_err());
    assert_eq!(events(&runtime).len(), before);
    let mut quiet = knowledge("quiet-risk", p::Impact::High);
    quiet.allow_questions = false;
    runtime
        .register_learning_goal(
            &f.owner(),
            p::IdempotencyKey("quiet".into()),
            p::GoalFrameRef("quiet-goal".into()),
            quiet,
        )
        .unwrap();
    runtime
        .register_learning_goal(
            &f.owner(),
            p::IdempotencyKey("low".into()),
            p::GoalFrameRef("low-goal".into()),
            knowledge("low", p::Impact::Low),
        )
        .unwrap();
    runtime.cognitive_tick_at(now()).unwrap();
    assert!(!events(&runtime)
        .iter()
        .any(|e| e.kind == p::EventKind::ProactiveProposalEmitted));
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
