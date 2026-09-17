use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use forme_protocol as p;

pub trait CognitiveStrategyRegistry {
    fn resolve(
        &self,
        domain: p::StrategyDomain,
        version: &p::StrategyVersionRef,
    ) -> p::Result<p::M3CStrategySpec>;
}

pub struct InMemoryCognitiveStrategyRegistry {
    seeds: BTreeMap<p::StrategyDomain, p::StrategyVersionRef>,
    specs: Mutex<BTreeMap<(p::StrategyDomain, p::StrategyVersionRef), p::M3CStrategySpec>>,
}

impl InMemoryCognitiveStrategyRegistry {
    pub fn with_seeds(seeds: Vec<p::M3CStrategySpec>) -> p::Result<Self> {
        let mut seed_versions = BTreeMap::new();
        let mut specs = BTreeMap::new();
        for spec in seeds {
            spec.validate()?;
            let envelope = spec.envelope();
            if seed_versions
                .insert(envelope.domain, envelope.version.clone())
                .is_some()
            {
                return Err(p::Error(
                    "cognitive strategy seeds contain a duplicate domain".into(),
                ));
            }
            specs.insert((envelope.domain, envelope.version.clone()), spec);
        }
        let expected = BTreeSet::from([
            p::StrategyDomain::StrategyMemory,
            p::StrategyDomain::AgentSelf,
            p::StrategyDomain::Partnership,
            p::StrategyDomain::TrustDelegation,
            p::StrategyDomain::Proactivity,
            p::StrategyDomain::Communication,
        ]);
        if seed_versions.keys().copied().collect::<BTreeSet<_>>() != expected {
            return Err(p::Error(
                "cognitive strategy registry requires one seed per M3-C domain".into(),
            ));
        }
        Ok(Self {
            seeds: seed_versions,
            specs: Mutex::new(specs),
        })
    }

    pub fn seed_version(&self, domain: p::StrategyDomain) -> Option<&p::StrategyVersionRef> {
        self.seeds.get(&domain)
    }

    pub fn register(&self, spec: p::M3CStrategySpec) -> p::Result<()> {
        spec.validate()?;
        let envelope = spec.envelope();
        let key = (envelope.domain, envelope.version.clone());
        let mut specs = self.lock()?;
        if let Some(existing) = specs.get(&key) {
            return if existing == &spec {
                Ok(())
            } else {
                Err(p::Error(
                    "cognitive strategy version is already bound to different content".into(),
                ))
            };
        }
        specs.insert(key, spec);
        Ok(())
    }

    fn lock(
        &self,
    ) -> p::Result<
        MutexGuard<'_, BTreeMap<(p::StrategyDomain, p::StrategyVersionRef), p::M3CStrategySpec>>,
    > {
        self.specs
            .lock()
            .map_err(|_| p::Error("cognitive strategy registry is unavailable".into()))
    }
}

