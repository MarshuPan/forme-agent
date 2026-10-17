//! Durable project and route facts. The store validates and folds these in its
//! append transaction; executors cannot manufacture a completed node by replay.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskPartition {
    Whole,
    Sequential,
    Parallel,
    Iterative,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskDependency {
    Independent,
    Interdependent,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskVerification {
    SelfCheck,
    Environment,
    IndependentReview,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskClarity {
    Clear,
    Clarify,
    Explore,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskExtent {
    Single,
    MultiStage,
    LongRunning,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskReversibility {
    LowRiskReversible,
    HighRiskIrreversible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSignature {
    pub schema_version: SchemaVersion,
    pub partition: TaskPartition,
    pub dependency: TaskDependency,
    pub verification: TaskVerification,
    pub clarity: TaskClarity,
    pub extent: TaskExtent,
    pub reversibility: TaskReversibility,
    pub anchors: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteRequirement {
    OutputContains(String),
    FileDigest {
        relative_path: String,
        digest: String,
    },
    ActionSucceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProject {
    pub schema_version: SchemaVersion,
    pub reference: String,
    pub workspace: WorkspaceRef,
    pub goal: GoalRef,
    pub done: DoneContractRef,
    pub summary: String,
    pub signature: TaskSignature,
    pub requirements: Vec<RouteRequirement>,
    pub scope: Scope,
    pub capabilities: Vec<CapabilityRef>,
    pub model: ModelProfileRef,
    pub budget_units: u64,
    pub node_units: u64,
    pub cadence_ms: u64,
    pub deadline: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteSelectionScore {
    pub schema_version: SchemaVersion,
    pub pattern: OrchestrationPatternRef,
    pub dimensions: [u16; 6],
    pub anchors: u16,
    pub fitness: u16,
    pub total: u16,
    pub compatible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNode {
    pub schema_version: SchemaVersion,
    pub id: String,
    pub role: RoleRef,
    pub instruction: String,
    pub intent: ActionId,
    pub scope: Scope,
    pub capabilities: Vec<CapabilityRef>,
    pub model: ModelProfileRef,
    pub budget_units: u64,
    pub max_attempts: u32,
    pub requirements: Vec<RouteRequirement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteFailurePolicy {
    Abort,
    Continue,
    Replan,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkDependency {
    pub schema_version: SchemaVersion,
    pub from: String,
    pub to: String,
    pub on_failure: RouteFailurePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDeclaration {
    pub schema_version: SchemaVersion,
    pub route: ExecutionRouteRef,
    pub project: String,
    pub parent: RunId,
    pub goal: GoalRef,
    pub done: DoneContractRef,
    pub pattern: Option<OrchestrationPatternRef>,
    pub signature: TaskSignature,
    pub scores: Vec<RouteSelectionScore>,
    pub threshold: u16,
    pub confidence_millis: u16,
    pub checkpoint_required: bool,
    pub budget_units: u64,
    pub nodes: Vec<WorkNode>,
    pub main: WorkNode,
    pub dependencies: Vec<WorkDependency>,
    pub charter_digest: SchemaDigest,
    pub input_digest: SchemaDigest,
    pub review_digest: SchemaDigest,
    pub created_at: Timestamp,
}
impl RouteDeclaration {
    pub fn node_run_id(&self, node: &str, attempt: u32) -> RunId {
        if self.nodes.is_empty() {
            if attempt == 1 {
                self.parent.clone()
            } else {
                RunId(format!("project-main:{}:attempt:{attempt}", self.parent.0))
            }
        } else {
            RunId(format!("child:{}:{node}:attempt:{attempt}", self.parent.0))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkNodeStatus {
    Pending,
    Running,
    Waiting,
    Done,
    Failed,
    Cancelled,
    OutcomeUnknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkRouteStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    ReplanRequested,
    OutcomeUnknown,
    Conflict,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResultConflictKind {
    Fact,
    Design,
    Quality,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteConflict {
    pub schema_version: SchemaVersion,
    pub kind: ResultConflictKind,
    pub nodes: Vec<String>,
    pub evidence: Vec<EvidenceRef>,
    pub reason: ReasonRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrchestrationOperation {
    ProjectDeclared(WorkspaceProject),
    ProjectRevised {
        project: WorkspaceProject,
        expected_digest: SchemaDigest,
    },
    RouteDeclared(Box<RouteDeclaration>),
    NodeStarted {
        route: ExecutionRouteRef,
        node: String,
        intent: ActionId,
    },
    NodeFinished {
        route: ExecutionRouteRef,
        node: String,
        status: WorkNodeStatus,
        consumed: u64,
        result: Option<ResultRef>,
        summary: String,
        verification: VerificationOutcome,
        evidence: Vec<EvidenceRef>,
        effects_absent: bool,
    },
    NodeRetried {
        route: ExecutionRouteRef,
        node: String,
        intent: ActionId,
    },
    RouteCancelled {
        route: ExecutionRouteRef,
    },
    ConflictReported {
        route: ExecutionRouteRef,
        conflict: RouteConflict,
    },
    RouteClosed {
        route: ExecutionRouteRef,
        status: WorkRouteStatus,
    },
    Checkpoint {
        project: String,
        route: ExecutionRouteRef,
        input_digest: SchemaDigest,
        at: Timestamp,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationFact {
    pub schema_version: SchemaVersion,
    pub workspace: WorkspaceRef,
    pub command: IdempotencyKey,
    pub expected_revision: u64,
    pub operation: OrchestrationOperation,
    pub digest: SchemaDigest,
}
impl OrchestrationFact {
    pub fn seal(
        workspace: WorkspaceRef,
        command: IdempotencyKey,
        expected_revision: u64,
        operation: OrchestrationOperation,
    ) -> Result<Self> {
        let mut value = Self {
            schema_version: SchemaVersion(1),
            workspace,
            command,
            expected_revision,
            operation,
            digest: SchemaDigest(String::new()),
        };
        value.digest = value.calculate_digest()?;
        value.validate()?;
        Ok(value)
    }
    fn calculate_digest(&self) -> Result<SchemaDigest> {
        canonical_digest(&(
            self.schema_version,
            &self.workspace,
            &self.command,
            self.expected_revision,
            &self.operation,
        ))
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.command.0.trim().is_empty()
            || self.command.0.len() > 512
            || self.workspace.0.trim().is_empty()
            || self.digest != self.calculate_digest()?
            || serde_json::to_vec(self)
                .map_err(|_| Error("route fact encoding failed".into()))?
                .len()
                > 262_144
        {
            return Err(Error(
                "invalid, oversized or altered orchestration fact".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkNodeRecord {
    pub schema_version: SchemaVersion,
    pub status: WorkNodeStatus,
    pub attempts: u32,
    pub intents: BTreeSet<ActionId>,
    pub reserved: u64,
    pub consumed: u64,
    pub result: Option<ResultRef>,
    pub summary: String,
    pub verification: VerificationOutcome,
    pub evidence: Vec<EvidenceRef>,
    pub effects_absent: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRouteRecord {
    pub schema_version: SchemaVersion,
    pub declaration: RouteDeclaration,
    pub nodes: BTreeMap<String, WorkNodeRecord>,
    pub remaining: u64,
    pub spent: u64,
    pub status: WorkRouteStatus,
    pub closed: bool,
    pub checkpointed: bool,
    pub conflicts: Vec<RouteConflict>,
}
impl WorkRouteRecord {
    pub fn node_spec(&self, id: &str) -> Option<&WorkNode> {
        if self.declaration.nodes.is_empty() {
            (self.declaration.main.id == id).then_some(&self.declaration.main)
        } else {
            self.declaration.nodes.iter().find(|n| n.id == id)
        }
    }
    pub fn runnable(&self) -> Vec<String> {
        if self.closed || self.status != WorkRouteStatus::Running {
            return Vec::new();
        }
        let specs = if self.declaration.nodes.is_empty() {
            std::slice::from_ref(&self.declaration.main)
        } else {
            &self.declaration.nodes
        };
        specs
            .iter()
            .filter(|n| self.nodes[&n.id].status == WorkNodeStatus::Pending)
            .filter(|n| {
                self.declaration
                    .dependencies
                    .iter()
                    .filter(|e| e.to == n.id)
                    .all(|e| {
                        let status = self.nodes[&e.from].status;
                        status == WorkNodeStatus::Done
                            || (status == WorkNodeStatus::Failed
                                && e.on_failure == RouteFailurePolicy::Continue)
                    })
            })
            .map(|n| n.id.clone())
            .collect()
    }
    pub fn has_inflight(&self) -> bool {
        self.nodes.values().any(|n| {
            matches!(
                n.status,
                WorkNodeStatus::Running | WorkNodeStatus::Waiting | WorkNodeStatus::OutcomeUnknown
            )
        })
    }
    pub fn terminal_status(&self) -> Option<WorkRouteStatus> {
        if self.has_inflight() {
            return self
                .nodes
                .values()
                .any(|n| n.status == WorkNodeStatus::OutcomeUnknown)
                .then_some(WorkRouteStatus::OutcomeUnknown);
        }
        if !self.conflicts.is_empty() {
            return Some(WorkRouteStatus::Conflict);
        }
        if self.status != WorkRouteStatus::Running {
            return Some(self.status);
        }
        if self
            .nodes
            .values()
            .all(|n| n.status == WorkNodeStatus::Done)
        {
            return Some(WorkRouteStatus::Completed);
        }
        if self.runnable().iter().all(|id| {
            self.node_spec(id)
                .is_none_or(|n| n.budget_units > self.remaining)
        }) {
            Some(WorkRouteStatus::Failed)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCheckpoint {
    pub schema_version: SchemaVersion,
    pub route: ExecutionRouteRef,
    pub input_digest: SchemaDigest,
    pub at: Timestamp,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationLedger {
    pub schema_version: SchemaVersion,
    pub revision: u64,
    pub projects: BTreeMap<String, WorkspaceProject>,
    pub routes: BTreeMap<ExecutionRouteRef, WorkRouteRecord>,
    pub checkpoints: BTreeMap<String, ProjectCheckpoint>,
    pub commands: BTreeMap<IdempotencyKey, SchemaDigest>,
}
impl Default for OrchestrationLedger {
    fn default() -> Self {
        Self {
            schema_version: SchemaVersion(1),
            revision: 0,
            projects: BTreeMap::new(),
            routes: BTreeMap::new(),
            checkpoints: BTreeMap::new(),
            commands: BTreeMap::new(),
        }
    }
}
impl OrchestrationLedger {
    pub fn fold(&mut self, event: &Event) -> Result<()> {
        if let EventPayload::DecisionTraceRecorded(p) = &event.payload {
            if let Some(fact) = &p.orchestration {
                self.apply(fact)?;
            }
        }
        Ok(())
    }
    pub fn apply(&mut self, fact: &OrchestrationFact) -> Result<()> {
        fact.validate()?;
        if let Some(digest) = self.commands.get(&fact.command) {
            return if *digest == fact.digest {
                Ok(())
            } else {
                Err(Error("orchestration command identity conflict".into()))
            };
        }
        if self.revision != fact.expected_revision {
            return Err(Error("orchestration expected revision conflict".into()));
        }
        // Validation is transactional even when used without a database.
        let mut next = self.clone();
        next.apply_operation(&fact.workspace, &fact.operation)?;
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| Error("orchestration revision exhausted".into()))?;
        next.commands
            .insert(fact.command.clone(), fact.digest.clone());
        *self = next;
        Ok(())
    }
    fn apply_operation(
        &mut self,
        workspace: &WorkspaceRef,
        operation: &OrchestrationOperation,
    ) -> Result<()> {
        match operation {
            OrchestrationOperation::ProjectRevised {
                project,
                expected_digest,
            } => {
                let previous = self
                    .projects
                    .get(&project.reference)
                    .cloned()
                    .ok_or_else(|| Error("project is not declared".into()))?;
                if canonical_digest(&previous)? != *expected_digest
                    || previous.workspace != project.workspace
                    || previous.goal != project.goal
                    || previous.done != project.done
                {
                    return Err(Error(
                        "project revision changes identity or has a stale digest".into(),
                    ));
                }
                self.projects.remove(&project.reference);
                self.apply_operation(
                    workspace,
                    &OrchestrationOperation::ProjectDeclared(project.clone()),
                )?;
            }
            OrchestrationOperation::ProjectDeclared(project) => {
                if project.workspace != *workspace
                    || project.schema_version != SchemaVersion(1)
                    || project.reference.trim().is_empty()
                    || project.goal.0.trim().is_empty()
                    || project.done.0.trim().is_empty()
                    || project.summary.trim().is_empty()
                    || project.summary.len() > 8192
                    || project.signature.schema_version != SchemaVersion(1)
                    || project.signature.anchors.len() > 64
                    || project.requirements.is_empty()
                    || project.requirements.len() > 16
                    || project.budget_units == 0
                    || project.node_units == 0
                    || project.node_units > project.budget_units
                    || project.cadence_ms == 0
                    || project.capabilities.len() > 64
                    || project.scope.0.is_empty()
                    || self.projects.contains_key(&project.reference)
                {
                    return Err(Error(
                        "project declaration is incomplete, duplicated or outside workspace".into(),
                    ));
                }
                self.projects
                    .insert(project.reference.clone(), project.clone());
            }
            OrchestrationOperation::RouteDeclared(route) => {
                let project = self
                    .projects
                    .get(&route.project)
                    .ok_or_else(|| Error("route project is not registered".into()))?;
                validate_route_declaration(route, project)?;
                if self.routes.contains_key(&route.route)
                    || self.routes.values().any(|r| {
                        r.declaration.project == route.project
                            && (!r.closed || r.has_inflight() || !r.checkpointed)
                    })
                {
                    return Err(Error(
                        "route exists or the previous segment has not checkpointed".into(),
                    ));
                }
                let specs = if route.nodes.is_empty() {
                    std::slice::from_ref(&route.main)
                } else {
                    &route.nodes
                };
                let nodes = specs
                    .iter()
                    .map(|n| {
                        (
                            n.id.clone(),
                            WorkNodeRecord {
                                schema_version: SchemaVersion(1),
                                status: WorkNodeStatus::Pending,
                                attempts: 0,
                                intents: BTreeSet::new(),
                                reserved: 0,
                                consumed: 0,
                                result: None,
                                summary: String::new(),
                                verification: VerificationOutcome::Unverifiable(ReasonRef(
                                    "not executed".into(),
                                )),
                                evidence: Vec::new(),
                                effects_absent: true,
                            },
                        )
                    })
                    .collect();
                self.routes.insert(
                    route.route.clone(),
                    WorkRouteRecord {
                        schema_version: SchemaVersion(1),
                        declaration: route.as_ref().clone(),
                        nodes,
                        remaining: route.budget_units,
                        spent: 0,
                        status: WorkRouteStatus::Running,
                        closed: false,
                        checkpointed: false,
                        conflicts: Vec::new(),
                    },
                );
            }
            OrchestrationOperation::NodeStarted {
                route,
                node,
                intent,
            } => {
                let r = self.route_mut(route)?;
                if !r.runnable().contains(node) {
                    return Err(Error("node is not runnable".into()));
                }
                let spec = r
                    .node_spec(node)
                    .cloned()
                    .ok_or_else(|| Error("unknown node".into()))?;
                let record = r.nodes.get_mut(node).expect("validated node");
                if record.attempts >= spec.max_attempts
                    || record.intents.contains(intent)
                    || r.remaining < spec.budget_units
                    || *intent != spec.intent
                {
                    return Err(Error(
                        "duplicate intent, exhausted attempts or insufficient parent budget".into(),
                    ));
                }
                record.intents.insert(intent.clone());
                record.attempts += 1;
                record.status = WorkNodeStatus::Running;
                record.reserved = spec.budget_units;
                r.remaining -= spec.budget_units;
            }
            OrchestrationOperation::NodeFinished {
                route,
                node,
                status,
                consumed,
                result,
                summary,
                verification,
                evidence,
                effects_absent,
            } => {
                let r = self.route_mut(route)?;
                let n = r
                    .nodes
                    .get_mut(node)
                    .ok_or_else(|| Error("unknown node".into()))?;
                if !matches!(n.status, WorkNodeStatus::Running | WorkNodeStatus::Waiting)
                    || !matches!(
                        status,
                        WorkNodeStatus::Done
                            | WorkNodeStatus::Failed
                            | WorkNodeStatus::Waiting
                            | WorkNodeStatus::OutcomeUnknown
                            | WorkNodeStatus::Cancelled
                    )
                    || *consumed > n.reserved
                    || summary.len() > 8192
                    || evidence.len() > 128
                    || (*status == WorkNodeStatus::Done
                        && (*verification != VerificationOutcome::Pass
                            || result.is_none()
                            || evidence.is_empty()))
                    || (*status == WorkNodeStatus::Cancelled && !effects_absent)
                {
                    return Err(Error(
                        "invalid node completion or unverified success".into(),
                    ));
                }
                n.status = *status;
                n.result = result.clone();
                n.summary = summary.clone();
                n.verification = verification.clone();
                n.evidence = evidence.clone();
                n.effects_absent = *effects_absent;
                // Unknown/waiting retains reservation until explicit reconciliation.
                if !matches!(
                    status,
                    WorkNodeStatus::OutcomeUnknown | WorkNodeStatus::Waiting
                ) {
                    r.remaining = r
                        .remaining
                        .checked_add(n.reserved - consumed)
                        .ok_or_else(|| Error("budget overflow".into()))?;
                    r.spent += consumed;
                    n.consumed += consumed;
                    n.reserved = 0;
                }
                if *status == WorkNodeStatus::Failed {
                    let policies = r
                        .declaration
                        .dependencies
                        .iter()
                        .filter(|e| e.from == *node)
                        .map(|e| e.on_failure)
                        .collect::<Vec<_>>();
                    if policies.contains(&RouteFailurePolicy::Abort) {
                        r.status = WorkRouteStatus::Failed;
                    } else if policies.contains(&RouteFailurePolicy::Replan) {
                        r.status = WorkRouteStatus::ReplanRequested;
                    }
                    if r.status != WorkRouteStatus::Running {
                        cancel_pending(r);
                    }
                }
            }
            OrchestrationOperation::NodeRetried {
                route,
                node,
                intent,
            } => {
                let r = self.route_mut(route)?;
                if r.closed || r.status == WorkRouteStatus::Cancelled || !r.conflicts.is_empty() {
                    return Err(Error("closed/conflicted route cannot retry".into()));
                }
                let spec = if r.declaration.nodes.is_empty() {
                    &mut r.declaration.main
                } else {
                    r.declaration
                        .nodes
                        .iter_mut()
                        .find(|n| &n.id == node)
                        .ok_or_else(|| Error("unknown node".into()))?
                };
                let n = r.nodes.get_mut(node).expect("validated node");
                if n.status != WorkNodeStatus::Failed
                    || !n.effects_absent
                    || n.attempts >= spec.max_attempts
                    || n.intents.contains(intent)
                {
                    return Err(Error(
                        "retry requires a new intent and proven absence of side effects".into(),
                    ));
                }
                spec.intent = intent.clone();
                n.status = WorkNodeStatus::Pending;
                r.status = WorkRouteStatus::Running;
                for n in r.nodes.values_mut() {
                    if n.status == WorkNodeStatus::Cancelled && n.attempts == 0 {
                        n.status = WorkNodeStatus::Pending;
                    }
                }
            }
            OrchestrationOperation::RouteCancelled { route } => {
                let r = self.route_mut(route)?;
                if r.closed {
                    return Err(Error("closed route cannot be cancelled".into()));
                }
                r.status = WorkRouteStatus::Cancelled;
                cancel_pending(r);
                // Running children must report their actual terminal state; a
                // cancellation request never proves a side effect was absent.
            }
            OrchestrationOperation::ConflictReported { route, conflict } => {
                let r = self.route_mut(route)?;
                if r.closed
                    || conflict.schema_version != SchemaVersion(1)
                    || conflict.nodes.len() < 2
                    || conflict.evidence.is_empty()
                    || conflict.reason.0.is_empty()
                    || conflict
                        .nodes
                        .iter()
                        .any(|n| !r.nodes.get(n).is_some_and(|r| r.result.is_some()))
                {
                    return Err(Error("conflict lacks participants or evidence".into()));
                }
                r.conflicts.push(conflict.clone());
                r.status = WorkRouteStatus::Conflict;
                cancel_pending(r);
            }
            OrchestrationOperation::RouteClosed { route, status } => {
                let r = self.route_mut(route)?;
                if r.closed || r.terminal_status() != Some(*status) {
                    return Err(Error(
                        "route is not terminal or status is fabricated".into(),
                    ));
                }
                r.closed = true;
                r.status = *status;
                if *status != WorkRouteStatus::Completed {
                    cancel_pending(r);
                }
            }
            OrchestrationOperation::Checkpoint {
                project,
                route,
                input_digest,
                at,
            } => {
                let r = self
                    .routes
                    .get(route)
                    .ok_or_else(|| Error("unknown checkpoint route".into()))?;
                if !r.closed
                    || r.has_inflight()
                    || r.declaration.project != *project
                    || r.declaration.input_digest != *input_digest
                    || *at < r.declaration.created_at
                {
                    return Err(Error(
                        "checkpoint cannot pass an active or unknown segment".into(),
                    ));
                }
                self.routes
                    .get_mut(route)
                    .expect("validated route")
                    .checkpointed = true;
                self.checkpoints.insert(
                    project.clone(),
                    ProjectCheckpoint {
                        schema_version: SchemaVersion(1),
                        route: route.clone(),
                        input_digest: input_digest.clone(),
                        at: *at,
                    },
                );
            }
        }
        Ok(())
    }
    fn route_mut(&mut self, route: &ExecutionRouteRef) -> Result<&mut WorkRouteRecord> {
        self.routes
            .get_mut(route)
            .ok_or_else(|| Error("unknown route".into()))
    }
}
fn cancel_pending(route: &mut WorkRouteRecord) {
    for node in route.nodes.values_mut() {
        if node.status == WorkNodeStatus::Pending {
            node.status = WorkNodeStatus::Cancelled;
        }
    }
}
fn validate_route_declaration(route: &RouteDeclaration, project: &WorkspaceProject) -> Result<()> {
    if route.schema_version != SchemaVersion(1)
        || route.route.0.is_empty()
        || route.parent.0.is_empty()
        || route.goal != project.goal
        || route.done != project.done
        || route.signature.schema_version != SchemaVersion(1)
        || route.budget_units == 0
        || route.budget_units > project.budget_units
        || route.nodes.len() > 32
        || route.dependencies.len() > 128
        || route.charter_digest.0.is_empty()
        || route.input_digest.0.is_empty()
        || route.review_digest.0.is_empty()
        || route.scores.len() > 64
        || route.threshold == 0
        || route.threshold > 6400
        || route.confidence_millis > 1000
        || route.scores.iter().any(|s| {
            s.schema_version != SchemaVersion(1)
                || s.pattern.0.is_empty()
                || s.dimensions.iter().any(|d| *d != 0 && *d != 1000)
                || s.anchors > 200
                || s.fitness > 200
                || s.total != s.dimensions.iter().sum::<u16>() + s.anchors + s.fitness
        })
        || route.pattern.as_ref().is_some_and(|pattern| {
            !route
                .scores
                .iter()
                .any(|s| &s.pattern == pattern && s.compatible && s.total >= route.threshold)
        })
        || route.pattern.is_none()
            && route
                .scores
                .iter()
                .any(|s| s.compatible && s.total >= route.threshold)
        || (route.pattern.is_none()
            && (route.confidence_millis > 250
                || !route.checkpoint_required
                || !route.nodes.iter().any(|n| n.role.0 == "evaluator")))
    {
        return Err(Error(
            "invalid route declaration or unsafe custom route".into(),
        ));
    }
    let ids = route
        .nodes
        .iter()
        .map(|n| n.id.clone())
        .collect::<BTreeSet<_>>();
    if ids.len() != route.nodes.len()
        || route
            .nodes
            .iter()
            .chain(std::iter::once(&route.main))
            .any(|n| {
                n.schema_version != SchemaVersion(1)
                    || n.id.is_empty()
                    || n.role.0.is_empty()
                    || n.instruction.is_empty()
                    || n.instruction.len() > 16384
                    || n.intent.0.is_empty()
                    || n.model != project.model
                    || !charter_scope_contains(&project.scope, &n.scope)
                    || n.capabilities
                        .iter()
                        .any(|c| !project.capabilities.contains(c))
                    || n.budget_units == 0
                    || n.budget_units > route.budget_units
                    || n.max_attempts == 0
                    || n.max_attempts > 3
                    || n.requirements.is_empty()
                    || n.requirements.len() > 16
            })
    {
        return Err(Error(
            "route node exceeds project scope, resources or bounds".into(),
        ));
    }
    let mut visited = BTreeSet::new();
    if route.dependencies.iter().any(|e| {
        e.schema_version != SchemaVersion(1)
            || !ids.contains(&e.from)
            || !ids.contains(&e.to)
            || e.from == e.to
    }) {
        return Err(Error("route dependency is invalid".into()));
    }
    loop {
        let before = visited.len();
        for id in &ids {
            if route
                .dependencies
                .iter()
                .filter(|e| &e.to == id)
                .all(|e| visited.contains(&e.from))
            {
                visited.insert(id.clone());
            }
        }
        if visited.len() == ids.len() {
            break;
        }
        if visited.len() == before {
            return Err(Error("route dependency cycle".into()));
        }
    }
    Ok(())
}
