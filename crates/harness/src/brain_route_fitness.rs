//! Only evidence explicitly bound to an activated strategy may affect route fitness.
use crate::{production::ProductionRuntime, ProductionStrategySpec};
use forme_coordination as c;
use forme_protocol as p;
use forme_store::{EventStore, EvolutionProjection};
use std::collections::{BTreeMap, BTreeSet};

impl ProductionRuntime {
    pub(crate) fn active_project_coordination(
        &self,
        project: &p::WorkspaceProject,
        task: &p::TaskSignature,
    ) -> p::Result<Option<p::CoordinationStrategySpec>> {
        let services = self.services();
        let store = &services.store;
        if !store.has_active_strategies(&project.scope)? {
            return Ok(None);
        }
        let snapshot = store.snapshot(project.scope.clone())?;
        let h = self.harness();
        let runtime = h
            .m3_domains
            .as_ref()
            .ok_or_else(|| p::Error("production M3 runtime is absent".into()))?;
        let model = services
            .model_registry
            .get(&project.model)
            .ok_or_else(|| p::Error("project model is unavailable".into()))?;
        let binding = runtime.bind(&snapshot, &project.scope, model.profile())?;
        Ok(binding.coordination.filter(|s| applicable(s, task)))
    }

    pub(crate) fn project_orchestration_library(
        &self,
        project: &p::WorkspaceProject,
        task: &p::TaskSignature,
        ledger: &p::OrchestrationLedger,
    ) -> p::Result<(
        c::SeedOrchestrationLibrary,
        Option<p::CoordinationStrategySpec>,
    )> {
        let services = self.services();
        let library = services.orchestration.as_ref();
        let Some(spec) = self.active_project_coordination(project, task)? else {
            return Ok((library.clone(), None));
        };
        let events = self
            .services()
            .store
            .read_run(self.services().aggregates.memory_cognition.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let candidate = events.iter().find_map(|e| match &e.payload {
            p::EventPayload::CandidateCreated(p) => p
                .strategy_candidate
                .as_ref()
                .filter(|c| {
                    c.proposed_version == spec.version
                        && c.domain == p::StrategyDomain::Coordination
                        && c.scope == spec.scope
                        && c.spec_ref == spec.content_ref
                        && c.spec_digest == spec.content_digest
                })
                .map(|c| (e.stream_seq, c)),
            _ => None,
        });
        let mut fitness = BTreeMap::new();
        if let Some((sequence, candidate)) = candidate {
            let approved = candidate.evidence.iter().cloned().collect::<BTreeSet<_>>();
            let episodes = events
                .iter()
                .filter_map(|e| match &e.payload {
                    p::EventPayload::MemoryNodeAppended(n)
                        if e.stream_seq < sequence
                            && n.kind.0 == "episodic"
                            && e.provenance.trust_tier == p::TrustTier::VerifiedProcess
                            && p::charter_scope_contains(&spec.scope, &n.scope)
                            && approved.contains(&p::EvidenceRef(n.node_id.0.clone())) =>
                    {
                        Some(n.node_id.0.clone())
                    }
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            let mut totals = BTreeMap::<p::OrchestrationPatternRef, (u32, u32)>::new();
            for route in ledger.routes.values().filter(|r| {
                r.closed
                    && p::charter_scope_contains(&spec.scope, &r.declaration.main.scope)
                    && same_shape(task, &r.declaration.signature)
                    && episodes.contains(&format!("route-episode:{}", r.declaration.route.0))
            }) {
                if let Some(pattern) = &route.declaration.pattern {
                    let (pass, fail) = totals.entry(pattern.clone()).or_default();
                    if route.status == p::WorkRouteStatus::Completed
                        && route.conflicts.is_empty()
                        && route
                            .nodes
                            .values()
                            .all(|n| n.verification == p::VerificationOutcome::Pass)
                    {
                        *pass += 1;
                    } else {
                        *fail += 1;
                    }
                }
            }
            for (pattern, (pass, fail)) in totals {
                fitness.insert(
                    pattern,
                    p::Confidence((pass + 1) as f32 / (pass + fail + 2) as f32),
                );
            }
        }
        let library = library
            .with_active_patterns(
                &spec
                    .patterns
                    .iter()
                    .map(|p| p.pattern.clone())
                    .collect::<Vec<_>>(),
            )?
            .with_active_fitness(&fitness)?;
        Ok((library, Some(spec)))
    }

    pub(crate) fn route_strategy_body(
        &self,
        record: &p::WorkRouteRecord,
        version: p::StrategyVersionRef,
    ) -> p::Result<ProductionStrategySpec> {
        let task =
            super::brain_orchestration::signature_from_protocol(&record.declaration.signature);
        let index = self.services().orchestration.index();
        let roles = index
            .iter()
            .flat_map(|e| e.roles.iter().cloned())
            .chain(std::iter::once("owner-brain".to_owned()))
            .collect::<BTreeSet<_>>();
        let count = roles.len() as u16;
        let weights = roles
            .into_iter()
            .enumerate()
            .map(|(i, role)| p::CoordinationRoleWeight {
                schema_version: p::SchemaVersion(1),
                role: p::RoleRef(role),
                weight_basis_points: 10_000 / count + u16::from((i as u16) < 10_000 % count),
            })
            .collect();
        let spec = p::CoordinationStrategySpec {
            schema_version: p::SchemaVersion(1),
            version,
            scope: record.declaration.main.scope.clone(),
            content_ref: p::ContentRef("unsealed".into()),
            content_digest: p::SchemaDigest("unsealed".into()),
            compatibility: p::StrategyRuntimeCompatibility {
                schema_version: p::SchemaVersion(1),
                minimum_runtime_schema: p::SchemaVersion(1),
                event_schema: p::SchemaVersion(1),
                model_profile: None,
                tool_schema: None,
                backend_schema: None,
            },
            applicability: p::CoordinationApplicabilitySpec {
                schema_version: p::SchemaVersion(1),
                scale: match task.scale {
                    c::TaskScale::Single => p::CoordinationTaskScale::Single,
                    c::TaskScale::MultiStage => p::CoordinationTaskScale::MultiStage,
                    c::TaskScale::LongRunning => p::CoordinationTaskScale::LongRunning,
                },
                decomposability: match task.decomposability {
                    c::Decomposability::Whole => p::CoordinationDecomposability::Whole,
                    c::Decomposability::Parallel => p::CoordinationDecomposability::Parallel,
                    c::Decomposability::Sequential => p::CoordinationDecomposability::Sequential,
                    c::Decomposability::Iterative => p::CoordinationDecomposability::Iterative,
                },
                verifiability: match task.verifiability {
                    c::Verifiability::SelfCheck => p::CoordinationVerifiability::SelfCheck,
                    c::Verifiability::Environment => p::CoordinationVerifiability::Environment,
                    c::Verifiability::IndependentReview => {
                        p::CoordinationVerifiability::IndependentReview
                    }
                },
            },
            patterns: index
                .into_iter()
                .map(|e| p::CoordinationPatternSpec {
                    schema_version: p::SchemaVersion(1),
                    pattern: e.pattern.reference,
                    topology: match e.pattern.spec.topology {
                        c::RouteTopology::Direct
                        | c::RouteTopology::Retrieve
                        | c::RouteTopology::ToolLoop
                        | c::RouteTopology::Clarify => p::CoordinationTopology::Direct,
                        c::RouteTopology::ReviewGate | c::RouteTopology::PlannerIntegrator => {
                            p::CoordinationTopology::ReviewGate
                        }
                        _ => p::CoordinationTopology::IndependentSlices,
                    },
                })
                .collect(),
            mode: p::CoordinationMode::Multi,
            role_weights: weights,
            max_subagents: 8,
            checkpoint_topology: p::CheckpointTopology::PerRoute,
        };
        self.register_strategy_body(ProductionStrategySpec::Coordination(spec), false)
    }
}

fn applicable(s: &p::CoordinationStrategySpec, t: &p::TaskSignature) -> bool {
    use p::*;
    matches!(
        (s.applicability.scale, t.extent),
        (CoordinationTaskScale::Single, TaskExtent::Single)
            | (CoordinationTaskScale::MultiStage, TaskExtent::MultiStage)
            | (CoordinationTaskScale::LongRunning, TaskExtent::LongRunning)
    ) && matches!(
        (s.applicability.decomposability, t.partition),
        (CoordinationDecomposability::Whole, TaskPartition::Whole)
            | (
                CoordinationDecomposability::Parallel,
                TaskPartition::Parallel
            )
            | (
                CoordinationDecomposability::Sequential,
                TaskPartition::Sequential
            )
            | (
                CoordinationDecomposability::Iterative,
                TaskPartition::Iterative
            )
    ) && matches!(
        (s.applicability.verifiability, t.verification),
        (
            CoordinationVerifiability::SelfCheck,
            TaskVerification::SelfCheck
        ) | (
            CoordinationVerifiability::Environment,
            TaskVerification::Environment
        ) | (
            CoordinationVerifiability::IndependentReview,
            TaskVerification::IndependentReview
        )
    )
}
fn same_shape(a: &p::TaskSignature, b: &p::TaskSignature) -> bool {
    a.partition == b.partition
        && a.dependency == b.dependency
        && a.verification == b.verification
        && a.clarity == b.clarity
        && a.extent == b.extent
        && a.reversibility == b.reversibility
}
