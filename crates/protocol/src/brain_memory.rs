//! Inline, content-addressed graph maintenance. No new event kinds.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const PREFIX: &str = "graph-deltas:v1:";
pub const MAX_MEMORY_DELTAS: usize = 64;
const MAX_DELTA_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum MemoryMaintenanceDelta {
    EdgeWeight {
        edge: EdgeId,
        expected: Weight,
        next: Weight,
    },
    RestingActivation {
        node: NodeId,
        expected: RestingActivation,
        next: RestingActivation,
    },
    ConfidenceDecay {
        node: NodeId,
        expected: Confidence,
        next: Confidence,
    },
    Consolidate {
        node: NodeId,
        expected: ContentRef,
        next: ContentRef,
        evidence: Vec<EvidenceRef>,
    },
    PruneEdge {
        edge: EdgeId,
        expected: Weight,
    },
    PruneNode {
        node: NodeId,
        expected: ContentRef,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMaintenanceDeltaSet {
    pub schema_version: SchemaVersion,
    pub command: IdempotencyKey,
    /// Full aggregate stream head, not a wall clock or event id.
    pub expected_version: u64,
    pub scope: Scope,
    pub at: Timestamp,
    pub deltas: Vec<MemoryMaintenanceDelta>,
}

impl MemoryMaintenanceDeltaSet {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.command.0.trim().is_empty()
            || self.command.0.len() > 256
            || self.scope.0.trim().is_empty()
            || self.scope.0.len() > 512
            || self.at < 0
            || self.deltas.is_empty()
            || self.deltas.len() > MAX_MEMORY_DELTAS
        {
            return Err(Error("invalid bounded graph maintenance".into()));
        }
        let mut targets = BTreeSet::new();
        for delta in &self.deltas {
            let target = match delta {
                MemoryMaintenanceDelta::EdgeWeight {
                    edge,
                    expected,
                    next,
                } => {
                    unit(expected.0)?;
                    unit(next.0)?;
                    format!("weight:{}", edge.0)
                }
                MemoryMaintenanceDelta::RestingActivation {
                    node,
                    expected,
                    next,
                } => {
                    unit(expected.0)?;
                    unit(next.0)?;
                    format!("rest:{}", node.0)
                }
                MemoryMaintenanceDelta::ConfidenceDecay {
                    node,
                    expected,
                    next,
                } => {
                    unit(expected.0)?;
                    unit(next.0)?;
                    if next.0 > expected.0 {
                        return Err(Error("maintenance cannot raise confidence".into()));
                    }
                    format!("confidence:{}", node.0)
                }
                MemoryMaintenanceDelta::Consolidate {
                    node,
                    expected,
                    next,
                    evidence,
                } => {
                    if expected.0.is_empty()
                        || next.0.is_empty()
                        || next.0.len() > 2048
                        || evidence.is_empty()
                        || evidence.len() > 32
                    {
                        return Err(Error("consolidation requires bounded evidence".into()));
                    }
                    format!("content:{}", node.0)
                }
                MemoryMaintenanceDelta::PruneEdge { edge, expected } => {
                    unit(expected.0)?;
                    format!("prune-edge:{}", edge.0)
                }
                MemoryMaintenanceDelta::PruneNode { node, expected } => {
                    if expected.0.is_empty() {
                        return Err(Error("prune requires the previous content".into()));
                    }
                    format!("prune-node:{}", node.0)
                }
            };
            if !targets.insert(target) {
                return Err(Error("duplicate graph maintenance target".into()));
            }
        }
        Ok(())
    }
    pub fn into_ref(&self) -> Result<MemoryDeltasRef> {
        self.validate()?;
        let json = serde_json::to_string(self)
            .map_err(|_| Error("cannot encode graph maintenance".into()))?;
        if json.len() > MAX_DELTA_BYTES {
            return Err(Error("graph maintenance exceeds byte bound".into()));
        }
        Ok(MemoryDeltasRef(format!(
            "{PREFIX}{}:{json}",
            canonical_digest(self)?.0
        )))
    }
    pub fn from_ref(reference: &MemoryDeltasRef) -> Result<Option<Self>> {
        let Some(tail) = reference.0.strip_prefix(PREFIX) else {
            if reference.0.starts_with("graph-deltas:") {
                return Err(Error("unsupported graph maintenance envelope".into()));
            }
            return Ok(None);
        };
        if tail.len() > MAX_DELTA_BYTES + 128 {
            return Err(Error("graph maintenance exceeds byte bound".into()));
        }
        // Digest itself is sha256:<hex>, so split at the JSON boundary.
        let index = tail
            .find(":{")
            .ok_or_else(|| Error("graph maintenance object is absent".into()))?;
        let digest = &tail[..index];
        let set: Self = serde_json::from_str(&tail[index + 1..])
            .map_err(|_| Error("invalid graph maintenance object".into()))?;
        set.validate()?;
        if canonical_digest(&set)?.0 != digest {
            return Err(Error("graph maintenance digest mismatch".into()));
        }
        Ok(Some(set))
    }
    pub fn validate_actor(&self, provenance: &Provenance) -> Result<()> {
        self.validate()?;
        let owner = provenance.actor == Actor::Owner
            && provenance.trust_tier == TrustTier::OwnerInput
            && matches!(provenance.source, Source::OwnerControl | Source::UserTurn);
        let process = provenance.actor == Actor::System
            && provenance.trust_tier == TrustTier::VerifiedProcess
            && provenance.source == Source::Internal;
        if !owner && !process {
            return Err(Error(
                "graph maintenance requires owner or verified process".into(),
            ));
        }
        if !owner
            && self.deltas.iter().any(|delta| match delta {
                MemoryMaintenanceDelta::EdgeWeight { expected, next, .. } => next.0 > expected.0,
                MemoryMaintenanceDelta::RestingActivation { expected, next, .. } => {
                    next.0 > expected.0
                }
                MemoryMaintenanceDelta::Consolidate { .. } => true,
                _ => false,
            })
        {
            return Err(Error(
                "only owner may strengthen or consolidate graph memory".into(),
            ));
        }
        Ok(())
    }
}
fn unit(value: f32) -> Result<()> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(Error("graph value must be finite in [0,1]".into()))
    }
}

