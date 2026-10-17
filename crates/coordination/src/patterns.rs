//! Repository-owned pattern index and deterministic, explainable selection.
use crate::*;

#[derive(Debug, Clone, PartialEq)]
pub struct PatternIndexEntry {
    pub schema_version: p::SchemaVersion,
    pub pattern: OrchestrationPattern,
    pub purpose: String,
    pub roles: Vec<String>,
    pub operation_guide: Vec<String>,
    pub checkpoint_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternMatchScore {
    pub pattern: p::OrchestrationPatternRef,
    pub dimensions: [u16; 6],
    pub anchors: u16,
    pub fitness: u16,
    pub total: u16,
    pub hard_compatible: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PatternSelection {
    pub schema_version: p::SchemaVersion,
    pub pattern: Option<OrchestrationPattern>,
    pub scores: Vec<PatternMatchScore>,
    pub threshold: u16,
    pub custom: bool,
    pub confidence: p::Confidence,
    pub checkpoint_required: bool,
    pub reason: p::ReasonRef,
}

pub fn signature_score(
    task: &ApplicabilitySignature,
    pattern: &OrchestrationPattern,
) -> PatternMatchScore {
    let sig = &pattern.signature;
    let dimensions = [
        task.decomposability == sig.decomposability,
        task.dependency == sig.dependency,
        task.verifiability == sig.verifiability,
        task.clarity == sig.clarity,
        task.scale == sig.scale,
        task.reversibility == sig.reversibility,
    ]
    .map(|matched| if matched { 1000 } else { 0 });
    // Case anchors and fitness cannot turn a structurally unsuitable plan
    // into a match, nor compensate for a hard governance incompatibility.
    let anchors = task
        .case_anchors
        .iter()
        .filter(|id| sig.case_anchors.contains(id))
        .count()
        .min(2) as u16
        * 100;
    let fitness = (pattern.fitness.0.clamp(0.0, 1.0) * 200.0) as u16;
    let hard_compatible = task.schema_version.0 > 0
        && task.fitness.0.is_finite()
        && (task.reversibility != Reversibility::HighRiskIrreversible
            || matches!(
                pattern.spec.topology,
                RouteTopology::ReviewGate
                    | RouteTopology::PlannerIntegrator
                    | RouteTopology::LongRunning
            ))
        && (task.clarity != GoalClarity::Clarify
            || pattern.spec.topology == RouteTopology::Clarify)
        && (task.scale != TaskScale::LongRunning
            || pattern.spec.topology == RouteTopology::LongRunning);
    PatternMatchScore {
        pattern: pattern.reference.clone(),
        dimensions,
        anchors,
        fitness,
        total: dimensions.iter().sum::<u16>() + anchors + fitness,
        hard_compatible,
    }
}

impl SeedOrchestrationLibrary {
    pub fn index(&self) -> Vec<PatternIndexEntry> {
        self.patterns
            .iter()
            .map(|pattern| {
                let (purpose, roles, guide, checkpoint) = description(pattern.spec.topology);
                PatternIndexEntry {
                    schema_version: p::SchemaVersion(1),
                    pattern: pattern.clone(),
                    purpose: purpose.into(),
                    roles: roles.iter().map(|s| (*s).into()).collect(),
                    operation_guide: guide.iter().map(|s| (*s).into()).collect(),
                    checkpoint_required: checkpoint,
                }
            })
            .collect()
    }

    pub fn select(&self, signature: &ApplicabilitySignature) -> PatternSelection {
        let mut scores = self
            .patterns
            .iter()
            .map(|pattern| signature_score(signature, pattern))
            .collect::<Vec<_>>();
        scores.sort_by(|a, b| {
            b.total
                .cmp(&a.total)
                .then_with(|| a.pattern.cmp(&b.pattern))
        });
        let threshold = u16::from(self.threshold) * 1000;
        let best = scores
            .iter()
            .find(|score| score.hard_compatible && score.total >= threshold);
        let pattern = best
            .and_then(|score| self.patterns.iter().find(|p| p.reference == score.pattern))
            .cloned();
        let custom = pattern.is_none();
        PatternSelection {
            schema_version: p::SchemaVersion(1),
            confidence: p::Confidence(if custom { 0.25 } else { best.map_or(0.0, |s| f32::from(s.total) / 6400.0) }),
            checkpoint_required: custom || pattern.as_ref().is_some_and(|p| description(p.spec.topology).3),
            pattern, scores, threshold, custom,
            reason: p::ReasonRef(if custom { "no compatible index entry met the threshold; independent evaluation and checkpoint required" }
                else { "highest compatible six-dimension score with case and active fitness evidence" }.into()),
        }
    }
}

impl Default for SeedOrchestrationLibrary {
    fn default() -> Self {
        let definitions = [
            (
                "single-run",
                RouteTopology::Direct,
                Decomposability::Whole,
                DependencyShape::Independent,
                Verifiability::SelfCheck,
                GoalClarity::Clear,
                TaskScale::Single,
                Reversibility::LowRiskReversible,
            ),
            (
                "retrieve-then-answer",
                RouteTopology::Retrieve,
                Decomposability::Whole,
                DependencyShape::Independent,
                Verifiability::Environment,
                GoalClarity::Explore,
                TaskScale::Single,
                Reversibility::LowRiskReversible,
            ),
            (
                "tool-loop",
                RouteTopology::ToolLoop,
                Decomposability::Iterative,
                DependencyShape::Interdependent,
                Verifiability::Environment,
                GoalClarity::Clear,
                TaskScale::Single,
                Reversibility::LowRiskReversible,
            ),
            (
                "clarify-then-act",
                RouteTopology::Clarify,
                Decomposability::Whole,
                DependencyShape::Independent,
                Verifiability::SelfCheck,
                GoalClarity::Clarify,
                TaskScale::Single,
                Reversibility::LowRiskReversible,
            ),
            (
                "generator-evaluator",
                RouteTopology::ReviewGate,
                Decomposability::Sequential,
                DependencyShape::Interdependent,
                Verifiability::IndependentReview,
                GoalClarity::Clear,
                TaskScale::MultiStage,
                Reversibility::LowRiskReversible,
            ),
            (
                "parallel-research-fanout",
                RouteTopology::IndependentSlices,
                Decomposability::Parallel,
                DependencyShape::Independent,
                Verifiability::Environment,
                GoalClarity::Explore,
                TaskScale::MultiStage,
                Reversibility::LowRiskReversible,
            ),
            (
                "planner-builder-evaluator-integrator",
                RouteTopology::PlannerIntegrator,
                Decomposability::Sequential,
                DependencyShape::Interdependent,
                Verifiability::Environment,
                GoalClarity::Clear,
                TaskScale::MultiStage,
                Reversibility::HighRiskIrreversible,
            ),
            (
                "long-running-harness",
                RouteTopology::LongRunning,
                Decomposability::Iterative,
                DependencyShape::Interdependent,
                Verifiability::IndependentReview,
                GoalClarity::Clear,
                TaskScale::LongRunning,
                Reversibility::LowRiskReversible,
            ),
        ];
        Self {
            threshold: 5,
            patterns: definitions
                .into_iter()
                .map(
                    |(
                        name,
                        topology,
                        decomposability,
                        dependency,
                        verifiability,
                        clarity,
                        scale,
                        reversibility,
                    )| OrchestrationPattern {
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
                            clarity,
                            scale,
                            reversibility,
                            case_anchors: vec![p::NodeId(format!("case:{name}"))],
                            fitness: p::Confidence(0.5),
                        },
                        fitness: p::Confidence(0.5),
                    },
                )
                .collect(),
        }
    }
}

impl OrchestrationLibrary for SeedOrchestrationLibrary {
    fn match_pattern(&self, sig: ApplicabilitySignature) -> Option<OrchestrationPattern> {
        self.select(&sig).pattern
    }

