//! Explicit owner initialization of the already-running built-in policy.
//! This records identity/equivalence, never measured intelligence or quality.
use crate::{production::ProductionRuntime, M3EvolutionHarness, ProductionStrategySpec};
use forme_cognition::{EvolutionDecision, StrategyEvolutionGovernor};
use forme_protocol as p;
use forme_store::{EventStore, EvolutionEventStore, EvolutionProjection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionSeedSpec {
    pub schema_version: p::SchemaVersion,
    pub domain: p::StrategyDomain,
    pub version: p::StrategyVersionRef,
    pub scope: p::Scope,
    pub content_ref: p::ContentRef,
    pub content_digest: p::SchemaDigest,
    pub implementation_digest: p::SchemaDigest,
}
impl ProductionSeedSpec {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.scope.0.trim().is_empty()
            || !self.version.0.starts_with("repository-seed:")
            || self.content_ref.0.is_empty()
            || self.content_digest.0.is_empty()
            || self.implementation_digest.0.is_empty()
            || !supported(self.domain)
        {
            return Err(p::Error(
                "repository baseline identity is incomplete".into(),
            ));
        }
        Ok(())
    }
}
fn supported(d: p::StrategyDomain) -> bool {
    matches!(
        d,
        p::StrategyDomain::Loop
            | p::StrategyDomain::Coordination
            | p::StrategyDomain::CapabilitySelection
            | p::StrategyDomain::ModelSelection
            | p::StrategyDomain::BackendSelection
            | p::StrategyDomain::ModelAdaptation
    )
}

pub(crate) fn repository_seed(
    domain: p::StrategyDomain,
    scope: &p::Scope,
) -> p::Result<ProductionStrategySpec> {
    if !supported(domain) || scope.0.trim().is_empty() {
        return Err(p::Error(
            "this runtime has no built-in seed for that domain or scope".into(),
        ));
    }
    // Fingerprint the repository implementations that actually execute seed
    // behavior. A changed build cannot silently reinterpret an old baseline.
    let implementation_digest = p::sha256_content_digest(
        concat!(
            include_str!("lib.rs"),
            include_str!("m3_b.rs"),
            include_str!("brain_orchestration.rs"),
            include_str!("formal_tools.rs"),
            include_str!("../../loop/src/lib.rs"),
            include_str!("../../coordination/src/lib.rs"),
            include_str!("../../coordination/src/patterns.rs"),
            include_str!("../../models/src/lib.rs"),
            include_str!("../../capabilities/src/lib.rs"),
        )
        .replace("\r\n", "\n")
        .as_bytes(),
    );
    let key = p::canonical_digest(&(domain, scope, &implementation_digest))?;
    ProductionStrategySpec::Seed(ProductionSeedSpec {
        schema_version: p::SchemaVersion(1),
        domain,
        scope: scope.clone(),
        version: p::StrategyVersionRef(format!("repository-seed:{:?}:{}", domain, key.0)),
        content_ref: p::ContentRef("unsealed".into()),
        content_digest: p::SchemaDigest("unsealed".into()),
        implementation_digest,
    })
    .seal_identity()
}

struct InitialSeedGovernor(p::StrategyCandidate);
impl StrategyEvolutionGovernor for InitialSeedGovernor {
    fn decide(
        &self,
        c: &p::StrategyCandidate,
        e: &p::EvolutionEvaluation,
        current: &p::EvolutionSnapshot,
    ) -> EvolutionDecision {
        if c != &self.0
            || c.validate().is_err()
            || e.validate().is_err()
            || current.validate().is_err()
            || current
                .strategies
                .iter()
                .any(|s| s.domain == c.domain && s.scope == c.scope)
            || e.candidate != c.proposed_version
            || e.baseline != c.baseline
            || e.verdict != p::EvaluationVerdict::Pass
            || e.ground_truth != c.evidence
        {
            EvolutionDecision::Reject
        } else {
            EvolutionDecision::NeedOwner
        }
    }
}

