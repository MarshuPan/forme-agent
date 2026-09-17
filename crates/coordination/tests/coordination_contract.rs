use std::collections::{BTreeMap, BTreeSet};

use forme_capabilities::{Capability, Toolset, ToolsetItem};
use forme_coordination::*;
use forme_protocol as p;

fn toolset(name: &str, scope: &str) -> Toolset {
    Toolset {
        schema_version: p::SchemaVersion(1),
        toolset_ref: p::ToolsetRef(format!("toolset:{name}")),
        items: vec![ToolsetItem {
            schema_version: p::SchemaVersion(1),
            id: p::CapabilityRef(format!("capability:{name}")),
            capability: Capability::Tool(p::ToolRef(format!("tool:{name}"))),
            sources: vec![p::CapabilitySourceRef("fixture:self-authored".into())],
        }],
        scope: p::Scope(scope.into()),
        sources: vec![p::CapabilitySourceRef("fixture:self-authored".into())],
    }
}

fn envelope(scope: &str, units: u64) -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope(scope.into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef("capability:builder".into())],
            permissions: vec![p::PermissionRef("permission:workspace".into())],
        },
        action_type: vec![p::ActionType::Analyze, p::ActionType::Prepare],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget(format!("units:{units}")),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 10,
            expires_at: 100,
            max_turns: 4,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

fn profile(name: &str, budget: u64) -> SubagentProfile {
    let scope = format!("workspace:child:{name}");
    SubagentProfile {
        schema_version: p::SchemaVersion(1),
        role: p::RoleRef(format!("role:{name}")),
        toolset: toolset(name, &scope),
        model: p::ModelProfileRef("model:m0".into()),
        permission: p::Scope(scope),
        budget: p::Budget(format!("units:{budget}")),
    }
}

fn context() -> CoordinationContext {
    let mut trusted = BTreeSet::new();
    trusted.insert(p::CapabilityRef("capability:builder".into()));
    CoordinationContext {
        schema_version: p::SchemaVersion(1),
        situation: SituationModel {
            schema_version: p::SchemaVersion(1),
            known: vec![Fact {
                schema_version: p::SchemaVersion(1),
                reference: p::EvidenceRef("event:known".into()),
                statement: "the workspace is available".into(),
            }],
            missing: Vec::new(),
        },
        inventory: ResourceInventory {
            schema_version: p::SchemaVersion(1),
            tools: vec![p::CapabilityRef("capability:builder".into())],
            skills: Vec::new(),
            mcp: Vec::new(),
            subagents: vec![profile("builder", 4), profile("reviewer", 3)],
            trusted,
        },
        done_contract: DoneContract::final_output(p::DoneContractRef("done:goal".into())),
        autonomy_envelope: envelope("workspace:parent", 10),
        decision_refs: p::DecisionRefs {
            map: Some(p::JudgmentFrameRef("map:frame".into())),
            user: Some(p::UserAttributeRef("user:quality".into())),
            agent_self: Some(p::AgentSelfModelRef("self:builder".into())),
            trust: Some(p::TrustProfileRef("trust:workspace".into())),
            failure: vec![p::FailureEvidenceRef("failure:prior".into())],
        },
        workspace_snapshot: AgentWorkspaceSnapshot {
            schema_version: p::SchemaVersion(1),
            reference: p::AgentWorkspaceSnapshotRef("agent-workspace:42".into()),
            event_refs: vec![p::EventId("event:42".into())],
        },
        resource_graph: None,
        resource_required: true,
    }
}

#[test]
fn s6_reasoner_outputs_constraints_and_binds_trace_to_the_decision_snapshot() {
    let reasoner = RuleBasedCoordinationReasoner;
    let frame = reasoner.frame(
        GoalInput::new(p::GoalRef("goal:m0".into()), "finish the M0 acceptance"),
        &context(),
    );
    let (plan, done, produced_envelope, trace) = reasoner.plan(&frame).unwrap();

    assert_eq!(
        plan.selected,
        vec![p::CapabilityRef("capability:builder".into())]
    );
    assert!(done.is_verifiable());
    assert_eq!(produced_envelope.scope, p::Scope("workspace:parent".into()));
    assert_eq!(trace.refs, frame.context().decision_refs);
    assert_eq!(
        trace.workspace_snapshot,
        p::AgentWorkspaceSnapshotRef("agent-workspace:42".into())
    );
    assert!(!trace.rationale.0.contains("step 1"));
}

#[test]
fn s6_blocked_goal_never_claims_to_have_a_route() {
    let reasoner = RuleBasedCoordinationReasoner;
    let mut blocked = context();
    blocked.done_contract.criteria.clear();
    let frame = reasoner.frame(
        GoalInput::new(p::GoalRef("goal:blocked".into()), "ambiguous work"),
        &blocked,
    );
    let error = reasoner.plan(&frame).unwrap_err();
    assert!(error.0.starts_with("goal_framing_failure"));

    let mut no_resource = context();
    no_resource.inventory.trusted.clear();
    let frame = reasoner.frame(
        GoalInput::new(p::GoalRef("goal:no-resource".into()), "resource work"),
        &no_resource,
    );
    assert!(reasoner
        .plan(&frame)
        .unwrap_err()
        .0
        .starts_with("resource_selection_failure"));
}

