//! Project review is part of the same persistent owner tick, never another brain.
use crate::{now_ms, production::ProductionRuntime};
use forme_protocol as p;
use forme_store::EventStore;

impl ProductionRuntime {
    pub fn revise_project(
        &self,
        owner: &p::VerifiedPrincipal,
        command: p::IdempotencyKey,
        project: p::WorkspaceProject,
        expected_digest: p::SchemaDigest,
    ) -> p::Result<p::EventId> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        self.validate_project_resources(&project)?;
        self.append_orchestration(
            command,
            p::OrchestrationOperation::ProjectRevised {
                project,
                expected_digest,
            },
            true,
            now_ms(),
        )
    }
    pub(crate) fn supervise_workspace_snapshot(
        &self,
        claim: &forme_store::CognitiveTickClaim,
    ) -> p::Result<Vec<p::EventId>> {
        if self.harness().foreground_active()? {
            return Ok(Vec::new());
        }
        let events = self.owner_brain_events_at(Some(&claim.range.to))?;
        let mut snapshot = p::OrchestrationLedger::default();
        let charter_stream = self.services().aggregates.charter.clone();
        for event in events.iter().filter(|e| e.run_id == charter_stream) {
            snapshot.fold(event)?;
        }
        if snapshot.projects.is_empty() {
            return Ok(Vec::new());
        }
        let mut charters = std::collections::BTreeMap::new();
        for event in events.iter().filter(|e| e.run_id == charter_stream) {
            if let p::EventPayload::WorkspaceCharterChanged(p) = &event.payload {
                charters.insert(p.charter.workspace.clone(), p.charter.clone());
            }
        }
        let before = self.services().store.run_head(&charter_stream)?;
        let restoration = self.services().store.restored_execution_fence()?;
        for charter in charters.values() {
            if self
                .services()
                .store
                .workspace_charter(&charter.workspace)?
                .as_ref()
                .map(|c| &c.digest)
                != Some(&charter.digest)
            {
                continue;
            }
            for review in forme_coordination::WorkspaceSupervisor::reviews(
                &snapshot,
                charter,
                &events,
                claim.window,
            )? {
                // Project knowledge survives; only an owner-planned new route
                // grants fresh authority to continue this project's work.
                if let Some(fence) = &restoration {
                    let fresh_route = snapshot.routes.values().any(|r| {
                        r.declaration.project == review.project
                            && !fence.contains(&r.declaration.parent)
                    });
                    if !fresh_route {
                        continue;
                    }
                }
                let current = self.orchestration_snapshot()?;
                // Late foreground project changes and in-flight segments are left
                // to the next snapshot; the tick cannot overwrite either.
                if !current.project_supervision(&review.project).enabled
                    || current.projects.get(&review.project)
                        != snapshot.projects.get(&review.project)
                    || current.routes.values().any(|r| {
                        r.declaration.project == review.project && (!r.closed || r.has_inflight())
                    })
                {
                    continue;
                }
                let result = self.plan_project_segment(
                    p::IdempotencyKey(format!(
                        "workspace-review:{}:{}:{}",
                        claim.window, review.project, review.input_digest.0
                    )),
                    &review.project,
                    review.signature,
                    review.input_digest.clone(),
                    Some(review.input_digest),
                    claim.window,
                );
                if let Err(error) = result {
                    if error.0 != "project background planning became stale" {
                        return Err(error);
                    }
                }
            }
        }
        self.services()
            .store
            .read_run(charter_stream)
            .after(before)
            .map(|e| e.map(|e| e.event_id))
            .collect()
    }
}
