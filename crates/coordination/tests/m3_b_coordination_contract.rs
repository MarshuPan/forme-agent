use forme_capabilities::Toolset;
use forme_coordination::{
    apply_coordination_strategy, compare_coordination_fitness, CoordinationOutcomeMetrics,
    DoneContract, ExecutionRoute, InMemoryCoordinationRegistry, RetryPolicy, RouteNode,
    SubagentProfile, Subtask,
};
use forme_protocol as p;

fn spec(
    version: &str,
    mode: p::CoordinationMode,
    max_subagents: u16,
) -> p::CoordinationStrategySpec {
    p::CoordinationStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef(version.into()),
        scope: p::Scope("workspace:m3-b".into()),
        content_ref: p::ContentRef(format!("spec:{version}")),
        content_digest: p::SchemaDigest(format!("digest:{version}")),
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
            scale: p::CoordinationTaskScale::MultiStage,
            decomposability: p::CoordinationDecomposability::Sequential,
            verifiability: p::CoordinationVerifiability::IndependentReview,
        },
        patterns: vec![p::CoordinationPatternSpec {
            schema_version: p::SchemaVersion(1),
            pattern: p::OrchestrationPatternRef("pattern:review".into()),
            topology: p::CoordinationTopology::ReviewGate,
        }],
        mode,
        role_weights: vec![
            p::CoordinationRoleWeight {
                schema_version: p::SchemaVersion(1),
                role: p::RoleRef("role:worker".into()),
                weight_basis_points: 4_000,
            },
            p::CoordinationRoleWeight {
                schema_version: p::SchemaVersion(1),
                role: p::RoleRef("role:reviewer".into()),
                weight_basis_points: 6_000,
            },
        ],
        max_subagents,
        checkpoint_topology: p::CheckpointTopology::ReviewGate,
    }
}

fn node(id: &str, role: &str, permission: &str, budget: &str) -> RouteNode {
    let toolset = Toolset {
        schema_version: p::SchemaVersion(1),
        toolset_ref: p::ToolsetRef(format!("toolset:{id}")),
        items: Vec::new(),
        scope: p::Scope(permission.into()),
        sources: Vec::new(),
    };
    RouteNode {
        schema_version: p::SchemaVersion(1),
        subtask: Subtask {
            schema_version: p::SchemaVersion(1),
            id: id.into(),
            instruction: format!("complete {id}"),
            intent_id: p::ActionId(format!("intent:{id}")),
        },
        role: SubagentProfile {
            schema_version: p::SchemaVersion(1),
            role: p::RoleRef(role.into()),
            toolset: toolset.clone(),
            model: p::ModelProfileRef("model:m3-b".into()),
            permission: p::Scope(permission.into()),
            budget: p::Budget(budget.into()),
        },
        resource_slice: toolset,
        done: DoneContract::final_output(p::DoneContractRef(format!("done:{id}"))),
        retry: RetryPolicy {
            schema_version: p::SchemaVersion(1),
            max_attempts: 1,
        },
    }
}

#[test]
fn s59_coordination_strategy_selects_existing_roles_without_expanding_child_authority() {
    let v1 = spec("coordination:m3-b:v1", p::CoordinationMode::Multi, 2);
    let v2 = spec("coordination:m3-b:v2", p::CoordinationMode::Single, 1);
    let registry = InMemoryCoordinationRegistry::with_seed(v1).unwrap();
    registry.register(v2.clone()).unwrap();
    let worker = node("worker", "role:worker", "workspace:m3-b:worker", "units:10");
    let reviewer = node(
        "reviewer",
        "role:reviewer",
        "workspace:m3-b:reviewer",
        "units:5",
    );
    let reviewer_profile = reviewer.role.clone();
    let route = ExecutionRoute {
        schema_version: p::SchemaVersion(1),
        reference: p::ExecutionRouteRef("route:m3-b".into()),
        pattern_ref: Some(p::OrchestrationPatternRef("pattern:review".into())),
        nodes: vec![worker, reviewer],
        edges: Vec::new(),
    };
    let applied = apply_coordination_strategy(&v2, &route).unwrap();
    assert_eq!(applied.route.nodes.len(), 1);
    assert_eq!(applied.route.nodes[0].role, reviewer_profile);
    assert_eq!(
        applied.route.nodes[0].role.permission.0,
        "workspace:m3-b:reviewer"
    );
    assert_eq!(applied.route.nodes[0].role.budget.0, "units:5");
}

#[test]
fn s59_lower_cost_never_outvotes_incomplete_quality_or_over_delegation() {
    let baseline = CoordinationOutcomeMetrics {
        schema_version: p::SchemaVersion(1),
        verified_complete: true,
        correctness_basis_points: 9_500,
        cost_microunits: 100,
        latency_ms: 100,
        delegated_children: 1,
        required_children: 1,
        handoff_failures: 0,
    };
    let low_quality = CoordinationOutcomeMetrics {
        verified_complete: false,
        correctness_basis_points: 9_000,
        cost_microunits: 1,
        latency_ms: 1,
        ..baseline.clone()
    };
    assert_eq!(
        compare_coordination_fitness(&baseline, &low_quality)
            .unwrap()
            .verdict,
        p::EvaluationVerdict::Fail
    );
    let over_delegated = CoordinationOutcomeMetrics {
        delegated_children: 4,
        cost_microunits: 1,
        latency_ms: 1,
        ..baseline.clone()
    };
    assert_eq!(
        compare_coordination_fitness(&baseline, &over_delegated)
            .unwrap()
            .verdict,
        p::EvaluationVerdict::Fail
    );
}
