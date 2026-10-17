use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelevanceMode {
    DirectModel,
    Diffusion,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelevanceEvidenceLevel {
    ControlledHttp,
    LiveProvider,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelevanceLabelSource {
    Owner,
    IndependentFixture,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelevanceMetrics {
    pub hits: u64,
    pub selected: u64,
    pub relevant: u64,
    pub errors: u64,
    pub precision_bps: u64,
    pub recall_bps: u64,
    pub latency_us: u64,
    pub usage: ModelUsage,
    pub cost_microunits: u64,
}
impl RelevanceMetrics {
    pub fn measured(
        selected: &[NodeId],
        truth: &[NodeId],
        latency_us: u64,
        usage: ModelUsage,
        cost_microunits: u64,
    ) -> Self {
        let hits = selected.iter().filter(|id| truth.contains(id)).count() as u64;
        let count = selected.len() as u64;
        let relevant = truth.len() as u64;
        Self {
            hits,
            selected: count,
            relevant,
            errors: count - hits,
            precision_bps: (hits * 10_000).checked_div(count).unwrap_or(0),
            recall_bps: (hits * 10_000).checked_div(relevant).unwrap_or(10_000),
            latency_us,
            usage,
            cost_microunits,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelevanceComparisonArtifact {
    pub schema_version: SchemaVersion,
    pub case_id: String,
    pub graph_snapshot: SchemaDigest,
    pub task_digest: SchemaDigest,
    pub strategy_snapshot: SchemaDigest,
    pub candidates: Vec<NodeId>,
    pub ground_truth: Vec<NodeId>,
    pub label_source: RelevanceLabelSource,
    pub k: usize,
    pub diffusion: Vec<NodeId>,
    pub direct_model: Vec<NodeId>,
    pub diffusion_metrics: RelevanceMetrics,
    pub direct_metrics: RelevanceMetrics,
    pub model_profile: ModelProfileRef,
    pub call_id: ModelCallId,
    pub evidence_level: RelevanceEvidenceLevel,
    pub hard_invariants: Vec<(String, bool)>,
    pub recommended: RelevanceMode,
    pub digest: SchemaDigest,
}
impl RelevanceComparisonArtifact {
    pub fn seal(mut self) -> Result<Self> {
        self.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&self)?;
        self.validate()?;
        Ok(self)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.case_id.is_empty()
            || self.case_id.len() > 128
            || !self
                .case_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_:".contains(&c))
            || self.k == 0
            || self.k > 64
            || self.candidates.is_empty()
            || self.candidates.len() > 256
            || self.ground_truth.is_empty()
            || self.model_profile.0.is_empty()
            || self.call_id.0.is_empty()
        {
            return Err(Error("invalid relevance comparison metadata".into()));
        }
        for digest in [
            &self.graph_snapshot,
            &self.task_digest,
            &self.strategy_snapshot,
        ] {
            sha256_digest_bytes(digest)?;
        }
        let unique = |ids: &[NodeId]| ids.iter().collect::<BTreeSet<_>>().len() == ids.len();
        if !unique(&self.candidates)
            || !unique(&self.ground_truth)
            || !unique(&self.diffusion)
            || !unique(&self.direct_model)
            || self.diffusion.len() > self.k
            || self.direct_model.len() > self.k
            || self
                .ground_truth
                .iter()
                .chain(&self.diffusion)
                .chain(&self.direct_model)
                .any(|id| !self.candidates.contains(id))
        {
            return Err(Error(
                "comparison selections or labels escape the common candidate set".into(),
            ));
        }
        for (selection, metrics) in [
            (&self.diffusion, &self.diffusion_metrics),
            (&self.direct_model, &self.direct_metrics),
        ] {
            let expected = RelevanceMetrics::measured(
                selection,
                &self.ground_truth,
                metrics.latency_us,
                metrics.usage,
                metrics.cost_microunits,
            );
            if expected != *metrics {
                return Err(Error(
                    "relevance metrics do not match independent labels".into(),
                ));
            }
        }
        let required = [
            "same-snapshot",
            "same-candidates",
            "same-task",
            "same-k",
            "scope-filtered",
            "governance-preserved",
        ];
        if self.hard_invariants.len() != required.len()
            || required.iter().any(|name| {
                !self
                    .hard_invariants
                    .iter()
                    .any(|(key, pass)| key == name && *pass)
            })
        {
            return Err(Error("relevance hard invariant failed or absent".into()));
        }
        let better = self.diffusion_metrics.recall_bps > self.direct_metrics.recall_bps
            && self.diffusion_metrics.precision_bps >= self.direct_metrics.precision_bps;
        if self.recommended
            != if better {
                RelevanceMode::Diffusion
            } else {
                RelevanceMode::DirectModel
            }
        {
            return Err(Error(
                "recommendation hides a non-improving diffusion result".into(),
            ));
        }
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if canonical_digest(&material)? != self.digest {
            return Err(Error("relevance comparison digest mismatch".into()));
        }
        Ok(())
    }
}
