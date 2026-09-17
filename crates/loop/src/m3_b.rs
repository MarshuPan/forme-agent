use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use forme_protocol as p;

use crate::Budget;

pub type LoopSpec = p::LoopStrategySpec;

pub trait LoopRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<LoopSpec>;
}

pub struct InMemoryLoopRegistry {
    seed: p::StrategyVersionRef,
    specs: Mutex<BTreeMap<p::StrategyVersionRef, LoopSpec>>,
}

impl InMemoryLoopRegistry {
    pub fn with_seed(seed: LoopSpec) -> p::Result<Self> {
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

    pub fn register(&self, spec: LoopSpec) -> p::Result<()> {
        spec.validate()?;
        let mut specs = self.lock()?;
        if let Some(existing) = specs.get(&spec.version) {
            return if existing == &spec {
                Ok(())
            } else {
                Err(p::Error(
                    "loop strategy version is already bound to different content".into(),
                ))
            };
        }
        specs.insert(spec.version.clone(), spec);
        Ok(())
    }

    fn lock(&self) -> p::Result<MutexGuard<'_, BTreeMap<p::StrategyVersionRef, LoopSpec>>> {
        self.specs
            .lock()
            .map_err(|_| p::Error("loop strategy registry is unavailable".into()))
    }
}

impl LoopRegistry for InMemoryLoopRegistry {
    fn resolve(&self, version: &p::StrategyVersionRef) -> p::Result<LoopSpec> {
        self.lock()?
            .get(version)
            .cloned()
            .ok_or_else(|| p::Error(format!("unsupported loop strategy version {}", version.0)))
    }
}

pub fn apply_loop_strategy(spec: &LoopSpec, hard: &Budget) -> p::Result<Budget> {
    spec.validate()?;
    if hard.schema_version.0 == 0 || hard.max_turns == 0 {
        return Err(p::Error("runtime loop budget is incomplete".into()));
    }
    reject_wider_u64("token", spec.budget.max_tokens, hard.tokens)?;
    reject_wider_u64(
        "wall-time",
        spec.budget.max_wall_time_ms,
        hard.wall_time.map(|value| value.0),
    )?;
    reject_wider_u64(
        "cost",
        spec.budget.max_cost_microunits,
        hard.cost_microunits,
    )?;
    reject_wider_u32("tool-call", spec.budget.max_tool_calls, hard.tool_calls)?;
    if spec.budget.max_turns > hard.max_turns {
        return Err(p::Error(
            "loop strategy exceeds the runtime turn limit".into(),
        ));
    }
    Ok(Budget {
        schema_version: hard.schema_version,
        tokens: Some(spec.budget.max_tokens),
        wall_time: Some(p::DurationMs(spec.budget.max_wall_time_ms)),
        cost_microunits: Some(spec.budget.max_cost_microunits),
        tool_calls: Some(spec.budget.max_tool_calls),
        max_turns: spec.budget.max_turns,
    })
}

fn reject_wider_u64(label: &str, strategy: u64, hard: Option<u64>) -> p::Result<()> {
    if hard.is_some_and(|limit| strategy > limit) {
        Err(p::Error(format!(
            "loop strategy exceeds the runtime {label} limit"
        )))
    } else {
        Ok(())
    }
}

fn reject_wider_u32(label: &str, strategy: u32, hard: Option<u32>) -> p::Result<()> {
    if hard.is_some_and(|limit| strategy > limit) {
        Err(p::Error(format!(
            "loop strategy exceeds the runtime {label} limit"
        )))
    } else {
        Ok(())
    }
}
