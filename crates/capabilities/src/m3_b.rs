use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use forme_protocol as p;

use crate::Toolset;

pub type SelectionPolicy = p::SelectionStrategySpec;

pub trait SelectionPolicyRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<SelectionPolicy>;
}

pub struct InMemorySelectionPolicyRegistry {
    seed: p::StrategyVersionRef,
    specs: Mutex<BTreeMap<p::StrategyVersionRef, SelectionPolicy>>,
}

impl InMemorySelectionPolicyRegistry {
    pub fn with_seed(seed: SelectionPolicy) -> p::Result<Self> {
        seed.validate()?;
        let version = seed.version.clone();
        Ok(Self {
            seed: version.clone(),
            specs: Mutex::new(BTreeMap::from([(version, seed)])),
        })
    }

    pub fn seed_version(&self) -> &p::StrategyVersionRef {
        &self.seed
    }

    pub fn register(&self, spec: SelectionPolicy) -> p::Result<()> {
        spec.validate()?;
        let mut specs = self.lock()?;
        if let Some(existing) = specs.get(&spec.version) {
            return if existing == &spec {
                Ok(())
            } else {
                Err(p::Error(
                    "selection strategy version is already bound to different content".into(),
                ))
            };
        }
        specs.insert(spec.version.clone(), spec);
        Ok(())
    }

    fn lock(&self) -> p::Result<MutexGuard<'_, BTreeMap<p::StrategyVersionRef, SelectionPolicy>>> {
        self.specs
            .lock()
            .map_err(|_| p::Error("selection strategy registry is unavailable".into()))
    }
}

