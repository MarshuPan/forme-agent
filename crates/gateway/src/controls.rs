//! Gateway controls boundary; public facade remains in the crate root.
use super::*;

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessGatewayControl + Send + Sync,
{
    pub fn start_run(&self, request: p::RunRequest, auth: AuthContext) -> p::Result<p::RunId> {
        self.validate_run_submission(&request, auth)?;
        HarnessGatewayControl::start_run(self.harness.clone(), request)
    }

    pub fn control_profile(
        &self,
        surface: p::SurfaceRef,
        auth: AuthContext,
    ) -> p::Result<p::ControlProfile> {
        self.validate_owner_control(auth)?;
        let profile = p::ControlProfile {
            schema_version: p::SchemaVersion(1),
            owner: self.owner.clone(),
            gateway: HarnessGatewayControl::gateway_profile(self.harness.as_ref(), surface)?,
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn event_page(&self, cursor: p::EventCursor, auth: AuthContext) -> p::Result<p::EventPage> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::stream_event_page(self.harness.as_ref(), cursor)
    }

    pub fn list_runs(&self, auth: AuthContext) -> p::Result<Vec<p::RunSummary>> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::list_runs(self.harness.as_ref())
    }

    pub fn run_summary(&self, run: p::RunId, auth: AuthContext) -> p::Result<p::RunSummary> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::run_summary(self.harness.as_ref(), run)
    }

    pub fn pending_approvals(
        &self,
        session: p::SessionId,
        auth: AuthContext,
    ) -> p::Result<Vec<p::PendingApproval>> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::pending_approvals(self.harness.as_ref(), session)
    }

    pub fn control_run(
        &self,
        run: p::RunId,
        control: p::RunControl,
        auth: AuthContext,
    ) -> p::Result<()> {
        self.validate_owner_control(auth)?;
        if let p::RunControl::ResolveApproval(decision) = &control {
            if decision.approver != self.owner {
                return Err(p::Error(
                    "approval principal does not match the authenticated owner".into(),
                ));
            }
        }
        HarnessGatewayControl::control(self.harness.as_ref(), run, control)
    }

    pub fn trace_view(&self, run: p::RunId, auth: AuthContext) -> p::Result<p::TraceView> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::trace_view(self.harness.as_ref(), run)
    }

    pub fn review_candidate(
        &self,
        command: p::CandidateReviewCommand,
        auth: AuthContext,
    ) -> p::Result<()> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::review_candidate(self.harness.as_ref(), command)
    }

    pub(super) fn validate_owner_control(&self, auth: AuthContext) -> p::Result<()> {
        auth.validate(now_ms())?;
        match auth.principal {
            AuthPrincipal::Owner(owner) if owner == self.owner => Ok(()),
            _ => Err(p::Error(
                "gateway control requires the authenticated owner".into(),
            )),
        }
    }

    pub fn register_learning_goal(
        &self,
        command: p::IdempotencyKey,
        goal: p::GoalFrameRef,
        knowledge: p::GoalKnowledge,
        auth: AuthContext,
    ) -> p::Result<p::EventId> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("learning requires the production brain".into()))?
            .register_learning_goal(&self.owner, command, goal, knowledge)
    }

    pub fn workspace_charter(
        &self,
        workspace: p::WorkspaceRef,
        auth: AuthContext,
    ) -> p::Result<Option<p::WorkspaceCharterRecord>> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("charter requires the production brain".into()))?
            .workspace_charter(&self.owner, &workspace)
    }

    pub fn orchestration_state(&self, auth: AuthContext) -> p::Result<p::OrchestrationLedger> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("projects require the production brain".into()))?
            .orchestration_state(&self.owner)
    }
    pub fn register_project(
        &self,
        command: p::IdempotencyKey,
        project: p::WorkspaceProject,
        auth: AuthContext,
    ) -> p::Result<p::EventId> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("projects require the production brain".into()))?
            .register_project(&self.owner, command, project)
    }
    pub fn revise_project(
        &self,
        command: p::IdempotencyKey,
        project: p::WorkspaceProject,
        expected: p::SchemaDigest,
        auth: AuthContext,
    ) -> p::Result<p::EventId> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("projects require the production brain".into()))?
            .revise_project(&self.owner, command, project, expected)
    }
    pub fn plan_project_route(
        &self,
        command: p::IdempotencyKey,
        project: String,
        signature: p::TaskSignature,
        input: p::SchemaDigest,
        auth: AuthContext,
    ) -> p::Result<p::RouteDeclaration> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("projects require the production brain".into()))?
            .plan_project_route(&self.owner, command, &project, signature, input, now_ms())
    }
    pub fn execute_project_route(
        &self,
        route: p::ExecutionRouteRef,
        auth: AuthContext,
    ) -> p::Result<p::WorkRouteRecord> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("projects require the production brain".into()))?
            .execute_project_route(&self.owner, &route)
    }
    pub fn cancel_project_route(
        &self,
        command: p::IdempotencyKey,
        route: p::ExecutionRouteRef,
        auth: AuthContext,
    ) -> p::Result<()> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("projects require the production brain".into()))?
            .cancel_project_route(&self.owner, &route, command)
    }

    pub fn apply_workspace_charter(
        &self,
        command: p::IdempotencyKey,
        change: p::WorkspaceCharterChangedPayload,
        auth: AuthContext,
    ) -> p::Result<p::EventId> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("charter requires the production brain".into()))?
            .apply_workspace_charter(&self.owner, command, change)
    }

    pub fn agent_workspace(
        &self,
        query: forme_harness::AgentWorkspaceQuery,
        auth: AuthContext,
    ) -> p::Result<forme_harness::OwnerAgentWorkspace> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("attention requires the production brain".into()))?
            .agent_workspace(&self.owner, &query)
    }

    pub fn answer_learning(
        &self,
        command: p::IdempotencyKey,
        reply: p::LearningReplyRecord,
        auth: AuthContext,
    ) -> p::Result<p::CandidateId> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("learning requires the production brain".into()))?
            .answer_learning(&self.owner, command, reply)
    }

    pub fn resolve_learning(
        &self,
        command: p::IdempotencyKey,
        proposal: p::ProposalRef,
        outcome: p::ProposalOutcome,
        defer_until: Option<p::Timestamp>,
        auth: AuthContext,
    ) -> p::Result<p::EventId> {
        self.validate_owner_control(auth)?;
        self.production_runtime
            .as_ref()
            .ok_or_else(|| p::Error("learning requires the production brain".into()))?
            .resolve_learning(&self.owner, command, proposal, outcome, defer_until)
    }
}
