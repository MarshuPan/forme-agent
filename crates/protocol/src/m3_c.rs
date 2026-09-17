use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CognitiveStrategyEvidencePolicy {
    pub schema_version: SchemaVersion,
    pub minimum_verified_outcomes: u16,
    pub minimum_distinct_timepoints: u16,
    pub freshness_window: DurationMs,
    pub decay_after: DurationMs,
    pub expire_after: DurationMs,
    pub require_owner_feedback: bool,
}

impl CognitiveStrategyEvidencePolicy {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.minimum_verified_outcomes == 0
            || self.minimum_verified_outcomes > 64
            || self.minimum_distinct_timepoints == 0
            || self.minimum_distinct_timepoints > self.minimum_verified_outcomes
            || self.freshness_window.0 == 0
            || self.decay_after.0 < self.freshness_window.0
            || self.expire_after.0 < self.decay_after.0
        {
            return Err(Error(
                "cognitive strategy evidence policy is incomplete or unbounded".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CognitiveStrategyEnvelope {
    pub schema_version: SchemaVersion,
    pub domain: StrategyDomain,
    pub version: StrategyVersionRef,
    pub scope: Scope,
    pub content_ref: ContentRef,
    pub content_digest: SchemaDigest,
    pub compatibility: StrategyRuntimeCompatibility,
    pub evidence_policy: CognitiveStrategyEvidencePolicy,
    pub rollback_policy: StrategyRollbackPolicy,
}

impl CognitiveStrategyEnvelope {
    pub fn validate_for(&self, expected: StrategyDomain) -> Result<()> {
        self.compatibility.validate()?;
        self.evidence_policy.validate()?;
        self.rollback_policy.validate()?;
        if self.schema_version.0 == 0
            || self.domain != expected
            || blank(&self.version.0)
            || blank(&self.scope.0)
            || blank(&self.content_ref.0)
            || blank(&self.content_digest.0)
        {
            return Err(Error(
                "cognitive strategy envelope identity is incomplete or mismatched".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrategyConflictPolicy {
    PreserveAndReevaluate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrategyFreshnessBasis {
    VerifiedEventTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UntrustedEdgePolicy {
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrategyMemorySpec {
    pub envelope: CognitiveStrategyEnvelope,
    pub conflict_policy: StrategyConflictPolicy,
    pub freshness_basis: StrategyFreshnessBasis,
    pub untrusted_edge_policy: UntrustedEdgePolicy,
    pub decay_step_basis_points: u16,
    pub maximum_derived_edges: u16,
    pub additive_schema_only: bool,
    pub rollback_on_active_evidence_loss: bool,
}

impl StrategyMemorySpec {
    pub fn validate(&self) -> Result<()> {
        self.envelope.validate_for(StrategyDomain::StrategyMemory)?;
        if self.decay_step_basis_points == 0
            || self.decay_step_basis_points > 10_000
            || self.maximum_derived_edges == 0
            || self.maximum_derived_edges > 256
            || !self.additive_schema_only
            || !self.rollback_on_active_evidence_loss
        {
            return Err(Error(
                "strategy memory policy permits silent, breaking, or unbounded evolution".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSelfAggregation {
    ScopedReliabilityAndGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelfAssessmentEffect {
    LowerCeilingOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSelfStrategySpec {
    pub envelope: CognitiveStrategyEnvelope,
    pub aggregation: AgentSelfAggregation,
    pub self_assessment_effect: SelfAssessmentEffect,
    pub evidence_window: DurationMs,
    pub verified_success_weight_basis_points: u16,
    pub verified_failure_weight_basis_points: u16,
    pub minimum_scope_evidence: u16,
}

impl AgentSelfStrategySpec {
    pub fn validate(&self) -> Result<()> {
        self.envelope.validate_for(StrategyDomain::AgentSelf)?;
        let weights = u32::from(self.verified_success_weight_basis_points)
            .checked_add(u32::from(self.verified_failure_weight_basis_points));
        if self.evidence_window.0 == 0
            || self.evidence_window.0 > self.envelope.evidence_policy.expire_after.0
            || self.verified_success_weight_basis_points == 0
            || self.verified_failure_weight_basis_points == 0
            || weights != Some(10_000)
            || self.minimum_scope_evidence < self.envelope.evidence_policy.minimum_verified_outcomes
        {
            return Err(Error(
                "agent self strategy lacks scoped result evidence or bounded weights".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PartnershipAxis {
    Complementarity,
    Collaboration,
    Correction,
    ExpressionPreference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartnershipOwnerCorrection {
    AuthoritativeEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExternalFeedbackRole {
    ContextOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartnershipStrategySpec {
    pub envelope: CognitiveStrategyEnvelope,
    pub axes: Vec<PartnershipAxis>,
    pub owner_correction: PartnershipOwnerCorrection,
    pub external_feedback: ExternalFeedbackRole,
    pub minimum_collaboration_timepoints: u16,
    pub short_term_exception_can_stabilize: HistoricalFalse,
}

impl PartnershipStrategySpec {
    pub fn validate(&self) -> Result<()> {
        self.envelope.validate_for(StrategyDomain::Partnership)?;
        let axes = self.axes.iter().copied().collect::<BTreeSet<_>>();
        if self.axes.is_empty()
            || axes.len() != self.axes.len()
            || !axes.contains(&PartnershipAxis::Correction)
            || self.minimum_collaboration_timepoints < 2
            || self.minimum_collaboration_timepoints
                < self.envelope.evidence_policy.minimum_distinct_timepoints
        {
            return Err(Error(
                "partnership strategy cannot stabilize bounded correction evidence".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustAutomaticDirection {
    CautionOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DelegationRecommendationCeiling {
    Suggest,
    Prepare,
    RequestNarrowGrant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustDelegationStrategySpec {
    pub envelope: CognitiveStrategyEnvelope,
    pub automatic_direction: TrustAutomaticDirection,
    pub maximum_recommendation: DelegationRecommendationCeiling,
    pub verified_successes_before_recommendation: u16,
    pub failures_before_downgrade: u16,
    pub owner_review_required_for_expansion: bool,
}

impl TrustDelegationStrategySpec {
    pub fn validate(&self) -> Result<()> {
        self.envelope
            .validate_for(StrategyDomain::TrustDelegation)?;
        if self.verified_successes_before_recommendation
            < self.envelope.evidence_policy.minimum_verified_outcomes
            || self.failures_before_downgrade == 0
            || !self.owner_review_required_for_expansion
        {
            return Err(Error(
                "trust delegation strategy can expand authority without owner review".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttentionCostProfile {
    pub schema_version: SchemaVersion,
    pub capacity: u32,
    pub interrupt_cost: u32,
    pub digest_cost: u32,
    pub ask_to_learn_cost: u32,
    pub rejection_cooldown_ticks: u32,
}

impl AttentionCostProfile {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.capacity == 0
            || self.interrupt_cost == 0
            || self.digest_cost == 0
            || self.ask_to_learn_cost == 0
            || self.interrupt_cost > self.capacity
            || self.digest_cost > self.capacity
            || self.ask_to_learn_cost > self.capacity
            || self.rejection_cooldown_ticks == 0
        {
            return Err(Error(
                "attention cost profile is incomplete or unbounded".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitmentHandling {
    DeterministicAndPreserved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProactivityStrategySpec {
    pub envelope: CognitiveStrategyEnvelope,
    pub minimum_value_basis_points: u16,
    pub preferred_delivery: DeliveryMode,
    pub attention: AttentionCostProfile,
    pub commitment_handling: CommitmentHandling,
    pub maximum_interruptions_per_tick: u16,
}

impl ProactivityStrategySpec {
    pub fn validate(&self) -> Result<()> {
        self.envelope.validate_for(StrategyDomain::Proactivity)?;
        self.attention.validate()?;
        if self.minimum_value_basis_points == 0
            || self.minimum_value_basis_points > 10_000
            || self.maximum_interruptions_per_tick == 0
            || self.maximum_interruptions_per_tick > 16
        {
            return Err(Error(
                "proactivity strategy can bypass value or attention boundaries".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationExpressionProfile {
    Concise,
    Contextual,
    ExplicitUncertainty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationSummaryProfile {
    PerDecision,
    PerCheckpoint,
    FinalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationSurfacePreference {
    CurrentAuthorizedSurface,
    OwnerLocalDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommunicationStrategySpec {
    pub envelope: CognitiveStrategyEnvelope,
    pub expression: CommunicationExpressionProfile,
    pub summary: CommunicationSummaryProfile,
    pub surface: CommunicationSurfacePreference,
    pub external_feedback: ExternalFeedbackRole,
    pub maximum_summary_items: u16,
}

impl CommunicationStrategySpec {
    pub fn validate(&self) -> Result<()> {
        self.envelope.validate_for(StrategyDomain::Communication)?;
        if self.maximum_summary_items == 0 || self.maximum_summary_items > 64 {
            return Err(Error(
                "communication strategy summary boundary is zero or unbounded".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "strategy_domain", content = "strategy", deny_unknown_fields)]
pub enum M3CStrategySpec {
    StrategyMemory(StrategyMemorySpec),
    AgentSelf(AgentSelfStrategySpec),
    Partnership(PartnershipStrategySpec),
    TrustDelegation(TrustDelegationStrategySpec),
    Proactivity(ProactivityStrategySpec),
    Communication(CommunicationStrategySpec),
}

impl M3CStrategySpec {
    pub fn envelope(&self) -> &CognitiveStrategyEnvelope {
        match self {
            Self::StrategyMemory(spec) => &spec.envelope,
            Self::AgentSelf(spec) => &spec.envelope,
            Self::Partnership(spec) => &spec.envelope,
            Self::TrustDelegation(spec) => &spec.envelope,
            Self::Proactivity(spec) => &spec.envelope,
            Self::Communication(spec) => &spec.envelope,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::StrategyMemory(spec) => spec.validate(),
            Self::AgentSelf(spec) => spec.validate(),
            Self::Partnership(spec) => spec.validate(),
            Self::TrustDelegation(spec) => spec.validate(),
            Self::Proactivity(spec) => spec.validate(),
            Self::Communication(spec) => spec.validate(),
        }
    }
}

fn blank(value: &str) -> bool {
    value.trim().is_empty()
}
