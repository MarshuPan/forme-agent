//! Read-only authoritative project views and atomic owner supervision controls.
use super::*;
use forme_coordination::WorkspaceSupervisor;

impl ProductionRuntime {
    /// Records one CAS-protected supervision change, without planning or driving
    /// any route. An exact retry returns the original event, even after edits.
    pub fn control_project_supervision(
        &self,
        owner: &p::VerifiedPrincipal,
        command: p::ProjectSupervisionCommand,
    ) -> p::Result<p::WorkbenchCommandReceipt> {
        let _data = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        command.validate()?;
        let binding = p::canonical_digest(&(owner, &command))?;
        let event = self.append_orchestration(
            command.command.clone(),
            p::OrchestrationOperation::ProjectSupervisionChanged {
                project: command.project,
                expected_project_digest: command.expected_project_digest,
                expected_version: command.expected_version,
                enabled: command.enabled,
            },
            true,
            now_ms(),
        )?;
        Ok(p::WorkbenchCommandReceipt {
            schema_version: p::SchemaVersion(1),
            command: command.command,
            binding,
            status: p::WorkbenchCommandStatus::Confirmed,
            event,
        })
    }

    pub fn workspace_control_snapshot(
        &self,
        owner: &p::VerifiedPrincipal,
    ) -> p::Result<p::WorkspaceControlSnapshot> {
        let _data = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        let store = &self.services().store;
        let heads = store.run_heads()?;
        let state = self.orchestration_snapshot()?;
        if state.projects.len() > 256 {
            return Err(p::Error(
                "workspace control snapshot exceeds its bounded capacity".into(),
            ));
        }
        let events = self.owner_brain_events_at(None)?;
        let mut workspaces = BTreeSet::from([self.config().workspace.clone()]);
        for event in &events {
            if event.run_id == self.services().aggregates.charter {
                if let p::EventPayload::WorkspaceCharterChanged(change) = &event.payload {
                    workspaces.insert(change.charter.workspace.clone());
                }
            }
        }
        workspaces.extend(
            state
                .projects
                .values()
                .map(|project| project.workspace.clone()),
        );
        if workspaces.len() > 256 {
            return Err(p::Error(
                "workspace control snapshot exceeds its bounded capacity".into(),
            ));
        }
        let observed_at = now_ms();
        let restoration = store.restored_execution_fence()?;
        let mut projects = Vec::new();
        for workspace in &workspaces {
            let charter = store.workspace_charter(workspace)?;
            let reviews = charter
                .as_ref()
                .map(|c| WorkspaceSupervisor::reviews(&state, c, &events, observed_at))
                .transpose()?;
            for project in state
                .projects
                .values()
                .filter(|p| p.workspace == *workspace)
            {
                crate::brain_workspaces::check_workspace(
                    store,
                    &self.config().workspace,
                    workspace,
                    &project.scope,
                )?;
                let supervision = state.project_supervision(&project.reference);
                let checkpoint = state.checkpoints.get(&project.reference).cloned();
                let active_routes: Vec<_> = state
                    .routes
                    .values()
                    .filter(|r| {
                        r.declaration.project == project.reference
                            && (!r.closed || r.has_inflight() || !r.checkpointed)
                    })
                    .map(|r| r.declaration.route.clone())
                    .collect();
                let review = reviews
                    .as_ref()
                    .and_then(|r| r.iter().find(|r| r.project == project.reference));
                let unassessed = reviews.as_ref().is_some_and(|r| {
                    r.len() == 16
                        && r.last()
                            .is_some_and(|last| project.reference > last.project)
                });
                let restore_blocked = restoration.as_ref().is_some_and(|fence| {
                    !state.routes.values().any(|r| {
                        r.declaration.project == project.reference
                            && !fence.contains(&r.declaration.parent)
                    })
                });
                let (status, reasons) = if !supervision.enabled {
                    (p::ProjectReviewStatus::Paused, vec!["owner-paused".into()])
                } else if !active_routes.is_empty() {
                    (
                        p::ProjectReviewStatus::ActiveRoute,
                        vec!["segment-awaits-completion-or-checkpoint".into()],
                    )
                } else if restore_blocked {
                    (
                        p::ProjectReviewStatus::Unverifiable,
                        vec!["restored-project-awaits-owner-route".into()],
                    )
                } else if let Some(review) = review {
                    (p::ProjectReviewStatus::Due, review.reasons.clone())
                } else if reviews.is_none() || unassessed {
                    (
                        p::ProjectReviewStatus::Unverifiable,
                        vec![if unassessed {
                            "review-capacity-reached"
                        } else {
                            "charter-unavailable"
                        }
                        .into()],
                    )
                } else {
                    (
                        p::ProjectReviewStatus::NotDue,
                        vec!["no-new-review-trigger".into()],
                    )
                };
                let cadence = charter
                    .as_ref()
                    .and_then(|c| c.review_cadence)
                    .map_or(project.cadence_ms, |c| c.0.min(project.cadence_ms));
                projects.push(p::WorkspaceProjectControlView {
                    schema_version: p::SchemaVersion(1),
                    project: project.reference.clone(),
                    workspace: workspace.clone(),
                    project_digest: p::canonical_digest(project)?,
                    supervision,
                    next_cadence: checkpoint.as_ref().map(|c| {
                        c.at.saturating_add(i64::try_from(cadence).unwrap_or(i64::MAX))
                    }),
                    checkpoint,
                    active_routes,
                    review: status,
                    reasons,
                    evidence: review.map_or_else(Vec::new, |r| r.evidence.clone()),
                });
            }
        }
        if store.run_heads()? != heads {
            return Err(p::Error(
                "workspace control snapshot changed; reload status".into(),
            ));
        }
        Ok(p::WorkspaceControlSnapshot {
            schema_version: p::SchemaVersion(1),
            observed_at,
            revision: state.revision,
            workspaces: workspaces.into_iter().collect(),
            projects,
            stream_heads: heads.0,
        })
    }
}
