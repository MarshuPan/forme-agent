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
        self.require_brain_owner(owner)?;
        self.validate_project_resources(&project)?;
        if project.workspace != self.config().workspace {
            return Err(p::Error("project revision crosses workspace".into()));
        }
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
        let Some(charter) = events.iter().rev().find_map(|e| match &e.payload {
            p::EventPayload::WorkspaceCharterChanged(p)
                if p.charter.workspace == self.config().workspace =>
            {
                Some(p.charter.clone())
            }
            _ => None,
        }) else {
            return Ok(Vec::new());
        };
        if self
            .services()
            .store
            .workspace_charter(&charter.workspace)?
            .as_ref()
            .map(|c| &c.digest)
            != Some(&charter.digest)
        {
            return Ok(Vec::new());
        }
        let before = self.services().store.run_head(&charter_stream)?;
        for review in forme_coordination::WorkspaceSupervisor::reviews(
            &snapshot,
            &charter,
            &events,
            claim.window,
        )? {
            let current = self.orchestration_snapshot()?;
            // Late foreground project changes and in-flight segments are left
            // to the next snapshot; the tick cannot overwrite either.
            if current.projects.get(&review.project) != snapshot.projects.get(&review.project)
                || current.routes.values().any(|r| {
                    r.declaration.project == review.project && (!r.closed || r.has_inflight())
                })
            {
                continue;
            }
            self.plan_project_segment(
                p::IdempotencyKey(format!(
                    "workspace-review:{}:{}:{}",
                    claim.window, review.project, review.input_digest.0
                )),
                &review.project,
                review.signature,
                review.input_digest.clone(),
                Some(review.input_digest),
                claim.window,
            )?;
        }
        self.services()
            .store
            .read_run(charter_stream)
            .after(before)
            .map(|e| e.map(|e| e.event_id))
            .collect()
    }
}
