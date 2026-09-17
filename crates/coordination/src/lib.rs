//! Coordination kernel: constraints, orchestration routes, and bounded child work.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Mutex, MutexGuard};

use forme_capabilities::Toolset;
use forme_protocol as p;

mod m2_c;
mod m3_b;
mod m4;

pub use m2_c::*;
pub use m3_b::*;
pub use m4::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    pub schema_version: p::SchemaVersion,
    pub reference: p::GoalRef,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalInput {
    pub schema_version: p::SchemaVersion,
    pub goal: Goal,
    pub constraints: Vec<p::Constraint>,
}

impl GoalInput {
    pub fn new(reference: p::GoalRef, summary: impl Into<String>) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            goal: Goal {
                schema_version: p::SchemaVersion(1),
                reference,
                summary: summary.into(),
            },
            constraints: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub schema_version: p::SchemaVersion,
    pub reference: p::EvidenceRef,
    pub statement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    pub schema_version: p::SchemaVersion,
    pub reference: p::EvidenceRef,
    pub statement: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SituationModel {
    pub schema_version: p::SchemaVersion,
    pub known: Vec<Fact>,
    pub missing: Vec<Gap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceSnapshot {
    pub schema_version: p::SchemaVersion,
    pub reference: p::AgentWorkspaceSnapshotRef,
    pub event_refs: Vec<p::EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceInventory {
    pub schema_version: p::SchemaVersion,
    pub tools: Vec<p::CapabilityRef>,
    pub skills: Vec<p::CapabilityRef>,
    pub mcp: Vec<p::CapabilityRef>,
    pub subagents: Vec<SubagentProfile>,
    pub trusted: BTreeSet<p::CapabilityRef>,
}

impl ResourceInventory {
    pub fn empty() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            tools: Vec::new(),
            skills: Vec::new(),
            mcp: Vec::new(),
            subagents: Vec::new(),
            trusted: BTreeSet::new(),
        }
    }

    pub fn trusted_capabilities(&self) -> Vec<p::CapabilityRef> {
        self.tools
            .iter()
            .chain(&self.skills)
            .chain(&self.mcp)
            .filter(|capability| self.trusted.contains(*capability))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSelector {
    FinalOutput,
    ActionResult,
    EventSegment,
    FileState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    Exists,
    Equals(String),
    Contains(String),
    EventKindSeen(p::EventKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tolerance {
    pub schema_version: p::SchemaVersion,
    pub allowed_deviation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneCriterion {
    pub schema_version: p::SchemaVersion,
    pub evidence: EvidenceSelector,
    pub predicate: Predicate,
    pub tolerance: Option<Tolerance>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopCondition {
    AllCriteriaSatisfied,
    BudgetExhausted,
    PolicyDenied,
    OwnerCancelled,
    Unverifiable(p::ReasonRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneContract {
    pub schema_version: p::SchemaVersion,
    pub reference: p::DoneContractRef,
    pub criteria: Vec<DoneCriterion>,
    pub stop_conditions: Vec<StopCondition>,
}

impl DoneContract {
    pub fn final_output(reference: p::DoneContractRef) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            reference,
            criteria: vec![DoneCriterion {
                schema_version: p::SchemaVersion(1),
                evidence: EvidenceSelector::FinalOutput,
                predicate: Predicate::Exists,
                tolerance: None,
            }],
            stop_conditions: vec![
                StopCondition::AllCriteriaSatisfied,
                StopCondition::BudgetExhausted,
                StopCondition::PolicyDenied,
                StopCondition::OwnerCancelled,
            ],
        }
    }

    pub fn is_verifiable(&self) -> bool {
        !self.criteria.is_empty()
            && self
                .stop_conditions
                .contains(&StopCondition::AllCriteriaSatisfied)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePlan {
    pub schema_version: p::SchemaVersion,
    pub reference: p::ResourcePlanRef,
    pub selected: Vec<p::CapabilityRef>,
    pub rationale: p::Rationale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionTrace {
    pub schema_version: p::SchemaVersion,
    pub reference: p::DecisionTraceRef,
    pub refs: p::DecisionRefs,
    pub rationale: p::Rationale,
    pub workspace_snapshot: p::AgentWorkspaceSnapshotRef,
    pub resource_graph_snapshot: Option<p::ResourceGraphSnapshotRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationContext {
    pub schema_version: p::SchemaVersion,
    pub situation: SituationModel,
    pub inventory: ResourceInventory,
    pub done_contract: DoneContract,
    pub autonomy_envelope: p::AutonomyEnvelope,
    pub decision_refs: p::DecisionRefs,
    pub workspace_snapshot: AgentWorkspaceSnapshot,
    pub resource_graph: Option<p::ResourceGraphSnapshot>,
    pub resource_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalFrame {
    pub schema_version: p::SchemaVersion,
    pub reference: p::GoalFrameRef,
    pub goal: Goal,
    pub constraints: Vec<p::Constraint>,
    context: CoordinationContext,
}

impl GoalFrame {
    pub fn context(&self) -> &CoordinationContext {
        &self.context
    }
}

pub trait CoordinationReasoner {
    fn frame(&self, goal: GoalInput, ctx: &CoordinationContext) -> GoalFrame;
    fn plan(
        &self,
        frame: &GoalFrame,
    ) -> p::Result<(
        ResourcePlan,
        DoneContract,
        p::AutonomyEnvelope,
        DecisionTrace,
    )>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RuleBasedCoordinationReasoner;

impl CoordinationReasoner for RuleBasedCoordinationReasoner {
    fn frame(&self, goal: GoalInput, ctx: &CoordinationContext) -> GoalFrame {
        GoalFrame {
            schema_version: goal.schema_version,
            reference: p::GoalFrameRef(format!("goal-frame:{}", goal.goal.reference.0)),
            goal: goal.goal,
            constraints: goal.constraints,
            context: ctx.clone(),
        }
    }

    fn plan(
        &self,
        frame: &GoalFrame,
    ) -> p::Result<(
        ResourcePlan,
        DoneContract,
        p::AutonomyEnvelope,
        DecisionTrace,
    )> {
        validate_frame(frame)?;
        let mut selected = frame.context.inventory.trusted_capabilities();
        if let Some(graph) = &frame.context.resource_graph {
            selected.sort_by(|left, right| {
                resource_rank(graph, right)
                    .cmp(&resource_rank(graph, left))
                    .then_with(|| left.cmp(right))
            });
        }
        if frame.context.resource_required && selected.is_empty() {
            return Err(p::Error(
                "resource_selection_failure: no trusted resource satisfies the goal".into(),
            ));
        }
        let selected_text = if selected.is_empty() {
            "the model can handle this goal without an external resource".to_owned()
        } else {
            format!(
                "selected trusted resources: {}",
                selected
                    .iter()
                    .map(|item| item.0.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let resource_plan = ResourcePlan {
            schema_version: p::SchemaVersion(1),
            reference: p::ResourcePlanRef(format!("resource-plan:{}", frame.goal.reference.0)),
            selected,
            rationale: p::Rationale(selected_text.clone()),
        };
        let trace = DecisionTrace {
            schema_version: p::SchemaVersion(1),
            reference: p::DecisionTraceRef(format!("decision-trace:{}", frame.goal.reference.0)),
            refs: frame.context.decision_refs.clone(),
            rationale: p::Rationale(format!(
                "goal was framed with {} constraint(s); {selected_text}",
                frame.constraints.len()
            )),
            workspace_snapshot: frame.context.workspace_snapshot.reference.clone(),
            resource_graph_snapshot: frame
                .context
                .resource_graph
                .as_ref()
                .map(|graph| graph.reference.clone()),
        };
        Ok((
            resource_plan,
            frame.context.done_contract.clone(),
            frame.context.autonomy_envelope.clone(),
            trace,
        ))
    }
}

fn resource_rank(graph: &p::ResourceGraphSnapshot, capability: &p::CapabilityRef) -> i64 {
    graph
        .nodes
        .iter()
        .filter(|node| node.resource.0 == capability.0)
        .map(|node| node.score.rank())
        .max()
        .unwrap_or(0)
}

fn validate_frame(frame: &GoalFrame) -> p::Result<()> {
    if frame.schema_version.0 == 0
        || frame.goal.schema_version.0 == 0
        || frame.goal.reference.0.trim().is_empty()
        || frame.goal.summary.trim().is_empty()
    {
        return Err(p::Error(
            "goal_framing_failure: goal identity and summary are required".into(),
        ));
    }
    if frame
        .context
        .situation
        .missing
        .iter()
        .any(|gap| gap.blocking)
    {
        return Err(p::Error(
            "goal_framing_failure: a blocking information gap remains".into(),
        ));
    }
    if !frame.context.done_contract.is_verifiable() {
        return Err(p::Error(
            "goal_framing_failure: DoneContract has no executable completion criterion".into(),
        ));
    }
    if frame
        .context
        .workspace_snapshot
        .reference
        .0
        .trim()
        .is_empty()
    {
        return Err(p::Error(
            "context_failure: DecisionTrace requires an AgentWorkspace snapshot".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decomposability {
    Whole,
    Parallel,
    Sequential,
    Iterative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyShape {
    Independent,
    Interdependent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verifiability {
    SelfCheck,
    IndependentReview,
    Environment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalClarity {
    Clear,
    Clarify,
    Explore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskScale {
    Single,
    MultiStage,
    LongRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reversibility {
    LowRiskReversible,
    HighRiskIrreversible,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApplicabilitySignature {
    pub schema_version: p::SchemaVersion,
    pub decomposability: Decomposability,
    pub dependency: DependencyShape,
    pub verifiability: Verifiability,
    pub clarity: GoalClarity,
    pub scale: TaskScale,
    pub reversibility: Reversibility,
    pub case_anchors: Vec<p::NodeId>,
    pub fitness: p::Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteTopology {
    Direct,
    IndependentSlices,
    ReviewGate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkPattern {
    pub schema_version: p::SchemaVersion,
    pub name: String,
    pub topology: RouteTopology,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrchestrationPattern {
    pub schema_version: p::SchemaVersion,
    pub reference: p::OrchestrationPatternRef,
    pub spec: WorkPattern,
    pub signature: ApplicabilitySignature,
    pub fitness: p::Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentProfile {
    pub schema_version: p::SchemaVersion,
    pub role: p::RoleRef,
    pub toolset: Toolset,
    pub model: p::ModelProfileRef,
    pub permission: p::Scope,
    pub budget: p::Budget,
}

impl SubagentProfile {
    pub fn budget_units(&self) -> p::Result<u64> {
        parse_budget_units(&self.budget)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtask {
    pub schema_version: p::SchemaVersion,
    pub id: String,
    pub instruction: String,
    pub intent_id: p::ActionId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub schema_version: p::SchemaVersion,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteNode {
    pub schema_version: p::SchemaVersion,
    pub subtask: Subtask,
    pub role: SubagentProfile,
    pub resource_slice: Toolset,
    pub done: DoneContract,
    pub retry: RetryPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyFailurePolicy {
    AbortRoute,
    Continue,
    Replan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEdge {
    pub schema_version: p::SchemaVersion,
    pub from: String,
    pub to: String,
    pub on_failure: DependencyFailurePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRoute {
    pub schema_version: p::SchemaVersion,
    pub reference: p::ExecutionRouteRef,
    pub pattern_ref: Option<p::OrchestrationPatternRef>,
    pub nodes: Vec<RouteNode>,
    pub edges: Vec<RouteEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteNodeState {
    Pending,
    Running,
    Done,
    Failed,
    Cancelled,
    OutcomeUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    ReplanRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteOutcome {
    pub schema_version: p::SchemaVersion,
    pub status: RouteStatus,
    pub node_states: BTreeMap<String, RouteNodeState>,
    pub result_refs: BTreeMap<String, p::ResultRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    Retry,
    Exhausted,
    OutcomeUnknown,
}

#[derive(Debug)]
struct BudgetState {
    remaining: u64,
    reservations: BTreeMap<String, u64>,
}

#[derive(Debug)]
pub struct BudgetLedger {
    state: Mutex<BudgetState>,
}

impl BudgetLedger {
    pub fn new(total: u64) -> Self {
        Self {
            state: Mutex::new(BudgetState {
                remaining: total,
                reservations: BTreeMap::new(),
            }),
        }
    }

    pub fn reserve(&self, node: &str, amount: u64) -> p::Result<()> {
        if amount == 0 {
            return Err(p::Error("subagent budget must be positive".into()));
        }
        let mut state = self.lock()?;
        if state.reservations.contains_key(node) {
            return Err(p::Error("subagent budget is already reserved".into()));
        }
        if amount > state.remaining {
            return Err(p::Error(
                "subagent budgets exceed the parent's remaining budget".into(),
            ));
        }
        state.remaining -= amount;
        state.reservations.insert(node.to_owned(), amount);
        Ok(())
    }

    pub fn release(&self, node: &str) -> p::Result<u64> {
        let mut state = self.lock()?;
        let amount = state.reservations.remove(node).unwrap_or(0);
        state.remaining = state.remaining.saturating_add(amount);
        Ok(amount)
    }

    pub fn remaining(&self) -> p::Result<u64> {
        Ok(self.lock()?.remaining)
    }

    fn lock(&self) -> p::Result<MutexGuard<'_, BudgetState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("route budget ledger is unavailable".into()))
    }
}

#[derive(Debug)]
pub struct RouteRuntime {
    route: ExecutionRoute,
    states: BTreeMap<String, RouteNodeState>,
    attempts: BTreeMap<String, u32>,
    intent_attempts: BTreeSet<(String, p::ActionId)>,
    results: BTreeMap<String, p::ResultRef>,
    status: RouteStatus,
    budget: BudgetLedger,
}

impl RouteRuntime {
    pub fn new(route: ExecutionRoute, parent_budget: u64) -> p::Result<Self> {
        validate_route(&route)?;
        let states: BTreeMap<String, RouteNodeState> = route
            .nodes
            .iter()
            .map(|node| (node.subtask.id.clone(), RouteNodeState::Pending))
            .collect();
        let status = if states.is_empty() {
            RouteStatus::Completed
        } else {
            RouteStatus::Running
        };
        Ok(Self {
            route,
            states,
            attempts: BTreeMap::new(),
            intent_attempts: BTreeSet::new(),
            results: BTreeMap::new(),
            status,
            budget: BudgetLedger::new(parent_budget),
        })
    }

    pub fn route(&self) -> &ExecutionRoute {
        &self.route
    }

    pub fn status(&self) -> RouteStatus {
        self.status
    }

    pub fn state(&self, node: &str) -> Option<RouteNodeState> {
        self.states.get(node).copied()
    }

    pub fn runnable(&self) -> Vec<&RouteNode> {
        if self.status != RouteStatus::Running {
            return Vec::new();
        }
        self.route
            .nodes
            .iter()
            .filter(|node| self.state(&node.subtask.id) == Some(RouteNodeState::Pending))
            .filter(|node| self.dependencies_satisfied(&node.subtask.id))
            .collect()
    }

    pub fn start(&mut self, node: &str) -> p::Result<()> {
        if self.status != RouteStatus::Running
            || self.state(node) != Some(RouteNodeState::Pending)
            || !self.dependencies_satisfied(node)
        {
            return Err(p::Error("route node is not runnable".into()));
        }
        let budget = self.node(node)?.role.budget_units()?;
        self.budget.reserve(node, budget)?;
        self.states.insert(node.to_owned(), RouteNodeState::Running);
        *self.attempts.entry(node.to_owned()).or_insert(0) += 1;
        Ok(())
    }

    pub fn complete(&mut self, node: &str, result: p::ResultRef) -> p::Result<()> {
        self.require_running(node)?;
        self.budget.release(node)?;
        self.states.insert(node.to_owned(), RouteNodeState::Done);
        self.results.insert(node.to_owned(), result);
        if self.all_terminal_with_tolerated_failures() {
            self.status = RouteStatus::Completed;
        }
        Ok(())
    }

    pub fn fail(&mut self, node: &str) -> p::Result<()> {
        self.require_running(node)?;
        self.budget.release(node)?;
        self.states.insert(node.to_owned(), RouteNodeState::Failed);
        let policies = self
            .route
            .edges
            .iter()
            .filter(|edge| edge.from == node)
            .map(|edge| edge.on_failure)
            .collect::<Vec<_>>();
        if policies.contains(&DependencyFailurePolicy::AbortRoute) {
            self.status = RouteStatus::Failed;
            self.cancel_unfinished()?;
        } else if policies.contains(&DependencyFailurePolicy::Replan) {
            self.status = RouteStatus::ReplanRequested;
            self.cancel_unfinished()?;
        } else if self.runnable().is_empty() {
            self.status = RouteStatus::Failed;
        }
        Ok(())
    }

    pub fn retry(
        &mut self,
        node: &str,
        intent_id: p::ActionId,
        side_effect_known_absent: bool,
    ) -> p::Result<RetryDecision> {
        if self.state(node) != Some(RouteNodeState::Failed) {
            return Err(p::Error("only a failed node can be retried".into()));
        }
        if !side_effect_known_absent {
            self.states
                .insert(node.to_owned(), RouteNodeState::OutcomeUnknown);
            self.status = RouteStatus::Failed;
            return Ok(RetryDecision::OutcomeUnknown);
        }
        let key = (node.to_owned(), intent_id);
        if !self.intent_attempts.insert(key) {
            return Ok(RetryDecision::Exhausted);
        }
        let attempts = self.attempts.get(node).copied().unwrap_or(0);
        if attempts >= self.node(node)?.retry.max_attempts {
            return Ok(RetryDecision::Exhausted);
        }
        self.states.insert(node.to_owned(), RouteNodeState::Pending);
        self.status = RouteStatus::Running;
        Ok(RetryDecision::Retry)
    }

    pub fn cancel_parent(&mut self) -> p::Result<()> {
        self.status = RouteStatus::Cancelled;
        self.cancel_unfinished()
    }

    pub fn outcome(&self) -> RouteOutcome {
        RouteOutcome {
            schema_version: p::SchemaVersion(1),
            status: self.status,
            node_states: self.states.clone(),
            result_refs: self.results.clone(),
        }
    }

    pub fn remaining_budget(&self) -> p::Result<u64> {
        self.budget.remaining()
    }

    fn dependencies_satisfied(&self, node: &str) -> bool {
        self.route
            .edges
            .iter()
            .filter(|edge| edge.to == node)
            .all(|edge| match self.state(&edge.from) {
                Some(RouteNodeState::Done) => true,
                Some(RouteNodeState::Failed) => {
                    edge.on_failure == DependencyFailurePolicy::Continue
                }
                _ => false,
            })
    }

    fn all_terminal_with_tolerated_failures(&self) -> bool {
        self.states.iter().all(|(node, state)| match state {
            RouteNodeState::Done => true,
            RouteNodeState::Failed => self
                .route
                .edges
                .iter()
                .filter(|edge| edge.from == *node)
                .all(|edge| edge.on_failure == DependencyFailurePolicy::Continue),
            RouteNodeState::Pending
            | RouteNodeState::Running
            | RouteNodeState::Cancelled
            | RouteNodeState::OutcomeUnknown => false,
        })
    }

    fn node(&self, node: &str) -> p::Result<&RouteNode> {
        self.route
            .nodes
            .iter()
            .find(|candidate| candidate.subtask.id == node)
            .ok_or_else(|| p::Error("route node does not exist".into()))
    }

    fn require_running(&self, node: &str) -> p::Result<()> {
        if self.state(node) == Some(RouteNodeState::Running) {
            Ok(())
        } else {
            Err(p::Error("route node is not running".into()))
        }
    }

    fn cancel_unfinished(&mut self) -> p::Result<()> {
        let ids = self
            .states
            .iter()
            .filter_map(|(id, state)| {
                matches!(state, RouteNodeState::Pending | RouteNodeState::Running)
                    .then_some(id.clone())
            })
            .collect::<Vec<_>>();
        for id in ids {
            self.budget.release(&id)?;
            self.states.insert(id, RouteNodeState::Cancelled);
        }
        Ok(())
    }
}

pub trait OrchestrationLibrary {
    fn match_pattern(&self, sig: ApplicabilitySignature) -> Option<OrchestrationPattern>;
    fn route(&self, pattern: Option<OrchestrationPattern>, frame: &GoalFrame) -> ExecutionRoute;
    fn sediment(&self, route: &ExecutionRoute, outcome: &RouteOutcome) -> Option<p::CandidateId>;
}

#[derive(Debug, Clone)]
pub struct SeedOrchestrationLibrary {
    patterns: Vec<OrchestrationPattern>,
    threshold: u8,
}

impl Default for SeedOrchestrationLibrary {
    fn default() -> Self {
        Self {
            patterns: vec![
                seed_pattern(
                    "direct",
                    RouteTopology::Direct,
                    Decomposability::Whole,
                    DependencyShape::Independent,
                    Verifiability::SelfCheck,
                    TaskScale::Single,
                ),
                seed_pattern(
                    "independent-slices",
                    RouteTopology::IndependentSlices,
                    Decomposability::Parallel,
                    DependencyShape::Independent,
                    Verifiability::Environment,
                    TaskScale::MultiStage,
                ),
                seed_pattern(
                    "review-gate",
                    RouteTopology::ReviewGate,
                    Decomposability::Sequential,
                    DependencyShape::Interdependent,
                    Verifiability::IndependentReview,
                    TaskScale::MultiStage,
                ),
            ],
            threshold: 4,
        }
    }
}

impl OrchestrationLibrary for SeedOrchestrationLibrary {
    fn match_pattern(&self, sig: ApplicabilitySignature) -> Option<OrchestrationPattern> {
        self.patterns
            .iter()
            .map(|pattern| (signature_score(&sig, &pattern.signature), pattern))
            .filter(|(score, _)| *score >= self.threshold)
            .max_by_key(|(score, _)| *score)
            .map(|(_, pattern)| pattern.clone())
    }

    fn route(&self, pattern: Option<OrchestrationPattern>, frame: &GoalFrame) -> ExecutionRoute {
        let topology = pattern
            .as_ref()
            .map(|pattern| pattern.spec.topology)
            .unwrap_or(RouteTopology::Direct);
        let mut profiles = frame.context.inventory.subagents.clone();
        let take = match topology {
            RouteTopology::Direct => 0,
            RouteTopology::IndependentSlices => profiles.len(),
            RouteTopology::ReviewGate => profiles.len().min(2),
        };
        profiles.truncate(take);
        let nodes = profiles
            .into_iter()
            .enumerate()
            .map(|(index, profile)| RouteNode {
                schema_version: p::SchemaVersion(1),
                subtask: Subtask {
                    schema_version: p::SchemaVersion(1),
                    id: format!("node-{index}"),
                    instruction: frame.goal.summary.clone(),
                    intent_id: p::ActionId(format!("route:{}:{index}", frame.goal.reference.0)),
                },
                resource_slice: profile.toolset.clone(),
                role: profile,
                done: frame.context.done_contract.clone(),
                retry: RetryPolicy {
                    schema_version: p::SchemaVersion(1),
                    max_attempts: 1,
                },
            })
            .collect::<Vec<_>>();
        let edges = if topology == RouteTopology::ReviewGate && nodes.len() == 2 {
            vec![RouteEdge {
                schema_version: p::SchemaVersion(1),
                from: nodes[0].subtask.id.clone(),
                to: nodes[1].subtask.id.clone(),
                on_failure: DependencyFailurePolicy::AbortRoute,
            }]
        } else {
            Vec::new()
        };
        ExecutionRoute {
            schema_version: p::SchemaVersion(1),
            reference: p::ExecutionRouteRef(format!("route:{}", frame.goal.reference.0)),
            pattern_ref: pattern.as_ref().map(|pattern| pattern.reference.clone()),
            nodes,
            edges,
        }
    }

    fn sediment(&self, route: &ExecutionRoute, outcome: &RouteOutcome) -> Option<p::CandidateId> {
        (outcome.status == RouteStatus::Completed)
            .then(|| p::CandidateId(format!("orchestration-candidate:{}", route.reference.0)))
    }
}

fn seed_pattern(
    name: &str,
    topology: RouteTopology,
    decomposability: Decomposability,
    dependency: DependencyShape,
    verifiability: Verifiability,
    scale: TaskScale,
) -> OrchestrationPattern {
    OrchestrationPattern {
        schema_version: p::SchemaVersion(1),
        reference: p::OrchestrationPatternRef(format!("pattern:{name}")),
        spec: WorkPattern {
            schema_version: p::SchemaVersion(1),
            name: name.into(),
            topology,
        },
        signature: ApplicabilitySignature {
            schema_version: p::SchemaVersion(1),
            decomposability,
            dependency,
            verifiability,
            clarity: GoalClarity::Clear,
            scale,
            reversibility: Reversibility::LowRiskReversible,
            case_anchors: Vec::new(),
            fitness: p::Confidence(0.5),
        },
        fitness: p::Confidence(0.5),
    }
}

fn signature_score(left: &ApplicabilitySignature, right: &ApplicabilitySignature) -> u8 {
    u8::from(left.decomposability == right.decomposability)
        + u8::from(left.dependency == right.dependency)
        + u8::from(left.verifiability == right.verifiability)
        + u8::from(left.clarity == right.clarity)
        + u8::from(left.scale == right.scale)
        + u8::from(left.reversibility == right.reversibility)
}

fn validate_route(route: &ExecutionRoute) -> p::Result<()> {
    if route.schema_version.0 == 0 || route.reference.0.trim().is_empty() {
        return Err(p::Error("execution route is incomplete".into()));
    }
    let ids = route
        .nodes
        .iter()
        .map(|node| node.subtask.id.clone())
        .collect::<BTreeSet<_>>();
    if ids.len() != route.nodes.len()
        || route
            .nodes
            .iter()
            .any(|node| node.subtask.id.trim().is_empty() || !node.done.is_verifiable())
    {
        return Err(p::Error(
            "execution route contains duplicate or incomplete nodes".into(),
        ));
    }
    if route
        .edges
        .iter()
        .any(|edge| !ids.contains(&edge.from) || !ids.contains(&edge.to) || edge.from == edge.to)
    {
        return Err(p::Error("execution route contains an invalid edge".into()));
    }
    let mut indegree = ids
        .iter()
        .map(|id| (id.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    for edge in &route.edges {
        *indegree.entry(edge.to.clone()).or_default() += 1;
    }
    let mut queue = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(id.clone()))
        .collect::<VecDeque<_>>();
    let mut visited = 0;
    while let Some(id) = queue.pop_front() {
        visited += 1;
        for edge in route.edges.iter().filter(|edge| edge.from == id) {
            if let Some(degree) = indegree.get_mut(&edge.to) {
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(edge.to.clone());
                }
            }
        }
    }
    if visited != ids.len() {
        return Err(p::Error("execution route must be acyclic".into()));
    }
    Ok(())
}

fn parse_budget_units(budget: &p::Budget) -> p::Result<u64> {
    let raw = budget.0.trim();
    let raw = raw.strip_prefix("units:").unwrap_or(raw);
    raw.parse::<u64>()
        .map_err(|_| p::Error("subagent budget must be an integer or units:<integer>".into()))
}

pub fn capability_refs(toolset: &Toolset) -> Vec<p::CapabilityRef> {
    toolset.items.iter().map(|item| item.id.clone()).collect()
}