/// Shared logical fold used by append validation and memory replay.
#[derive(Debug, Clone, Default)]
pub struct MemoryGraphLedger {
    pub version: u64,
    pub nodes: BTreeMap<NodeId, MemoryNodeAppendedPayload>,
    pub edges: BTreeMap<EdgeId, MemoryEdgeAppendedPayload>,
}
impl MemoryGraphLedger {
    pub fn fold(&mut self, event: &Event) -> Result<()> {
        match &event.payload {
            EventPayload::MemoryNodeAppended(payload) => {
                self.nodes.insert(payload.node_id.clone(), payload.clone());
            }
            EventPayload::MemoryEdgeAppended(payload) => {
                self.edges.insert(payload.edge_id.clone(), payload.clone());
            }
            EventPayload::MemoryMaintenanceApplied(payload) => {
                if let Some(set) = MemoryMaintenanceDeltaSet::from_ref(&payload.deltas_ref)? {
                    set.validate_actor(&event.provenance)?;
                    self.apply(&set)?;
                }
            }
            _ => {}
        }
        self.version = event.stream_seq;
        Ok(())
    }
    pub fn validate_set(&self, set: &MemoryMaintenanceDeltaSet) -> Result<()> {
        let mut next = self.clone();
        next.apply(set)
    }
    pub fn apply(&mut self, set: &MemoryMaintenanceDeltaSet) -> Result<()> {
        set.validate()?;
        if self.version != set.expected_version {
            return Err(Error("stale graph maintenance version".into()));
        }
        // Validate every precondition on a private clone; a late conflict cannot partially mutate.
        let mut nodes = self.nodes.clone();
        let mut edges = self.edges.clone();
        let allowed = |id: &NodeId| -> Result<()> {
            let node = self
                .nodes
                .get(id)
                .ok_or_else(|| Error("maintenance references an unknown node".into()))?;
            let label = if node.scope.0.starts_with("memory|") {
                node.scope
                    .0
                    .splitn(4, '|')
                    .nth(3)
                    .unwrap_or("")
                    .replace("%7C", "|")
                    .replace("%25", "%")
            } else {
                node.scope.0.clone()
            };
            if set.scope.0 != "*"
                && label != set.scope.0
                && !label
                    .strip_prefix(&set.scope.0)
                    .is_some_and(|s| s.starts_with(':') || s.starts_with('/'))
            {
                return Err(Error("graph maintenance crosses scope".into()));
            }
            Ok(())
        };
        for delta in &set.deltas {
            match delta {
                MemoryMaintenanceDelta::EdgeWeight {
                    edge,
                    expected,
                    next,
                } => {
                    let value = edges
                        .get_mut(edge)
                        .ok_or_else(|| Error("unknown maintenance edge".into()))?;
                    allowed(&value.from)?;
                    allowed(&value.to)?;
                    if value.weight != *expected {
                        return Err(Error("edge weight precondition conflict".into()));
                    }
                    value.weight = *next;
                }
                MemoryMaintenanceDelta::RestingActivation {
                    node,
                    expected,
                    next,
                } => {
                    allowed(node)?;
                    let value = nodes
                        .get_mut(node)
                        .ok_or_else(|| Error("unknown maintenance node".into()))?;
                    if value.resting_activation != *expected {
                        return Err(Error("resting activation precondition conflict".into()));
                    }
                    value.resting_activation = *next;
                }
                MemoryMaintenanceDelta::ConfidenceDecay {
                    node,
                    expected,
                    next,
                } => {
                    allowed(node)?;
                    let value = nodes
                        .get_mut(node)
                        .ok_or_else(|| Error("unknown maintenance node".into()))?;
                    if matches!(
                        value.tier,
                        StabilityTier::Fixed | StabilityTier::Constitutional
                    ) || value.confidence != *expected
                    {
                        return Err(Error("confidence decay precondition conflict".into()));
                    }
                    value.confidence = *next;
                }
                MemoryMaintenanceDelta::Consolidate {
                    node,
                    expected,
                    next,
                    ..
                } => {
                    allowed(node)?;
                    let value = nodes
                        .get_mut(node)
                        .ok_or_else(|| Error("unknown consolidation node".into()))?;
                    if matches!(
                        value.tier,
                        StabilityTier::Fixed | StabilityTier::Constitutional
                    ) || value.content_ref != *expected
                    {
                        return Err(Error("consolidation precondition conflict".into()));
                    }
                    value.content_ref = next.clone();
                }
                MemoryMaintenanceDelta::PruneEdge { edge, expected } => {
                    let value = edges
                        .get(edge)
                        .ok_or_else(|| Error("unknown prune edge".into()))?;
                    allowed(&value.from)?;
                    allowed(&value.to)?;
                    if value.weight != *expected {
                        return Err(Error("edge prune precondition conflict".into()));
                    }
                    edges.remove(edge);
                }
                MemoryMaintenanceDelta::PruneNode { node, expected } => {
                    allowed(node)?;
                    let value = nodes
                        .get(node)
                        .ok_or_else(|| Error("unknown prune node".into()))?;
                    if matches!(
                        value.tier,
                        StabilityTier::Fixed
                            | StabilityTier::Constitutional
                            | StabilityTier::Stable
                    ) || value.content_ref != *expected
                    {
                        return Err(Error("node prune precondition conflict".into()));
                    }
                    nodes.remove(node);
                    edges.retain(|_, edge| &edge.from != node && &edge.to != node);
                }
            }
        }
        self.nodes = nodes;
        self.edges = edges;
        Ok(())
    }
}
