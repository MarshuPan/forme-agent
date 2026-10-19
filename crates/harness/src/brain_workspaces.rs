//! Workspaces partition one owner's brain; registration is not a permission grant.
use crate::{production::ProductionRuntime, PreparedSubmission};
use forme_protocol as p;
use forme_store::EventStore;
use forme_store::SqliteEventStore;

pub(crate) fn check_workspace(
    store: &SqliteEventStore,
    default: &p::WorkspaceRef,
    workspace: &p::WorkspaceRef,
    scope: &p::Scope,
) -> p::Result<()> {
    if !p::charter_scope_contains(&p::Scope(workspace.0.clone()), scope)
        || (*workspace != *default && store.workspace_charter(workspace)?.is_none())
    {
        return Err(p::Error(
            "workspace is unregistered or the requested scope crosses its boundary".into(),
        ));
    }
    Ok(())
}

impl ProductionRuntime {
    pub fn submit_workspace_run(
        &self,
        owner: &p::VerifiedPrincipal,
        workspace: &p::WorkspaceRef,
        mut request: p::RunRequest,
    ) -> p::Result<p::RunId> {
        let _guard = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        check_workspace(
            &self.services().store,
            &self.config().workspace,
            workspace,
            &p::Scope(workspace.0.clone()),
        )?;
        request.source = p::Source::UserTurn;
        let h = self.harness();
        match h.prepare_workspace_ingress(request, Vec::new(), None, workspace)? {
            PreparedSubmission::Existing(run) => Ok(run),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                h.drive_prepared_run(&run, &handle, &session)?;
                Ok(run)
            }
        }
    }
}

impl crate::ReactiveHarness {
    pub(crate) fn workspace_for_scope(&self, scope: &p::Scope) -> p::Result<p::WorkspaceRef> {
        let mut workspace = self.config.workspace.clone();
        if let Some(services) = &self.brain_services {
            for event in services.store.read_run(services.aggregates.charter.clone()) {
                if let p::EventPayload::WorkspaceCharterChanged(c) = event?.payload {
                    if p::charter_scope_contains(&p::Scope(c.charter.workspace.0.clone()), scope)
                        && (workspace == self.config.workspace
                            || c.charter.workspace.0.len() > workspace.0.len())
                    {
                        workspace = c.charter.workspace;
                    }
                }
            }
        }
        Ok(workspace)
    }
    pub(crate) fn check_workspace_action(
        &self,
        workspace: &p::WorkspaceRef,
        intent: &p::ActionIntent,
    ) -> p::Result<()> {
        if self.brain_services.is_some()
            && (!p::charter_scope_contains(&p::Scope(workspace.0.clone()), &intent.scope)
                || !p::charter_scope_contains(
                    &p::Scope(self.config.workspace.0.clone()),
                    &intent.scope,
                ))
        {
            return Err(p::Error(
                "workspace registration cannot widen an execution grant".into(),
            ));
        }
        Ok(())
    }
}
