//! Explicit seeds and immutable graph snapshots keep background activation bounded.
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct ActivationConfig {
    pub max_hops: u32,
    pub top_k_frontier: usize,
    pub surface_threshold: f32,
    pub per_tick_nodes: usize,
    pub per_hop_decay: f32,
    pub recency_half_life_ms: u64,
}
impl Default for ActivationConfig {
    fn default() -> Self {
        Self {
            max_hops: 3,
            top_k_frontier: 16,
            surface_threshold: 0.25,
            per_tick_nodes: 128,
            per_hop_decay: 0.65,
            recency_half_life_ms: 30 * 86_400_000,
        }
    }
}
impl ActivationConfig {
    pub fn validate(&self) -> p::Result<()> {
        if self.max_hops > 8
            || self.top_k_frontier == 0
            || self.top_k_frontier > 64
            || self.per_tick_nodes == 0
            || self.per_tick_nodes > 4096
            || !self.surface_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.surface_threshold)
            || !self.per_hop_decay.is_finite()
            || !(0.0..1.0).contains(&self.per_hop_decay)
            || self.recency_half_life_ms == 0
        {
            return Err(p::Error("invalid activation hard bounds".into()));
        }
        Ok(())
    }
    pub fn from_budget(budget: ActivationBudget) -> p::Result<Self> {
        if budget.schema_version != p::SchemaVersion(1) {
            return Err(p::Error("invalid activation budget schema".into()));
        }
        let config = Self {
            max_hops: u32::from(budget.max_hops),
            top_k_frontier: usize::from(budget.top_k_frontier),
            per_tick_nodes: budget.per_tick as usize,
            ..Self::default()
        };
        config.validate()?;
        Ok(config)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryGraphSnapshot {
    pub schema_version: p::SchemaVersion,
    pub version: u64,
    pub digest: p::SchemaDigest,
    pub nodes: BTreeMap<p::NodeId, MemoryNode>,
    pub edges: BTreeMap<p::EdgeId, MemoryEdge>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ActivationRun {
    pub schema_version: p::SchemaVersion,
    pub snapshot: Option<p::SchemaDigest>,
    pub shape: ActivationShape,
    pub considered: usize,
    pub hops: u32,
    pub truncated: bool,
    pub surfaced: Vec<Activated>,
}
pub trait RelevanceBaseline: Send + Sync {
    fn select(
        &self,
        cue: &RecallCue,
        candidates: &[MemoryNode],
        k: usize,
    ) -> p::Result<Vec<p::NodeId>>;
}

impl MemoryGraphSnapshot {
    pub fn activate(
        &self,
        seeds: &[p::NodeId],
        shape: ActivationShape,
        scope: &MemoryScope,
        config: &ActivationConfig,
        now: p::Timestamp,
    ) -> p::Result<ActivationRun> {
        config.validate()?;
        let mut result = ActivationRun {
            schema_version: p::SchemaVersion(1),
            snapshot: None,
            shape,
            considered: 0,
            hops: 0,
            truncated: false,
            surfaced: vec![],
        };
        if seeds.is_empty() {
            return Ok(result);
        }
        result.snapshot = Some(self.digest.clone());
        let usable = |node: &MemoryNode| {
            memory_scope_contains(scope, &node.scope)
                && node.provenance.trust_tier != p::TrustTier::Untrusted
        };
        let factor = |node: &MemoryNode| {
            let age = now.saturating_sub(node.recency).max(0) as f64;
            let recency = 2f64.powf(-age / config.recency_half_life_ms as f64) as f32;
            let tier = match node.tier {
                p::StabilityTier::Fixed | p::StabilityTier::Constitutional => 1.0,
                p::StabilityTier::Stable => 0.95,
                p::StabilityTier::Working => 0.85,
                p::StabilityTier::Session => 0.75,
                p::StabilityTier::Ephemeral => 0.65,
            };
            let boost = match shape {
                ActivationShape::Association => 1.0,
                ActivationShape::Change => 1.1,
                ActivationShape::Pressure if node.kind == NodeKind::Goal => 1.3,
                ActivationShape::Gap if node.confidence.0 < 0.5 => 1.3,
                _ => 1.0,
            };
            (tier * (0.5 + 0.5 * recency) * boost + node.resting_activation.0 * 0.2).min(1.5)
        };
        let mut levels = BTreeMap::<p::NodeId, f32>::new();
        let mut frontier = BTreeMap::<p::NodeId, f32>::new();
        let mut visited = BTreeSet::new();
        for seed in seeds.iter().collect::<BTreeSet<_>>() {
            if let Some(node) = self.nodes.get(seed).filter(|n| usable(n)) {
                frontier.insert(seed.clone(), factor(node).min(1.0));
            }
        }
        let mut adjacency = BTreeMap::<p::NodeId, Vec<&MemoryEdge>>::new();
        for edge in self
            .edges
            .values()
            .filter(|e| e.provenance.trust_tier != p::TrustTier::Untrusted)
        {
            adjacency.entry(edge.from.clone()).or_default().push(edge);
        }
        for hop in 0..=config.max_hops {
            let mut ranked = frontier.into_iter().collect::<Vec<_>>();
            ranked.sort_by(|(a, x), (b, y)| y.total_cmp(x).then(a.cmp(b)));
            if ranked.len() > config.top_k_frontier {
                result.truncated = true;
                ranked.truncate(config.top_k_frontier);
            }
            let mut next = BTreeMap::<p::NodeId, f32>::new();
            for (id, level) in ranked {
                if !visited.insert(id.clone()) {
                    continue;
                }
                if result.considered >= config.per_tick_nodes {
                    result.truncated = true;
                    break;
                }
                result.considered += 1;
                result.hops = hop;
                levels
                    .entry(id.clone())
                    .and_modify(|v| *v = (*v + level).min(1.0))
                    .or_insert(level.min(1.0));
                if hop == config.max_hops {
                    continue;
                }
                for edge in adjacency
                    .get(&id)
                    .into_iter()
                    .flatten()
                    .take(config.per_tick_nodes)
                {
                    if visited.contains(&edge.to) {
                        continue;
                    }
                    let Some(node) = self.nodes.get(&edge.to).filter(|n| usable(n)) else {
                        continue;
                    };
                    let relation = if shape == ActivationShape::Tension
                        && edge.kind == EdgeKind::Contradicts
                    {
                        1.4
                    } else {
                        1.0
                    };
                    let contribution =
                        level * edge.weight.0 * config.per_hop_decay * factor(node) * relation;
                    if contribution > 0.0 {
                        next.entry(edge.to.clone())
                            .and_modify(|v| *v = (*v + contribution).min(1.0))
                            .or_insert(contribution.min(1.0));
                    }
                }
            }
            if result.considered >= config.per_tick_nodes {
                break;
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        result.surfaced = levels
            .into_iter()
            .filter(|(_, level)| *level >= config.surface_threshold)
            .map(|(node, level)| Activated {
                schema_version: p::SchemaVersion(1),
                node,
                level: p::Confidence(level),
            })
            .collect();
        result
            .surfaced
            .sort_by(|a, b| b.level.0.total_cmp(&a.level.0).then(a.node.cmp(&b.node)));
        result.surfaced.truncate(config.top_k_frontier);
        Ok(result)
    }
}

impl<S: EventStore> EventSourcedMemory<S> {
    pub fn graph_snapshot_at(&self, through: Option<u64>) -> p::Result<MemoryGraphSnapshot> {
        let mut state = MemoryState::default();
        let mut ledger = p::MemoryGraphLedger::default();
        let mut cursor = Vec::new();
        for event in self.store.read_run(self.aggregate_run.clone()) {
            let event = event?;
            if through.is_some_and(|head| event.stream_seq > head) {
                break;
            }
            ledger.fold(&event)?;
            fold_event(&mut state, &event);
            cursor.push((event.stream_seq, event.event_id));
        }
        Ok(MemoryGraphSnapshot {
            schema_version: p::SchemaVersion(1),
            version: ledger.version,
            digest: p::canonical_digest(&(self.aggregate_run.clone(), cursor))?,
            nodes: state.nodes,
            edges: state.edges,
        })
    }
    pub fn activate_at(
        &self,
        seeds: &[p::NodeId],
        shape: ActivationShape,
        scope: &MemoryScope,
        config: &ActivationConfig,
        now: p::Timestamp,
        through: Option<u64>,
    ) -> p::Result<ActivationRun> {
        config.validate()?;
        if seeds.is_empty() {
            return Ok(ActivationRun {
                schema_version: p::SchemaVersion(1),
                snapshot: None,
                shape,
                considered: 0,
                hops: 0,
                truncated: false,
                surfaced: vec![],
            });
        }
        self.graph_snapshot_at(through)?
            .activate(seeds, shape, scope, config, now)
    }
    pub fn apply_maintenance(
        &self,
        set: p::MemoryMaintenanceDeltaSet,
        provenance: p::Provenance,
    ) -> p::Result<p::EventId> {
        set.validate_actor(&provenance)?;
        let reference = set.into_ref()?;
        let event = p::Event::new(
            p::EventId(format!(
                "graph-maintenance:{}:{}",
                self.aggregate_run.0,
                p::canonical_digest(&set)?.0
            )),
            self.aggregate_run.clone(),
            None,
            p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
                deltas_ref: reference,
            }),
            p::SchemaVersion(1),
            set.at,
            provenance,
        );
        let mut state = self.lock_state()?;
        let id = self.store.append(event)?;
        let snapshot = self.graph_snapshot_at(None)?;
        state.nodes = snapshot.nodes;
        state.edges = snapshot.edges;
        Ok(id)
    }
}

pub(crate) fn fold_graph_delta(state: &mut MemoryState, set: &p::MemoryMaintenanceDeltaSet) {
    let mut ledger = p::MemoryGraphLedger {
        version: set.expected_version,
        nodes: state
            .nodes
            .values()
            .map(|node| {
                (
                    node.id.clone(),
                    p::MemoryNodeAppendedPayload {
                        node_id: node.id.clone(),
                        kind: p::MemoryNodeType(node.kind.as_str().into()),
                        content_ref: node.content_ref.clone(),
                        tier: node.tier,
                        confidence: node.confidence,
                        scope: node.scope.event_scope(),
                        resting_activation: node.resting_activation,
                        recency: p::Recency(node.recency),
                    },
                )
            })
            .collect(),
        edges: state
            .edges
            .values()
            .map(|edge| {
                (
                    edge.id.clone(),
                    p::MemoryEdgeAppendedPayload {
                        edge_id: edge.id.clone(),
                        from: edge.from.clone(),
                        to: edge.to.clone(),
                        kind: p::MemoryEdgeType(edge.kind.as_str().into()),
                        weight: edge.weight,
                    },
                )
            })
            .collect(),
    };
    if ledger.apply(set).is_err() {
        return;
    }
    state.nodes.retain(|id, _| ledger.nodes.contains_key(id));
    for (id, payload) in ledger.nodes {
        if let Some(node) = state.nodes.get_mut(&id) {
            node.content_ref = payload.content_ref;
            node.confidence = payload.confidence;
            node.resting_activation = payload.resting_activation;
        }
    }
    state.edges.retain(|id, _| ledger.edges.contains_key(id));
    for (id, payload) in ledger.edges {
        if let Some(edge) = state.edges.get_mut(&id) {
            edge.weight = payload.weight;
        }
    }
}
