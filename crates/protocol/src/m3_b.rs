use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyRuntimeCompatibility {
    pub schema_version: SchemaVersion,
    pub minimum_runtime_schema: SchemaVersion,
    pub event_schema: SchemaVersion,
    pub model_profile: Option<ModelProfileRef>,
    pub tool_schema: Option<SchemaDigest>,
    pub backend_schema: Option<SchemaDigest>,
}

impl StrategyRuntimeCompatibility {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.minimum_runtime_schema.0 == 0
            || self.event_schema.0 == 0
            || self
                .model_profile
                .as_ref()
                .is_some_and(|value| blank(&value.0))
            || self
                .tool_schema
                .as_ref()
                .is_some_and(|value| blank(&value.0))
            || self
                .backend_schema
                .as_ref()
                .is_some_and(|value| blank(&value.0))
        {
            return Err(Error("strategy runtime compatibility is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LoopPhase {
    Context,
    Deliberate,
    Policy,
    Approval,
    Execute,
    Verify,
    Checkpoint,
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LoopTrigger {
    Reactive,
    Scheduled,
    CheckpointResume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopFailureFallback {
    Stop,
    AskOwner,
    PreserveCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopBudgetProfile {
    pub schema_version: SchemaVersion,
    pub max_turns: u32,
    pub max_tokens: u64,
    pub max_wall_time_ms: u64,
    pub max_cost_microunits: u64,
    pub max_tool_calls: u32,
}

impl LoopBudgetProfile {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.max_turns == 0
            || self.max_tokens == 0
            || self.max_wall_time_ms == 0
            || self.max_cost_microunits == 0
            || self.max_tool_calls == 0
        {
            return Err(Error("loop budget profile is zero or unbounded".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopStrategySpec {
    pub schema_version: SchemaVersion,
    pub version: StrategyVersionRef,
    pub scope: Scope,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub compatibility: StrategyRuntimeCompatibility,
    pub phases: Vec<LoopPhase>,
    pub triggers: Vec<LoopTrigger>,
    pub checkpoint_cadence_turns: u32,
    pub verification_cadence_turns: u32,
    pub budget: LoopBudgetProfile,
    pub failure_fallback: LoopFailureFallback,
}

impl LoopStrategySpec {
    pub fn validate(&self) -> Result<()> {
        validate_spec_identity(
            self.schema_version,
            &self.version,
            &self.scope,
            &self.content_ref,
            &self.content_digest,
            &self.compatibility,
        )?;
        self.budget.validate()?;
        let phases = self.phases.iter().copied().collect::<BTreeSet<_>>();
        let triggers = self.triggers.iter().copied().collect::<BTreeSet<_>>();
        if self.phases.is_empty()
            || phases.len() != self.phases.len()
            || self.triggers.is_empty()
            || triggers.len() != self.triggers.len()
            || self.checkpoint_cadence_turns == 0
            || self.verification_cadence_turns == 0
            || self.checkpoint_cadence_turns > self.budget.max_turns
            || self.verification_cadence_turns > self.budget.max_turns
            || !contains_all(
                &phases,
                &[
                    LoopPhase::Context,
                    LoopPhase::Deliberate,
                    LoopPhase::Policy,
                    LoopPhase::Approval,
                    LoopPhase::Execute,
                    LoopPhase::Verify,
                    LoopPhase::Finish,
                ],
            )
            || !ordered_before(&self.phases, LoopPhase::Context, LoopPhase::Deliberate)
            || !ordered_before(&self.phases, LoopPhase::Policy, LoopPhase::Approval)
            || !ordered_before(&self.phases, LoopPhase::Approval, LoopPhase::Execute)
            || !ordered_before(&self.phases, LoopPhase::Execute, LoopPhase::Verify)
            || !ordered_before(&self.phases, LoopPhase::Verify, LoopPhase::Finish)
        {
            return Err(Error(
                "loop strategy removes a governed phase or has invalid cadence".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinationMode {
    Single,
    Multi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinationTaskScale {
    Single,
    MultiStage,
    LongRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinationDecomposability {
    Whole,
    Parallel,
    Sequential,
    Iterative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinationVerifiability {
    SelfCheck,
    IndependentReview,
    Environment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinationTopology {
    Direct,
    IndependentSlices,
    ReviewGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckpointTopology {
    PerNode,
    ReviewGate,
    PerRoute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinationApplicabilitySpec {
    pub schema_version: SchemaVersion,
    pub scale: CoordinationTaskScale,
    pub decomposability: CoordinationDecomposability,
    pub verifiability: CoordinationVerifiability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinationPatternSpec {
    pub schema_version: SchemaVersion,
    pub pattern: OrchestrationPatternRef,
    pub topology: CoordinationTopology,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinationRoleWeight {
    pub schema_version: SchemaVersion,
    pub role: RoleRef,
    pub weight_basis_points: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinationStrategySpec {
    pub schema_version: SchemaVersion,
    pub version: StrategyVersionRef,
    pub scope: Scope,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub compatibility: StrategyRuntimeCompatibility,
    pub applicability: CoordinationApplicabilitySpec,
    pub patterns: Vec<CoordinationPatternSpec>,
    pub mode: CoordinationMode,
    pub role_weights: Vec<CoordinationRoleWeight>,
    pub max_subagents: u16,
    pub checkpoint_topology: CheckpointTopology,
}

impl CoordinationStrategySpec {
    pub fn validate(&self) -> Result<()> {
        validate_spec_identity(
            self.schema_version,
            &self.version,
            &self.scope,
            &self.content_ref,
            &self.content_digest,
            &self.compatibility,
        )?;
        let patterns = self
            .patterns
            .iter()
            .map(|pattern| &pattern.pattern)
            .collect::<BTreeSet<_>>();
        let roles = self
            .role_weights
            .iter()
            .map(|role| &role.role)
            .collect::<BTreeSet<_>>();
        let weight_sum = self.role_weights.iter().try_fold(0_u32, |sum, role| {
            sum.checked_add(u32::from(role.weight_basis_points))
        });
        if self.applicability.schema_version.0 == 0
            || self.patterns.is_empty()
            || patterns.len() != self.patterns.len()
            || self
                .patterns
                .iter()
                .any(|pattern| pattern.schema_version.0 == 0 || blank(&pattern.pattern.0))
            || self.role_weights.is_empty()
            || roles.len() != self.role_weights.len()
            || self.role_weights.iter().any(|role| {
                role.schema_version.0 == 0
                    || blank(&role.role.0)
                    || role.weight_basis_points == 0
                    || role.weight_basis_points > 10_000
            })
            || weight_sum != Some(10_000)
            || self.max_subagents == 0
            || self.max_subagents > 16
            || (self.mode == CoordinationMode::Single && self.max_subagents != 1)
        {
            return Err(Error(
                "coordination strategy is empty, duplicated, overflowing, or unbounded".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SelectionTarget {
    Capability,
    Model,
    Backend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SelectionFeature {
    VerifiedSuccess,
    FailurePenalty,
    Compatibility,
    Reliability,
    Cost,
    Latency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionTieBreaker {
    StableIdentity,
    LowerCost,
    RecentVerifiedSuccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionFallback {
    NoSelection,
    SeedOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionFeatureWeight {
    pub schema_version: SchemaVersion,
    pub feature: SelectionFeature,
    pub weight_basis_points: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionStrategySpec {
    pub schema_version: SchemaVersion,
    pub version: StrategyVersionRef,
    pub scope: Scope,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub compatibility: StrategyRuntimeCompatibility,
    pub target: SelectionTarget,
    pub weights: Vec<SelectionFeatureWeight>,
    pub tie_breaker: SelectionTieBreaker,
    pub fallback: SelectionFallback,
    pub max_results: u16,
}

impl SelectionStrategySpec {
    pub fn validate(&self) -> Result<()> {
        validate_spec_identity(
            self.schema_version,
            &self.version,
            &self.scope,
            &self.content_ref,
            &self.content_digest,
            &self.compatibility,
        )?;
        let features = self
            .weights
            .iter()
            .map(|weight| weight.feature)
            .collect::<BTreeSet<_>>();
        let sum = self.weights.iter().try_fold(0_u32, |sum, weight| {
            sum.checked_add(u32::from(weight.weight_basis_points))
        });
        if self.weights.is_empty()
            || features.len() != self.weights.len()
            || !features.contains(&SelectionFeature::VerifiedSuccess)
            || !features.contains(&SelectionFeature::FailurePenalty)
            || !features.contains(&SelectionFeature::Compatibility)
            || self.weights.iter().any(|weight| {
                weight.schema_version.0 == 0
                    || weight.weight_basis_points == 0
                    || weight.weight_basis_points > 10_000
            })
            || sum != Some(10_000)
            || self.max_results == 0
            || self.max_results > 256
        {
            return Err(Error(
                "selection strategy lacks result evidence or has invalid weights".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ModelStrengthBand {
    Basic,
    Standard,
    Strong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilityPredicate {
    pub schema_version: SchemaVersion,
    pub minimum_context_window: u32,
    pub requires_tool_use: bool,
    pub minimum_strength: ModelStrengthBand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelScaffoldProfile {
    pub schema_version: SchemaVersion,
    pub externalized_steps: u16,
    pub verification_passes: u16,
    pub checkpoint_cadence_steps: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAdaptationSpec {
    pub schema_version: SchemaVersion,
    pub version: StrategyVersionRef,
    pub scope: Scope,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub compatibility: StrategyRuntimeCompatibility,
    pub predicate: ModelCapabilityPredicate,
    pub scaffold: ModelScaffoldProfile,
}

impl ModelAdaptationSpec {
    pub fn validate(&self) -> Result<()> {
        validate_spec_identity(
            self.schema_version,
            &self.version,
            &self.scope,
            &self.content_ref,
            &self.content_digest,
            &self.compatibility,
        )?;
        if self.predicate.schema_version.0 == 0
            || self.predicate.minimum_context_window == 0
            || self.scaffold.schema_version.0 == 0
            || self.scaffold.externalized_steps == 0
            || self.scaffold.externalized_steps > 16
            || self.scaffold.verification_passes == 0
            || self.scaffold.verification_passes > 8
            || self.scaffold.checkpoint_cadence_steps == 0
            || self.scaffold.checkpoint_cadence_steps > self.scaffold.externalized_steps
        {
            return Err(Error(
                "model adaptation strategy is zero, unbounded, or unverifiable".into(),
            ));
        }
        Ok(())
    }
}

fn validate_spec_identity(
    schema_version: SchemaVersion,
    version: &StrategyVersionRef,
    scope: &Scope,
    content_ref: &ContentRef,
    content_digest: &SchemaDigest,
    compatibility: &StrategyRuntimeCompatibility,
) -> Result<()> {
    compatibility.validate()?;
    if schema_version.0 == 0
        || blank(&version.0)
        || blank(&scope.0)
        || blank(&content_ref.0)
        || blank(&content_digest.0)
    {
        return Err(Error("strategy spec identity is incomplete".into()));
    }
    Ok(())
}

fn contains_all<T: Ord>(actual: &BTreeSet<T>, required: &[T]) -> bool {
    required.iter().all(|value| actual.contains(value))
}

fn ordered_before<T: PartialEq>(values: &[T], left: T, right: T) -> bool {
    let left = values.iter().position(|value| value == &left);
    let right = values.iter().position(|value| value == &right);
    matches!((left, right), (Some(left), Some(right)) if left < right)
}

fn blank(value: &str) -> bool {
    value.trim().is_empty()
}