impl SelectionPolicyRegistry for InMemorySelectionPolicyRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<SelectionPolicy> {
        self.lock()?.get(version).cloned().ok_or_else(|| {
            p::Error(format!(
                "unsupported selection strategy version {}",
                version.0
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionCandidate {
    pub schema_version: p::SchemaVersion,
    pub reference: p::ResourceRef,
    pub evidence_key: p::CapabilityRef,
    pub cost_microunits: u64,
    pub latency_ms: u64,
    pub compatible: bool,
    pub lifecycle_active: bool,
    pub managed_allowed: bool,
    pub permission_allowed: bool,
    pub scope_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefilteredSelectionSet {
    target: p::SelectionTarget,
    candidates: Vec<SelectionCandidate>,
}

impl PrefilteredSelectionSet {
    pub fn from_authorized(
        target: p::SelectionTarget,
        candidates: Vec<SelectionCandidate>,
        authorized: &BTreeSet<p::ResourceRef>,
    ) -> p::Result<Self> {
        if candidates.iter().any(|candidate| {
            candidate.schema_version.0 == 0
                || candidate.reference.0.trim().is_empty()
                || candidate.evidence_key.0.trim().is_empty()
        }) {
            return Err(p::Error("selection candidate is incomplete".into()));
        }
        let mut seen = BTreeSet::new();
        let candidates = candidates
            .into_iter()
            .filter(|candidate| {
                candidate.lifecycle_active
                    && candidate.managed_allowed
                    && candidate.permission_allowed
                    && candidate.scope_allowed
                    && authorized.contains(&candidate.reference)
            })
            .filter(|candidate| seen.insert(candidate.reference.clone()))
            .collect();
        Ok(Self { target, candidates })
    }

    pub fn from_toolset(toolset: &Toolset) -> p::Result<Self> {
        if toolset.schema_version.0 == 0 || toolset.toolset_ref.0.trim().is_empty() {
            return Err(p::Error("resolved toolset is incomplete".into()));
        }
        let authorized = toolset
            .items
            .iter()
            .map(|item| p::ResourceRef(item.id.0.clone()))
            .collect::<BTreeSet<_>>();
        let candidates = toolset
            .items
            .iter()
            .map(|item| SelectionCandidate {
                schema_version: item.schema_version,
                reference: p::ResourceRef(item.id.0.clone()),
                evidence_key: item.id.clone(),
                cost_microunits: 0,
                latency_ms: 0,
                compatible: true,
                lifecycle_active: true,
                managed_allowed: true,
                permission_allowed: true,
                scope_allowed: true,
            })
            .collect();
        Self::from_authorized(p::SelectionTarget::Capability, candidates, &authorized)
    }

    pub fn candidates(&self) -> &[SelectionCandidate] {
        &self.candidates
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionEvidenceSet {
    pub schema_version: p::SchemaVersion,
    pub capability: Vec<p::CapabilityEvidence>,
    pub failures: BTreeMap<p::CapabilityRef, Vec<p::FailureEvidenceRef>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedSelection {
    pub schema_version: p::SchemaVersion,
    pub strategy: p::StrategyVersionRef,
    pub considered: Vec<p::ResourceRef>,
    pub ordered: Vec<p::ResourceRef>,
    pub verified_evidence: BTreeMap<p::ResourceRef, Vec<p::CapabilityEvidenceRef>>,
}

pub fn rank_prefiltered(
    policy: &SelectionPolicy,
    candidates: &PrefilteredSelectionSet,
    evidence: &SelectionEvidenceSet,
) -> p::Result<RankedSelection> {
    policy.validate()?;
    if evidence.schema_version.0 == 0 || policy.target != candidates.target {
        return Err(p::Error(
            "selection evidence or target does not match the active strategy".into(),
        ));
    }
    let weights = policy
        .weights
        .iter()
        .map(|weight| (weight.feature, i128::from(weight.weight_basis_points)))
        .collect::<BTreeMap<_, _>>();
    let mut scored = candidates
        .candidates
        .iter()
        .enumerate()
        .map(|(seed_index, candidate)| {
            let matching = evidence
                .capability
                .iter()
                .filter(|item| item.capability == candidate.evidence_key)
                .collect::<Vec<_>>();
            let verified_success = matching
                .iter()
                .filter(|item| is_verified(item) && is_success(&item.outcome.0))
                .count() as i128;
            let verified_failure = matching
                .iter()
                .filter(|item| is_verified(item) && is_failure(&item.outcome.0))
                .count() as i128
                + evidence
                    .failures
                    .get(&candidate.evidence_key)
                    .map_or(0_i128, |items| items.len() as i128);
            let reliability = matching.iter().filter(|item| is_verified(item)).count() as i128;
            let compatibility = i128::from(candidate.compatible);
            let score = weights[&p::SelectionFeature::VerifiedSuccess] * verified_success
                - weights[&p::SelectionFeature::FailurePenalty] * verified_failure
                + weights[&p::SelectionFeature::Compatibility] * compatibility
                + weights
                    .get(&p::SelectionFeature::Reliability)
                    .copied()
                    .unwrap_or(0)
                    * reliability
                - weights
                    .get(&p::SelectionFeature::Cost)
                    .copied()
                    .unwrap_or(0)
                    * i128::from(candidate.cost_microunits.min(10_000))
                - weights
                    .get(&p::SelectionFeature::Latency)
                    .copied()
                    .unwrap_or(0)
                    * i128::from(candidate.latency_ms.min(10_000));
            let refs = matching
                .iter()
                .filter(|item| is_verified(item))
                .map(|item| {
                    p::CapabilityEvidenceRef(format!(
                        "capability-evidence:{}:{}:{}",
                        item.capability.0, item.outcome.0, item.reliability.0
                    ))
                })
                .collect::<Vec<_>>();
            (candidate, seed_index, score, verified_success, refs)
        })
        .filter(|(_, _, _, verified_success, _)| {
            *verified_success > 0 || policy.fallback == p::SelectionFallback::SeedOrder
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .2
            .cmp(&left.2)
            .then_with(|| match policy.tie_breaker {
                p::SelectionTieBreaker::StableIdentity => left.0.reference.cmp(&right.0.reference),
                p::SelectionTieBreaker::LowerCost => left
                    .0
                    .cost_microunits
                    .cmp(&right.0.cost_microunits)
                    .then_with(|| left.0.reference.cmp(&right.0.reference)),
                p::SelectionTieBreaker::RecentVerifiedSuccess => right
                    .3
                    .cmp(&left.3)
                    .then_with(|| left.0.reference.cmp(&right.0.reference)),
            })
            .then_with(|| left.1.cmp(&right.1))
    });
    scored.truncate(usize::from(policy.max_results));
    let considered = candidates
        .candidates
        .iter()
        .map(|candidate| candidate.reference.clone())
        .collect();
    let ordered = scored
        .iter()
        .map(|(candidate, _, _, _, _)| candidate.reference.clone())
        .collect();
    let verified_evidence = scored
        .into_iter()
        .map(|(candidate, _, _, _, refs)| (candidate.reference.clone(), refs))
        .collect();
    Ok(RankedSelection {
        schema_version: p::SchemaVersion(1),
        strategy: policy.version.clone(),
        considered,
        ordered,
        verified_evidence,
    })
}

fn is_verified(evidence: &p::CapabilityEvidence) -> bool {
    matches!(
        evidence.reliability.0.to_ascii_lowercase().as_str(),
        "verified" | "independent" | "owner-verified"
    )
}

fn is_success(outcome: &str) -> bool {
    matches!(
        outcome.to_ascii_lowercase().as_str(),
        "pass" | "passed" | "success" | "succeeded"
    )
}

fn is_failure(outcome: &str) -> bool {
    matches!(
        outcome.to_ascii_lowercase().as_str(),
        "fail" | "failed" | "failure" | "revoked"
    )
}
