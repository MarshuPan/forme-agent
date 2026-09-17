use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use forme_protocol as p;

use crate::{ExecutionRoute, RouteNode};

pub type CoordinationPolicy = p::CoordinationStrategySpec;

pub trait CoordinationRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<CoordinationPolicy>;
}

pub struct InMemoryCoordinationRegistry {
    seed: p::StrategyVersionRef,
    specs: Mutex<BTreeMap<p::StrategyVersionRef, CoordinationPolicy>>,
}

impl InMemoryCoordinationRegistry {
    pub fn with_seed(seed: CoordinationPolicy) -> p::Result<Self> {
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

    pub fn register(&self, spec: CoordinationPolicy) -> p::Result<()> {
        spec.validate()?;
        let mut specs = self.lock()?;
        if let Some(existing) = specs.get(&spec.version) {
            return if existing == &spec {
                Ok(())
            } else {
                Err(p::Error(
                    "coordination strategy version is already bound to different content".into(),
                ))
            };
        }
        specs.insert(spec.version.clone(), spec);
        Ok(())
    }

    fn lock(
        &self,
    ) -> p::Result<MutexGuard<'_, BTreeMap<p::StrategyVersionRef, CoordinationPolicy>>> {
        self.specs
            .lock()
            .map_err(|_| p::Error("coordination strategy registry is unavailable".into()))
    }
}

impl CoordinationRegistry for InMemoryCoordinationRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<CoordinationPolicy> {
        self.lock()?.get(version).cloned().ok_or_else(|| {
            p::Error(format!(
                "unsupported coordination strategy version {}",
                version.0
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationOutcomeMetrics {
    pub schema_version: p::SchemaVersion,
    pub verified_complete: bool,
    pub correctness_basis_points: u16,
    pub cost_microunits: u64,
    pub latency_ms: u64,
    pub delegated_children: u16,
    pub required_children: u16,
    pub handoff_failures: u16,
}

impl CoordinationOutcomeMetrics {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.correctness_basis_points > 10_000
            || self.delegated_children < self.required_children
        {
            return Err(p::Error("coordination fitness input is invalid".into()));
        }
        Ok(())
    }

    pub fn over_delegation(&self) -> u16 {
        self.delegated_children - self.required_children
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationFitnessResult {
    pub schema_version: p::SchemaVersion,
    pub verdict: p::EvaluationVerdict,
    pub hard_failures: Vec<String>,
    pub candidate_improvements: Vec<String>,
}

pub fn compare_coordination_fitness(
    baseline: &CoordinationOutcomeMetrics,
    candidate: &CoordinationOutcomeMetrics,
) -> p::Result<CoordinationFitnessResult> {
    baseline.validate()?;
    candidate.validate()?;
    let mut hard_failures = Vec::new();
    if !candidate.verified_complete {
        hard_failures.push("candidate result is not independently complete".into());
    }
    if candidate.correctness_basis_points < baseline.correctness_basis_points {
        hard_failures.push("candidate correctness regressed".into());
    }
    if candidate.handoff_failures > baseline.handoff_failures {
        hard_failures.push("candidate handoff failures increased".into());
    }
    if candidate.over_delegation() > baseline.over_delegation() {
        hard_failures.push("candidate over-delegation increased".into());
    }
    let mut improvements = Vec::new();
    if candidate.correctness_basis_points > baseline.correctness_basis_points {
        improvements.push("correctness".into());
    }
    if candidate.cost_microunits < baseline.cost_microunits {
        improvements.push("cost".into());
    }
    if candidate.latency_ms < baseline.latency_ms {
        improvements.push("latency".into());
    }
    if candidate.over_delegation() < baseline.over_delegation() {
        improvements.push("over-delegation".into());
    }
    let verdict = if !hard_failures.is_empty() {
        p::EvaluationVerdict::Fail
    } else if !baseline.verified_complete || improvements.is_empty() {
        p::EvaluationVerdict::Unverifiable
    } else {
        p::EvaluationVerdict::Pass
    };
    Ok(CoordinationFitnessResult {
        schema_version: p::SchemaVersion(1),
        verdict,
        hard_failures,
        candidate_improvements: improvements,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedCoordinationRoute {
    pub schema_version: p::SchemaVersion,
    pub strategy: p::StrategyVersionRef,
    pub checkpoint_topology: p::CheckpointTopology,
    pub route: ExecutionRoute,
}

pub fn apply_coordination_strategy(
    spec: &CoordinationPolicy,
    route: &ExecutionRoute,
) -> p::Result<AppliedCoordinationRoute> {
    spec.validate()?;
    let pattern = route.pattern_ref.as_ref().ok_or_else(|| {
        p::Error("coordination strategy requires an identified work pattern".into())
    })?;
    if !spec
        .patterns
        .iter()
        .any(|candidate| &candidate.pattern == pattern)
    {
        return Err(p::Error(
            "execution route pattern is outside the active coordination strategy".into(),
        ));
    }
    let weights = spec
        .role_weights
        .iter()
        .map(|weight| (weight.role.clone(), weight.weight_basis_points))
        .collect::<BTreeMap<_, _>>();
    if route
        .nodes
        .iter()
        .any(|node| !weights.contains_key(&node.role.role))
    {
        return Err(p::Error(
            "execution route contains a role absent from the active strategy".into(),
        ));
    }

    let take = match spec.mode {
        p::CoordinationMode::Single => 1,
        p::CoordinationMode::Multi => usize::from(spec.max_subagents),
    };
    let mut selected = route.nodes.iter().collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        weights[&right.role.role]
            .cmp(&weights[&left.role.role])
            .then_with(|| left.subtask.id.cmp(&right.subtask.id))
    });
    selected.truncate(take.min(selected.len()));
    let selected_ids = selected
        .iter()
        .map(|node| node.subtask.id.clone())
        .collect::<BTreeSet<_>>();
    let nodes = route
        .nodes
        .iter()
        .filter(|node| selected_ids.contains(&node.subtask.id))
        .cloned()
        .collect::<Vec<RouteNode>>();
    let edges = route
        .edges
        .iter()
        .filter(|edge| selected_ids.contains(&edge.from) && selected_ids.contains(&edge.to))
        .cloned()
        .collect();
    Ok(AppliedCoordinationRoute {
        schema_version: p::SchemaVersion(1),
        strategy: spec.version.clone(),
        checkpoint_topology: spec.checkpoint_topology,
        route: ExecutionRoute {
            schema_version: route.schema_version,
            reference: route.reference.clone(),
            pattern_ref: route.pattern_ref.clone(),
            nodes,
            edges,
        },
    })
}
