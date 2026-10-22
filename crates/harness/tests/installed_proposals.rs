#[path = "support/core_brain.rs"]
mod support;
use forme_harness::ProductionRuntime;
use forme_protocol as p;
use forme_store::EventStore;
use std::sync::atomic::Ordering;
use support::*;

fn emit(runtime: &ProductionRuntime, proposal: &str) -> p::WorkbenchProposalCommand {
    let run = runtime.services().aggregates.memory_cognition.clone();
    runtime
        .services()
        .store
        .append(p::Event::new(
            p::EventId(format!("emitted:{proposal}")),
            run.clone(),
            None,
            p::EventPayload::ProactiveProposalEmitted(p::ProactiveProposalEmittedPayload {
                proposal_ref: p::ProposalRef(proposal.into()),
                proposal_kind: p::ProposalKind("owner-suggestion".into()),
                level: p::InterventionLevel::L1Suggest,
                guard: p::EmissionGuard {
                    value_gate_passed: true,
                    competence_gate_passed: true,
                    policy_and_envelope_passed: true,
                },
                delivery: p::DeliveryMode::Hitchhike,
                attention_cost: 1,
                learning: None,
            }),
            p::SchemaVersion(1),
            now(),
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        ))
        .unwrap();
    p::WorkbenchProposalCommand {
        schema_version: p::SchemaVersion(1),
        command: p::IdempotencyKey(format!("resolve:{proposal}")),
        run,
        proposal: p::ProposalRef(proposal.into()),
        outcome: p::ProposalOutcome::Adopt,
        defer_until: None,
    }
}

#[test]
fn proposal_outcomes_are_durable_and_never_authorize_or_execute_actions() {
    let f = Fixture::new();
    let runtime = f.build();
    let mut saved = Vec::new();
    for (id, outcome) in [
        ("adopt", p::ProposalOutcome::Adopt),
        ("reject", p::ProposalOutcome::Reject),
        ("defer", p::ProposalOutcome::Defer),
    ] {
        let mut cmd = emit(&runtime, id);
        cmd.outcome = outcome;
        cmd.defer_until = (outcome == p::ProposalOutcome::Defer).then(|| now() + 60_000);
        let receipt = runtime
            .resolve_workbench_proposal(&f.owner(), cmd.clone())
            .unwrap();
        assert_eq!(receipt.status, p::WorkbenchCommandStatus::Confirmed);
        let before = events(&runtime);
        assert_eq!(
            runtime
                .resolve_workbench_proposal(&f.owner(), cmd.clone())
                .unwrap(),
            receipt
        );
        let mut conflict = cmd.clone();
        conflict.outcome = p::ProposalOutcome::Reject;
        conflict.defer_until = None;
        if outcome == p::ProposalOutcome::Reject {
            conflict.outcome = p::ProposalOutcome::Adopt;
        }
        assert!(runtime
            .resolve_workbench_proposal(&f.owner(), conflict)
            .is_err());
        assert_eq!(events(&runtime), before);
        saved.push((cmd, receipt));
    }
    let recorded = events(&runtime);
    assert_eq!(
        recorded
            .iter()
            .filter(|e| e.kind == p::EventKind::ProactiveProposalResolved)
            .count(),
        3
    );
    assert_eq!(
        recorded
            .iter()
            .filter(|e| e.kind == p::EventKind::ProspectiveIntentionCreated)
            .count(),
        1
    );
    assert!(!recorded.iter().any(|e| matches!(
        e.kind,
        p::EventKind::ActionStarted | p::EventKind::ApprovalRequested | p::EventKind::RunAccepted
    )));
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    drop(runtime);
    let runtime = f.build();
    for (cmd, receipt) in saved {
        assert_eq!(
            runtime.resolve_workbench_proposal(&f.owner(), cmd).unwrap(),
            receipt
        );
    }
    assert_eq!(events(&runtime), recorded);
}

#[test]
fn invalid_and_interrupted_proposal_controls_are_zero_effect_even_with_new_command_key() {
    let f = Fixture::new();
    let runtime = f.build();
    let cmd = emit(&runtime, "pending");
    let before = events(&runtime);
    assert!(runtime
        .resolve_workbench_proposal(&p::VerifiedPrincipal("other".into()), cmd.clone())
        .is_err());
    let mut past = cmd.clone();
    past.outcome = p::ProposalOutcome::Defer;
    past.defer_until = Some(now() - 1);
    assert!(runtime
        .resolve_workbench_proposal(&f.owner(), past)
        .is_err());
    let mut wrong_run = cmd.clone();
    wrong_run.run = p::RunId("unrelated".into());
    assert!(runtime
        .resolve_workbench_proposal(&f.owner(), wrong_run)
        .is_err());
    assert_eq!(events(&runtime), before);
    let key = p::canonical_digest(&(&f.owner(), &cmd.command)).unwrap();
    let binding = p::canonical_digest(&(&f.owner(), &cmd)).unwrap();
    let target = p::canonical_digest(&(&cmd.run, &cmd.proposal)).unwrap();
    runtime
        .services()
        .store
        .append(p::Event::new(
            p::EventId(format!("workbench-proposal-v1:{}:started", key.0)),
            cmd.run.clone(),
            None,
            p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
                deltas_ref: p::MemoryDeltasRef(format!(
                    "workbench-proposal-v1|{}|{}|started|{}",
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
        ))
        .unwrap();
    drop(runtime);
    let runtime = f.build();
    let before = events(&runtime);
    assert_eq!(
        runtime
            .resolve_workbench_proposal(&f.owner(), cmd.clone())
            .unwrap()
            .status,
        p::WorkbenchCommandStatus::Unknown
    );
    let mut fresh = cmd;
    fresh.command = p::IdempotencyKey("new-key".into());
    assert!(runtime
        .resolve_workbench_proposal(&f.owner(), fresh)
        .is_err());
    assert_eq!(events(&runtime), before);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