    fn route(&self, pattern: Option<OrchestrationPattern>, frame: &GoalFrame) -> ExecutionRoute {
        let topology = pattern
            .as_ref()
            .map_or(RouteTopology::Custom, |p| p.spec.topology);
        let (_, roles, _, _) = description(topology);
        let profiles = &frame.context.inventory.subagents;
        let mut nodes = Vec::new();
        if !profiles.is_empty() {
            for (index, role) in roles.iter().enumerate() {
                let mut profile = profiles
                    .iter()
                    .find(|p| p.role.0 == *role)
                    .unwrap_or(&profiles[index % profiles.len()])
                    .clone();
                profile.role = p::RoleRef((*role).into());
                nodes.push(RouteNode {
                    schema_version: p::SchemaVersion(1),
                    subtask: Subtask { schema_version: p::SchemaVersion(1), id: format!("node-{index}"),
                        instruction: format!("Role: {role}. Address the owner goal within the supplied constraints. Report evidence and unresolved conflicts. Goal: {}", frame.goal.summary),
                        intent_id: p::ActionId(format!("route:{}:{index}", frame.goal.reference.0)) },
                    resource_slice: profile.toolset.clone(), role: profile,
                    done: frame.context.done_contract.clone(), retry: RetryPolicy { schema_version: p::SchemaVersion(1), max_attempts: 2 },
                });
            }
        }
        let mut edges = Vec::new();
        if topology == RouteTopology::IndependentSlices && nodes.len() == 3 {
            for from in 0..2 {
                edges.push(RouteEdge {
                    schema_version: p::SchemaVersion(1),
                    from: nodes[from].subtask.id.clone(),
                    to: nodes[2].subtask.id.clone(),
                    on_failure: DependencyFailurePolicy::Replan,
                });
            }
        } else {
            for pair in nodes.windows(2) {
                edges.push(RouteEdge {
                    schema_version: p::SchemaVersion(1),
                    from: pair[0].subtask.id.clone(),
                    to: pair[1].subtask.id.clone(),
                    on_failure: DependencyFailurePolicy::AbortRoute,
                });
            }
        }
        ExecutionRoute {
            schema_version: p::SchemaVersion(1),
            reference: p::ExecutionRouteRef(format!("route:{}", frame.goal.reference.0)),
            pattern_ref: pattern.map(|p| p.reference),
            nodes,
            edges,
        }
    }

