use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StrategyDomain {
    Loop,
    Coordination,
    CapabilitySelection,
    ModelSelection,
    BackendSelection,
    ModelAdaptation,
    StrategyMemory,
    AgentSelf,
    Partnership,
    TrustDelegation,
    Proactivity,
    Communication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EvolutionImpact {
    Cautious,
    Bounded,
    Expansive,
    Constitutional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EffectMode {
    ExactReplay,
    CounterfactualDeny,
    LiveGoverned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EvaluationVerdict {
    Pass,
    Fail,
    Unverifiable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FitnessOutcome {
    Pass,
    Fail,
    Unverifiable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FitnessDimension {
    Quality,
    Verification,
    Cost,
    Latency,
    RiskExposure,
    Interruption,
    Delegation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FitnessUnit {
    Count,
    Milliseconds,
    Tokens,
    BasisPoints,
    Boolean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InFlightDisposition {
    KeepPinned,
    Cancel,
    WaitForOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HistoricalFalse;

impl Serialize for HistoricalFalse {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bool(false)
    }
}

impl<'de> Deserialize<'de> for HistoricalFalse {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if bool::deserialize(deserializer)? {
            Err(serde::de::Error::custom("expected false"))
        } else {
            Ok(Self)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyRollbackPolicy {
    pub schema_version: SchemaVersion,
    pub known_good: StrategyVersionRef,
    pub rollback_on_hard_regression: bool,
    pub owner_on_unverifiable: bool,
}

impl StrategyRollbackPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || blank(&self.known_good.0) {
            return Err(Error("strategy rollback policy is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyCandidate {
    pub schema_version: SchemaVersion,
    pub candidate: CandidateId,
    pub domain: StrategyDomain,
    pub scope: Scope,
    pub target_tier: StabilityTier,
    pub proposed_version: StrategyVersionRef,
    pub baseline: StrategyVersionRef,
    pub spec_ref: ContentRef,
    pub spec_digest: SchemaDigest,
    pub evidence: Vec<EvidenceRef>,
    pub provenance: Provenance,
    pub impact: EvolutionImpact,
    pub rollback_policy: StrategyRollbackPolicy,
}

impl StrategyCandidate {
    pub fn validate(&self) -> Result<()> {
        self.rollback_policy.validate()?;
        if self.schema_version.0 == 0
            || blank(&self.candidate.0)
            || blank(&self.scope.0)
            || self.target_tier != StabilityTier::Stable
            || blank(&self.proposed_version.0)
            || blank(&self.baseline.0)
            || self.proposed_version == self.baseline
            || self.rollback_policy.known_good != self.baseline
            || blank(&self.spec_ref.0)
            || blank(&self.spec_digest.0)
            || !all_nonempty(&self.evidence, |value| &value.0)
        {
            return Err(Error("strategy candidate is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvolutionAggregateVersion {
    pub schema_version: SchemaVersion,
    pub aggregate: EvolutionAggregateRef,
    pub value: u64,
}

impl EvolutionAggregateVersion {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || blank(&self.aggregate.0) {
            return Err(Error("evolution aggregate version is incomplete".into()));
        }
        Ok(())
    }

    pub fn next(&self) -> Result<Self> {
        Ok(Self {
            schema_version: self.schema_version,
            aggregate: self.aggregate.clone(),
            value: self
                .value
                .checked_add(1)
                .ok_or_else(|| Error("evolution aggregate version is exhausted".into()))?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveStrategyRef {
    pub schema_version: SchemaVersion,
    pub id: ActiveStrategyId,
    pub aggregate: EvolutionAggregateRef,
    pub domain: StrategyDomain,
    pub scope: Scope,
    pub version: StrategyVersionRef,
    pub spec_ref: ContentRef,
    pub spec_digest: SchemaDigest,
    pub activation_event: EventId,
}

impl ActiveStrategyRef {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || blank(&self.id.0)
            || blank(&self.aggregate.0)
            || blank(&self.scope.0)
            || blank(&self.version.0)
            || blank(&self.spec_ref.0)
            || blank(&self.spec_digest.0)
            || blank(&self.activation_event.0)
        {
            return Err(Error("active strategy reference is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvolutionSnapshot {
    pub schema_version: SchemaVersion,
    pub snapshot: EvolutionSnapshotRef,
    pub aggregates: Vec<EvolutionAggregateVersion>,
    pub strategies: Vec<ActiveStrategyRef>,
    pub digest: SchemaDigest,
}

impl EvolutionSnapshot {
    pub fn validate(&self) -> Result<()> {
        let aggregates = self
            .aggregates
            .iter()
            .map(|version| &version.aggregate)
            .collect::<BTreeSet<_>>();
        let strategies = self
            .strategies
            .iter()
            .map(|strategy| (&strategy.aggregate, strategy.domain, &strategy.scope))
            .collect::<BTreeSet<_>>();
        if self.schema_version.0 == 0
            || blank(&self.snapshot.0)
            || self.aggregates.is_empty()
            || aggregates.len() != self.aggregates.len()
            || self
                .aggregates
                .iter()
                .any(|version| version.validate().is_err())
            || strategies.len() != self.strategies.len()
            || self
                .strategies
                .iter()
                .any(|strategy| strategy.validate().is_err())
            || self.strategies.iter().any(|strategy| {
                !self
                    .aggregates
                    .iter()
                    .any(|version| version.aggregate == strategy.aggregate)
            })
            || blank(&self.digest.0)
        {
            return Err(Error("evolution snapshot is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaySnapshot {
    pub schema_version: SchemaVersion,
    pub event_schema: SchemaVersion,
    pub policy: PolicyProfileRef,
    pub loop_spec: LoopSpecRef,
    pub model: ModelProfileRef,
    pub tool_schema: SchemaDigest,
    pub driver_profiles: Vec<DriverProfileRef>,
    pub evolution: EvolutionSnapshot,
    pub migration_graph_digest: SchemaDigest,
}

impl ReplaySnapshot {
    pub fn validate(&self) -> Result<()> {
        self.evolution.validate()?;
        if self.schema_version.0 == 0
            || self.event_schema.0 == 0
            || blank(&self.policy.0)
            || blank(&self.loop_spec.0)
            || blank(&self.model.0)
            || blank(&self.tool_schema.0)
            || !all_nonempty(&self.driver_profiles, |value| &value.0)
            || blank(&self.migration_graph_digest.0)
        {
            return Err(Error("replay snapshot is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayRequest {
    pub schema_version: SchemaVersion,
    pub bundle: ReplayBundleRef,
    pub source_runs: Vec<RunId>,
    pub event_refs: Vec<EventId>,
    pub cases: Vec<EvaluationCaseRef>,
    pub snapshot: ReplaySnapshot,
    pub effect_mode: EffectMode,
}

impl ReplayRequest {
    pub fn validate(&self) -> Result<()> {
        self.snapshot.validate()?;
        if self.schema_version.0 == 0
            || blank(&self.bundle.0)
            || !all_nonempty(&self.source_runs, |value| &value.0)
            || !all_nonempty(&self.event_refs, |value| &value.0)
            || !all_nonempty(&self.cases, |value| &value.0)
            || !unique(&self.source_runs)
            || !unique(&self.event_refs)
            || !unique(&self.cases)
            || self.effect_mode == EffectMode::LiveGoverned
        {
            return Err(Error(
                "replay request is incomplete or permits live effects".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayBundle {
    pub schema_version: SchemaVersion,
    pub bundle: ReplayBundleRef,
    pub source_runs: Vec<RunId>,
    pub event_refs: Vec<EventId>,
    pub cases: Vec<EvaluationCaseRef>,
    pub snapshot: ReplaySnapshot,
    pub effect_mode: EffectMode,
    pub content_digest: SchemaDigest,
}

impl ReplayBundle {
    pub fn validate(&self) -> Result<()> {
        ReplayRequest {
            schema_version: self.schema_version,
            bundle: self.bundle.clone(),
            source_runs: self.source_runs.clone(),
            event_refs: self.event_refs.clone(),
            cases: self.cases.clone(),
            snapshot: self.snapshot.clone(),
            effect_mode: self.effect_mode,
        }
        .validate()?;
        if blank(&self.content_digest.0) {
            return Err(Error("replay bundle digest is empty".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactReplayReport {
    pub schema_version: SchemaVersion,
    pub report: ExactReplayReportRef,
    pub bundle: ReplayBundleRef,
    pub event_count: u64,
    pub projections_match: bool,
    pub history_unchanged: bool,
    pub effect_calls: u64,
    pub digest: SchemaDigest,
}

impl ExactReplayReport {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || blank(&self.report.0)
            || blank(&self.bundle.0)
            || self.event_count == 0
            || !self.projections_match
            || !self.history_unchanged
            || self.effect_calls != 0
            || blank(&self.digest.0)
        {
            return Err(Error(
                "exact replay report does not prove a safe replay".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FitnessMetric {
    pub schema_version: SchemaVersion,
    pub dimension: FitnessDimension,
    pub outcome: FitnessOutcome,
    pub measured: Option<i64>,
    pub unit: FitnessUnit,
    pub evidence: Vec<EvidenceRef>,
}

impl FitnessMetric {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.measured.is_some_and(|value| value < 0)
            || !all_nonempty(&self.evidence, |value| &value.0)
        {
            return Err(Error("fitness metric is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvariantResult {
    pub schema_version: SchemaVersion,
    pub reference: InvariantResultRef,
    pub name: String,
    pub outcome: FitnessOutcome,
    pub evidence: Vec<EvidenceRef>,
}

impl InvariantResult {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || blank(&self.reference.0)
            || blank(&self.name)
            || !all_nonempty(&self.evidence, |value| &value.0)
        {
            return Err(Error("invariant result is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvolutionComparison {
    pub schema_version: SchemaVersion,
    pub evaluation: EvolutionEvaluationRef,
    pub bundle: ReplayBundleRef,
    pub baseline: StrategyVersionRef,
    pub candidate: StrategyVersionRef,
    pub case_set_digest: SchemaDigest,
    pub holdout_digest: SchemaDigest,
    pub metrics: Vec<FitnessMetric>,
    pub hard_invariants: Vec<InvariantResult>,
    pub ground_truth: Vec<EvidenceRef>,
    pub independent_verifier: bool,
    pub self_eval_only: bool,
}

impl EvolutionComparison {
    pub fn validate(&self) -> Result<()> {
        let metric_dimensions = self
            .metrics
            .iter()
            .map(|metric| metric.dimension)
            .collect::<BTreeSet<_>>();
        let invariants = self
            .hard_invariants
            .iter()
            .map(|result| &result.reference)
            .collect::<BTreeSet<_>>();
        if self.schema_version.0 == 0
            || blank(&self.evaluation.0)
            || blank(&self.bundle.0)
            || blank(&self.baseline.0)
            || blank(&self.candidate.0)
            || self.baseline == self.candidate
            || blank(&self.case_set_digest.0)
            || blank(&self.holdout_digest.0)
            || self.case_set_digest == self.holdout_digest
            || self.metrics.is_empty()
            || metric_dimensions.len() != self.metrics.len()
            || self.metrics.iter().any(|metric| metric.validate().is_err())
            || self.hard_invariants.is_empty()
            || invariants.len() != self.hard_invariants.len()
            || self
                .hard_invariants
                .iter()
                .any(|result| result.validate().is_err())
            || self
                .ground_truth
                .iter()
                .any(|reference| blank(&reference.0))
        {
            return Err(Error("evolution comparison is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvolutionEvaluation {
    pub schema_version: SchemaVersion,
    pub evaluation: EvolutionEvaluationRef,
    pub bundle: ReplayBundleRef,
    pub baseline: StrategyVersionRef,
    pub candidate: StrategyVersionRef,
    pub case_set_digest: SchemaDigest,
    pub holdout_digest: SchemaDigest,
    pub metrics: Vec<FitnessMetric>,
    pub hard_invariants: Vec<InvariantResult>,
    pub ground_truth: Vec<EvidenceRef>,
    pub independent_verifier: bool,
    pub verdict: EvaluationVerdict,
}

impl EvolutionEvaluation {
    pub fn validate(&self) -> Result<()> {
        EvolutionComparison {
            schema_version: self.schema_version,
            evaluation: self.evaluation.clone(),
            bundle: self.bundle.clone(),
            baseline: self.baseline.clone(),
            candidate: self.candidate.clone(),
            case_set_digest: self.case_set_digest.clone(),
            holdout_digest: self.holdout_digest.clone(),
            metrics: self.metrics.clone(),
            hard_invariants: self.hard_invariants.clone(),
            ground_truth: self.ground_truth.clone(),
            independent_verifier: self.independent_verifier,
            self_eval_only: false,
        }
        .validate()?;
        if self.verdict == EvaluationVerdict::Pass
            && (!self.independent_verifier
                || self.ground_truth.is_empty()
                || self
                    .metrics
                    .iter()
                    .any(|metric| metric.outcome != FitnessOutcome::Pass)
                || self
                    .hard_invariants
                    .iter()
                    .any(|result| result.outcome != FitnessOutcome::Pass))
        {
            return Err(Error(
                "passing evaluation lacks independent ground truth".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyActivation {
    pub schema_version: SchemaVersion,
    pub aggregate: EvolutionAggregateRef,
    pub domain: StrategyDomain,
    pub scope: Scope,
    pub from: Option<StrategyVersionRef>,
    pub to: StrategyVersionRef,
    pub spec_ref: ContentRef,
    pub spec_digest: SchemaDigest,
    pub evaluation: EvolutionEvaluationRef,
    pub promotion: EventId,
    pub owner_confirmation: Option<OwnerControlRef>,
    pub impact: EvolutionImpact,
    pub expected_version: EvolutionAggregateVersion,
    pub committed_version: EvolutionAggregateVersion,
}

impl StrategyActivation {
    pub fn validate(&self) -> Result<()> {
        validate_version_step(
            &self.aggregate,
            &self.expected_version,
            &self.committed_version,
        )?;
        let owner_required = matches!(
            self.impact,
            EvolutionImpact::Bounded | EvolutionImpact::Expansive
        );
        if self.schema_version.0 == 0
            || blank(&self.aggregate.0)
            || blank(&self.scope.0)
            || self.from.as_ref().is_some_and(|value| blank(&value.0))
            || blank(&self.to.0)
            || self.from.as_ref() == Some(&self.to)
            || blank(&self.spec_ref.0)
            || blank(&self.spec_digest.0)
            || blank(&self.evaluation.0)
            || blank(&self.promotion.0)
            || self
                .owner_confirmation
                .as_ref()
                .is_some_and(|value| blank(&value.0))
            || (owner_required && self.owner_confirmation.is_none())
            || self.impact == EvolutionImpact::Constitutional
        {
            return Err(Error(
                "strategy activation is incomplete or unauthorized".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrategyRollback {
    pub schema_version: SchemaVersion,
    pub aggregate: EvolutionAggregateRef,
    pub domain: StrategyDomain,
    pub scope: Scope,
    pub failed: StrategyVersionRef,
    pub restored: StrategyVersionRef,
    pub restored_spec_ref: ContentRef,
    pub restored_spec_digest: SchemaDigest,
    pub triggers: Vec<EvidenceRef>,
    pub expected_version: EvolutionAggregateVersion,
    pub committed_version: EvolutionAggregateVersion,
    pub in_flight: InFlightDisposition,
    pub external_effects_reverted: HistoricalFalse,
}

impl StrategyRollback {
    pub fn validate(&self) -> Result<()> {
        validate_version_step(
            &self.aggregate,
            &self.expected_version,
            &self.committed_version,
        )?;
        if self.schema_version.0 == 0
            || blank(&self.aggregate.0)
            || blank(&self.scope.0)
            || blank(&self.failed.0)
            || blank(&self.restored.0)
            || self.failed == self.restored
            || blank(&self.restored_spec_ref.0)
            || blank(&self.restored_spec_digest.0)
            || !all_nonempty(&self.triggers, |value| &value.0)
        {
            return Err(Error("strategy rollback is incomplete".into()));
        }
        Ok(())
    }
}

fn validate_version_step(
    aggregate: &EvolutionAggregateRef,
    expected: &EvolutionAggregateVersion,
    committed: &EvolutionAggregateVersion,
) -> Result<()> {
    expected.validate()?;
    committed.validate()?;
    if expected.aggregate != *aggregate
        || committed.aggregate != *aggregate
        || committed.value != expected.value.checked_add(1).unwrap_or(0)
    {
        return Err(Error("evolution aggregate version step is invalid".into()));
    }
    Ok(())
}

fn blank(value: &str) -> bool {
    value.trim().is_empty()
}

fn all_nonempty<T>(values: &[T], text: impl Fn(&T) -> &str) -> bool {
    !values.is_empty() && values.iter().all(|value| !blank(text(value)))
}

fn unique<T: Ord>(values: &[T]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}
