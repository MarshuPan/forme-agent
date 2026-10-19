//! Controlled independent-comparison fixture; baselines use the production API.
use forme_harness::{EvolutionGatewayControl, ProductionRuntime, ProductionStrategySpec};
use forme_protocol as p;

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}
pub fn candidate(
    body: &ProductionStrategySpec,
    domain: p::StrategyDomain,
    baseline: p::StrategyVersionRef,
    evidence: Vec<p::EvidenceRef>,
) -> p::StrategyCandidate {
    p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId(format!("candidate:{}", body.version().0)),
        domain,
        scope: body.scope().clone(),
        target_tier: p::StabilityTier::Stable,
        proposed_version: body.version().clone(),
        baseline: baseline.clone(),
        spec_ref: body.content_ref().clone(),
        spec_digest: body.digest().clone(),
        evidence,
        provenance: provenance(),
        impact: p::EvolutionImpact::Cautious,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: baseline,
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    }
}
pub fn comparison(c: &p::StrategyCandidate) -> p::EvolutionComparison {
    p::EvolutionComparison {
        schema_version: p::SchemaVersion(1),
        evaluation: p::EvolutionEvaluationRef(format!("evaluation:{}", c.candidate.0)),
        bundle: p::ReplayBundleRef(format!("bundle:{}", c.candidate.0)),
        baseline: c.baseline.clone(),
        candidate: c.proposed_version.clone(),
        case_set_digest: p::SchemaDigest("owned-case-set".into()),
        holdout_digest: p::SchemaDigest("owned-holdout".into()),
        metrics: vec![p::FitnessMetric {
            schema_version: p::SchemaVersion(1),
            dimension: p::FitnessDimension::Quality,
            outcome: p::FitnessOutcome::Pass,
            measured: Some(10_000),
            unit: p::FitnessUnit::BasisPoints,
            evidence: c.evidence.clone(),
        }],
        hard_invariants: vec![p::InvariantResult {
            schema_version: p::SchemaVersion(1),
            reference: p::InvariantResultRef("owned-invariant".into()),
            name: "governance_preserved".into(),
            outcome: p::FitnessOutcome::Pass,
            evidence: c.evidence.clone(),
        }],
        ground_truth: c.evidence.clone(),
        independent_verifier: true,
        self_eval_only: false,
    }
}
pub fn activate(runtime: &ProductionRuntime, c: &p::StrategyCandidate, already_recorded: bool) {
    let h = runtime.harness();
    let run = p::RunId(format!("upgrade:{}", c.candidate.0));
    if !already_recorded {
        h.record_strategy_candidate(run.clone(), c.clone()).unwrap();
    }
    let evaluated = h.evaluate_strategy(run.clone(), c, comparison(c)).unwrap();
    let promotion = h
        .promote_strategy(run.clone(), c, &evaluated.evaluation)
        .unwrap();
    h.activate_strategy(
        run,
        p::EvolutionAggregateRef(format!("evolution:{:?}:{}", c.domain, c.scope.0)),
        c,
        &evaluated.evaluation,
        promotion,
        Some(p::OwnerControlRef("owner-reviewed-test-upgrade".into())),
    )
    .unwrap();
}
