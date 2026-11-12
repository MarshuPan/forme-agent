//! Private in-memory run state shared by ingress, execution and recovery.
use super::*;

#[derive(Clone)]
pub(super) struct PendingAction {
    pub(super) intent: p::ActionIntent,
    pub(super) plan: ExecutionPlan,
    pub(super) ticket: ApprovalTicket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RunMode {
    Loop,
    DirectAction,
}

#[derive(Clone)]
pub(super) struct EvolutionSimulationContext {
    pub(super) candidate: p::StrategyCandidate,
    pub(super) comparison: p::EvolutionComparison,
}

pub(super) struct RunRecord {
    pub(super) request: p::RunRequest,
    pub(super) workspace: p::WorkspaceRef,
    pub(super) loop_ctx: forme_loop::RunCtx,
    pub(super) result: Option<p::RunResult>,
    pub(super) final_output: Option<String>,
    pub(super) resume_state: Option<ResumeState>,
    pub(super) pending_action: Option<PendingAction>,
    pub(super) recovered_unknown: Option<p::ActionId>,
    pub(super) approval: InMemoryApprovalBroker,
    pub(super) mode: RunMode,
    pub(super) direct_intent: Option<p::ActionIntent>,
    pub(super) owner_discovery_bound: bool,
    pub(super) tools: Option<Arc<run_tools::RunToolSnapshot>>,
    pub(super) bound_envelope: Option<p::AutonomyEnvelope>,
    pub(super) effect_mode: Option<p::EffectMode>,
    pub(super) evolution_snapshot: Option<p::EvolutionSnapshotRef>,
    pub(super) evolution_snapshot_full: Option<p::EvolutionSnapshot>,
    pub(super) federation_snapshot: Option<p::FederationSnapshotRef>,
    pub(super) m3_binding: Option<BoundM3Domains>,
    pub(super) remaining_action_budget: Option<u64>,
    pub(super) competence_inputs: cognition::CompetenceInputs,
    pub(super) skills_prepared: bool,
    pub(super) evolution_simulation: Option<EvolutionSimulationContext>,
    pub(super) long_horizon: Option<m3_b::PendingLongHorizonCheckpoint>,
    pub(super) model_selection: Option<ModelSelection>,
    pub(super) active_model: Option<p::ModelProfileRef>,
    pub(super) fallback_index: usize,
}

pub(super) struct RunHandle {
    pub(super) record: Mutex<RunRecord>,
    pub(super) changed: Condvar,
    pub(super) cancelled: AtomicBool,
    pub(super) cancel_token: CancelToken,
}

pub(super) enum PreparedSubmission {
    Existing(p::RunId),
    New {
        run: p::RunId,
        handle: Arc<RunHandle>,
        session: p::SessionId,
    },
}

impl RunHandle {
    pub(super) fn new(record: RunRecord) -> Self {
        Self {
            record: Mutex::new(record),
            changed: Condvar::new(),
            cancelled: AtomicBool::new(false),
            cancel_token: CancelToken::default(),
        }
    }
}