    fn sediment(&self, route: &ExecutionRoute, outcome: &RouteOutcome) -> Option<p::CandidateId> {
        (outcome.status != RouteStatus::Running).then(|| {
            p::CandidateId(format!(
                "orchestration-candidate:{}:{:?}",
                route.reference.0, outcome.status
            ))
        })
    }
}

type Description = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    bool,
);
fn description(topology: RouteTopology) -> Description {
    match topology {
        RouteTopology::Direct => (
            "Answer a bounded clear goal in the owner brain",
            &[],
            &["Use the main run", "Verify its result"],
            false,
        ),
        RouteTopology::Retrieve => (
            "Retrieve scoped evidence before answering",
            &[],
            &["Load bounded recall", "Cite evidence in the main answer"],
            false,
        ),
        RouteTopology::ToolLoop => (
            "Use the main governed tool loop",
            &[],
            &["Propose an action", "Enforce and verify each tool result"],
            false,
        ),
        RouteTopology::Clarify => (
            "Close a blocking information gap before action",
            &[],
            &[
                "Ask the owner",
                "Reframe after an answer; do not execute an ambiguous goal",
            ],
            true,
        ),
        RouteTopology::ReviewGate => (
            "Generate and independently evaluate",
            &["generator", "evaluator"],
            &[
                "Generate a result",
                "Review against the contract",
                "Escalate conflicts",
            ],
            true,
        ),
        RouteTopology::IndependentSlices => (
            "Research independent evidence and integrate",
            &["researcher-a", "researcher-b", "integrator"],
            &[
                "Fan out independent slices",
                "Join their verified summaries",
                "Preserve disagreement",
            ],
            true,
        ),
        RouteTopology::PlannerIntegrator => (
            "Plan, build, evaluate, then integrate",
            &["planner", "builder", "evaluator", "integrator"],
            &[
                "Establish a plan",
                "Build within its constraints",
                "Evaluate independently",
                "Integrate verified results",
            ],
            true,
        ),
        RouteTopology::LongRunning => (
            "Run one fresh-context segment between durable checkpoints",
            &["planner", "worker", "evaluator", "checkpoint"],
            &[
                "Plan the next segment",
                "Use a fresh child context",
                "Verify and checkpoint",
                "Return to the brain before replanning",
            ],
            true,
        ),
        RouteTopology::Custom => (
            "Construct a conservative route when seeds do not fit",
            &["generator", "evaluator"],
            &[
                "Keep low confidence",
                "Evaluate independently",
                "Checkpoint and retain a candidate",
            ],
            true,
        ),
    }
}