fn route(policy: DependencyFailurePolicy) -> ExecutionRoute {
    let done = DoneContract::final_output(p::DoneContractRef("done:route".into()));
    let make_node = |id: &str, budget: u64| RouteNode {
        schema_version: p::SchemaVersion(1),
        subtask: Subtask {
            schema_version: p::SchemaVersion(1),
            id: id.into(),
            instruction: format!("perform {id}"),
            intent_id: p::ActionId(format!("intent:{id}")),
        },
        role: profile(id, budget),
        resource_slice: toolset(id, &format!("workspace:child:{id}")),
        done: done.clone(),
        retry: RetryPolicy {
            schema_version: p::SchemaVersion(1),
            max_attempts: 2,
        },
    };
    ExecutionRoute {
        schema_version: p::SchemaVersion(1),
        reference: p::ExecutionRouteRef("route:runtime".into()),
        pattern_ref: None,
        nodes: vec![make_node("build", 6), make_node("review", 5)],
        edges: vec![RouteEdge {
            schema_version: p::SchemaVersion(1),
            from: "build".into(),
            to: "review".into(),
            on_failure: policy,
        }],
    }
}

#[test]
fn route_runtime_reserves_budget_releases_it_and_applies_edge_failure_semantics() {
    let mut runtime = RouteRuntime::new(route(DependencyFailurePolicy::Continue), 10).unwrap();
    assert_eq!(runtime.runnable()[0].subtask.id, "build");
    runtime.start("build").unwrap();
    assert_eq!(runtime.remaining_budget().unwrap(), 4);
    runtime.fail("build").unwrap();
    assert_eq!(runtime.remaining_budget().unwrap(), 10);
    assert_eq!(runtime.runnable()[0].subtask.id, "review");
    runtime.start("review").unwrap();
    runtime
        .complete("review", p::ResultRef("result:review".into()))
        .unwrap();
    assert_eq!(runtime.status(), RouteStatus::Completed);

    let mut aborting = RouteRuntime::new(route(DependencyFailurePolicy::AbortRoute), 10).unwrap();
    aborting.start("build").unwrap();
    aborting.fail("build").unwrap();
    assert_eq!(aborting.status(), RouteStatus::Failed);
    assert_eq!(aborting.state("review"), Some(RouteNodeState::Cancelled));

    let mut replanning = RouteRuntime::new(route(DependencyFailurePolicy::Replan), 10).unwrap();
    replanning.start("build").unwrap();
    replanning.fail("build").unwrap();
    assert_eq!(replanning.status(), RouteStatus::ReplanRequested);
}

#[test]
fn route_runtime_cascades_parent_cancel_and_never_blindly_retries_unknown_effects() {
    let mut runtime = RouteRuntime::new(route(DependencyFailurePolicy::Continue), 10).unwrap();
    runtime.start("build").unwrap();
    runtime.fail("build").unwrap();
    assert_eq!(
        runtime
            .retry("build", p::ActionId("intent:retry".into()), false)
            .unwrap(),
        RetryDecision::OutcomeUnknown
    );
    assert_eq!(runtime.state("build"), Some(RouteNodeState::OutcomeUnknown));

    let mut cancelled = RouteRuntime::new(route(DependencyFailurePolicy::Continue), 10).unwrap();
    cancelled.start("build").unwrap();
    cancelled.cancel_parent().unwrap();
    assert_eq!(cancelled.status(), RouteStatus::Cancelled);
    assert_eq!(cancelled.state("build"), Some(RouteNodeState::Cancelled));
    assert_eq!(cancelled.state("review"), Some(RouteNodeState::Cancelled));
    assert_eq!(cancelled.remaining_budget().unwrap(), 10);
}

#[test]
fn seed_library_matches_or_customizes_and_only_sediments_completed_routes() {
    let library = SeedOrchestrationLibrary::default();
    let signature = ApplicabilitySignature {
        schema_version: p::SchemaVersion(1),
        decomposability: Decomposability::Sequential,
        dependency: DependencyShape::Interdependent,
        verifiability: Verifiability::IndependentReview,
        clarity: GoalClarity::Clear,
        scale: TaskScale::MultiStage,
        reversibility: Reversibility::LowRiskReversible,
        case_anchors: Vec::new(),
        fitness: p::Confidence(0.5),
    };
    let pattern = library.match_pattern(signature).unwrap();
    let reasoner = RuleBasedCoordinationReasoner;
    let frame = reasoner.frame(
        GoalInput::new(p::GoalRef("goal:orchestration".into()), "build then review"),
        &context(),
    );
    let execution = library.route(Some(pattern), &frame);
    assert_eq!(execution.nodes.len(), 2);
    assert_eq!(execution.edges.len(), 1);
    let completed = RouteOutcome {
        schema_version: p::SchemaVersion(1),
        status: RouteStatus::Completed,
        node_states: BTreeMap::new(),
        result_refs: BTreeMap::new(),
    };
    assert!(library.sediment(&execution, &completed).is_some());
}
