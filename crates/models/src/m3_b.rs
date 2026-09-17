use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use forme_protocol as p;

use crate::{ModelProfile, ModelStrength};

pub trait ModelAdaptationRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<p::ModelAdaptationSpec>;
}

pub struct InMemoryModelAdaptationRegistry {
    seed: p::StrategyVersionRef,
    specs: Mutex<BTreeMap<p::StrategyVersionRef, p::ModelAdaptationSpec>>,
}

impl InMemoryModelAdaptationRegistry {
    pub fn with_seed(seed: p::ModelAdaptationSpec) -> p::Result<Self> {
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

    pub fn register(&self, spec: p::ModelAdaptationSpec) -> p::Result<()> {
        spec.validate()?;
        let mut specs = self.lock()?;
        if let Some(existing) = specs.get(&spec.version) {
            return if existing == &spec {
                Ok(())
            } else {
                Err(p::Error(
                    "model adaptation version is already bound to different content".into(),
                ))
            };
        }
        specs.insert(spec.version.clone(), spec);
        Ok(())
    }

    fn lock(
        &self,
    ) -> p::Result<MutexGuard<'_, BTreeMap<p::StrategyVersionRef, p::ModelAdaptationSpec>>> {
        self.specs
            .lock()
            .map_err(|_| p::Error("model adaptation registry is unavailable".into()))
    }
}

impl ModelAdaptationRegistry for InMemoryModelAdaptationRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<p::ModelAdaptationSpec> {
        self.lock()?.get(version).cloned().ok_or_else(|| {
            p::Error(format!(
                "unsupported model adaptation version {}",
                version.0
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOutcomeEvidence {
    pub schema_version: p::SchemaVersion,
    pub profile: p::ModelProfileRef,
    pub measured_strength: p::ModelStrengthBand,
    pub verified_outcomes: u32,
    pub failures: u32,
    pub evidence_refs: Vec<p::EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelScaffoldPlan {
    pub schema_version: p::SchemaVersion,
    pub strategy: p::StrategyVersionRef,
    pub effective_strength: p::ModelStrengthBand,
    pub scaffold: p::ModelScaffoldProfile,
    pub verification_required: bool,
    pub high_impact_trace_required: bool,
    pub high_impact_approval_required: bool,
    pub evidence_refs: Vec<p::EvidenceRef>,
}

pub fn adapt_model_scaffold(
    spec: &p::ModelAdaptationSpec,
    profile: &ModelProfile,
    evidence: &ModelOutcomeEvidence,
    high_impact: bool,
) -> p::Result<ModelScaffoldPlan> {
    spec.validate()?;
    profile.validate()?;
    if evidence.schema_version.0 == 0
        || evidence.profile != profile.profile_ref()
        || evidence.verified_outcomes == 0
        || evidence.evidence_refs.is_empty()
        || evidence
            .evidence_refs
            .iter()
            .any(|reference| reference.0.trim().is_empty())
    {
        return Err(p::Error(
            "model adaptation requires measured result evidence for this profile".into(),
        ));
    }
    if spec
        .compatibility
        .model_profile
        .as_ref()
        .is_some_and(|required| required != &profile.profile_ref())
    {
        return Err(p::Error(
            "active model adaptation is incompatible with the selected model".into(),
        ));
    }
    let declared = strength_band(profile.capability.strength);
    let effective = declared.min(evidence.measured_strength);
    if profile.capability.context_window < spec.predicate.minimum_context_window
        || (spec.predicate.requires_tool_use && !profile.capability.tool_use)
        || effective < spec.predicate.minimum_strength
    {
        return Err(p::Error(
            "measured model capability does not satisfy the active adaptation predicate".into(),
        ));
    }
    Ok(ModelScaffoldPlan {
        schema_version: p::SchemaVersion(1),
        strategy: spec.version.clone(),
        effective_strength: effective,
        scaffold: spec.scaffold.clone(),
        verification_required: true,
        high_impact_trace_required: high_impact,
        high_impact_approval_required: high_impact,
        evidence_refs: evidence.evidence_refs.clone(),
    })
}

fn strength_band(strength: ModelStrength) -> p::ModelStrengthBand {
    match strength {
        ModelStrength::Basic => p::ModelStrengthBand::Basic,
        ModelStrength::Standard => p::ModelStrengthBand::Standard,
        ModelStrength::Strong => p::ModelStrengthBand::Strong,
    }
}
