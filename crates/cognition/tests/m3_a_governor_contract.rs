use forme_cognition::{
    ConservativeStrategyEvolutionGovernor, EvolutionDecision, StrategyEvolutionGovernor,
};
use forme_protocol as p;

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn candidate() -> p::StrategyCandidate {
    p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId("candidate:loop:v2".into()),
        domain: p::StrategyDomain::Loop,
        scope: p::Scope("workspace".into()),
        target_tier: p::StabilityTier::Stable,
        proposed_version: p::StrategyVersionRef("loop:v2".into()),
        baseline: p::StrategyVersionRef("loop:v1".into()),
        spec_ref: p::ContentRef("content:loop:v2".into()),
        spec_digest: p::SchemaDigest("digest:loop:v2".into()),
        evidence: vec![p::EvidenceRef("evidence:ground-truth".into())],
        provenance: provenance(),
        impact: p::EvolutionImpact::Cautious,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef("loop:v1".into()),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    }
}

fn evaluation(verdict: p::EvaluationVerdict) -> p::EvolutionEvaluation {
    let outcome = match verdict {
        p::EvaluationVerdict::Pass => p::FitnessOutcome::Pass,
        p::EvaluationVerdict::Fail => p::FitnessOutcome::Fail,
        p::EvaluationVerdict::Unverifiable => p::FitnessOutcome::Unverifiable,
    };
    p::EvolutionEvaluation {
        schema_version: p::SchemaVersion(1),
        evaluation: p::EvolutionEvaluationRef("evaluation:loop:v2".into()),
        bundle: p::ReplayBundleRef("bundle:loop:v2".into()),
        baseline: p::StrategyVersionRef("loop:v1".into()),
        candidate: p::StrategyVersionRef("loop:v2".into()),
        case_set_digest: p::SchemaDigest("digest:train".into()),
        holdout_digest: p::SchemaDigest("digest:holdout".into()),
        metrics: vec![p::FitnessMetric {
            schema_version: p::SchemaVersion(1),
            dimension: p::FitnessDimension::Quality,
            outcome,
            measured: Some(1),
            unit: p::FitnessUnit::Count,
            evidence: vec![p::EvidenceRef("evidence:quality".into())],
        }],
        hard_invariants: vec![p::InvariantResult {
            schema_version: p::SchemaVersion(1),
            reference: p::InvariantResultRef("invariant:harness-first".into()),
            name: "harness_first".into(),
            outcome,
            evidence: vec![p::EvidenceRef("evidence:invariant".into())],
        }],
        ground_truth: vec![p::EvidenceRef("evidence:ground-truth".into())],
        independent_verifier: true,
        verdict,
    }
}

fn current(version: &str) -> p::EvolutionSnapshot {
    p::EvolutionSnapshot {
        schema_version: p::SchemaVersion(1),
        snapshot: p::EvolutionSnapshotRef("snapshot:current".into()),
        aggregates: vec![p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
            value: 1,
        }],
        strategies: vec![p::ActiveStrategyRef {
            schema_version: p::SchemaVersion(1),
            id: p::ActiveStrategyId("active:loop".into()),
            aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
            domain: p::StrategyDomain::Loop,
            scope: p::Scope("workspace".into()),
            version: p::StrategyVersionRef(version.into()),
            spec_ref: p::ContentRef(format!("content:{version}")),
            spec_digest: p::SchemaDigest(format!("digest:{version}")),
            activation_event: p::EventId("event:activation".into()),
        }],
        digest: p::SchemaDigest("digest:current".into()),
    }
}

#[test]
fn s56_governor_keeps_promotion_owner_and_rollback_decisions_distinct() {
    let governor = ConservativeStrategyEvolutionGovernor;
    let cautious = candidate();
    assert_eq!(
        governor.decide(
            &cautious,
            &evaluation(p::EvaluationVerdict::Pass),
            &current("loop:v1")
        ),
        EvolutionDecision::Promote
    );

    let mut bounded = cautious.clone();
    bounded.impact = p::EvolutionImpact::Bounded;
    assert_eq!(
        governor.decide(
            &bounded,
            &evaluation(p::EvaluationVerdict::Pass),
            &current("loop:v1")
        ),
        EvolutionDecision::NeedOwner
    );

    let mut constitutional = cautious.clone();
    constitutional.impact = p::EvolutionImpact::Constitutional;
    assert_eq!(
        governor.decide(
            &constitutional,
            &evaluation(p::EvaluationVerdict::Pass),
            &current("loop:v1")
        ),
        EvolutionDecision::Reject
    );

    assert_eq!(
        governor.decide(
            &cautious,
            &evaluation(p::EvaluationVerdict::Unverifiable),
            &current("loop:v1")
        ),
        EvolutionDecision::NeedOwner
    );

    assert_eq!(
        governor.decide(
            &cautious,
            &evaluation(p::EvaluationVerdict::Fail),
            &current("loop:v2")
        ),
        EvolutionDecision::Rollback
    );
}

#[test]
fn s56_untrusted_or_stale_baseline_cannot_promote() {
    let governor = ConservativeStrategyEvolutionGovernor;
    let mut untrusted = candidate();
    untrusted.provenance.trust_tier = p::TrustTier::Untrusted;
    assert_eq!(
        governor.decide(
            &untrusted,
            &evaluation(p::EvaluationVerdict::Pass),
            &current("loop:v1")
        ),
        EvolutionDecision::Reject
    );
    let mut spoofed_verified_process = candidate();
    spoofed_verified_process.provenance.actor =
        p::Actor::External(p::ParticipantId("participant:spoofed".into()));
    assert_eq!(
        governor.decide(
            &spoofed_verified_process,
            &evaluation(p::EvaluationVerdict::Pass),
            &current("loop:v1")
        ),
        EvolutionDecision::Reject
    );
    assert_eq!(
        governor.decide(
            &candidate(),
            &evaluation(p::EvaluationVerdict::Pass),
            &current("loop:v0")
        ),
        EvolutionDecision::Reject
    );
}
