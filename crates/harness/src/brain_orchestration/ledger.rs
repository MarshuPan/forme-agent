use super::support::empty_refs;
use crate::{now_ms, production::ProductionRuntime};
use forme_protocol as p;
use forme_store::EventStore;

impl ProductionRuntime {
    pub fn orchestration_state(
        &self,
        owner: &p::VerifiedPrincipal,
    ) -> p::Result<p::OrchestrationLedger> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        self.orchestration_snapshot()
    }

    pub(crate) fn orchestration_snapshot(&self) -> p::Result<p::OrchestrationLedger> {
        self.services()
            .store
            .orchestration_ledger(&self.services().aggregates.charter)
    }

    pub fn register_project(
        &self,
        owner: &p::VerifiedPrincipal,
        command: p::IdempotencyKey,
        project: p::WorkspaceProject,
    ) -> p::Result<p::EventId> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        self.validate_project_resources(&project)?;
        self.append_orchestration(
            command,
            p::OrchestrationOperation::ProjectDeclared(project),
            true,
            now_ms(),
        )
    }

    pub(crate) fn validate_project_resources(
        &self,
        project: &p::WorkspaceProject,
    ) -> p::Result<()> {
        let harness = self.harness();
        crate::brain_workspaces::check_workspace(
            &self.services().store,
            &self.config().workspace,
            &project.workspace,
            &project.scope,
        )?;
        if project.model != harness.config.model_profile
            || project
                .capabilities
                .iter()
                .any(|c| !harness.governance.visible_capabilities.contains(c))
            || (!project.capabilities.is_empty()
                && !harness
                    .governance
                    .envelope
                    .as_ref()
                    .is_some_and(|e| p::charter_scope_contains(&e.scope, &project.scope)))
        {
            return Err(p::Error(
                "project requests unavailable model, capability or delegated scope".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn append_orchestration(
        &self,
        command: p::IdempotencyKey,
        operation: p::OrchestrationOperation,
        owner: bool,
        now: p::Timestamp,
    ) -> p::Result<p::EventId> {
        let _serial = self
            .orchestration_commands
            .lock()
            .map_err(|_| p::Error("orchestration command queue is unavailable".into()))?;
        let store = self.services().store.clone();
        let run = self.services().aggregates.charter.clone();
        let id = p::EventId(format!(
            "orchestration-command:{}",
            p::canonical_digest(&(&run, &command))?.0
        ));
        // A durable command retry retains its original expected revision and
        // envelope. The transaction also deduplicates a concurrent retry.
        for event in store.read_run(run.clone()) {
            let event = event?;
            if event.event_id == id {
                if let p::EventPayload::DecisionTraceRecorded(payload) = event.payload {
                    if payload
                        .orchestration
                        .is_some_and(|f| f.operation == operation && f.command == command)
                    {
                        return Ok(id);
                    }
                }
                return Err(p::Error("orchestration command identity conflict".into()));
            }
        }
        let snapshot = self.orchestration_snapshot()?;
        let workspace = match &operation {
            p::OrchestrationOperation::ProjectDeclared(project)
            | p::OrchestrationOperation::ProjectRevised { project, .. } => {
                Some(project.workspace.clone())
            }
            p::OrchestrationOperation::RouteDeclared(route) => snapshot
                .projects
                .get(&route.project)
                .map(|p| p.workspace.clone()),
            p::OrchestrationOperation::NodeStarted { route, .. }
            | p::OrchestrationOperation::NodeFinished { route, .. }
            | p::OrchestrationOperation::NodeRetried { route, .. }
            | p::OrchestrationOperation::RouteCancelled { route }
            | p::OrchestrationOperation::ConflictReported { route, .. }
            | p::OrchestrationOperation::RouteClosed { route, .. }
            | p::OrchestrationOperation::Checkpoint { route, .. } => snapshot
                .routes
                .get(route)
                .and_then(|r| snapshot.projects.get(&r.declaration.project))
                .map(|p| p.workspace.clone()),
        }
        .ok_or_else(|| p::Error("orchestration operation has no registered workspace".into()))?;
        let revision = snapshot.revision;
        let fact = p::OrchestrationFact::seal(workspace, command, revision, operation)?;
        let provenance = if owner {
            p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            }
        } else {
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            }
        };
        store.append(p::Event::new(
            id,
            run,
            None,
            p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                trace_ref: p::DecisionTraceRef(format!("orchestration:{}", fact.digest.0)),
                refs: empty_refs(),
                rationale: p::Rationale(
                    "owner-brain orchestration command with versioned executable evidence".into(),
                ),
                workspace_snapshot: p::AgentWorkspaceSnapshotRef(format!(
                    "orchestration-revision:{revision}"
                )),
                resource_graph_snapshot: None,
                evolution_snapshot: None,
                federation_snapshot: None,
                orchestration: Some(fact),
            }),
            p::SchemaVersion(1),
            now,
            provenance,
        ))
    }
}
