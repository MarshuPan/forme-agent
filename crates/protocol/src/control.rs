use serde::{Deserialize, Serialize};

use crate::{
    Actor, ApprovalChoice, ApprovalId, ApprovalOutcome, AutonomyEnvelope, Budget, CandidateId,
    DurationMs, Error, EvalCaseRef, EvalRef, Event, EventId, EventKind, IntentionId,
    IntentionSource, IntentionTriggerRef, LineageRef, ModelProfileRef, Nonce, ObjectRef,
    PermissionRef, PlanDigest, PolicyProfileRef, Provenance, ReplaySnapshotRef, ResourceRef,
    Result, RollbackBoundary, RubricRef, RunId, RunRequest, RunResult, RunStatus, SchemaVersion,
    Scope, SeedRef, SessionId, Source, SurfaceRef, Timestamp, ToolsetRef, TrustTier,
    VerificationOutcome, VerifiedPrincipal, Version, WorkspaceRef,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SurfaceKind {
    Cli,
    LocalWeb,
    LocalNotification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceProfile {
    pub schema_version: SchemaVersion,
    pub surface: SurfaceRef,
    pub kind: SurfaceKind,
    pub trust: TrustTier,
    pub scope: Scope,
}

impl SurfaceProfile {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.surface.0.trim().is_empty()
            || self.scope.0.trim().is_empty()
        {
            return Err(Error("surface profile is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayProfile {
    pub schema_version: SchemaVersion,
    pub surface: SurfaceRef,
    pub policy: PolicyProfileRef,
    pub model: ModelProfileRef,
    pub toolset: ToolsetRef,
    pub workspace: WorkspaceRef,
}

impl GatewayProfile {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.surface.0.trim().is_empty()
            || self.policy.0.trim().is_empty()
            || self.model.0.trim().is_empty()
            || self.toolset.0.trim().is_empty()
            || self.workspace.0.trim().is_empty()
        {
            return Err(Error("gateway profile is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlProfile {
    pub schema_version: SchemaVersion,
    pub owner: VerifiedPrincipal,
    pub gateway: GatewayProfile,
}

impl ControlProfile {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.owner.0.trim().is_empty() {
            return Err(Error("control profile is incomplete".into()));
        }
        self.gateway.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventCursor {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub after_stream_seq: u64,
}

impl EventCursor {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.run.0.trim().is_empty()
            || self.after_stream_seq == u64::MAX
        {
            return Err(Error("event cursor is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventPage {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub after_stream_seq: u64,
    pub snapshot_upper_bound: u64,
    pub events: Vec<Event>,
}

impl EventPage {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.run.0.trim().is_empty()
            || self.snapshot_upper_bound < self.after_stream_seq
        {
            return Err(Error("event page boundary is invalid".into()));
        }
        let mut expected = self
            .after_stream_seq
            .checked_add(1)
            .ok_or_else(|| Error("event page cursor is exhausted".into()))?;
        for event in &self.events {
            if event.run_id != self.run
                || event.stream_seq != expected
                || event.stream_seq > self.snapshot_upper_bound
            {
                return Err(Error(
                    "event page is not a contiguous run cursor slice".into(),
                ));
            }
            expected = expected
                .checked_add(1)
                .ok_or_else(|| Error("event page sequence is exhausted".into()))?;
        }
        let returned_upper_bound = self
            .events
            .last()
            .map(|event| event.stream_seq)
            .unwrap_or(self.after_stream_seq);
        if returned_upper_bound != self.snapshot_upper_bound {
            return Err(Error("event page does not reach its snapshot bound".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub source: Source,
    pub session: SessionId,
    pub workspace: Option<WorkspaceRef>,
    pub status: RunStatus,
    pub last_stream_seq: u64,
    pub result: Option<RunResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub session: SessionId,
    pub approval_id: ApprovalId,
    pub action_summary: String,
    pub risk_level: crate::Risk,
    pub scope: Scope,
    pub requested_permissions: Vec<PermissionRef>,
    pub affected_resources: Vec<ResourceRef>,
    pub rollback_boundary: RollbackBoundary,
    pub expires_at: Timestamp,
    pub choices: Vec<ApprovalChoice>,
    pub plan_digest: PlanDigest,
    pub policy_version: Version,
    pub tool_schema_version: Version,
}

impl PendingApproval {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.run.0.trim().is_empty()
            || self.session.0.trim().is_empty()
            || self.approval_id.0.trim().is_empty()
            || self.action_summary.trim().is_empty()
            || self.scope.0.trim().is_empty()
            || self.choices.is_empty()
            || self.plan_digest.0.trim().is_empty()
            || self.policy_version.0 == 0
            || self.tool_schema_version.0 == 0
        {
            return Err(Error("pending approval is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceView {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub snapshot_upper_bound: u64,
    pub events: Vec<Event>,
    pub failure_refs: Vec<crate::FailureEvidenceRef>,
    pub verification_outcomes: Vec<VerificationOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub schema_version: SchemaVersion,
    pub approval_id: ApprovalId,
    pub outcome: ApprovalOutcome,
    pub approver: VerifiedPrincipal,
    pub bound_plan_digest: PlanDigest,
    pub policy_version: Version,
    pub tool_schema_version: Version,
    pub nonce: Nonce,
    pub use_by: Timestamp,
}

impl ApprovalDecision {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.approval_id.0.trim().is_empty()
            || self.approver.0.trim().is_empty()
            || self.bound_plan_digest.0.trim().is_empty()
            || self.nonce.0.trim().is_empty()
            || self.policy_version.0 == 0
            || self.tool_schema_version.0 == 0
            || self.use_by <= 0
        {
            return Err(Error("approval decision is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelRequest {
    pub schema_version: SchemaVersion,
    pub reason: crate::ReasonRef,
}

impl CancelRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.reason.0.trim().is_empty() {
            return Err(Error("cancel request is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunControl {
    ResolveApproval(ApprovalDecision),
    Cancel(CancelRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentionTrigger {
    At(Timestamp),
    OnEvent(EventKind),
    OnCondition(IntentionTriggerRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentionState {
    Pending,
    Fired,
    Done,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProspectiveIntention {
    pub schema_version: SchemaVersion,
    pub id: IntentionId,
    pub source: IntentionSource,
    pub trigger: IntentionTrigger,
    pub state: IntentionState,
    pub seed: SeedRef,
    pub provenance: Provenance,
    pub expires_at: Option<Timestamp>,
}

impl ProspectiveIntention {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.id.0.trim().is_empty()
            || self.seed.0.trim().is_empty()
            || self.state != IntentionState::Pending
            || self.expires_at.is_some_and(|expires_at| expires_at <= 0)
            || matches!(
                &self.trigger,
                IntentionTrigger::OnCondition(reference) if reference.0.trim().is_empty()
            )
        {
            return Err(Error("prospective intention is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleBinding {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub envelope: AutonomyEnvelope,
    pub budget: Budget,
}

impl ScheduleBinding {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.session.0.trim().is_empty()
            || self.envelope.schema_version.0 == 0
            || self.envelope.scope.0.trim().is_empty()
            || self.envelope.capability.schema_version.0 == 0
            || self.envelope.timebox.schema_version.0 == 0
            || self.envelope.rollback.schema_version.0 == 0
            || self.envelope.timebox.max_turns == 0
            || self.envelope.timebox.starts_at > self.envelope.timebox.expires_at
            || self.budget.0.trim().is_empty()
        {
            return Err(Error("schedule binding is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleCommand {
    pub schema_version: SchemaVersion,
    pub intention: ProspectiveIntention,
    pub session: SessionId,
    pub envelope: AutonomyEnvelope,
    pub budget: Budget,
}

impl ScheduleCommand {
    pub fn binding(&self) -> ScheduleBinding {
        ScheduleBinding {
            schema_version: self.schema_version,
            session: self.session.clone(),
            envelope: self.envelope.clone(),
            budget: self.budget.clone(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("schedule command is not versioned".into()));
        }
        self.intention.validate()?;
        self.binding().validate()?;
        if self
            .intention
            .expires_at
            .is_some_and(|expires_at| expires_at > self.envelope.timebox.expires_at)
        {
            return Err(Error(
                "intention expiry exceeds its autonomy timebox".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerConfig {
    pub schema_version: SchemaVersion,
    pub tick: DurationMs,
    pub lease: DurationMs,
    pub max_claims_per_tick: u32,
}

impl SchedulerConfig {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.tick.0 == 0
            || self.lease.0 == 0
            || self.max_claims_per_tick == 0
        {
            return Err(Error("scheduler configuration is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleClaim {
    pub schema_version: SchemaVersion,
    pub command: ScheduleCommand,
    pub lease_until: Timestamp,
    pub claim_event: EventId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub schema_version: SchemaVersion,
    pub intention: ProspectiveIntention,
    pub binding: ScheduleBinding,
    pub lease_until: Option<Timestamp>,
    pub run: Option<RunId>,
    pub run_status: Option<RunStatus>,
    pub manual_review: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerTickReport {
    pub schema_version: SchemaVersion,
    pub at: Timestamp,
    pub claimed: Vec<IntentionId>,
    pub started_runs: Vec<RunId>,
    pub deferred: Vec<IntentionId>,
    pub resolved: Vec<IntentionId>,
    pub manual_review: Vec<RunId>,
    pub follow_up_runs: Vec<RunId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub schema_version: SchemaVersion,
    pub at: Timestamp,
    pub reclaimable: Vec<IntentionId>,
    pub coalesced: Vec<IntentionId>,
    pub manual_review: Vec<RunId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateReviewState {
    Candidate,
    Promoted,
    Rejected,
    Downgraded,
    Decayed,
    Retracted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateReviewDecision {
    Promote,
    Reject,
    Downgrade,
    Retract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateRetraction {
    pub schema_version: SchemaVersion,
    pub target: ObjectRef,
    pub lineage: LineageRef,
    pub derived_refs: Vec<ObjectRef>,
}

impl CandidateRetraction {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.target.0.trim().is_empty()
            || self.lineage.0.trim().is_empty()
            || self.derived_refs.is_empty()
            || self
                .derived_refs
                .iter()
                .any(|reference| reference.0.trim().is_empty())
        {
            return Err(Error("candidate retraction is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateReviewCommand {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub candidate: CandidateId,
    pub expected_state: CandidateReviewState,
    pub decision: CandidateReviewDecision,
    pub actor: Actor,
    pub evidence: Vec<crate::EvidenceRef>,
    pub retraction: Option<CandidateRetraction>,
}

impl CandidateReviewCommand {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.run.0.trim().is_empty()
            || self.candidate.0.trim().is_empty()
            || self.actor != Actor::Owner
            || self.evidence.is_empty()
            || self
                .evidence
                .iter()
                .any(|evidence| evidence.0.trim().is_empty())
        {
            return Err(Error("candidate review command is not owner-bound".into()));
        }
        match (&self.decision, &self.retraction) {
            (CandidateReviewDecision::Retract, Some(retraction)) => retraction.validate()?,
            (CandidateReviewDecision::Retract, None) => {
                return Err(Error("retract review is missing lineage".into()))
            }
            (_, Some(_)) => return Err(Error("only retract review may carry lineage".into())),
            (_, None) => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GoldenTaskKind {
    FinalOnly,
    ToolApproval,
    LongContext,
    BackgroundProactive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualEvalCase {
    pub schema_version: SchemaVersion,
    pub case_ref: EvalCaseRef,
    pub kind: GoldenTaskKind,
    pub request: RunRequest,
    pub workspace: WorkspaceRef,
    pub done_contract: crate::DoneContractRef,
    pub allowed_capabilities: Vec<crate::CapabilityRef>,
    pub policy: PolicyProfileRef,
    pub rubric: RubricRef,
    pub required_events: Vec<crate::EventKind>,
    pub forbidden_events: Vec<crate::EventKind>,
}

impl ManualEvalCase {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.case_ref.0.trim().is_empty()
            || self.request.schema_version.0 == 0
            || self.request.session.0.trim().is_empty()
            || self.request.agent_profile.0.trim().is_empty()
            || self.request.input.0.trim().is_empty()
            || self
                .request
                .idempotency_key
                .as_ref()
                .is_none_or(|key| key.0.trim().is_empty())
            || self.workspace.0.trim().is_empty()
            || self.done_contract.0.trim().is_empty()
            || self.policy.0.trim().is_empty()
            || self.rubric.0.trim().is_empty()
            || self.required_events.is_empty()
            || self
                .required_events
                .iter()
                .any(|kind| self.forbidden_events.contains(kind))
        {
            return Err(Error("manual eval case is incomplete".into()));
        }
        let source_matches = match self.kind {
            GoldenTaskKind::FinalOnly
            | GoldenTaskKind::ToolApproval
            | GoldenTaskKind::LongContext => self.request.source == Source::UserTurn,
            GoldenTaskKind::BackgroundProactive => {
                matches!(self.request.source, Source::Schedule | Source::ProactiveJob)
            }
        };
        if !source_matches {
            return Err(Error(
                "manual eval case source does not match its kind".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalProfile {
    pub schema_version: SchemaVersion,
    pub eval_ref: EvalRef,
    pub model: ModelProfileRef,
    pub policy: PolicyProfileRef,
    pub toolset: ToolsetRef,
    pub workspace: WorkspaceRef,
    pub event_schema: SchemaVersion,
    pub replay_snapshot: ReplaySnapshotRef,
}

impl EvalProfile {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.eval_ref.0.trim().is_empty()
            || self.model.0.trim().is_empty()
            || self.policy.0.trim().is_empty()
            || self.toolset.0.trim().is_empty()
            || self.workspace.0.trim().is_empty()
            || self.event_schema.0 == 0
            || self.replay_snapshot.0.trim().is_empty()
        {
            return Err(Error("manual eval profile is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualEvalRequest {
    pub schema_version: SchemaVersion,
    pub case: ManualEvalCase,
    pub profile: EvalProfile,
}

impl ManualEvalRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("manual eval request is not versioned".into()));
        }
        self.case.validate()?;
        self.profile.validate()?;
        if self.case.policy != self.profile.policy || self.case.workspace != self.profile.workspace
        {
            return Err(Error(
                "manual eval request case does not match its profile".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualEvalReport {
    pub schema_version: SchemaVersion,
    pub eval_ref: EvalRef,
    pub case_ref: EvalCaseRef,
    pub run: RunId,
    pub trace_refs: Vec<EventId>,
    pub outcome: VerificationOutcome,
    pub rubric: RubricRef,
    pub snapshot: ReplaySnapshotRef,
    pub profile: EvalProfile,
}

impl ManualEvalReport {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.eval_ref.0.trim().is_empty()
            || self.case_ref.0.trim().is_empty()
            || self.run.0.trim().is_empty()
            || self.trace_refs.is_empty()
            || self
                .trace_refs
                .iter()
                .any(|reference| reference.0.trim().is_empty())
            || self.rubric.0.trim().is_empty()
            || self.snapshot.0.trim().is_empty()
        {
            return Err(Error("manual eval report is incomplete".into()));
        }
        self.profile.validate()?;
        if self.eval_ref != self.profile.eval_ref || self.snapshot != self.profile.replay_snapshot {
            return Err(Error(
                "manual eval report snapshot does not match its profile".into(),
            ));
        }
        Ok(())
    }
}