impl CognitiveStrategyRegistry for InMemoryCognitiveStrategyRegistry {
    fn resolve(
        &self,
        domain: p::StrategyDomain,
        version: &p::StrategyVersionRef,
    ) -> p::Result<p::M3CStrategySpec> {
        self.lock()?
            .get(&(domain, version.clone()))
            .cloned()
            .ok_or_else(|| {
                p::Error(format!(
                    "unsupported cognitive strategy domain/version {domain:?}/{}",
                    version.0
                ))
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CognitiveOutcomeKind {
    VerifiedPass,
    VerifiedFailure,
    OwnerCorrection,
    SelfAssessment,
    InitialProfile,
    ShortTermException,
    Revocation,
    ExternalInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveOutcomeEvidence {
    pub schema_version: p::SchemaVersion,
    pub reference: p::EvidenceRef,
    pub scope: p::Scope,
    pub observed_at: p::Timestamp,
    pub kind: CognitiveOutcomeKind,
    pub confidence_basis_points: u16,
    pub provenance: p::Provenance,
}

impl CognitiveOutcomeEvidence {
    fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.reference.0.trim().is_empty()
            || self.scope.0.trim().is_empty()
            || self.observed_at <= 0
            || self.confidence_basis_points > 10_000
        {
            return Err(p::Error("cognitive outcome evidence is incomplete".into()));
        }
        match self.kind {
            CognitiveOutcomeKind::VerifiedPass | CognitiveOutcomeKind::VerifiedFailure => {
                require_system_ground_truth(&self.provenance)?;
            }
            CognitiveOutcomeKind::OwnerCorrection => require_owner(&self.provenance)?,
            CognitiveOutcomeKind::Revocation => {
                if !system_or_owner(&self.provenance) {
                    return Err(p::Error("revocation evidence has no authority".into()));
                }
            }
            CognitiveOutcomeKind::SelfAssessment | CognitiveOutcomeKind::InitialProfile => {
                if !matches!(self.provenance.actor, p::Actor::Agent)
                    || self.provenance.trust_tier != p::TrustTier::ApprovedSource
                {
                    return Err(p::Error("self evidence is not agent-authored".into()));
                }
            }
            CognitiveOutcomeKind::ShortTermException => {
                if !system_or_owner(&self.provenance) {
                    return Err(p::Error("short-term evidence has no trusted source".into()));
                }
            }
            CognitiveOutcomeKind::ExternalInput => {
                if !matches!(self.provenance.actor, p::Actor::External(_))
                    || self.provenance.trust_tier != p::TrustTier::Untrusted
                {
                    return Err(p::Error(
                        "external evidence provenance was self-promoted".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CognitiveStrategyDecision {
    NeedMoreEvidence,
    PromoteCandidate,
    DowngradeAutomatically,
    OwnerReviewProposal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSelfEvolutionAssessment {
    pub schema_version: p::SchemaVersion,
    pub strategy: p::StrategyVersionRef,
    pub scope: p::Scope,
    pub result_ceiling_basis_points: u16,
    pub effective_ceiling_basis_points: u16,
    pub decision: CognitiveStrategyDecision,
    pub evidence: Vec<p::EvidenceRef>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AgentSelfEvolutionEngine;

impl AgentSelfEvolutionEngine {
    pub fn assess(
        &self,
        spec: &p::AgentSelfStrategySpec,
        evidence: &[CognitiveOutcomeEvidence],
        now: p::Timestamp,
    ) -> p::Result<AgentSelfEvolutionAssessment> {
        spec.validate()?;
        let scoped = validate_and_scope(evidence, &spec.envelope.scope, spec.evidence_window, now)?;
        let results = scoped
            .iter()
            .filter(|item| {
                matches!(
                    item.kind,
                    CognitiveOutcomeKind::VerifiedPass | CognitiveOutcomeKind::VerifiedFailure
                )
            })
            .collect::<Vec<_>>();
        let timepoints = results
            .iter()
            .map(|item| item.observed_at)
            .collect::<BTreeSet<_>>()
            .len();
        let minimum = usize::from(spec.minimum_scope_evidence);
        let enough = results.len() >= minimum
            && timepoints >= usize::from(spec.envelope.evidence_policy.minimum_distinct_timepoints);
        let passed = results
            .iter()
            .filter(|item| item.kind == CognitiveOutcomeKind::VerifiedPass)
            .count();
        let failed = results.len().saturating_sub(passed);
        let result_ceiling = weighted_result_ceiling(passed, failed, spec)?;
        let self_ceiling = scoped
            .iter()
            .filter(|item| item.kind == CognitiveOutcomeKind::SelfAssessment)
            .map(|item| item.confidence_basis_points)
            .min()
            .unwrap_or(10_000);
        let effective = result_ceiling.min(self_ceiling);
        let decision = if !enough {
            CognitiveStrategyDecision::NeedMoreEvidence
        } else if failed > passed {
            CognitiveStrategyDecision::DowngradeAutomatically
        } else {
            CognitiveStrategyDecision::PromoteCandidate
        };
        Ok(AgentSelfEvolutionAssessment {
            schema_version: p::SchemaVersion(1),
            strategy: spec.envelope.version.clone(),
            scope: spec.envelope.scope.clone(),
            result_ceiling_basis_points: result_ceiling,
            effective_ceiling_basis_points: effective,
            decision,
            evidence: results
                .into_iter()
                .map(|item| item.reference.clone())
                .collect(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartnershipEvolutionAssessment {
    pub schema_version: p::SchemaVersion,
    pub strategy: p::StrategyVersionRef,
    pub decision: CognitiveStrategyDecision,
    pub process_evidence: Vec<p::EvidenceRef>,
    pub owner_corrections: Vec<p::EvidenceRef>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PartnershipEvolutionEngine;

impl PartnershipEvolutionEngine {
    pub fn assess(
        &self,
        spec: &p::PartnershipStrategySpec,
        evidence: &[CognitiveOutcomeEvidence],
        now: p::Timestamp,
    ) -> p::Result<PartnershipEvolutionAssessment> {
        spec.validate()?;
        let scoped = validate_and_scope(
            evidence,
            &spec.envelope.scope,
            spec.envelope.evidence_policy.expire_after,
            now,
        )?;
        let process = scoped
            .iter()
            .filter(|item| item.kind == CognitiveOutcomeKind::VerifiedPass)
            .collect::<Vec<_>>();
        let corrections = scoped
            .iter()
            .filter(|item| item.kind == CognitiveOutcomeKind::OwnerCorrection)
            .collect::<Vec<_>>();
        let timepoints = process
            .iter()
            .chain(corrections.iter())
            .map(|item| item.observed_at)
            .collect::<BTreeSet<_>>()
            .len();
        let enough = timepoints >= usize::from(spec.minimum_collaboration_timepoints)
            && process.len() + corrections.len()
                >= usize::from(spec.envelope.evidence_policy.minimum_verified_outcomes)
            && (!spec.envelope.evidence_policy.require_owner_feedback || !corrections.is_empty());
        Ok(PartnershipEvolutionAssessment {
            schema_version: p::SchemaVersion(1),
            strategy: spec.envelope.version.clone(),
            decision: if enough {
                CognitiveStrategyDecision::PromoteCandidate
            } else {
                CognitiveStrategyDecision::NeedMoreEvidence
            },
            process_evidence: process
                .into_iter()
                .map(|item| item.reference.clone())
                .collect(),
            owner_corrections: corrections
                .into_iter()
                .map(|item| item.reference.clone())
                .collect(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustDelegationAssessment {
    pub schema_version: p::SchemaVersion,
    pub strategy: p::StrategyVersionRef,
    pub scope: p::Scope,
    pub decision: CognitiveStrategyDecision,
    pub recommendation_ceiling: p::DelegationRecommendationCeiling,
    pub evidence: Vec<p::EvidenceRef>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TrustDelegationEvolutionEngine;

impl TrustDelegationEvolutionEngine {
    pub fn assess(
        &self,
        spec: &p::TrustDelegationStrategySpec,
        evidence: &[CognitiveOutcomeEvidence],
        now: p::Timestamp,
    ) -> p::Result<TrustDelegationAssessment> {
        spec.validate()?;
        let scoped = validate_and_scope(
            evidence,
            &spec.envelope.scope,
            spec.envelope.evidence_policy.expire_after,
            now,
        )?;
        let successes = scoped
            .iter()
            .filter(|item| item.kind == CognitiveOutcomeKind::VerifiedPass)
            .count();
        let failures = scoped
            .iter()
            .filter(|item| {
                matches!(
                    item.kind,
                    CognitiveOutcomeKind::VerifiedFailure | CognitiveOutcomeKind::Revocation
                )
            })
            .count();
        let decision = if failures >= usize::from(spec.failures_before_downgrade) {
            CognitiveStrategyDecision::DowngradeAutomatically
        } else if successes >= usize::from(spec.verified_successes_before_recommendation) {
            CognitiveStrategyDecision::OwnerReviewProposal
        } else {
            CognitiveStrategyDecision::NeedMoreEvidence
        };
        let evidence = scoped
            .iter()
            .filter(|item| {
                matches!(
                    item.kind,
                    CognitiveOutcomeKind::VerifiedPass
                        | CognitiveOutcomeKind::VerifiedFailure
                        | CognitiveOutcomeKind::Revocation
                )
            })
            .map(|item| item.reference.clone())
            .collect();
        Ok(TrustDelegationAssessment {
            schema_version: p::SchemaVersion(1),
            strategy: spec.envelope.version.clone(),
            scope: spec.envelope.scope.clone(),
            decision,
            recommendation_ceiling: spec.maximum_recommendation,
            evidence,
        })
    }

    pub fn owner_review(
        &self,
        assessment: &TrustDelegationAssessment,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<OwnerReviewedDelegationRecommendation> {
        if assessment.decision != CognitiveStrategyDecision::OwnerReviewProposal
            || owner.0.trim().is_empty()
        {
            return Err(p::Error(
                "delegation recommendation lacks authenticated owner review".into(),
            ));
        }
        Ok(OwnerReviewedDelegationRecommendation {
            schema_version: p::SchemaVersion(1),
            strategy: assessment.strategy.clone(),
            scope: assessment.scope.clone(),
            ceiling: assessment.recommendation_ceiling,
            reviewed_by: owner,
            evidence: assessment.evidence.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerReviewedDelegationRecommendation {
    pub schema_version: p::SchemaVersion,
    pub strategy: p::StrategyVersionRef,
    pub scope: p::Scope,
    pub ceiling: p::DelegationRecommendationCeiling,
    pub reviewed_by: p::VerifiedPrincipal,
    pub evidence: Vec<p::EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProactivityFitnessSample {
    pub schema_version: p::SchemaVersion,
    pub adopted: u32,
    pub rejected: u32,
    pub deferred: u32,
    pub interrupt_regrets: u32,
    pub ask_to_learn_helpful: u32,
    pub missed_commitments: u32,
    pub attention_cost: u64,
    pub ground_truth: Vec<p::EvidenceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProactivityRuntimeDecision {
    Emit(p::DeliveryMode),
    SuppressBelowValue,
    SuppressAttentionBudget,
    BlockedByGovernance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProactivityOpportunity {
    pub schema_version: p::SchemaVersion,
    pub value_basis_points: u16,
    pub commitment: bool,
    pub requested_delivery: p::DeliveryMode,
    pub attention_remaining: u32,
    pub interruptions_used: u16,
    pub value_gate_passed: bool,
    pub competence_gate_passed: bool,
    pub policy_and_envelope_passed: bool,
}

pub fn compare_proactivity_strategy(
    spec: &p::ProactivityStrategySpec,
    baseline: &ProactivityFitnessSample,
    candidate: &ProactivityFitnessSample,
) -> p::Result<CognitiveStrategyDecision> {
    spec.validate()?;
    validate_proactivity_fitness(baseline)?;
    validate_proactivity_fitness(candidate)?;
    if candidate.missed_commitments != 0
        || candidate.adopted < baseline.adopted
        || candidate.ask_to_learn_helpful < baseline.ask_to_learn_helpful
        || candidate.interrupt_regrets > baseline.interrupt_regrets
        || candidate.attention_cost > baseline.attention_cost
    {
        return Ok(CognitiveStrategyDecision::DowngradeAutomatically);
    }
    Ok(CognitiveStrategyDecision::PromoteCandidate)
}

pub fn apply_proactivity_strategy(
    spec: &p::ProactivityStrategySpec,
    opportunity: &ProactivityOpportunity,
) -> p::Result<ProactivityRuntimeDecision> {
    spec.validate()?;
    if opportunity.schema_version.0 == 0
        || opportunity.value_basis_points > 10_000
        || opportunity.attention_remaining > spec.attention.capacity
    {
        return Err(p::Error("proactivity opportunity is incomplete".into()));
    }
    if !opportunity.value_gate_passed
        || !opportunity.competence_gate_passed
        || !opportunity.policy_and_envelope_passed
    {
        return Ok(ProactivityRuntimeDecision::BlockedByGovernance);
    }
    if !opportunity.commitment && opportunity.value_basis_points < spec.minimum_value_basis_points {
        return Ok(ProactivityRuntimeDecision::SuppressBelowValue);
    }
    let delivery = if opportunity.requested_delivery == p::DeliveryMode::Interrupt
        && opportunity.interruptions_used >= spec.maximum_interruptions_per_tick
    {
        p::DeliveryMode::Digest
    } else {
        opportunity.requested_delivery
    };
    let cost = match delivery {
        p::DeliveryMode::Interrupt => spec.attention.interrupt_cost,
        p::DeliveryMode::Digest | p::DeliveryMode::Hitchhike => spec.attention.digest_cost,
        p::DeliveryMode::Internal => 0,
    };
    if cost > opportunity.attention_remaining {
        if opportunity.commitment {
            return Ok(ProactivityRuntimeDecision::Emit(p::DeliveryMode::Internal));
        }
        return Ok(ProactivityRuntimeDecision::SuppressAttentionBudget);
    }
    Ok(ProactivityRuntimeDecision::Emit(delivery))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunicationRuntimeBoundary {
    pub schema_version: p::SchemaVersion,
    pub current_surface_authorized: bool,
    pub recipient_bound: bool,
    pub disclosure: p::DisclosureOutcome,
    pub outward: bool,
    pub risk: p::Risk,
    pub approval_granted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommunicationRuntimeDecision {
    PrepareOnAuthorizedSurface,
    NeedActionApproval,
    Refuse,
}

pub fn apply_communication_strategy(
    spec: &p::CommunicationStrategySpec,
    boundary: &CommunicationRuntimeBoundary,
) -> p::Result<CommunicationRuntimeDecision> {
    spec.validate()?;
    if boundary.schema_version.0 == 0 {
        return Err(p::Error(
            "communication runtime boundary is incomplete".into(),
        ));
    }
    if !boundary.current_surface_authorized
        || !boundary.recipient_bound
        || matches!(
            boundary.disclosure,
            p::DisclosureOutcome::Refuse | p::DisclosureOutcome::Blur
        )
    {
        return Ok(CommunicationRuntimeDecision::Refuse);
    }
    if boundary.outward && !boundary.approval_granted {
        return Ok(CommunicationRuntimeDecision::NeedActionApproval);
    }
    Ok(CommunicationRuntimeDecision::PrepareOnAuthorizedSurface)
}

fn validate_and_scope<'a>(
    evidence: &'a [CognitiveOutcomeEvidence],
    scope: &p::Scope,
    window: p::DurationMs,
    now: p::Timestamp,
) -> p::Result<Vec<&'a CognitiveOutcomeEvidence>> {
    if now <= 0 || evidence.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = BTreeSet::new();
    let mut scoped = Vec::new();
    for item in evidence {
        item.validate()?;
        if !seen.insert(item.reference.clone()) {
            return Err(p::Error("cognitive outcome evidence is duplicated".into()));
        }
        if &item.scope == scope
            && elapsed(item.observed_at, now).is_some_and(|age| age <= window.0)
            && item.kind != CognitiveOutcomeKind::ExternalInput
            && item.kind != CognitiveOutcomeKind::ShortTermException
            && item.kind != CognitiveOutcomeKind::InitialProfile
        {
            scoped.push(item);
        }
    }
    Ok(scoped)
}

fn weighted_result_ceiling(
    passed: usize,
    failed: usize,
    spec: &p::AgentSelfStrategySpec,
) -> p::Result<u16> {
    let pass = u64::try_from(passed)
        .ok()
        .and_then(|value| value.checked_mul(u64::from(spec.verified_success_weight_basis_points)))
        .ok_or_else(|| p::Error("agent self success evidence overflowed".into()))?;
    let fail = u64::try_from(failed)
        .ok()
        .and_then(|value| value.checked_mul(u64::from(spec.verified_failure_weight_basis_points)))
        .ok_or_else(|| p::Error("agent self failure evidence overflowed".into()))?;
    let total = pass
        .checked_add(fail)
        .ok_or_else(|| p::Error("agent self result evidence overflowed".into()))?;
    if total == 0 {
        return Ok(0);
    }
    u16::try_from(pass.saturating_mul(10_000) / total)
        .map_err(|_| p::Error("agent self result ceiling overflowed".into()))
}

fn validate_proactivity_fitness(sample: &ProactivityFitnessSample) -> p::Result<()> {
    if sample.schema_version.0 == 0
        || sample.ground_truth.is_empty()
        || sample
            .ground_truth
            .iter()
            .any(|reference| reference.0.trim().is_empty())
    {
        return Err(p::Error(
            "proactivity fitness lacks ground-truth evidence".into(),
        ));
    }
    Ok(())
}

fn require_system_ground_truth(provenance: &p::Provenance) -> p::Result<()> {
    if matches!(provenance.actor, p::Actor::System)
        && provenance.trust_tier == p::TrustTier::VerifiedProcess
    {
        Ok(())
    } else {
        Err(p::Error(
            "result evidence was not sealed by a verified process".into(),
        ))
    }
}

fn require_owner(provenance: &p::Provenance) -> p::Result<()> {
    if matches!(provenance.actor, p::Actor::Owner)
        && provenance.trust_tier == p::TrustTier::OwnerInput
    {
        Ok(())
    } else {
        Err(p::Error(
            "owner feedback provenance is not authenticated".into(),
        ))
    }
}

fn system_or_owner(provenance: &p::Provenance) -> bool {
    (matches!(provenance.actor, p::Actor::System)
        && provenance.trust_tier == p::TrustTier::VerifiedProcess)
        || (matches!(provenance.actor, p::Actor::Owner)
            && provenance.trust_tier == p::TrustTier::OwnerInput)
}

fn elapsed(occurred_at: p::Timestamp, now: p::Timestamp) -> Option<u64> {
    now.checked_sub(occurred_at)
        .and_then(|value| u64::try_from(value).ok())
}
