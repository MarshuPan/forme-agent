use super::support::{empty_refs, signature_from_protocol};
use crate::production::ProductionRuntime;
use forme_coordination::{self as c, OrchestrationLibrary};
use forme_protocol as p;
use forme_store::EventStore;

impl ProductionRuntime {
    pub fn plan_project_route(
        &self,
        owner: &p::VerifiedPrincipal,
        command: p::IdempotencyKey,
        project: &str,
        signature: p::TaskSignature,
        input_digest: p::SchemaDigest,
        now: p::Timestamp,
    ) -> p::Result<p::RouteDeclaration> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        self.plan_project_segment(command, project, signature, input_digest, None, now)
    }

    pub(crate) fn plan_project_segment(
        &self,
        command: p::IdempotencyKey,
        project: &str,
        signature: p::TaskSignature,
        input_digest: p::SchemaDigest,
        review_snapshot: Option<p::SchemaDigest>,
        now: p::Timestamp,
    ) -> p::Result<p::RouteDeclaration> {
        let state = self.orchestration_snapshot()?;
        let spec = state
            .projects
            .get(project)
            .ok_or_else(|| p::Error("project is not declared".into()))?;
        let route_key = p::canonical_digest(&(&spec.workspace, &command))?.0;
        let route_ref = p::ExecutionRouteRef(format!("project-route:{route_key}"));
        if let Some(record) = state.routes.get(&route_ref) {
            if record.declaration.project != project
                || record.declaration.signature != signature
                || record.declaration.input_digest != input_digest
            {
                return Err(p::Error("route planning command identity conflict".into()));
            }
            self.ensure_route_markers(&record.declaration)?;
            return Ok(record.declaration.clone());
        }
        let spec = state
            .projects
            .get(project)
            .ok_or_else(|| p::Error("project is not declared".into()))?;
        self.validate_project_resources(spec)?;
        let charter = self
            .services()
            .store
            .workspace_charter(&spec.workspace)?
            .ok_or_else(|| p::Error("project has no charter".into()))?;
        let rules = p::CharterRules::parse(&charter)?;
        let (library, active_coordination) =
            self.project_orchestration_library(spec, &signature, &state)?;
        let selection = library.select(&signature_from_protocol(&signature));
        let parent = p::RunId(format!("project-segment:{route_key}"));
        let profile = c::SubagentProfile {
            schema_version: p::SchemaVersion(1),
            role: p::RoleRef("executor".into()),
            toolset: self.project_toolset(spec)?,
            model: spec.model.clone(),
            permission: spec.scope.clone(),
            budget: p::Budget(format!("units:{}", spec.node_units)),
        };
        let mut envelope = self
            .harness()
            .governance
            .envelope
            .clone()
            .ok_or_else(|| p::Error("project has no delegation".into()))?;
        envelope.scope = spec.scope.clone();
        envelope.capability.capabilities = spec.capabilities.clone();
        envelope.budget = p::Budget(format!("units:{}", spec.budget_units));
        let ctx = c::CoordinationContext {
            schema_version: p::SchemaVersion(1),
            situation: c::SituationModel {
                schema_version: p::SchemaVersion(1),
                known: vec![c::Fact {
                    schema_version: p::SchemaVersion(1),
                    reference: p::EvidenceRef(input_digest.0.clone()),
                    statement: "fixed owner project snapshot".into(),
                }],
                missing: vec![],
            },
            inventory: c::ResourceInventory {
                schema_version: p::SchemaVersion(1),
                tools: spec.capabilities.clone(),
                skills: vec![],
                mcp: vec![],
                subagents: vec![profile],
                trusted: spec.capabilities.iter().cloned().collect(),
            },
            done_contract: c::DoneContract::final_output(spec.done.clone()),
            autonomy_envelope: envelope,
            decision_refs: empty_refs(),
            workspace_snapshot: c::AgentWorkspaceSnapshot {
                schema_version: p::SchemaVersion(1),
                reference: p::AgentWorkspaceSnapshotRef(input_digest.0.clone()),
                event_refs: vec![],
            },
            resource_graph: None,
            resource_required: !spec.capabilities.is_empty(),
        };
        let goal = c::GoalInput {
            schema_version: p::SchemaVersion(1),
            goal: c::Goal {
                schema_version: p::SchemaVersion(1),
                reference: spec.goal.clone(),
                summary: spec.summary.clone(),
            },
            constraints: charter.constraints.clone(),
        };
        let frame = self.services().coordination.frame(goal, &ctx);
        self.services().coordination.plan(&frame)?;
        let mut route = library.route(selection.pattern.clone(), &frame);
        if let Some(active) = active_coordination {
            if route.nodes.len() > usize::from(active.max_subagents) {
                return Err(p::Error(
                    "active coordination policy limits subagent count".into(),
                ));
            }
            // An active policy cannot be bypassed through custom fallback or
            // silently remove mandatory dependency/review nodes.
            route = c::apply_coordination_strategy(&active, &route)?.route;
        }
        route.reference = route_ref;
        if rules.max_nodes.is_some_and(|max| route.nodes.len() > max) {
            return Err(p::Error("charter limits route node count".into()));
        }
        if spec
            .capabilities
            .iter()
            .any(|cap| !rules.permits_capability(cap))
        {
            return Err(p::Error("charter blocks project resources".into()));
        }
        let make_node = |id: String, role: p::RoleRef, instruction: String| p::WorkNode {
            schema_version: p::SchemaVersion(1),
            intent: p::ActionId(format!("{}:{id}:attempt:1", route.reference.0)),
            id,
            role,
            instruction,
            scope: spec.scope.clone(),
            capabilities: spec.capabilities.clone(),
            model: spec.model.clone(),
            budget_units: spec.node_units,
            max_attempts: 2,
            requirements: spec.requirements.clone(),
        };
        let nodes = route
            .nodes
            .iter()
            .map(|n| {
                make_node(
                    n.subtask.id.clone(),
                    n.role.role.clone(),
                    n.subtask.instruction.clone(),
                )
            })
            .collect::<Vec<_>>();
        let declaration = p::RouteDeclaration {
            schema_version: p::SchemaVersion(1),
            route: route.reference.clone(),
            project: project.into(),
            parent,
            goal: spec.goal.clone(),
            done: spec.done.clone(),
            pattern: route.pattern_ref,
            signature,
            scores: selection
                .scores
                .iter()
                .map(|s| p::RouteSelectionScore {
                    schema_version: p::SchemaVersion(1),
                    pattern: s.pattern.clone(),
                    dimensions: s.dimensions,
                    anchors: s.anchors,
                    fitness: s.fitness,
                    total: s.total,
                    compatible: s.hard_compatible,
                })
                .collect(),
            threshold: selection.threshold,
            confidence_millis: (selection.confidence.0 * 1000.0) as u16,
            checkpoint_required: selection.checkpoint_required || rules.require_review,
            budget_units: spec.budget_units,
            nodes,
            main: make_node(
                "main".into(),
                p::RoleRef("owner-brain".into()),
                spec.summary.clone(),
            ),
            dependencies: route
                .edges
                .iter()
                .map(|e| p::WorkDependency {
                    schema_version: p::SchemaVersion(1),
                    from: e.from.clone(),
                    to: e.to.clone(),
                    on_failure: match e.on_failure {
                        c::DependencyFailurePolicy::AbortRoute => p::RouteFailurePolicy::Abort,
                        c::DependencyFailurePolicy::Continue => p::RouteFailurePolicy::Continue,
                        c::DependencyFailurePolicy::Replan => p::RouteFailurePolicy::Replan,
                    },
                })
                .collect(),
            charter_digest: charter.digest.clone(),
            input_digest,
            review_digest: match review_snapshot {
                Some(digest) => digest,
                None => c::WorkspaceSupervisor::input_digest(
                    spec,
                    &charter,
                    &self.owner_brain_events_at(None)?,
                    now,
                )?,
            },
            created_at: now,
        };
        self.append_orchestration(
            command,
            p::OrchestrationOperation::RouteDeclared(Box::new(declaration.clone())),
            false,
            now,
        )?;
        self.ensure_route_markers(&declaration)?;
        Ok(declaration)
    }

    pub(super) fn ensure_route_markers(&self, declaration: &p::RouteDeclaration) -> p::Result<()> {
        let store = self.services().store.clone();
        let run = declaration.parent.clone();
        for (suffix, payload) in [
            (
                "goal",
                p::EventPayload::GoalFramed(p::GoalFramedPayload {
                    goal_frame: p::GoalFrameRef(declaration.goal.0.clone()),
                    long_term: None,
                    knowledge: None,
                }),
            ),
            (
                "done",
                p::EventPayload::DoneContractSet(p::DoneContractSetPayload {
                    contract: declaration.done.clone(),
                }),
            ),
            (
                "route",
                p::EventPayload::OrchestrationRouteCreated(p::OrchestrationRouteCreatedPayload {
                    pattern_ref: declaration.pattern.clone(),
                    route: declaration.route.clone(),
                    goal_frame: Some(p::GoalFrameRef(declaration.goal.0.clone())),
                    checkpoint: None,
                }),
            ),
        ] {
            store.append(p::Event::new(
                p::EventId(format!("{}:{suffix}", declaration.route.0)),
                run.clone(),
                None,
                payload,
                p::SchemaVersion(1),
                declaration.created_at,
                crate::internal_provenance(p::Source::Internal),
            ))?;
        }
        Ok(())
    }
}