impl ProductionRuntime {
    /// Registers only this compiled runtime's fixed behavior, not a supplied
    /// strategy. Opening a store never calls this owner control implicitly.
    pub fn initialize_strategy_seed(
        &self,
        owner: &p::VerifiedPrincipal,
        domain: p::StrategyDomain,
        workspace: &p::WorkspaceRef,
    ) -> p::Result<ProductionStrategySpec> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        let _serial = self
            .orchestration_commands
            .lock()
            .map_err(|_| p::Error("strategy initialization queue is unavailable".into()))?;
        let scope = p::Scope(workspace.0.clone());
        crate::brain_workspaces::check_workspace(
            &self.services().store,
            &self.config().workspace,
            workspace,
            &scope,
        )?;
        let body = repository_seed(domain, &scope)?;
        let store = self.services().store.clone();
        let stable = store.stable_strategy(domain, &scope, body.version())?;
        let aggregate = p::EvolutionAggregateRef(format!("evolution:{:?}:{}", domain, scope.0));
        if store.has_active_strategies(&scope)?
            && store
                .snapshot(scope.clone())?
                .strategies
                .iter()
                .any(|s| s.domain == domain && s.scope == scope)
        {
            return if stable.is_some() {
                Ok(body)
            } else {
                Err(p::Error("a running domain cannot be reinitialized".into()))
            };
        }
        let registered = self.register_strategy_body(body.clone(), true)?;
        let registry = crate::brain_evolution::ProductionStrategyRegistry {
            store: (*store).clone(),
            owner: owner.clone(),
            aggregate: self.services().aggregates.memory_cognition.clone(),
        };
        let equivalent = registry.resolve(body.version())? == repository_seed(domain, &scope)?;
        if !equivalent || registered != body {
            return Err(p::Error("repository baseline equivalence failed".into()));
        }
        let evidence = vec![p::EvidenceRef(
            crate::brain_evolution::spec_node(body.version()).0,
        )];
        let c = p::StrategyCandidate {
            schema_version: p::SchemaVersion(1),
            candidate: p::CandidateId(format!("initial:{}", body.version().0)),
            domain,
            scope: scope.clone(),
            target_tier: p::StabilityTier::Stable,
            proposed_version: body.version().clone(),
            baseline: p::StrategyVersionRef("repository:uninitialized-v1".into()),
            spec_ref: body.content_ref().clone(),
            spec_digest: body.digest().clone(),
            evidence: evidence.clone(),
            provenance: p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            impact: p::EvolutionImpact::Bounded,
            rollback_policy: p::StrategyRollbackPolicy {
                schema_version: p::SchemaVersion(1),
                known_good: p::StrategyVersionRef("repository:uninitialized-v1".into()),
                rollback_on_hard_regression: true,
                owner_on_unverifiable: true,
            },
        };
        let initial = p::EvolutionSnapshot {
            schema_version: p::SchemaVersion(1),
            snapshot: p::EvolutionSnapshotRef(format!("initial:{}", body.digest().0)),
            aggregates: vec![store.evolution_version(&aggregate)?],
            strategies: vec![],
            digest: p::canonical_digest(&(domain, &scope, "uninitialized"))?,
        };
        let run = p::RunId(format!("initial:{}", body.version().0));
        let evolution = M3EvolutionHarness::new((*store).clone())
            .with_governor(Arc::new(InitialSeedGovernor(c.clone())))
            .with_initial_snapshot(initial.clone())?
            .continue_control_stream(&run)?;
        if store.strategy_candidate(&c.candidate)?.is_none() {
            evolution.record_candidate(run.clone(), c.clone())?;
        }
        let comparison = p::EvolutionComparison {
            schema_version: p::SchemaVersion(1),
            evaluation: p::EvolutionEvaluationRef(format!("initial:{}", body.digest().0)),
            bundle: p::ReplayBundleRef(format!("repository:{}", body.digest().0)),
            baseline: c.baseline.clone(),
            candidate: c.proposed_version.clone(),
            case_set_digest: p::canonical_digest(&(
                body.version(),
                body.content_ref(),
                body.digest(),
            ))?,
            holdout_digest: p::canonical_digest(&(
                domain,
                &scope,
                repository_seed(domain, &scope)?,
            ))?,
            metrics: vec![p::FitnessMetric {
                schema_version: p::SchemaVersion(1),
                dimension: p::FitnessDimension::Verification,
                outcome: p::FitnessOutcome::Pass,
                measured: Some(i64::from(equivalent)),
                unit: p::FitnessUnit::Boolean,
                evidence: evidence.clone(),
            }],
            hard_invariants: vec![p::InvariantResult {
                schema_version: p::SchemaVersion(1),
                reference: p::InvariantResultRef(format!("equivalence:{}", body.digest().0)),
                name: "registered_body_equals_compiled_builtin_behavior".into(),
                outcome: p::FitnessOutcome::Pass,
                evidence: evidence.clone(),
            }],
            ground_truth: evidence,
            independent_verifier: true,
            self_eval_only: false,
        };
        use forme_eval::EvolutionEvaluator;
        let expected = forme_eval::DeterministicEvolutionEvaluator.compare(comparison.clone())?;
        let recorded = store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?
            .into_iter()
            .find_map(|e| match e.payload {
                p::EventPayload::EvolutionEvaluationRecorded(p)
                    if p.evaluation == expected.evaluation =>
                {
                    Some(p)
                }
                _ => None,
            });
        let evaluation = if let Some(recorded) = recorded {
            if recorded.candidate != expected.candidate
                || recorded.baseline != expected.baseline
                || recorded.verdict != expected.verdict
                || recorded.ground_truth != expected.ground_truth
            {
                return Err(p::Error(
                    "initial evaluation differs from recorded evidence".into(),
                ));
            }
            expected
        } else {
            evolution.evaluate(run.clone(), &c, comparison)?.evaluation
        };
        if InitialSeedGovernor(c.clone()).decide(&c, &evaluation, &initial)
            != EvolutionDecision::NeedOwner
        {
            return Err(p::Error(
                "initialization governor rejected the repository seed".into(),
            ));
        }
        let promotion = match stable {
            Some(s) => s.promotion_event,
            None => evolution.promote(run.clone(), &c, &evaluation)?,
        };
        evolution.activate(
            run,
            aggregate,
            &c,
            &evaluation,
            promotion,
            Some(p::OwnerControlRef(format!(
                "{}:initialize:{}",
                owner.0,
                body.digest().0
            ))),
        )?;
        Ok(body)
    }
}
