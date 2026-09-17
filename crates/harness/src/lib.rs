//! Harness-first run container, governance choke points, recovery, and ticks (prd/03).
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_approval::{
    ApprovalAuthorization, ApprovalBroker, ApprovalRequest, ApprovalScope, GrantScope,
    InMemoryApprovalBroker,
};
pub use forme_approval::{ApprovalGrant, ApprovalTicket};
use forme_capabilities::{
    AppApiConnectorRegistry, CapabilityPackageRegistry, ExecutionCapabilityRechecker,
    InMemorySkillRegistry, LoadTrigger, ProjectAppApiConnector, PublisherPublicKey, SkillRegistry,
    SkillSearchQuery,
};
use forme_cognition as cognition;
use forme_context::{
    AgentWorkspace, AgentWorkspaceItem, AutomaticCompactor, CompactionPolicy, ContextBudget,
    ContextBuilder, ContextSources, LayeredContextBuilder, LoadedSkillBody, SkillMetadata,
};
use forme_coordination as coordination;
use forme_eval as eval;
use forme_execution::{
    ActionResult, ActionStatus, AppApiBackend, CancelToken, EventSink, ExecutionBackendRegistry,
    ExecutionPlan, HttpAppApiDriver, InMemoryContentResolver, NotificationBackend, OutputBudget,
    RejectingSecretResolver,
};
use forme_loop::{
    Budget as LoopBudget, HandoffResolution, LoopEffect, LoopEngine, LoopState, PendingKind,
    ReactiveLoopEngine, ResolvedOutcome, ResumeState, StopReason,
};
use forme_models::{
    ChatCompletionsProvider, Cost, ModelCapability, ModelProfile, ModelProvider, ModelStrength,
    RateLimit, ScriptedModelProvider, SecretString, Url,
};
use forme_policy::{
    ActionMatcher, ArgMatcher, DefaultPolicyEngine, DelegationGrant, DelegationSubject,
    EnvelopeDecision, PolicyContext, PolicyEngine, PolicyLayer, PolicyLayerSource, PolicyRule,
};
use forme_protocol as p;
use forme_store::{EventStore, EvolutionProjection, SqliteEventStore, StoreOptions};

mod m3_a;
mod m3_b;
mod m4;
mod m5;
pub use m3_a::{
    EvolutionActivationResult, EvolutionEvaluationResult, ExactReplayAuditResult,
    M3EvolutionHarness,
};
pub use m3_b::{
    BoundM3Domains, LongHorizonAdvance, LongHorizonCheckpointRecord, LongHorizonDisposition,
    LongHorizonOutwardStep, LongHorizonProject, M3DomainRuntime, M3RuntimeCompatibility,
};
pub use m4::{
    FederatedHarnessRuntime, FederationActionGateway, FederationControlResult,
    FederationGatewayControl, RemoteActionSubmission, RemoteGroundTruthSource,
    RemoteRecoveryResult, RetentionState,
};
pub use m5::{
    capability_distribution_operation, CapabilityDistributionGuard, CapabilityDistributionObserver,
    CapabilityPackageLedgerGroundTruth, CapabilityPackageReceiver, EcosystemGatewayControl,
    EcosystemHarnessRuntime, FileCapabilityPackageLedger, M5_PACKAGE_RECEIVER_LOGICAL_PATH,
};

#[derive(Debug, Clone)]
pub struct EventStream {
    events: VecDeque<p::Event>,
}

impl EventStream {
    pub fn new(events: Vec<p::Event>) -> Self {
        Self {
            events: events.into(),
        }
    }

    pub fn events(&self) -> Vec<p::Event> {
        self.events.iter().cloned().collect()
    }
}

impl Iterator for EventStream {
    type Item = p::Event;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeInput {
    Approval(forme_approval::ApprovalGrant),
    ToolOutcome(p::ActionId, ResolvedOutcome),
    Handoff(HandoffResolution),
}

pub trait AgentHarness {
    fn submit_run(&self, req: p::RunRequest) -> p::Result<p::RunId>;
    fn stream_events(&self, run: p::RunId) -> EventStream;
    fn wait(&self, run: p::RunId) -> p::Result<p::RunResult>;
    fn cancel(&self, run: p::RunId) -> p::Result<()>;
    fn resume(&self, run: p::RunId, input: ResumeInput) -> p::Result<()>;
    fn drain(&self, session: p::SessionId) -> p::Result<()>;
}

pub trait GatewayControl: AgentHarness + Send + Sync {
    fn start_run(self: Arc<Self>, request: p::RunRequest) -> p::Result<p::RunId>;
    fn gateway_profile(&self, surface: p::SurfaceRef) -> p::Result<p::GatewayProfile>;
    fn list_runs(&self) -> p::Result<Vec<p::RunSummary>>;
    fn stream_event_page(&self, cursor: p::EventCursor) -> p::Result<p::EventPage>;
    fn run_summary(&self, run: p::RunId) -> p::Result<p::RunSummary>;
    fn pending_approvals(&self, session: p::SessionId) -> p::Result<Vec<p::PendingApproval>>;
    fn control(&self, run: p::RunId, control: p::RunControl) -> p::Result<()>;
    fn trace_view(&self, run: p::RunId) -> p::Result<p::TraceView>;
    fn review_candidate(&self, command: p::CandidateReviewCommand) -> p::Result<()>;
}

pub trait SchedulerGatewayControl: GatewayControl {
    fn schedule(&self, command: p::ScheduleCommand) -> p::Result<p::IntentionId>;
    fn list_jobs(&self) -> p::Result<Vec<p::ScheduledJob>>;
}

pub trait SchedulerService {
    fn tick(&self, now: p::Timestamp) -> p::Result<p::SchedulerTickReport>;
    fn cancel(&self, intention: p::IntentionId, actor: p::Actor) -> p::Result<()>;
    fn recover(&self, now: p::Timestamp) -> p::Result<p::RecoveryReport>;
}

pub trait ManualEvaluator: GatewayControl {
    fn run_case(
        &self,
        case: p::ManualEvalCase,
        profile: p::EvalProfile,
    ) -> p::Result<p::ManualEvalReport>;
    fn export_report(&self, eval_ref: p::EvalRef) -> p::Result<p::ManualEvalReport>;
}

pub trait EvolutionGatewayControl: Send + Sync {
    fn evolution_snapshot(&self, scope: p::Scope) -> p::Result<p::EvolutionSnapshot>;
    fn record_strategy_candidate(
        &self,
        run: p::RunId,
        candidate: p::StrategyCandidate,
    ) -> p::Result<p::EventId>;
    fn evaluate_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        comparison: p::EvolutionComparison,
    ) -> p::Result<EvolutionEvaluationResult>;
    fn promote_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
    ) -> p::Result<p::EventId>;
    fn activate_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        promotion: p::EventId,
        owner_confirmation: Option<p::OwnerControlRef>,
    ) -> p::Result<EvolutionActivationResult>;
    #[allow(clippy::too_many_arguments)]
    fn rollback_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        domain: p::StrategyDomain,
        scope: p::Scope,
        restored: p::StrategyVersionRef,
        triggers: Vec<p::EvidenceRef>,
        in_flight: p::InFlightDisposition,
        owner_confirmation: Option<p::OwnerControlRef>,
    ) -> p::Result<EvolutionActivationResult>;
    fn set_auto_activation_paused(&self, paused: bool);
    fn auto_activation_paused(&self) -> bool;
}

#[derive(Clone, Default)]
pub struct IngressAuthority(Arc<()>);

impl std::fmt::Debug for IngressAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("IngressAuthority(<instance-bound>)")
    }
}

impl PartialEq for IngressAuthority {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for IngressAuthority {}

impl IngressAuthority {
    pub fn stamp(&self, payload: p::EventPayload, provenance: p::Provenance) -> IngressEvent {
        IngressEvent {
            schema_version: p::SchemaVersion(1),
            payload,
            provenance,
            authority: self.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngressEvent {
    schema_version: p::SchemaVersion,
    payload: p::EventPayload,
    provenance: p::Provenance,
    authority: IngressAuthority,
}

impl IngressEvent {
    pub fn is_authorized_by(&self, authority: &IngressAuthority) -> bool {
        self.schema_version.0 > 0 && self.authority == *authority
    }

    pub fn into_parts(self) -> (p::EventPayload, p::Provenance) {
        (self.payload, self.provenance)
    }

    pub fn payload(&self) -> &p::EventPayload {
        &self.payload
    }

    pub fn provenance(&self) -> &p::Provenance {
        &self.provenance
    }
}

pub trait HarnessIngress: AgentHarness {
    fn ingress_authority(&self) -> IngressAuthority;

    fn submit_ingress(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId>;
    fn resolve_approval(
        &self,
        ticket: ApprovalTicket,
        grant: forme_approval::ApprovalGrant,
    ) -> p::Result<()>;
    fn append_ingress_events(
        &self,
        run: p::RunId,
        events: Vec<IngressEvent>,
    ) -> p::Result<Vec<p::EventId>>;
    fn result_text(&self, run: p::RunId) -> p::Result<Option<String>>;
}

pub trait HarnessActionIngress: HarnessIngress {
    fn submit_action(
        &self,
        request: p::RunRequest,
        intent: p::ActionIntent,
        envelope: p::AutonomyEnvelope,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionTrace {
    pub schema_version: p::SchemaVersion,
    pub run: p::RunId,
    pub event_refs: Vec<p::EventId>,
    pub reasons: Vec<p::ReasonRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    pub schema_version: p::SchemaVersion,
    pub run: p::RunId,
    pub session: Option<p::SessionId>,
    pub state: Option<LoopState>,
    pub last_stream_seq: u64,
    pub result: Option<p::RunResult>,
}

pub trait Observability {
    fn decision_trace(&self, run: p::RunId) -> DecisionTrace;
    fn state(&self, run: p::RunId) -> RunView;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateResolution {
    Promote,
    Reject,
    Downgrade,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteeringCommand {
    CorrectMisunderstanding {
        run: p::RunId,
        target: p::ObjectRef,
        lineage: p::LineageRef,
    },
    AdjustProactivity {
        run: p::RunId,
        scope: p::Scope,
        level: p::InterventionLevel,
    },
    Forget {
        run: p::RunId,
        target: p::ObjectRef,
        lineage: p::LineageRef,
    },
    ResolveCandidate {
        run: p::RunId,
        candidate: p::CandidateId,
        resolution: CandidateResolution,
    },
    SetDelegation {
        run: p::RunId,
        envelope: p::AutonomyEnvelopeRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessConfig {
    pub schema_version: p::SchemaVersion,
    pub policy_profile: p::PolicyProfileRef,
    pub model_profile: p::ModelProfileRef,
    pub toolset_ref: p::ToolsetRef,
    pub workspace: p::WorkspaceRef,
    pub context_budget: ContextBudget,
    pub compaction: CompactionPolicy,
    pub loop_budget: LoopBudget,
    pub policy_version: p::Version,
    pub tool_schema_version: p::Version,
    pub approval_ttl_ms: u64,
}

impl HarnessConfig {
    pub fn for_model(profile: &ModelProfile) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            policy_profile: p::PolicyProfileRef("policy:default-deny".into()),
            model_profile: profile.profile_ref(),
            toolset_ref: p::ToolsetRef("toolset:empty".into()),
            workspace: p::WorkspaceRef("workspace:default".into()),
            context_budget: ContextBudget {
                schema_version: p::SchemaVersion(1),
                max_tokens: u64::from(profile.capability.context_window),
                reserve: u64::from(profile.capability.context_window / 8).max(1),
            },
            compaction: CompactionPolicy::default(),
            loop_budget: LoopBudget::default(),
            policy_version: p::Version(1),
            tool_schema_version: p::Version(1),
            approval_ttl_ms: 300_000,
        }
    }

    fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.model_profile.0.trim().is_empty()
            || self.policy_profile.0.trim().is_empty()
            || self.toolset_ref.0.trim().is_empty()
            || self.workspace.0.trim().is_empty()
            || self.approval_ttl_ms == 0
        {
            return Err(p::Error("harness configuration is incomplete".into()));
        }
        self.compaction.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernanceConfig {
    pub schema_version: p::SchemaVersion,
    pub layers: Vec<PolicyLayer>,
    pub visible_capabilities: Vec<p::CapabilityRef>,
    pub granted_permissions: Vec<p::PermissionRef>,
    pub allowed_scopes: Vec<p::Scope>,
    pub shell_allowlist: Vec<String>,
    pub file_roots: Vec<String>,
    pub mcp_allowlist: Vec<String>,
    pub external: forme_policy::ExternalPolicyLimits,
    pub network_allowed: bool,
    pub sandbox_available: bool,
    pub delegation: Option<DelegationGrant>,
    pub envelope: Option<p::AutonomyEnvelope>,
}

pub struct ProjectAppApiRuntimeConfig {
    pub schema_version: p::SchemaVersion,
    pub connector: p::ProviderId,
    pub base_url: String,
    pub schema_digest: p::SchemaDigest,
    pub credential_ref: Option<p::SecretRef>,
    pub scope: p::Scope,
    pub capability: p::CapabilityRef,
    pub permission: p::PermissionRef,
    pub allowed_mutations: Vec<p::AppApiMutationMethod>,
    pub requests_per_minute: u32,
    pub timeout: p::DurationMs,
    pub max_response_bytes: u64,
    pub envelope: p::AutonomyEnvelope,
    pub contents: Vec<(p::ContentRef, Vec<u8>)>,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            layers: Vec::new(),
            visible_capabilities: Vec::new(),
            granted_permissions: Vec::new(),
            allowed_scopes: Vec::new(),
            shell_allowlist: Vec::new(),
            file_roots: Vec::new(),
            mcp_allowlist: Vec::new(),
            external: forme_policy::ExternalPolicyLimits::default(),
            network_allowed: false,
            sandbox_available: false,
            delegation: None,
            envelope: None,
        }
    }
}

impl GovernanceConfig {
    fn policy_context(
        &self,
        session: p::SessionId,
        toolset: p::ToolsetRef,
        engine: &DefaultPolicyEngine,
        runtime_envelope: Option<&p::AutonomyEnvelope>,
    ) -> PolicyContext {
        PolicyContext {
            schema_version: p::SchemaVersion(1),
            session,
            toolset,
            policy: engine.merge(&self.layers),
            visible_capabilities: self.visible_capabilities.clone(),
            granted_permissions: self.granted_permissions.clone(),
            allowed_scopes: self.allowed_scopes.clone(),
            shell_allowlist: self.shell_allowlist.clone(),
            file_roots: self.file_roots.clone(),
            mcp_allowlist: self.mcp_allowlist.clone(),
            external: self.external.clone(),
            network_allowed: self.network_allowed,
            sandbox_available: self.sandbox_available,
            delegation: self.delegation.clone(),
            envelope: runtime_envelope.cloned().or_else(|| self.envelope.clone()),
        }
    }
}

type ContextFactory = Arc<
    dyn Fn(&p::RunRequest, &p::RunId, &HarnessConfig) -> p::Result<forme_context::RunCtx>
        + Send
        + Sync,
>;

pub struct FixedCompetenceGate {
    ceiling: cognition::InterventionLevel,
}

impl FixedCompetenceGate {
    pub fn new(ceiling: cognition::InterventionLevel) -> Self {
        Self { ceiling }
    }
}

impl cognition::CompetenceGate for FixedCompetenceGate {
    fn ceiling(
        &self,
        _scope: p::Scope,
        _risk: p::Risk,
        _ctx: &cognition::CompetenceInputs,
    ) -> cognition::InterventionLevel {
        self.ceiling
    }
}

#[derive(Default)]
pub struct NoopProactivity;

impl cognition::ProactivityEngine for NoopProactivity {
    fn tick(
        &self,
        _trigger: cognition::TickTrigger,
        _snap: &cognition::CognitionSnapshot,
    ) -> Vec<cognition::Impulse> {
        Vec::new()
    }

    fn emit(
        &self,
        _impulse: cognition::Impulse,
        _guard: cognition::EmissionGuard,
    ) -> Option<cognition::Proposal> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickReport {
    pub schema_version: p::SchemaVersion,
    pub ran: bool,
    pub skipped_reason: Option<String>,
    pub run: Option<p::RunId>,
    pub candidate_events: Vec<p::EventId>,
    pub proposal_events: Vec<p::EventId>,
    pub workspace_snapshot: Option<p::AgentWorkspaceSnapshotRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentExecution {
    pub schema_version: p::SchemaVersion,
    pub child_run: p::RunId,
    pub result: p::RunResult,
    pub summary: p::SummaryRef,
    pub result_ref: p::ResultRef,
}

#[derive(Debug, Default)]
pub struct TickScheduler;

impl TickScheduler {
    fn admit(
        &self,
        trigger: Option<cognition::TickTrigger>,
        foreground_active: bool,
    ) -> Result<cognition::TickTrigger, &'static str> {
        let trigger = trigger.ok_or("no meaningful trigger")?;
        if foreground_active {
            return Err("foreground commitment guard is active");
        }
        Ok(trigger)
    }
}

#[derive(Clone)]
struct PendingAction {
    intent: p::ActionIntent,
    plan: ExecutionPlan,
    ticket: ApprovalTicket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Loop,
    DirectAction,
}

#[derive(Clone)]
struct EvolutionSimulationContext {
    candidate: p::StrategyCandidate,
    comparison: p::EvolutionComparison,
}

struct RunRecord {
    request: p::RunRequest,
    loop_ctx: forme_loop::RunCtx,
    result: Option<p::RunResult>,
    final_output: Option<String>,
    resume_state: Option<ResumeState>,
    pending_action: Option<PendingAction>,
    recovered_unknown: Option<p::ActionId>,
    approval: InMemoryApprovalBroker,
    mode: RunMode,
    direct_intent: Option<p::ActionIntent>,
    bound_envelope: Option<p::AutonomyEnvelope>,
    effect_mode: Option<p::EffectMode>,
    evolution_snapshot: Option<p::EvolutionSnapshotRef>,
    evolution_snapshot_full: Option<p::EvolutionSnapshot>,
    federation_snapshot: Option<p::FederationSnapshotRef>,
    m3_binding: Option<BoundM3Domains>,
    remaining_action_budget: Option<u64>,
    competence_inputs: cognition::CompetenceInputs,
    skills_prepared: bool,
    evolution_simulation: Option<EvolutionSimulationContext>,
    long_horizon: Option<m3_b::PendingLongHorizonCheckpoint>,
}

struct RunHandle {
    record: Mutex<RunRecord>,
    changed: Condvar,
    cancelled: AtomicBool,
    cancel_token: CancelToken,
}

enum PreparedSubmission {
    Existing(p::RunId),
    New {
        run: p::RunId,
        handle: Arc<RunHandle>,
        session: p::SessionId,
    },
}

impl RunHandle {
    fn new(record: RunRecord) -> Self {
        Self {
            record: Mutex::new(record),
            changed: Condvar::new(),
            cancelled: AtomicBool::new(false),
            cancel_token: CancelToken::default(),
        }
    }
}

struct SchedulerRuntime {
    config: p::SchedulerConfig,
    intentions: Arc<dyn cognition::ScheduledIntentions>,
}

pub struct ReactiveHarness {
    store: SqliteEventStore,
    ingress_authority: IngressAuthority,
    model: Arc<dyn ModelProvider>,
    context_builder: Arc<dyn ContextBuilder + Send + Sync>,
    loop_engine: ReactiveLoopEngine,
    backends: Arc<ExecutionBackendRegistry>,
    policy: DefaultPolicyEngine,
    governance: GovernanceConfig,
    config: HarnessConfig,
    context_factory: ContextFactory,
    competence: Arc<dyn cognition::CompetenceGate + Send + Sync>,
    competence_snapshot: cognition::CompetenceInputs,
    proactivity: Arc<dyn cognition::ProactivityEngine + Send + Sync>,
    verifier: Arc<dyn eval::Verifier + Send + Sync>,
    coordination: Option<Arc<dyn coordination::CoordinationReasoner + Send + Sync>>,
    capability_rechecker: Option<Arc<dyn ExecutionCapabilityRechecker + Send + Sync>>,
    skill_registry: Option<Arc<InMemorySkillRegistry>>,
    skill_runtime_lock: Mutex<()>,
    runs: Mutex<BTreeMap<p::RunId, Arc<RunHandle>>>,
    idempotency: Mutex<BTreeMap<p::IdempotencyKey, p::RunId>>,
    session_locks: Mutex<BTreeMap<p::SessionId, Arc<Mutex<()>>>>,
    active_sessions: Mutex<BTreeSet<p::SessionId>>,
    run_sequence: AtomicU64,
    event_sequence: Arc<AtomicU64>,
    tick_sequence: AtomicU64,
    tick: TickScheduler,
    scheduler: Option<SchedulerRuntime>,
    proposal_origins: Mutex<BTreeMap<p::ProposalRef, String>>,
    suppressed_origins: Mutex<BTreeSet<String>>,
    manual_evals: eval::ManualEvalArchive,
    evolution_seed: Option<p::EvolutionSnapshot>,
    evolution_control: Arc<M3EvolutionHarness>,
    m3_domains: Option<Arc<M3DomainRuntime>>,
    m3_scope_override: Option<p::Scope>,
    federation: Arc<FederatedHarnessRuntime>,
    ecosystem: Arc<EcosystemHarnessRuntime>,
}

impl ReactiveHarness {
    pub fn new(
        store: SqliteEventStore,
        model: Arc<dyn ModelProvider>,
        backends: Arc<ExecutionBackendRegistry>,
        governance: GovernanceConfig,
        config: HarnessConfig,
    ) -> p::Result<Self> {
        config.validate()?;
        model.profile().validate()?;
        let context_builder: Arc<dyn ContextBuilder + Send + Sync> =
            Arc::new(LayeredContextBuilder::default());
        let loop_engine = ReactiveLoopEngine::new(context_builder.clone(), model.clone());
        let evolution_control = Arc::new(M3EvolutionHarness::new(store.clone()));
        let federation = Arc::new(FederatedHarnessRuntime::new(store.clone()));
        let ecosystem = Arc::new(EcosystemHarnessRuntime::local(store.clone()));
        let harness = Self {
            store,
            ingress_authority: IngressAuthority::default(),
            model,
            context_builder,
            loop_engine,
            backends,
            policy: DefaultPolicyEngine,
            governance,
            config,
            context_factory: Arc::new(default_context),
            competence: Arc::new(FixedCompetenceGate::new(
                cognition::InterventionLevel::L5HighImpact,
            )),
            competence_snapshot: cognition::CompetenceInputs::default(),
            proactivity: Arc::new(NoopProactivity),
            verifier: Arc::new(eval::DeterministicVerifier),
            coordination: None,
            capability_rechecker: None,
            skill_registry: None,
            skill_runtime_lock: Mutex::new(()),
            runs: Mutex::new(BTreeMap::new()),
            idempotency: Mutex::new(BTreeMap::new()),
            session_locks: Mutex::new(BTreeMap::new()),
            active_sessions: Mutex::new(BTreeSet::new()),
            run_sequence: AtomicU64::new(1),
            event_sequence: Arc::new(AtomicU64::new(1)),
            tick_sequence: AtomicU64::new(1),
            tick: TickScheduler,
            scheduler: None,
            proposal_origins: Mutex::new(BTreeMap::new()),
            suppressed_origins: Mutex::new(BTreeSet::new()),
            manual_evals: eval::ManualEvalArchive::default(),
            evolution_seed: None,
            evolution_control,
            m3_domains: None,
            m3_scope_override: None,
            federation,
            ecosystem,
        };
        harness.recover_unknown_outcomes()?;
        Ok(harness)
    }

    pub fn federation_runtime(&self) -> Arc<FederatedHarnessRuntime> {
        self.federation.clone()
    }

    pub fn ecosystem_runtime(&self) -> Arc<EcosystemHarnessRuntime> {
        self.ecosystem.clone()
    }

    pub fn submit_evolution_simulation(
        &self,
        request: p::RunRequest,
        candidate: p::StrategyCandidate,
        comparison: p::EvolutionComparison,
    ) -> p::Result<p::RunId> {
        candidate.validate()?;
        comparison.validate()?;
        if request.source != p::Source::Simulation
            || request.idempotency_key.is_some()
            || comparison.candidate != candidate.proposed_version
            || comparison.baseline != candidate.baseline
        {
            return Err(p::Error(
                "evolution simulation request does not match its candidate".into(),
            ));
        }
        match self.prepare_ingress_internal_as(request, Vec::new(), None)? {
            PreparedSubmission::Existing(_) => Err(p::Error(
                "evolution simulation cannot reuse an existing run".into(),
            )),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                handle
                    .record
                    .lock()
                    .map_err(|_| p::Error("run state is unavailable".into()))?
                    .evolution_simulation = Some(EvolutionSimulationContext {
                    candidate,
                    comparison,
                });
                self.drive_prepared_run(&run, &handle, &session)?;
                Ok(run)
            }
        }
    }

    pub fn project_owned_app_api(runtime: ProjectAppApiRuntimeConfig) -> p::Result<Self> {
        if runtime.schema_version.0 == 0
            || runtime.credential_ref.is_some()
            || runtime.scope.0.trim().is_empty()
            || runtime.capability.0.trim().is_empty()
            || runtime.permission.0.trim().is_empty()
            || runtime.max_response_bytes == 0
        {
            return Err(p::Error(
                "project-owned App API runtime is incomplete or requires an external SecretRef resolver"
                    .into(),
            ));
        }
        let connector = ProjectAppApiConnector::new(
            runtime.connector.clone(),
            runtime.base_url.clone(),
            runtime.schema_digest.clone(),
            runtime.credential_ref.clone(),
            runtime.scope.clone(),
            runtime.capability.clone(),
            runtime.permission.clone(),
            runtime.allowed_mutations.clone(),
            runtime.requests_per_minute,
            runtime.timeout,
        )?;
        let revoked = connector.revocation_flag();
        let connectors = Arc::new(AppApiConnectorRegistry::with_connectors(vec![connector])?);
        connectors.configure(runtime.connector.clone())?;
        connectors.enable(runtime.connector.clone())?;
        connectors.bind_trust(
            runtime.connector.clone(),
            p::TrustTier::ApprovedSource,
            p::Actor::Owner,
        )?;
        connectors.grant(runtime.connector.clone(), runtime.envelope.clone())?;

        let contents = Arc::new(InMemoryContentResolver::default());
        for (reference, bytes) in runtime.contents {
            contents.insert(reference, bytes)?;
        }
        let driver = Arc::new(HttpAppApiDriver::new(
            runtime.connector.clone(),
            runtime.base_url.clone(),
            runtime.schema_digest.clone(),
            runtime.credential_ref.clone(),
            runtime.allowed_mutations.clone(),
            runtime.requests_per_minute,
            runtime.timeout,
            runtime.max_response_bytes,
            revoked,
        )?);
        let backends = Arc::new(ExecutionBackendRegistry::default());
        backends.register(Arc::new(AppApiBackend::new(
            runtime.connector.clone(),
            driver,
            Arc::new(RejectingSecretResolver),
            contents,
            OutputBudget::truncate_at(runtime.max_response_bytes),
            runtime.timeout,
        )?))?;

        let profile = action_only_model_profile()?;
        let model = Arc::new(ScriptedModelProvider::new(profile.clone(), Vec::new())?);
        let policy = PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::User,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::AppApi),
                    capability: Some(runtime.capability.clone()),
                    action_type: None,
                    parameters: ArgMatcher::Any,
                },
                effect: p::PolicyDecision::Ask,
                scope: runtime.scope.clone(),
            }],
        };
        let governance = GovernanceConfig {
            schema_version: p::SchemaVersion(1),
            layers: vec![policy],
            visible_capabilities: vec![runtime.capability.clone()],
            granted_permissions: vec![runtime.permission.clone()],
            allowed_scopes: vec![runtime.scope.clone()],
            shell_allowlist: Vec::new(),
            file_roots: Vec::new(),
            mcp_allowlist: Vec::new(),
            external: forme_policy::ExternalPolicyLimits {
                schema_version: p::SchemaVersion(1),
                browser_origins: Vec::new(),
                computer_surfaces: Vec::new(),
                pty_programs: Vec::new(),
                pty_roots: Vec::new(),
                app_api_connectors: vec![forme_policy::AppApiConnectorLimit {
                    schema_version: p::SchemaVersion(1),
                    connector: runtime.connector,
                    base_url: runtime.base_url,
                    schema_digest: runtime.schema_digest,
                    credential_ref: runtime.credential_ref,
                    allowed_mutations: runtime.allowed_mutations,
                    max_timeout: runtime.timeout,
                }],
            },
            network_allowed: true,
            sandbox_available: true,
            delegation: Some(DelegationGrant {
                schema_version: p::SchemaVersion(1),
                subject: DelegationSubject::Owner,
                envelope: runtime.envelope.clone(),
                granted_by: p::Actor::Owner,
                audit_ref: p::EventId("delegation:project-app-api".into()),
            }),
            envelope: Some(runtime.envelope),
        };
        let mut config = HarnessConfig::for_model(&profile);
        config.toolset_ref = p::ToolsetRef("toolset:project-app-api".into());
        config.workspace = p::WorkspaceRef(runtime.scope.0);
        Self::new(
            SqliteEventStore::open_in_memory(StoreOptions::default())?,
            model,
            backends,
            governance,
            config,
        )
        .map(|harness| harness.with_capability_rechecker(connectors))
    }

    /// Provisions authority-local public verification material. The key is not
    /// accepted from a catalog or owner-control payload and is never evented.
    pub fn configure_ecosystem_publisher_key(
        &self,
        publisher: p::CapabilityPublisherRef,
        key: PublisherPublicKey,
    ) -> p::Result<()> {
        self.ecosystem.configure_publisher_key(publisher, key)
    }

    pub fn ecosystem_package_registry(&self) -> Arc<CapabilityPackageRegistry> {
        self.ecosystem.package_registry()
    }

    pub fn bind_capability_distribution(
        &self,
        run: p::RunId,
        plan: p::CapabilityInstallPlan,
        approval: p::CapabilityPackageApproval,
        ground_truth: Arc<CapabilityPackageLedgerGroundTruth>,
    ) -> p::Result<()> {
        let guard = Arc::new(CapabilityDistributionGuard::new(
            self.ecosystem.clone(),
            plan.clone(),
            approval,
        )?);
        let observer = Arc::new(CapabilityDistributionObserver::new(
            self.ecosystem.clone(),
            plan,
            ground_truth.clone(),
        )?);
        self.federation
            .configure_remote_governance(run, guard, observer)?;
        self.federation.configure_ground_truth(ground_truth)
    }

    pub fn from_environment(store: SqliteEventStore) -> p::Result<Self> {
        let provider_id = std::env::var("FORME_MODEL_PROVIDER")
            .unwrap_or_else(|_| "configured-chat-provider".into());
        let model = required_env("FORME_MODEL_NAME")?;
        let base_url = Url::parse(required_env("FORME_MODEL_BASE_URL")?)?;
        let secret = SecretString::new(required_env("FORME_MODEL_API_KEY")?)?;
        let profile = ModelProfile {
            schema_version: p::SchemaVersion(1),
            provider: p::ProviderId(provider_id),
            model,
            base_url,
            capability: ModelCapability {
                schema_version: p::SchemaVersion(1),
                context_window: optional_env_u32("FORME_MODEL_CONTEXT_WINDOW", 32_768)?,
                tool_use: false,
                strength: ModelStrength::Standard,
            },
            cost: Cost {
                schema_version: p::SchemaVersion(1),
                input_microunits_per_million: 0,
                output_microunits_per_million: 0,
            },
            rate_limit: RateLimit {
                schema_version: p::SchemaVersion(1),
                requests_per_minute: 60,
                tokens_per_minute: 1_000_000,
            },
            credential_ref: p::CredentialRef("env:FORME_MODEL_API_KEY".into()),
        };
        let provider = Arc::new(ChatCompletionsProvider::new(
            profile.clone(),
            secret,
            p::DurationMs(u64::from(optional_env_u32(
                "FORME_MODEL_TIMEOUT_MS",
                120_000,
            )?)),
        )?);
        let intention_services = cognition::event_sourced_intention_services(
            Arc::new(store.clone()),
            p::RunId("memory:proactivity-intentions".into()),
        )?;
        let proactivity =
            cognition::M0ProactivityEngine::new(cognition::ProactivityConfig::default())
                .with_intention_store(intention_services.proactive.clone());
        let notification_enabled = optional_env_bool("FORME_NOTIFICATION_ENABLED", true)?;
        let backends = Arc::new(ExecutionBackendRegistry::default());
        if notification_enabled {
            backends.register(Arc::new(NotificationBackend::default()))?;
        }
        let config = HarnessConfig::for_model(&profile);
        let governance = if notification_enabled {
            local_notification_governance(&config.workspace, now_ms())
        } else {
            GovernanceConfig::default()
        };
        let scheduler_enabled = optional_env_bool("FORME_SCHEDULER_ENABLED", true)?;
        let scheduler_config = p::SchedulerConfig {
            schema_version: p::SchemaVersion(1),
            tick: p::DurationMs(u64::from(optional_env_u32(
                "FORME_SCHEDULER_TICK_MS",
                1_000,
            )?)),
            lease: p::DurationMs(u64::from(optional_env_u32(
                "FORME_SCHEDULER_LEASE_MS",
                30_000,
            )?)),
            max_claims_per_tick: optional_env_u32("FORME_SCHEDULER_MAX_CLAIMS", 1)?,
        };
        let harness = Self::new(store, provider, backends, governance, config)?
            .with_coordination(Arc::new(coordination::RuleBasedCoordinationReasoner))
            .with_proactivity(Arc::new(proactivity))
            .with_competence_gate(Arc::new(cognition::EvidenceCompetenceGate::default()));
        if scheduler_enabled {
            harness.with_scheduler(intention_services.scheduled, scheduler_config)
        } else {
            Ok(harness)
        }
    }

    pub fn from_environment_local() -> p::Result<Self> {
        let path = std::env::var_os("FORME_STORE_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(".forme").join("forme.db"));
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                p::Error(format!("failed to create local store directory: {error}"))
            })?;
        }
        let store = SqliteEventStore::open(
            path,
            StoreOptions {
                fts_enabled: true,
                ..StoreOptions::default()
            },
        )?;
        Self::from_environment(store)
    }

    pub fn with_context_builder(mut self, builder: Arc<dyn ContextBuilder + Send + Sync>) -> Self {
        self.context_builder = builder.clone();
        self.loop_engine = ReactiveLoopEngine::new(builder, self.model.clone());
        self
    }

    pub fn with_context_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn(&p::RunRequest, &p::RunId, &HarnessConfig) -> p::Result<forme_context::RunCtx>
            + Send
            + Sync
            + 'static,
    {
        self.context_factory = Arc::new(factory);
        self
    }

    pub fn with_capability_rechecker(
        mut self,
        rechecker: Arc<dyn ExecutionCapabilityRechecker + Send + Sync>,
    ) -> Self {
        self.capability_rechecker = Some(rechecker);
        self
    }

    pub fn with_skill_registry(mut self, registry: Arc<InMemorySkillRegistry>) -> Self {
        self.skill_registry = Some(registry);
        self
    }

    pub fn with_competence_gate(
        mut self,
        gate: Arc<dyn cognition::CompetenceGate + Send + Sync>,
    ) -> Self {
        self.competence = gate;
        self
    }

    pub fn with_competence_snapshot(
        mut self,
        snapshot: cognition::CompetenceInputs,
    ) -> p::Result<Self> {
        validate_competence_snapshot(&snapshot)?;
        self.competence_snapshot = snapshot;
        Ok(self)
    }

    pub fn with_proactivity(
        mut self,
        engine: Arc<dyn cognition::ProactivityEngine + Send + Sync>,
    ) -> Self {
        self.proactivity = engine;
        self
    }

    pub fn with_verifier(mut self, verifier: Arc<dyn eval::Verifier + Send + Sync>) -> Self {
        self.verifier = verifier;
        self
    }

    pub fn with_evolution_seed(mut self, seed: p::EvolutionSnapshot) -> p::Result<Self> {
        seed.validate()?;
        self.evolution_seed = Some(seed);
        Ok(self)
    }

    pub fn with_m3_domain_runtime(mut self, runtime: Arc<M3DomainRuntime>) -> Self {
        self.m3_domains = Some(runtime);
        self
    }

    fn with_m3_scope_override(mut self, scope: p::Scope) -> Self {
        self.m3_scope_override = Some(scope);
        self
    }

    pub fn with_coordination(
        mut self,
        reasoner: Arc<dyn coordination::CoordinationReasoner + Send + Sync>,
    ) -> Self {
        self.coordination = Some(reasoner);
        self
    }

    pub fn with_scheduler(
        mut self,
        intentions: Arc<dyn cognition::ScheduledIntentions>,
        config: p::SchedulerConfig,
    ) -> p::Result<Self> {
        config.validate()?;
        self.scheduler = Some(SchedulerRuntime { config, intentions });
        Ok(self)
    }

    pub fn output_text(&self, run: p::RunId) -> p::Result<Option<String>> {
        Ok(self
            .run_handle(&run)?
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?
            .final_output
            .clone())
    }

    pub fn pending_approvals(&self, session: p::SessionId) -> p::Result<Vec<ApprovalRequest>> {
        let handles = self
            .runs
            .lock()
            .map_err(|_| p::Error("run registry is unavailable".into()))?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut pending = Vec::new();
        for handle in handles {
            let record = handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?;
            pending.extend(
                record
                    .approval
                    .pending(ApprovalScope::Session(session.clone())),
            );
        }
        Ok(pending)
    }

    pub fn state(&self, run: p::RunId) -> p::Result<LoopState> {
        Ok(self
            .run_handle(&run)?
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?
            .loop_ctx
            .state
            .clone())
    }

    pub fn steer(&self, command: SteeringCommand) -> p::Result<p::EventId> {
        let (run, payload) = match command {
            SteeringCommand::CorrectMisunderstanding {
                run,
                target,
                lineage,
            }
            | SteeringCommand::Forget {
                run,
                target,
                lineage,
            } => (
                run,
                p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                    target_object: target,
                    evidence_lineage: lineage,
                }),
            ),
            SteeringCommand::AdjustProactivity { run, scope, level } => (
                run,
                p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                    trace_ref: p::DecisionTraceRef(format!(
                        "steering:proactivity:{}:{level:?}",
                        scope.0
                    )),
                    refs: p::DecisionRefs {
                        map: None,
                        user: None,
                        agent_self: None,
                        trust: None,
                        failure: Vec::new(),
                    },
                    rationale: p::Rationale("owner adjusted proactivity".into()),
                    workspace_snapshot: p::AgentWorkspaceSnapshotRef("steering".into()),
                    resource_graph_snapshot: None,
                    evolution_snapshot: None,
                    federation_snapshot: None,
                }),
            ),
            SteeringCommand::ResolveCandidate {
                run,
                candidate,
                resolution,
            } => {
                let reason = p::ReasonRef("owner steering resolution".into());
                let payload = match resolution {
                    CandidateResolution::Promote => {
                        p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                            candidate_id: candidate,
                            by: p::DecisionActor::User,
                            reason,
                        })
                    }
                    CandidateResolution::Reject => {
                        p::EventPayload::CandidateRejected(p::CandidateRejectedPayload {
                            candidate_id: candidate,
                            by: p::DecisionActor::User,
                            reason,
                        })
                    }
                    CandidateResolution::Downgrade => {
                        p::EventPayload::CandidateDowngraded(p::CandidateDowngradedPayload {
                            candidate_id: candidate,
                            by: p::DecisionActor::User,
                            reason,
                        })
                    }
                };
                (run, payload)
            }
            SteeringCommand::SetDelegation { run, envelope } => (
                run,
                p::EventPayload::AutonomyEnvelopeSet(p::AutonomyEnvelopeSetPayload { envelope }),
            ),
        };
        self.run_handle(&run)?;
        self.append(
            run,
            None,
            p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            payload,
        )
    }

    pub fn tick(
        &self,
        session: p::SessionId,
        trigger: Option<cognition::TickTrigger>,
    ) -> p::Result<TickReport> {
        self.tick_with_snapshot(session, trigger, cognition::CognitionSnapshot::default())
    }

    pub fn tick_with_snapshot(
        &self,
        session: p::SessionId,
        trigger: Option<cognition::TickTrigger>,
        snapshot: cognition::CognitionSnapshot,
    ) -> p::Result<TickReport> {
        let foreground_active = self
            .active_sessions
            .lock()
            .map_err(|_| p::Error("foreground activity state is unavailable".into()))?
            .contains(&session);
        let trigger = match self.tick.admit(trigger, foreground_active) {
            Ok(trigger) => trigger,
            Err(reason) => {
                return Ok(TickReport {
                    schema_version: p::SchemaVersion(1),
                    ran: false,
                    skipped_reason: Some(reason.into()),
                    run: None,
                    candidate_events: Vec::new(),
                    proposal_events: Vec::new(),
                    workspace_snapshot: None,
                });
            }
        };
        let impulses = self.proactivity.tick(trigger, &snapshot);
        let tick_index = self.tick_sequence.fetch_add(1, Ordering::SeqCst);
        let run = p::RunId(format!("tick:{}:{tick_index}", session.0));
        let provenance = internal_provenance(p::Source::Internal);
        let mut candidate_events = Vec::new();
        let mut proposal_events = Vec::new();
        let mut workspace_snapshot = None;
        for (index, impulse) in impulses.into_iter().enumerate() {
            if self
                .suppressed_origins
                .lock()
                .map_err(|_| p::Error("proactivity suppression state is unavailable".into()))?
                .contains(&impulse.origin_key())
            {
                continue;
            }
            if impulse.source != cognition::ImpulseSource::Commitment {
                self.append(
                    run.clone(),
                    None,
                    provenance.clone(),
                    p::EventPayload::ObservationRecorded(p::ObservationRecordedPayload {
                        source: impulse.observation_source,
                        scope: impulse.scope.clone(),
                        grant_ref: impulse.grant_ref.clone(),
                    }),
                )?;
            }
            if let Some(shape) = impulse.activation_shape {
                self.append(
                    run.clone(),
                    None,
                    provenance.clone(),
                    p::EventPayload::OpportunityDetected(p::OpportunityDetectedPayload {
                        seed: impulse.seed.clone(),
                        activation_shape: Some(shape),
                    }),
                )?;
            }
            let value_passed = matches!(impulse.value, cognition::ValueDecision::Worth(_));
            let value_reason = match &impulse.value {
                cognition::ValueDecision::Worth(cognition::Value(value)) => p::ReasonRef(format!(
                    "estimated value {value} passed the attention floor"
                )),
                cognition::ValueDecision::NotWorth(reason) => reason.clone(),
            };
            self.append(
                run.clone(),
                None,
                provenance.clone(),
                p::EventPayload::ValueGateEvaluated(p::ValueGateEvaluatedPayload {
                    decision: p::GateDecision(if value_passed {
                        "worth".into()
                    } else {
                        "not-worth".into()
                    }),
                    reason: value_reason,
                }),
            )?;
            self.append(
                run.clone(),
                None,
                provenance.clone(),
                p::EventPayload::ImpulseRaised(p::ImpulseRaisedPayload {
                    source: protocol_impulse_source(impulse.source),
                    reach: protocol_reach(impulse.reach),
                    seed: impulse.seed.clone(),
                }),
            )?;
            let competence = self.competence.ceiling(
                impulse.scope.clone(),
                risk_for_level(impulse.requested_level),
                &snapshot.competence,
            );
            let requested_level = impulse.requested_level;
            self.append(
                run.clone(),
                None,
                provenance.clone(),
                p::EventPayload::CompetenceGateEvaluated(p::CompetenceGateEvaluatedPayload {
                    scope: impulse.scope.clone(),
                    risk: risk_for_level(impulse.requested_level),
                    max_level: protocol_intervention(competence),
                    reads: snapshot.competence.protocol_reads(),
                }),
            )?;
            let guard = cognition::EmissionGuard {
                schema_version: p::SchemaVersion(1),
                value: impulse.value.clone(),
                competence,
            };
            if let Some(mut proposal) = self.proactivity.emit(impulse, guard) {
                let initially_allowed = self.proposal_policy_allows(&proposal);
                if !initially_allowed {
                    proposal = proposal.downgraded(cognition::InterventionLevel::L2Prepare);
                }
                let policy_allowed = self.proposal_policy_allows(&proposal);
                if !policy_allowed {
                    continue;
                }
                self.proposal_origins
                    .lock()
                    .map_err(|_| p::Error("proactivity proposal state is unavailable".into()))?
                    .insert(proposal.reference(), proposal.core().origin_key.clone());
                let current_events = self
                    .store
                    .read_run(run.clone())
                    .collect::<p::Result<Vec<_>>>()?;
                let workspace = cognition::AgentWorkspaceProjection::rebuild(&current_events, 10);
                workspace_snapshot = Some(workspace.snapshot_ref.clone());
                self.append(
                    run.clone(),
                    None,
                    provenance.clone(),
                    p::EventPayload::DecisionTraceRecorded(
                        p::DecisionTraceRecordedPayload {
                            trace_ref: p::DecisionTraceRef(format!(
                                "proactive-trace:{}",
                                proposal.reference().0
                            )),
                            refs: p::DecisionRefs {
                                map: None,
                                user: None,
                                agent_self: snapshot
                                    .competence
                                    .self_model
                                    .as_ref()
                                    .map(|input| input.reference.clone()),
                                trust: snapshot
                                    .competence
                                    .trust
                                    .as_ref()
                                    .map(|input| input.reference.clone()),
                                failure: snapshot
                                    .competence
                                    .failure
                                    .iter()
                                    .map(|input| input.reference.clone())
                                    .collect(),
                            },
                            rationale: p::Rationale(if competence < requested_level {
                                "competence evidence lowered the intervention; add verified outcome evidence and review referenced failures"
                                    .into()
                            } else if initially_allowed {
                                "impulse entered AgentWorkspace and passed the emission guard".into()
                            } else {
                                "policy or envelope lowered the proposal to a no-effect level"
                                    .into()
                            }),
                            workspace_snapshot: workspace.snapshot_ref,
                            resource_graph_snapshot: None,
                            evolution_snapshot: None,
                            federation_snapshot: None,
                        },
                    ),
                )?;
                let event_id = self.append(
                    run.clone(),
                    None,
                    provenance.clone(),
                    p::EventPayload::ProactiveProposalEmitted(p::ProactiveProposalEmittedPayload {
                        proposal_ref: proposal.reference(),
                        proposal_kind: proposal.kind_ref(),
                        level: protocol_intervention(proposal.level()),
                        guard: p::EmissionGuard {
                            value_gate_passed: value_passed,
                            competence_gate_passed: proposal.level() <= competence,
                            policy_and_envelope_passed: policy_allowed,
                        },
                        delivery: protocol_delivery_mode(proposal.core().delivery),
                        attention_cost: u32::from(
                            proposal.core().delivery == cognition::DeliveryMode::Interrupt,
                        ),
                    }),
                )?;
                proposal_events.push(event_id);
            } else {
                let candidate_id = p::CandidateId(format!("tick-candidate:{tick_index}:{index}"));
                let event_id = self.append(
                    run.clone(),
                    None,
                    provenance.clone(),
                    p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                        candidate_id,
                        target: p::CandidateTargetRef(format!("tick:{}", session.0)),
                        evidence_refs: Vec::new(),
                        confidence: p::Confidence::new(0.1)?,
                        provenance: provenance.clone(),
                        target_tier: p::StabilityTier::Working,
                        capability_update: None,
                        strategy_candidate: None,
                    }),
                )?;
                candidate_events.push(event_id);
            }
        }
        Ok(TickReport {
            schema_version: p::SchemaVersion(1),
            ran: true,
            skipped_reason: None,
            run: Some(run),
            candidate_events,
            proposal_events,
            workspace_snapshot,
        })
    }

    pub fn resolve_proactive_proposal(
        &self,
        run: p::RunId,
        proposal: p::ProposalRef,
        outcome: p::ProposalOutcome,
        feedback: Option<p::FeedbackRef>,
    ) -> p::Result<p::EventId> {
        if outcome == p::ProposalOutcome::Reject {
            if let Some(origin) = self
                .proposal_origins
                .lock()
                .map_err(|_| p::Error("proactivity proposal state is unavailable".into()))?
                .get(&proposal)
                .cloned()
            {
                self.suppressed_origins
                    .lock()
                    .map_err(|_| p::Error("proactivity suppression state is unavailable".into()))?
                    .insert(origin);
            }
        }
        self.append(
            run,
            None,
            p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            p::EventPayload::ProactiveProposalResolved(p::ProactiveProposalResolvedPayload {
                proposal_ref: proposal,
                outcome,
                feedback,
            }),
        )
    }

    fn scheduler_runtime(&self) -> p::Result<&SchedulerRuntime> {
        self.scheduler
            .as_ref()
            .ok_or_else(|| p::Error("scheduler is not enabled".into()))
    }

    fn validate_schedule_runtime(
        &self,
        command: &p::ScheduleCommand,
        now: p::Timestamp,
    ) -> p::Result<()> {
        if !matches!(
            command.intention.state,
            p::IntentionState::Pending | p::IntentionState::Fired
        ) {
            return Err(p::Error("schedule command is no longer runnable".into()));
        }
        let mut definition = command.clone();
        definition.intention.state = p::IntentionState::Pending;
        definition.validate()?;
        if command.intention.provenance.trust_tier == p::TrustTier::Untrusted {
            return Err(p::Error(
                "untrusted provenance cannot schedule a background run".into(),
            ));
        }
        let Some(global) = self.governance.envelope.as_ref() else {
            return Err(p::Error(
                "schedule requires an active autonomy envelope".into(),
            ));
        };
        if !scope_within(&command.envelope.scope, &global.scope)
            || command
                .envelope
                .capability
                .capabilities
                .iter()
                .any(|capability| !global.capability.capabilities.contains(capability))
            || command
                .envelope
                .capability
                .permissions
                .iter()
                .any(|permission| !global.capability.permissions.contains(permission))
            || command
                .envelope
                .action_type
                .iter()
                .any(|action| !global.action_type.contains(action))
            || command.envelope.risk_limit > global.risk_limit
            || command.envelope.timebox.starts_at < global.timebox.starts_at
            || command.envelope.timebox.expires_at > global.timebox.expires_at
            || protocol_budget_units(&command.budget)? > protocol_budget_units(&global.budget)?
            || protocol_budget_units(&command.budget)?
                > protocol_budget_units(&command.envelope.budget)?
        {
            return Err(p::Error(
                "schedule command exceeds the active autonomy envelope".into(),
            ));
        }
        if command
            .intention
            .expires_at
            .is_some_and(|expires_at| expires_at <= now)
            || now < command.envelope.timebox.starts_at
            || now > command.envelope.timebox.expires_at
        {
            return Err(p::Error("schedule command is outside its timebox".into()));
        }
        Ok(())
    }

    fn drive_schedule_claim(
        &self,
        claim: &p::ScheduleClaim,
        now: p::Timestamp,
    ) -> p::Result<p::RunId> {
        let run = schedule_run_id(&claim.command.intention.id);
        let request = scheduled_request(&claim.command);
        let existing = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        if !existing.is_empty() {
            let terminal_or_waiting = existing.iter().any(|event| {
                matches!(
                    event.kind,
                    p::EventKind::RunComplete
                        | p::EventKind::RunAborted
                        | p::EventKind::RunFailed
                        | p::EventKind::RunLimited
                        | p::EventKind::RunWaiting
                        | p::EventKind::ActionOutcomeUnknown
                )
            });
            if terminal_or_waiting {
                return Ok(run);
            }
            let safely_unstarted = !existing.iter().any(|event| {
                matches!(
                    event.kind,
                    p::EventKind::SessionBound
                        | p::EventKind::TurnStarted
                        | p::EventKind::ModelCallStarted
                        | p::EventKind::ActionPlanned
                        | p::EventKind::ActionStarted
                )
            });
            if !safely_unstarted {
                return Err(p::Error(
                    "scheduled run stopped after work began and requires recovery review".into(),
                ));
            }
            if !self
                .runs
                .lock()
                .map_err(|_| p::Error("run registry is unavailable".into()))?
                .contains_key(&run)
            {
                let handle = self.scheduled_run_handle(&request, &run, claim, now)?;
                self.runs
                    .lock()
                    .map_err(|_| p::Error("run registry is unavailable".into()))?
                    .insert(run.clone(), handle.clone());
                if let Some(key) = request.idempotency_key.clone() {
                    self.idempotency
                        .lock()
                        .map_err(|_| p::Error("run idempotency state is unavailable".into()))?
                        .insert(key, run.clone());
                }
                let session = p::SessionId(request.session.0.clone());
                self.drive_prepared_run(&run, &handle, &session)?;
            }
            return Ok(run);
        }

        let prepared = self.prepare_ingress_internal_as(request.clone(), Vec::new(), Some(run))?;
        match prepared {
            PreparedSubmission::Existing(run) => Ok(run),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                {
                    let mut record = handle
                        .record
                        .lock()
                        .map_err(|_| p::Error("run state is unavailable".into()))?;
                    configure_scheduled_record(&mut record, claim, now)?;
                }
                if claim.command.intention.source == p::IntentionSource::Commitment {
                    self.append_commitment_proactive_events(&run, claim, now)?;
                }
                self.drive_prepared_run(&run, &handle, &session)?;
                Ok(run)
            }
        }
    }

    fn scheduled_run_handle(
        &self,
        request: &p::RunRequest,
        run: &p::RunId,
        claim: &p::ScheduleClaim,
        now: p::Timestamp,
    ) -> p::Result<Arc<RunHandle>> {
        let context = (self.context_factory)(request, run, &self.config)?;
        let mut record = RunRecord {
            request: request.clone(),
            loop_ctx: forme_loop::RunCtx::new_with_input_provenance(
                p::SessionId(request.session.0.clone()),
                request.input.clone(),
                context,
                self.config.context_budget,
                self.config.loop_budget.clone(),
                request_provenance(request),
            )?,
            result: None,
            final_output: None,
            resume_state: None,
            pending_action: None,
            recovered_unknown: None,
            approval: InMemoryApprovalBroker::default(),
            mode: RunMode::Loop,
            direct_intent: None,
            bound_envelope: None,
            effect_mode: None,
            evolution_snapshot: None,
            evolution_snapshot_full: None,
            federation_snapshot: None,
            m3_binding: None,
            remaining_action_budget: None,
            competence_inputs: self.competence_snapshot.clone(),
            skills_prepared: false,
            evolution_simulation: None,
            long_horizon: None,
        };
        configure_scheduled_record(&mut record, claim, now)?;
        Ok(Arc::new(RunHandle::new(record)))
    }

    fn append_commitment_proactive_events(
        &self,
        run: &p::RunId,
        claim: &p::ScheduleClaim,
        _now: p::Timestamp,
    ) -> p::Result<()> {
        let caused_by = Some(claim.claim_event.clone());
        let provenance = p::Provenance {
            source: p::Source::Schedule,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by,
        };
        let seed = vec![p::NodeId(claim.command.intention.seed.0.clone())];
        self.append(
            run.clone(),
            None,
            provenance.clone(),
            p::EventPayload::ObservationRecorded(p::ObservationRecordedPayload {
                source: p::Source::Schedule,
                scope: claim.command.envelope.scope.clone(),
                grant_ref: None,
            }),
        )?;
        self.append(
            run.clone(),
            None,
            provenance.clone(),
            p::EventPayload::OpportunityDetected(p::OpportunityDetectedPayload {
                seed: seed.clone(),
                activation_shape: None,
            }),
        )?;
        self.append(
            run.clone(),
            None,
            provenance.clone(),
            p::EventPayload::ValueGateEvaluated(p::ValueGateEvaluatedPayload {
                decision: p::GateDecision("worth".into()),
                reason: p::ReasonRef("due owner commitment passed the deterministic floor".into()),
            }),
        )?;
        self.append(
            run.clone(),
            None,
            provenance.clone(),
            p::EventPayload::ImpulseRaised(p::ImpulseRaisedPayload {
                source: p::ImpulseSource::Commitment,
                reach: p::Reach(2),
                seed,
            }),
        )?;
        let competence_inputs = cognition::CompetenceInputs::default();
        let ceiling = self.competence.ceiling(
            claim.command.envelope.scope.clone(),
            claim.command.envelope.risk_limit,
            &competence_inputs,
        );
        self.append(
            run.clone(),
            None,
            provenance.clone(),
            p::EventPayload::CompetenceGateEvaluated(p::CompetenceGateEvaluatedPayload {
                scope: claim.command.envelope.scope.clone(),
                risk: claim.command.envelope.risk_limit,
                max_level: protocol_intervention(ceiling),
                reads: competence_inputs.protocol_reads(),
            }),
        )?;
        let current = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let snapshot = cognition::AgentWorkspaceProjection::rebuild(&current, 10);
        self.append(
            run.clone(),
            None,
            provenance.clone(),
            p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                trace_ref: p::DecisionTraceRef(format!(
                    "commitment-trace:{}",
                    claim.command.intention.id.0
                )),
                refs: p::DecisionRefs {
                    map: None,
                    user: None,
                    agent_self: None,
                    trust: None,
                    failure: Vec::new(),
                },
                rationale: p::Rationale(
                    "a due commitment entered AgentWorkspace and remained bounded by its schedule envelope"
                        .into(),
                ),
                workspace_snapshot: snapshot.snapshot_ref,
                resource_graph_snapshot: None,
                evolution_snapshot: None,
                federation_snapshot: None,
            }),
        )?;
        let proposal = p::ProposalRef(format!(
            "proposal:intention:{}",
            claim.command.intention.id.0
        ));
        self.proposal_origins
            .lock()
            .map_err(|_| p::Error("proactivity proposal state is unavailable".into()))?
            .insert(
                proposal.clone(),
                format!("intention:{}", claim.command.intention.id.0),
            );
        self.append(
            run.clone(),
            None,
            provenance.clone(),
            p::EventPayload::ProactiveProposalEmitted(p::ProactiveProposalEmittedPayload {
                proposal_ref: proposal.clone(),
                proposal_kind: p::ProposalKind("communication".into()),
                level: protocol_intervention(cognition::InterventionLevel::L1Suggest.min(ceiling)),
                guard: p::EmissionGuard {
                    value_gate_passed: true,
                    competence_gate_passed: ceiling >= cognition::InterventionLevel::L1Suggest,
                    policy_and_envelope_passed: true,
                },
                delivery: p::DeliveryMode::Interrupt,
                attention_cost: 1,
            }),
        )?;
        self.append(
            run.clone(),
            None,
            claim.command.intention.provenance.clone(),
            p::EventPayload::ProactiveProposalResolved(p::ProactiveProposalResolvedPayload {
                proposal_ref: proposal,
                outcome: p::ProposalOutcome::Adopt,
                feedback: None,
            }),
        )?;
        Ok(())
    }

    fn run_failure_followups(&self, now: p::Timestamp) -> p::Result<Vec<p::RunId>> {
        let mut followups = Vec::new();
        let max = self.scheduler_runtime()?.config.max_claims_per_tick as usize;
        'runs: for source_run in self.store.run_ids()? {
            let events = self
                .store
                .read_run(source_run)
                .collect::<p::Result<Vec<_>>>()?;
            let Some(session) = events.iter().find_map(|event| match &event.payload {
                p::EventPayload::RunAccepted(payload) => Some(payload.session_ref.clone()),
                _ => None,
            }) else {
                continue;
            };
            if !events.iter().any(|event| {
                matches!(
                    event.kind,
                    p::EventKind::RunComplete
                        | p::EventKind::RunAborted
                        | p::EventKind::RunFailed
                        | p::EventKind::RunLimited
                )
            }) {
                continue;
            }
            for event in events {
                if followups.len() >= max {
                    break 'runs;
                }
                let observation = match &event.payload {
                    p::EventPayload::VerificationFinished(payload)
                        if payload.outcome != p::VerificationOutcome::Pass =>
                    {
                        Some((cognition::ImpulseSource::Gap, p::Impact::Medium, 80, 65))
                    }
                    p::EventPayload::FailureEvidenceRecorded(payload)
                        if payload.impact == p::Impact::High =>
                    {
                        Some((cognition::ImpulseSource::Pressure, p::Impact::High, 100, 95))
                    }
                    _ => None,
                };
                let Some((signal, impact, value, urgency)) = observation else {
                    continue;
                };
                if self.followup_recorded(&event.event_id)? {
                    continue;
                }
                if self
                    .active_sessions
                    .lock()
                    .map_err(|_| p::Error("foreground activity state is unavailable".into()))?
                    .contains(&session)
                {
                    continue;
                }
                let failure_ref = match &event.payload {
                    p::EventPayload::FailureEvidenceRecorded(payload) => {
                        Some(payload.failure_ref.clone())
                    }
                    _ => None,
                };
                let verification_ref = match &event.payload {
                    p::EventPayload::VerificationFinished(_) => {
                        Some(p::EvidenceRef(event.event_id.0.clone()))
                    }
                    _ => None,
                };
                let snapshot = cognition::CognitionSnapshot {
                    schema_version: p::SchemaVersion(1),
                    now,
                    observations: vec![cognition::Observation {
                        schema_version: p::SchemaVersion(1),
                        source: p::Source::Internal,
                        scope: p::Scope(self.config.workspace.0.clone()),
                        grant_ref: None,
                        authorized: true,
                        seed: vec![p::NodeId(format!("evidence:{}", event.event_id.0))],
                        signal,
                        estimated_value: value,
                        urgency,
                        requested_level: cognition::InterventionLevel::L1Suggest,
                        delivery: if impact == p::Impact::High {
                            cognition::DeliveryMode::Interrupt
                        } else {
                            cognition::DeliveryMode::Hitchhike
                        },
                        proposal_intent: cognition::ProposalIntent::Communication(
                            cognition::CommunicationPurpose::Reminder,
                        ),
                    }],
                    competence: cognition::CompetenceInputs {
                        schema_version: p::SchemaVersion(1),
                        failure: failure_ref
                            .into_iter()
                            .map(|reference| cognition::FailureEvidenceInput {
                                schema_version: p::SchemaVersion(1),
                                reference,
                                impact,
                            })
                            .collect(),
                        verification: verification_ref
                            .into_iter()
                            .map(|reference| cognition::VerificationEvidenceInput {
                                schema_version: p::SchemaVersion(1),
                                reference,
                                passed: false,
                            })
                            .collect(),
                        ..cognition::CompetenceInputs::default()
                    },
                };
                let report = self.tick_with_snapshot(
                    session.clone(),
                    Some(cognition::TickTrigger::Diff),
                    snapshot,
                )?;
                if let Some(run) = report.run {
                    self.append(
                        p::RunId(format!("followup-link:{}", event.event_id.0)),
                        None,
                        p::Provenance {
                            source: p::Source::Internal,
                            actor: p::Actor::System,
                            trust_tier: p::TrustTier::VerifiedProcess,
                            caused_by: Some(event.event_id.clone()),
                        },
                        p::EventPayload::ObservationRecorded(p::ObservationRecordedPayload {
                            source: p::Source::Internal,
                            scope: p::Scope(self.config.workspace.0.clone()),
                            grant_ref: None,
                        }),
                    )?;
                    followups.push(run);
                }
            }
        }
        Ok(followups)
    }

    fn followup_recorded(&self, source: &p::EventId) -> p::Result<bool> {
        for run in self.store.run_ids()? {
            if self
                .store
                .read_run(run)
                .collect::<p::Result<Vec<_>>>()?
                .iter()
                .any(|event| event.provenance.caused_by.as_ref() == Some(source))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn proposal_policy_allows(&self, proposal: &cognition::Proposal) -> bool {
        if proposal.level() <= cognition::InterventionLevel::L2Prepare {
            return true;
        }
        let Some(envelope) = &self.governance.envelope else {
            return false;
        };
        scope_within(&proposal.scope(), &envelope.scope)
            && envelope.approval_rule != p::ApprovalRule::Deny
            && match proposal.level() {
                cognition::InterventionLevel::L3ActWithApproval => true,
                cognition::InterventionLevel::L4Autonomous => {
                    envelope.approval_rule == p::ApprovalRule::Allow
                        && envelope.risk_limit == p::Risk::Low
                }
                cognition::InterventionLevel::L5HighImpact => false,
                _ => true,
            }
    }

    pub fn recover_unknown_outcomes(&self) -> p::Result<Vec<p::RunId>> {
        let mut recovered = Vec::new();
        for run in self.store.run_ids()? {
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            if events.iter().any(|event| {
                matches!(
                    event.kind,
                    p::EventKind::RunComplete
                        | p::EventKind::RunAborted
                        | p::EventKind::RunFailed
                        | p::EventKind::RunLimited
                )
            }) {
                continue;
            }
            let mut started = BTreeMap::new();
            let mut terminal = BTreeSet::new();
            let mut unknown = BTreeMap::new();
            let mut accepted = None;
            for event in &events {
                match &event.payload {
                    p::EventPayload::RunAccepted(payload) => accepted = Some(payload.clone()),
                    p::EventPayload::ActionStarted(payload) => {
                        started.insert(payload.intent_id.clone(), event.clone());
                    }
                    p::EventPayload::ActionCompleted(payload) => {
                        terminal.insert(payload.intent_id.clone());
                    }
                    p::EventPayload::ActionFailed(payload) => {
                        terminal.insert(payload.intent_id.clone());
                    }
                    p::EventPayload::ActionCancelled(payload) => {
                        terminal.insert(payload.intent_id.clone());
                    }
                    p::EventPayload::ActionDenied(payload) => {
                        terminal.insert(payload.intent_id.clone());
                    }
                    p::EventPayload::ActionOutcomeUnknown(payload) => {
                        unknown.insert(payload.intent_id.clone(), event.event_id.clone());
                    }
                    _ => {}
                }
            }
            let Some((intent, started_event)) = started
                .into_iter()
                .find(|(intent, _)| !terminal.contains(intent))
            else {
                continue;
            };
            if self
                .runs
                .lock()
                .map_err(|_| p::Error("run registry is unavailable".into()))?
                .contains_key(&run)
            {
                continue;
            }
            let accepted = accepted
                .ok_or_else(|| p::Error("recoverable run has no RunAccepted event".into()))?;
            let request = p::RunRequest {
                schema_version: p::SchemaVersion(1),
                source: accepted.source,
                session: p::SessionRef(accepted.session_ref.0.clone()),
                agent_profile: p::AgentProfileRef("agent:recovered".into()),
                input: p::RunInput(accepted.input_ref.0),
                budget: None,
                idempotency_key: accepted.idempotency_key,
            };
            let context = (self.context_factory)(&request, &run, &self.config)?;
            let mut loop_ctx = forme_loop::RunCtx::new_with_input_provenance(
                accepted.session_ref.clone(),
                request.input.clone(),
                context,
                self.config.context_budget,
                self.config.loop_budget.clone(),
                request_provenance(&request),
            )?;
            let snapshot_ref = if let Some(snapshot) = unknown.get(&intent) {
                snapshot.clone()
            } else {
                let unknown_event = self.append(
                    run.clone(),
                    started_event.turn_id.clone(),
                    internal_provenance(p::Source::Internal),
                    p::EventPayload::ActionOutcomeUnknown(p::ActionOutcomeUnknownPayload {
                        intent_id: intent.clone(),
                        probe_hint: p::ProbeHintRef(
                            "manual adjudication required; action was not retried".into(),
                        ),
                        remote_lease: None,
                    }),
                )?;
                self.append(
                    run.clone(),
                    started_event.turn_id.clone(),
                    internal_provenance(p::Source::Internal),
                    p::EventPayload::RunWaiting(p::RunWaitingPayload {
                        wait_reason: p::WaitReason("unknown_action_outcome".into()),
                        resume_ref: p::ResumeRef(format!("resume:{}", unknown_event.0)),
                    }),
                )?;
                unknown_event
            };
            let pending = PendingKind::ToolInterrupt(intent.clone());
            loop_ctx.suspend(pending.clone());
            let record = RunRecord {
                request,
                loop_ctx,
                result: None,
                final_output: None,
                resume_state: Some(ResumeState {
                    schema_version: p::SchemaVersion(1),
                    run: run.clone(),
                    at: LoopState::Suspended(pending.clone()),
                    pending,
                    snapshot_ref,
                }),
                pending_action: None,
                recovered_unknown: Some(intent),
                approval: InMemoryApprovalBroker::default(),
                mode: RunMode::Loop,
                direct_intent: None,
                bound_envelope: None,
                effect_mode: events.iter().find_map(|event| match &event.payload {
                    p::EventPayload::SessionBound(payload) => payload.effect_mode,
                    _ => None,
                }),
                evolution_snapshot: events.iter().find_map(|event| match &event.payload {
                    p::EventPayload::SessionBound(payload) => payload.evolution_snapshot.clone(),
                    _ => None,
                }),
                evolution_snapshot_full: None,
                federation_snapshot: events.iter().find_map(|event| match &event.payload {
                    p::EventPayload::SessionBound(payload) => payload.federation_snapshot.clone(),
                    _ => None,
                }),
                m3_binding: None,
                remaining_action_budget: None,
                competence_inputs: self.competence_snapshot.clone(),
                skills_prepared: false,
                evolution_simulation: None,
                long_horizon: None,
            };
            self.runs
                .lock()
                .map_err(|_| p::Error("run registry is unavailable".into()))?
                .insert(run.clone(), Arc::new(RunHandle::new(record)));
            recovered.push(run);
        }
        Ok(recovered)
    }

    fn run_handle(&self, run: &p::RunId) -> p::Result<Arc<RunHandle>> {
        self.runs
            .lock()
            .map_err(|_| p::Error("run registry is unavailable".into()))?
            .get(run)
            .cloned()
            .ok_or_else(|| p::Error("run is not registered".into()))
    }

    fn session_lock(&self, session: &p::SessionId) -> p::Result<Arc<Mutex<()>>> {
        Ok(self
            .session_locks
            .lock()
            .map_err(|_| p::Error("session queue registry is unavailable".into()))?
            .entry(session.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }

    fn append(
        &self,
        run: p::RunId,
        turn: Option<p::TurnId>,
        provenance: p::Provenance,
        payload: p::EventPayload,
    ) -> p::Result<p::EventId> {
        append_event(
            &self.store,
            &self.event_sequence,
            run,
            turn,
            provenance,
            payload,
        )
    }

    fn persist_loop_events(&self, run: &p::RunId, record: &mut RunRecord) -> p::Result<()> {
        let turn = p::TurnId(format!("turn:{}:{}", run.0, record.loop_ctx.turn_index));
        let provenance = internal_provenance(record.request.source);
        for payload in record.loop_ctx.take_events() {
            self.append(run.clone(), Some(turn.clone()), provenance.clone(), payload)?;
        }
        Ok(())
    }

    fn bind_and_drive(&self, run: &p::RunId, handle: &Arc<RunHandle>) -> p::Result<()> {
        let session = {
            let record = handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?;
            record.loop_ctx.session.clone()
        };
        self.active_sessions
            .lock()
            .map_err(|_| p::Error("foreground activity state is unavailable".into()))?
            .insert(session.clone());
        let result = self.bind_and_drive_inner(run, handle);
        self.active_sessions
            .lock()
            .map_err(|_| p::Error("foreground activity state is unavailable".into()))?
            .remove(&session);
        result
    }

    fn bind_and_drive_inner(&self, run: &p::RunId, handle: &Arc<RunHandle>) -> p::Result<()> {
        let mut record = handle
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?;
        let scope = p::Scope(self.config.workspace.0.clone());
        let strategy_scope = self.m3_scope_override.as_ref().unwrap_or(&scope);
        let resolved_snapshot = if self.m3_scope_override.is_some() {
            self.evolution_seed.clone()
        } else if self.store.has_active_strategies(strategy_scope)? {
            Some(self.store.snapshot(strategy_scope.clone())?)
        } else {
            self.evolution_seed.clone()
        };
        if let Some(snapshot) = resolved_snapshot.as_ref() {
            snapshot.validate()?;
            if !snapshot
                .strategies
                .iter()
                .any(|strategy| scope_within(strategy_scope, &strategy.scope))
            {
                return Err(p::Error(
                    "evolution snapshot has no strategy for the run workspace scope".into(),
                ));
            }
        }
        let m3_binding = match (&self.m3_domains, resolved_snapshot.as_ref()) {
            (Some(runtime), Some(snapshot)) => {
                Some(runtime.bind(snapshot, strategy_scope, &self.model.profile())?)
            }
            (Some(_), None) => {
                return Err(p::Error(
                    "M3 domain runtime requires a pinned evolution snapshot".into(),
                ));
            }
            (None, _) => None,
        };
        if let Some(binding) = &m3_binding {
            if let Some(spec) = &binding.loop_spec {
                record.loop_ctx.bind_strategy(spec.clone())?;
            }
            if let Some(plan) = &binding.model_scaffold {
                record.loop_ctx.bind_model_scaffold(plan.scaffold.clone())?;
            }
        }
        let effect_mode = match record.request.source {
            p::Source::Replay => Some(p::EffectMode::ExactReplay),
            p::Source::Simulation => Some(p::EffectMode::CounterfactualDeny),
            _ if resolved_snapshot.is_some() => Some(p::EffectMode::LiveGoverned),
            _ => None,
        };
        if matches!(
            record.request.source,
            p::Source::Replay | p::Source::Simulation
        ) && resolved_snapshot.is_none()
        {
            return Err(p::Error(
                "replay and simulation runs require a pinned evolution snapshot".into(),
            ));
        }
        record.effect_mode = effect_mode;
        record.evolution_snapshot = resolved_snapshot
            .as_ref()
            .map(|snapshot| snapshot.snapshot.clone());
        record.evolution_snapshot_full = resolved_snapshot.clone();
        let federation_snapshot = self
            .federation
            .snapshot(p::Scope(self.config.workspace.0.clone()))?;
        record.federation_snapshot = (!federation_snapshot.grants.is_empty())
            .then_some(p::FederationSnapshotRef(federation_snapshot.digest.0));
        record.m3_binding = m3_binding;
        self.append(
            run.clone(),
            None,
            internal_provenance(record.request.source),
            p::EventPayload::SessionBound(p::SessionBoundPayload {
                policy_profile: self.config.policy_profile.clone(),
                model_profile: self.config.model_profile.clone(),
                toolset_ref: self.config.toolset_ref.clone(),
                workspace: self.config.workspace.clone(),
                effect_mode,
                evolution_snapshot: record.evolution_snapshot.clone(),
                federation_snapshot: record.federation_snapshot.clone(),
            }),
        )?;
        record.loop_ctx.state = LoopState::SessionBound;
        if let Some(selection) = record
            .m3_binding
            .as_ref()
            .and_then(|binding| binding.selection_for(p::SelectionTarget::Capability))
        {
            self.append(
                run.clone(),
                None,
                internal_provenance(p::Source::Internal),
                p::EventPayload::ToolsetResolved(p::ToolsetResolvedPayload {
                    toolset_ref: self.config.toolset_ref.clone(),
                    sources: vec![p::CapabilitySourceRef(format!(
                        "m3-selection:{}",
                        selection.version.0
                    ))],
                }),
            )?;
        }
        if record.mode == RunMode::Loop {
            if let Some(pending) = record.long_horizon.clone() {
                self.append(
                    run.clone(),
                    None,
                    internal_provenance(p::Source::Internal),
                    p::EventPayload::GoalFramed(p::GoalFramedPayload {
                        goal_frame: pending.goal.goal_frame.clone(),
                        long_term: Some(pending.goal.clone()),
                    }),
                )?;
                drop(record);
                let route_outcome = self.execute_route(run.clone(), pending.route.clone())?;
                if route_outcome.status != coordination::RouteStatus::Completed {
                    return Err(p::Error(
                        "long-horizon checkpoint route did not satisfy its result contract".into(),
                    ));
                }
                record = handle
                    .record
                    .lock()
                    .map_err(|_| p::Error("run state is unavailable".into()))?;
                let evidence_refs = self
                    .store
                    .read_run(run.clone())
                    .map(|event| event.map(|event| event.event_id))
                    .collect::<p::Result<Vec<_>>>()?;
                let created_at = now_ms();
                let checkpoint = p::GoalCheckpoint {
                    schema_version: p::SchemaVersion(1),
                    reference: pending.checkpoint.clone(),
                    goal_frame: pending.goal.goal_frame.clone(),
                    intention: pending.intention.clone(),
                    route: pending.route.reference.clone(),
                    artifact: pending.artifact.clone(),
                    situation_digest: pending.goal.situation_digest.clone(),
                    evidence_refs,
                    created_at,
                };
                checkpoint.validate()?;
                self.append(
                    run.clone(),
                    None,
                    internal_provenance(p::Source::Internal),
                    p::EventPayload::OrchestrationRouteCreated(
                        p::OrchestrationRouteCreatedPayload {
                            pattern_ref: pending.route.pattern_ref.clone(),
                            route: pending.route.reference.clone(),
                            goal_frame: Some(pending.goal.goal_frame.clone()),
                            checkpoint: Some(checkpoint),
                        },
                    ),
                )?;
                self.append(
                    run.clone(),
                    None,
                    internal_provenance(p::Source::Internal),
                    p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                        node_id: p::NodeId(format!(
                            "long-horizon-checkpoint:{}:{}",
                            pending.goal.goal_frame.0, pending.index
                        )),
                        kind: p::MemoryNodeType("checkpoint".into()),
                        content_ref: pending.artifact,
                        tier: p::StabilityTier::Working,
                        confidence: p::Confidence(1.0),
                        scope: pending.goal.scope,
                        resting_activation: p::RestingActivation(0.5),
                        recency: p::Recency(created_at),
                    }),
                )?;
                if pending.deferred_for_foreground {
                    let resume_ref = p::ResumeRef(format!(
                        "long-horizon-resume:{}:{}",
                        pending.goal.goal_frame.0, pending.index
                    ));
                    self.append(
                        run.clone(),
                        None,
                        internal_provenance(p::Source::Internal),
                        p::EventPayload::RunWaiting(p::RunWaitingPayload {
                            wait_reason: p::WaitReason("foreground_priority".into()),
                            resume_ref: resume_ref.clone(),
                        }),
                    )?;
                    self.append(
                        run.clone(),
                        None,
                        internal_provenance(p::Source::Internal),
                        p::EventPayload::RunResumed(p::RunResumedPayload {
                            wait_reason: p::WaitReason("foreground_priority".into()),
                            resume_ref,
                        }),
                    )?;
                }
                record.long_horizon = None;
            }
        }
        if record.mode == RunMode::Loop {
            if let Some(reasoner) = &self.coordination {
                let context = self.coordination_context(run, &record)?;
                let goal = coordination::GoalInput::new(
                    p::GoalRef(format!("goal:{}", run.0)),
                    record.request.input.0.clone(),
                );
                let frame = reasoner.frame(goal, &context);
                match reasoner.plan(&frame) {
                    Ok((plan, done, envelope, trace)) => {
                        for payload in coordination_events(
                            &frame,
                            &plan,
                            &done,
                            &envelope,
                            &trace,
                            record.evolution_snapshot.clone(),
                            record.federation_snapshot.clone(),
                        ) {
                            self.append(
                                run.clone(),
                                None,
                                internal_provenance(p::Source::Internal),
                                payload,
                            )?;
                        }
                    }
                    Err(error) => {
                        let class = if error.0.starts_with("resource_selection_failure") {
                            p::FailureClass::ResourceSelectionFailure
                        } else if error.0.starts_with("context_failure") {
                            p::FailureClass::ContextFailure
                        } else {
                            p::FailureClass::GoalFramingFailure
                        };
                        self.append_failure(run, &record, class, &error.0)?;
                        record
                            .loop_ctx
                            .terminate(p::RunStatus::Aborted, StopReason::RetryExhausted);
                        self.finish_terminal(run, handle, &mut record)?;
                        return Ok(());
                    }
                }
            }
        }
        self.refresh_agent_workspace(run, &mut record)?;
        if let Some(binding) = &record.m3_binding {
            let workspace_snapshot = record
                .loop_ctx
                .context
                .sources
                .agent_workspace
                .as_ref()
                .map(|workspace| workspace.snapshot_ref.clone())
                .ok_or_else(|| {
                    p::Error("M3 domain trace requires an AgentWorkspace snapshot".into())
                })?;
            let versions = binding
                .active_versions()
                .into_iter()
                .map(|version| version.0)
                .collect::<Vec<_>>()
                .join(",");
            self.append(
                run.clone(),
                None,
                internal_provenance(p::Source::Internal),
                p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                    trace_ref: p::DecisionTraceRef(format!("m3-domain-trace:{}", run.0)),
                    refs: p::DecisionRefs {
                        map: None,
                        user: None,
                        agent_self: None,
                        trust: None,
                        failure: Vec::new(),
                    },
                    rationale: p::Rationale(format!("run-pinned M3 domain strategies: {versions}")),
                    workspace_snapshot,
                    resource_graph_snapshot: None,
                    evolution_snapshot: Some(binding.snapshot.clone()),
                    federation_snapshot: record.federation_snapshot.clone(),
                }),
            )?;
        }
        self.prepare_skill_context(run, &mut record)?;
        if record.mode == RunMode::DirectAction {
            let intent = record
                .direct_intent
                .take()
                .ok_or_else(|| p::Error("direct action run has no bound intent".into()))?;
            let tool = forme_models::ModelToolCall {
                schema_version: p::SchemaVersion(1),
                call_id: p::ToolCallId(format!("direct:{}", intent.intent_id.0)),
                tool: p::ToolRef(format!("backend:{:?}", intent.backend_hint)),
                arguments: serde_json::to_value(&intent.parameters).map_err(|error| {
                    p::Error(format!(
                        "failed to normalize direct action parameters: {error}"
                    ))
                })?,
                intent: Some(intent),
            };
            return match self.handle_tool(run, handle, &mut record, tool)? {
                Flow::Suspended | Flow::Terminal => Ok(()),
                Flow::Continue => Err(p::Error(
                    "direct action did not converge to a terminal or waiting state".into(),
                )),
            };
        }
        drop(record);
        self.drive(run, handle)
    }

    fn refresh_agent_workspace(&self, run: &p::RunId, record: &mut RunRecord) -> p::Result<()> {
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let projection = cognition::AgentWorkspaceProjection::rebuild(&events, 10);
        let by_id = events
            .iter()
            .map(|event| (event.event_id.clone(), event.provenance.clone()))
            .collect::<BTreeMap<_, _>>();
        record.loop_ctx.context.sources.agent_workspace = Some(AgentWorkspace {
            schema_version: p::SchemaVersion(1),
            snapshot_ref: projection.snapshot_ref,
            items: projection
                .items
                .into_iter()
                .map(|item| AgentWorkspaceItem {
                    schema_version: p::SchemaVersion(1),
                    id: item.id.clone(),
                    content: format!("{:?}:{}", item.kind, item.id),
                    value: item.value,
                    urgency: item.urgency,
                    provenance: by_id
                        .get(&item.event_ref)
                        .cloned()
                        .unwrap_or_else(|| internal_provenance(p::Source::Internal)),
                })
                .collect(),
        });
        record.loop_ctx.context.brain_call = self.coordination.is_some();
        Ok(())
    }

    fn coordination_context(
        &self,
        run: &p::RunId,
        record: &RunRecord,
    ) -> p::Result<coordination::CoordinationContext> {
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let event_refs = events
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>();
        let last_seq = events.last().map(|event| event.stream_seq).unwrap_or(0);
        let mut capabilities = self.governance.visible_capabilities.clone();
        if let (Some(runtime), Some(binding)) = (&self.m3_domains, &record.m3_binding) {
            capabilities = runtime.rank_capabilities(binding, capabilities)?;
        }
        let trusted = capabilities.iter().cloned().collect::<BTreeSet<_>>();
        let now = now_ms();
        let requested_budget = record
            .request
            .budget
            .clone()
            .unwrap_or_else(|| p::Budget("units:1".into()));
        let envelope = self
            .governance
            .envelope
            .clone()
            .unwrap_or_else(|| p::AutonomyEnvelope {
                schema_version: p::SchemaVersion(1),
                scope: p::Scope(self.config.workspace.0.clone()),
                capability: p::CapabilitySet {
                    schema_version: p::SchemaVersion(1),
                    capabilities: capabilities.clone(),
                    permissions: self.governance.granted_permissions.clone(),
                },
                action_type: vec![p::ActionType::Analyze, p::ActionType::Prepare],
                risk_limit: p::Risk::Low,
                approval_rule: p::ApprovalRule::Ask,
                budget: requested_budget,
                timebox: p::Timebox {
                    schema_version: p::SchemaVersion(1),
                    starts_at: now,
                    expires_at: now.saturating_add(300_000),
                    max_turns: self.config.loop_budget.max_turns,
                },
                rollback: p::RollbackReq {
                    schema_version: p::SchemaVersion(1),
                    required: false,
                    boundary: None,
                },
            });
        Ok(coordination::CoordinationContext {
            schema_version: p::SchemaVersion(1),
            situation: coordination::SituationModel {
                schema_version: p::SchemaVersion(1),
                known: Vec::new(),
                missing: Vec::new(),
            },
            inventory: coordination::ResourceInventory {
                schema_version: p::SchemaVersion(1),
                tools: capabilities,
                skills: Vec::new(),
                mcp: Vec::new(),
                subagents: Vec::new(),
                trusted,
            },
            done_contract: coordination::DoneContract::final_output(p::DoneContractRef(format!(
                "done-contract:{}",
                run.0
            ))),
            autonomy_envelope: envelope,
            decision_refs: p::DecisionRefs {
                map: None,
                user: None,
                agent_self: None,
                trust: None,
                failure: Vec::new(),
            },
            workspace_snapshot: coordination::AgentWorkspaceSnapshot {
                schema_version: p::SchemaVersion(1),
                reference: p::AgentWorkspaceSnapshotRef(format!(
                    "agent-workspace:{}:{last_seq}",
                    run.0
                )),
                event_refs,
            },
            resource_graph: None,
            resource_required: false,
        })
    }

    pub fn execute_route(
        &self,
        parent: p::RunId,
        route: coordination::ExecutionRoute,
    ) -> p::Result<coordination::RouteOutcome> {
        let handle = self.run_handle(&parent)?;
        let route = {
            let record = handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?;
            if let Some(spec) = record
                .m3_binding
                .as_ref()
                .and_then(|binding| binding.coordination.as_ref())
            {
                coordination::apply_coordination_strategy(spec, &route)?.route
            } else {
                route
            }
        };
        self.append(
            parent.clone(),
            None,
            internal_provenance(p::Source::Internal),
            p::EventPayload::OrchestrationRouteCreated(p::OrchestrationRouteCreatedPayload {
                pattern_ref: route.pattern_ref.clone(),
                route: route.reference.clone(),
                goal_frame: None,
                checkpoint: None,
            }),
        )?;
        let parent_budget = self
            .governance
            .envelope
            .as_ref()
            .ok_or_else(|| p::Error("subagent route requires a parent autonomy envelope".into()))?
            .budget
            .clone();
        let mut runtime =
            coordination::RouteRuntime::new(route, protocol_budget_units(&parent_budget)?)?;
        while runtime.status() == coordination::RouteStatus::Running {
            let runnable = runtime
                .runnable()
                .into_iter()
                .map(|node| node.subtask.id.clone())
                .collect::<Vec<_>>();
            if runnable.is_empty() {
                return Err(p::Error("execution route has no runnable node".into()));
            }
            for node_id in runnable {
                runtime.start(&node_id)?;
                let node = runtime
                    .route()
                    .nodes
                    .iter()
                    .find(|node| node.subtask.id == node_id)
                    .cloned()
                    .ok_or_else(|| p::Error("route node disappeared before spawn".into()))?;
                match self.spawn_subagent(parent.clone(), &node) {
                    Ok(execution) if execution.result.status == p::RunStatus::Complete => {
                        runtime.complete(&node_id, execution.result_ref)?;
                    }
                    Ok(_) | Err(_) => {
                        runtime.fail(&node_id)?;
                    }
                }
                if runtime.status() != coordination::RouteStatus::Running {
                    break;
                }
            }
        }
        Ok(runtime.outcome())
    }

    pub fn spawn_subagent(
        &self,
        parent: p::RunId,
        node: &coordination::RouteNode,
    ) -> p::Result<SubagentExecution> {
        let parent_handle = self.run_handle(&parent)?;
        let (parent_evolution, parent_snapshot_ref) = {
            let record = parent_handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?;
            (
                record.evolution_snapshot_full.clone(),
                record.evolution_snapshot.clone(),
            )
        };
        if node.schema_version.0 == 0
            || node.role.schema_version.0 == 0
            || node.role.model != self.config.model_profile
            || node.role.toolset.toolset_ref != node.resource_slice.toolset_ref
        {
            return Err(p::Error(
                "subagent profile is incomplete or selects an unavailable model/toolset".into(),
            ));
        }
        let parent_grant = self.governance.delegation.as_ref().ok_or_else(|| {
            p::Error("subagent spawn requires an explicit delegation grant".into())
        })?;
        let parent_envelope = self
            .governance
            .envelope
            .as_ref()
            .ok_or_else(|| p::Error("subagent spawn requires an autonomy envelope".into()))?;
        if !scope_within(&node.role.permission, &parent_envelope.scope) {
            return Err(p::Error(
                "subagent permission is outside the parent's autonomy envelope".into(),
            ));
        }
        let child_budget = node.role.budget_units()?;
        if child_budget > protocol_budget_units(&parent_envelope.budget)? {
            return Err(p::Error(
                "subagent budget exceeds the parent's remaining budget".into(),
            ));
        }
        let child_capabilities = coordination::capability_refs(&node.resource_slice);
        if child_capabilities.iter().any(|capability| {
            !self.governance.visible_capabilities.contains(capability)
                || !parent_envelope.capability.capabilities.contains(capability)
        }) {
            return Err(p::Error(
                "subagent resource slice exceeds the parent's visible capabilities".into(),
            ));
        }
        let child_run = p::RunId(format!("child:{}:{}", parent.0, node.subtask.id));
        let child_workspace = p::WorkspaceRef(format!(
            "workspace:isolated:{}:{}",
            parent.0, node.subtask.id
        ));
        self.append(
            parent.clone(),
            None,
            internal_provenance(p::Source::Internal),
            p::EventPayload::SubagentSpawned(p::SubagentSpawnedPayload {
                child_run: child_run.clone(),
                role: node.role.role.clone(),
                toolset_ref: node.resource_slice.toolset_ref.clone(),
                model_profile: node.role.model.clone(),
                permission: p::PermissionProfileRef(format!(
                    "subagent-scope:{}",
                    node.role.permission.0
                )),
                budget: node.role.budget.clone(),
            }),
        )?;

        let mut child_envelope = parent_envelope.clone();
        child_envelope.scope = node.role.permission.clone();
        child_envelope.capability.capabilities = child_capabilities.clone();
        child_envelope.budget = node.role.budget.clone();
        let child_grant = DelegationGrant {
            schema_version: p::SchemaVersion(1),
            subject: forme_policy::DelegationSubject::Subagent(child_run.clone()),
            envelope: child_envelope.clone(),
            granted_by: parent_grant.granted_by.clone(),
            audit_ref: parent_grant.audit_ref.clone(),
        };
        let mut child_governance = self.governance.clone();
        child_governance.visible_capabilities = child_capabilities;
        child_governance.allowed_scopes = vec![node.role.permission.clone()];
        child_governance.file_roots = node
            .role
            .permission
            .0
            .strip_prefix("path:")
            .map(|path| vec![path.to_owned()])
            .unwrap_or_default();
        child_governance.network_allowed = false;
        child_governance.delegation = Some(child_grant);
        child_governance.envelope = Some(child_envelope);
        let mut child_config = self.config.clone();
        child_config.model_profile = node.role.model.clone();
        child_config.toolset_ref = node.resource_slice.toolset_ref.clone();
        child_config.workspace = child_workspace;
        let mut child_harness = ReactiveHarness::new(
            self.store.clone(),
            self.model.clone(),
            self.backends.clone(),
            child_governance,
            child_config,
        )?;
        if let Some(snapshot) = parent_evolution {
            child_harness = child_harness
                .with_evolution_seed(snapshot)?
                .with_m3_scope_override(p::Scope(self.config.workspace.0.clone()));
            if let Some(runtime) = &self.m3_domains {
                child_harness = child_harness.with_m3_domain_runtime(runtime.clone());
            }
        }
        let child_request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::Subagent,
            session: p::SessionRef(format!("session:child:{}:{}", parent.0, node.subtask.id)),
            agent_profile: p::AgentProfileRef(node.role.role.0.clone()),
            input: p::RunInput(node.subtask.instruction.clone()),
            budget: Some(node.role.budget.clone()),
            idempotency_key: Some(p::IdempotencyKey(format!(
                "subagent:{}:{}",
                parent.0, node.subtask.intent_id.0
            ))),
        };
        child_harness.submit_ingress_internal_as(
            child_request,
            Vec::new(),
            Some(child_run.clone()),
        )?;
        let result = child_harness.wait(child_run.clone())?;
        let summary = p::SummaryRef(format!("summary:{}", child_run.0));
        let result_ref = p::ResultRef(format!("result:{}", child_run.0));
        self.append(
            parent.clone(),
            None,
            internal_provenance(p::Source::Internal),
            p::EventPayload::SubagentResultReturned(p::SubagentResultReturnedPayload {
                child_run: child_run.clone(),
                summary: summary.clone(),
                result_ref: result_ref.clone(),
                status: result.status,
            }),
        )?;
        let child_failures = child_harness
            .stream_events(child_run.clone())
            .filter_map(|event| match event.payload {
                p::EventPayload::FailureEvidenceRecorded(payload) => Some(payload.failure_ref),
                _ => None,
            })
            .collect::<Vec<_>>();
        let parent_events = self.stream_events(parent.clone()).events();
        let last_seq = parent_events
            .last()
            .map(|event| event.stream_seq)
            .unwrap_or(0);
        self.append(
            parent.clone(),
            None,
            internal_provenance(p::Source::Internal),
            p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                trace_ref: p::DecisionTraceRef(format!("subagent-trace:{}", child_run.0)),
                refs: p::DecisionRefs {
                    map: None,
                    user: None,
                    agent_self: None,
                    trust: None,
                    failure: child_failures,
                },
                rationale: p::Rationale(
                    "child execution returned only its summary and result contract".into(),
                ),
                workspace_snapshot: p::AgentWorkspaceSnapshotRef(format!(
                    "agent-workspace:{}:{last_seq}",
                    parent.0
                )),
                resource_graph_snapshot: None,
                evolution_snapshot: parent_snapshot_ref,
                federation_snapshot: None,
            }),
        )?;
        Ok(SubagentExecution {
            schema_version: p::SchemaVersion(1),
            child_run,
            result,
            summary,
            result_ref,
        })
    }

    fn drive(&self, run: &p::RunId, handle: &Arc<RunHandle>) -> p::Result<()> {
        loop {
            let mut record = handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?;
            if handle.cancelled.load(Ordering::SeqCst) {
                record
                    .loop_ctx
                    .terminate(p::RunStatus::Aborted, StopReason::UserCancel);
                self.finish_terminal(run, handle, &mut record)?;
                return Ok(());
            }
            self.compact_context_if_needed(run, &mut record)?;
            let state = match self.loop_engine.drive(run.clone(), &mut record.loop_ctx) {
                Ok(state) => state,
                Err(error) => {
                    record
                        .loop_ctx
                        .terminate(p::RunStatus::Failed, StopReason::RetryExhausted);
                    self.append_failure(
                        run,
                        &record,
                        p::FailureClass::ExecutionFailure,
                        &error.to_string(),
                    )?;
                    self.finish_terminal(run, handle, &mut record)?;
                    return Ok(());
                }
            };
            self.persist_loop_events(run, &mut record)?;
            if state.is_terminal() {
                self.finish_terminal(run, handle, &mut record)?;
                return Ok(());
            }
            match record.loop_ctx.effect.clone() {
                LoopEffect::Final(_) => {
                    if !self.verify(run, &record)? {
                        record
                            .loop_ctx
                            .terminate(p::RunStatus::Failed, StopReason::VerifyUnfixable);
                    } else {
                        record.loop_ctx.complete_final()?;
                    }
                    self.persist_loop_events(run, &mut record)?;
                    self.finish_terminal(run, handle, &mut record)?;
                    return Ok(());
                }
                LoopEffect::Tool(tool) => {
                    match self.handle_tool(run, handle, &mut record, *tool)? {
                        Flow::Continue => continue,
                        Flow::Suspended | Flow::Terminal => return Ok(()),
                    }
                }
                LoopEffect::Handoff(handoff) => {
                    self.append(
                        run.clone(),
                        Some(current_turn(run, record.loop_ctx.turn_index)),
                        internal_provenance(record.request.source),
                        p::EventPayload::HandoffRequested(p::HandoffRequestedPayload {
                            target: handoff.target.clone(),
                            reason: handoff.reason,
                        }),
                    )?;
                    let pending = PendingKind::Handoff(handoff.target);
                    let waiting = self.append_waiting(run, &record, "handoff", &pending)?;
                    record.loop_ctx.suspend(pending.clone());
                    record.resume_state = Some(ResumeState {
                        schema_version: p::SchemaVersion(1),
                        run: run.clone(),
                        at: LoopState::Handoff,
                        pending,
                        snapshot_ref: waiting,
                    });
                    handle.changed.notify_all();
                    return Ok(());
                }
                LoopEffect::None => {
                    return Err(p::Error(
                        "loop returned a nonterminal state without an effect".into(),
                    ));
                }
            }
        }
    }

    fn compact_context_if_needed(&self, run: &p::RunId, record: &mut RunRecord) -> p::Result<()> {
        let compactor = AutomaticCompactor::new(self.store.clone(), self.config.compaction)?;
        let report = compactor.compact_if_needed(
            &mut record.loop_ctx.context,
            record.loop_ctx.context_budget,
            p::SummaryRef(format!("context-summary:{}", run.0)),
        )?;
        if report.is_none() {
            return Ok(());
        }
        for payload in compactor.take_events() {
            self.append(
                run.clone(),
                Some(current_turn(run, record.loop_ctx.turn_index)),
                internal_provenance(p::Source::Internal),
                payload,
            )?;
        }
        Ok(())
    }

    fn prepare_skill_context(&self, run: &p::RunId, record: &mut RunRecord) -> p::Result<()> {
        let Some(registry) = &self.skill_registry else {
            return Ok(());
        };
        if record.skills_prepared {
            return Ok(());
        }
        let _guard = self
            .skill_runtime_lock
            .lock()
            .map_err(|_| p::Error("skill runtime selection is unavailable".into()))?;
        let hits = registry.search(SkillSearchQuery {
            schema_version: p::SchemaVersion(1),
            text: record.request.input.0.clone(),
            scope: record.loop_ctx.context.scope.clone(),
            limit: 8,
        })?;
        let mut events = registry.take_events();
        record.loop_ctx.context.sources.skills_metadata = hits
            .iter()
            .map(|hit| SkillMetadata {
                schema_version: hit.metadata.schema_version,
                id: hit.metadata.id.clone(),
                summary: hit.metadata.summary.clone(),
                scope: hit.metadata.scope.clone(),
                version: hit.metadata.version,
                trust: hit.metadata.trust,
            })
            .collect();
        if let Some(selected) = hits.first() {
            let body = registry.load_body(selected.metadata.id.clone(), LoadTrigger::Selected)?;
            events.extend(registry.take_events());
            record.loop_ctx.context.selected_skills = vec![selected.metadata.id.clone()];
            record
                .loop_ctx
                .context
                .sources
                .loaded_skill_bodies
                .push(LoadedSkillBody {
                    schema_version: p::SchemaVersion(1),
                    id: selected.metadata.id.clone(),
                    body: body.0,
                    trigger: p::SkillTriggerRef("selected".into()),
                    provenance: p::Provenance {
                        source: p::Source::Internal,
                        actor: p::Actor::System,
                        trust_tier: selected.metadata.trust,
                        caused_by: None,
                    },
                });
        }
        for payload in events {
            self.append(
                run.clone(),
                Some(current_turn(run, record.loop_ctx.turn_index)),
                internal_provenance(p::Source::Internal),
                payload,
            )?;
        }
        record.skills_prepared = true;
        Ok(())
    }

    fn handle_tool(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        tool: forme_models::ModelToolCall,
    ) -> p::Result<Flow> {
        let turn = Some(current_turn(run, record.loop_ctx.turn_index));
        let provenance = internal_provenance(record.request.source);
        self.append(
            run.clone(),
            turn.clone(),
            provenance.clone(),
            p::EventPayload::ToolCallProposed(p::ToolCallProposedPayload {
                call_id: tool.call_id.clone(),
                tool: tool.tool.clone(),
                args: tool.arguments.clone(),
            }),
        )?;
        let Some(mut intent) = tool.intent.clone() else {
            self.append(
                run.clone(),
                turn,
                provenance,
                p::EventPayload::ActionDenied(p::ActionDeniedPayload {
                    intent_id: p::ActionId(tool.call_id.0),
                    reason: p::ReasonRef(
                        "tool proposal could not resolve to an action intent".into(),
                    ),
                }),
            )?;
            record
                .loop_ctx
                .terminate(p::RunStatus::Aborted, StopReason::HandoffNoTargetOrLoop);
            self.finish_terminal(run, handle, record)?;
            return Ok(Flow::Terminal);
        };
        intent.source = record.request.source;
        let context = self.governance.policy_context(
            record.loop_ctx.session.clone(),
            self.config.toolset_ref.clone(),
            &self.policy,
            record.bound_envelope.as_ref(),
        );
        let mut evaluation = self.policy.evaluate_detailed(&context, &intent);
        if record.effect_mode == Some(p::EffectMode::CounterfactualDeny) {
            self.append(run.clone(), turn, provenance, evaluation.event_payload())?;
            record.loop_ctx.mark_policy(p::PolicyDecision::Deny);
            if record.evolution_simulation.is_some() {
                self.complete_evolution_simulation(run, handle, record, &intent)?;
            } else {
                self.deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    p::ReasonRef("simulation_effect_denied".into()),
                    StopReason::ApprovalDenied,
                )?;
            }
            return Ok(Flow::Terminal);
        }
        let plan = if evaluation.decision == p::PolicyDecision::Deny {
            None
        } else {
            Some(self.backends.backend(intent.backend_hint)?.plan(&intent)?)
        };
        let external_floor = external_policy_floor(
            &intent,
            plan.as_ref().map(|plan| &plan.rollback_boundary),
            evaluation.decision,
            context.envelope.as_ref(),
            &record.competence_inputs,
            self.competence.as_ref(),
        );
        let effective_decision = untrusted_ingress_policy_floor(&intent, external_floor);
        if effective_decision != evaluation.decision {
            let external_floor_changed = external_floor != evaluation.decision;
            evaluation.decision = effective_decision;
            if external_floor_changed {
                evaluation.rule_source = p::RuleSourceRef("external-action-floor".into());
                evaluation.reason = p::ReasonRef(
                    "external action requires plan-bound approval under canonical section 24"
                        .into(),
                );
            } else {
                evaluation.rule_source = p::RuleSourceRef("untrusted-ingress-floor".into());
                evaluation.reason = p::ReasonRef(
                    "an action derived from untrusted communication requires owner approval".into(),
                );
            }
        }
        self.append(
            run.clone(),
            turn.clone(),
            provenance.clone(),
            evaluation.event_payload(),
        )?;
        record.loop_ctx.mark_policy(effective_decision);
        match effective_decision {
            p::PolicyDecision::Deny => {
                self.deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    evaluation.reason,
                    StopReason::ApprovalDenied,
                )?;
                Ok(Flow::Terminal)
            }
            p::PolicyDecision::Allow | p::PolicyDecision::Ask => {
                let plan = plan.ok_or_else(|| {
                    p::Error("allowed action is missing its immutable execution plan".into())
                })?;
                if effective_decision == p::PolicyDecision::Ask {
                    let approval_id =
                        p::ApprovalId(format!("approval:{}:{}", run.0, record.loop_ctx.turn_index));
                    let request = approval_request(
                        approval_id.clone(),
                        &record.loop_ctx.session,
                        &intent,
                        &plan,
                        &self.config,
                    )?;
                    let ticket = record.approval.request(request)?;
                    for payload in record.approval.take_events() {
                        self.append(run.clone(), turn.clone(), provenance.clone(), payload)?;
                    }
                    let pending = PendingKind::ApprovalWait(approval_id);
                    let waiting = self.append_waiting(run, record, "approval", &pending)?;
                    record.loop_ctx.suspend(pending.clone());
                    record.resume_state = Some(ResumeState {
                        schema_version: p::SchemaVersion(1),
                        run: run.clone(),
                        at: LoopState::ApprovalWait,
                        pending,
                        snapshot_ref: waiting,
                    });
                    record.pending_action = Some(PendingAction {
                        intent,
                        plan,
                        ticket,
                    });
                    handle.changed.notify_all();
                    Ok(Flow::Suspended)
                } else {
                    self.execute_action(run, handle, record, intent, plan, None)?;
                    Ok(if record.loop_ctx.state.is_terminal() {
                        Flow::Terminal
                    } else if record.loop_ctx.state.is_suspended() {
                        Flow::Suspended
                    } else {
                        Flow::Continue
                    })
                }
            }
        }
    }

    fn execute_action(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        intent: p::ActionIntent,
        mut plan: ExecutionPlan,
        approval: Option<p::ApprovalId>,
    ) -> p::Result<()> {
        let turn = Some(current_turn(run, record.loop_ctx.turn_index));
        let provenance = internal_provenance(record.request.source);
        let policy_context = self.governance.policy_context(
            record.loop_ctx.session.clone(),
            self.config.toolset_ref.clone(),
            &self.policy,
            record.bound_envelope.as_ref(),
        );
        let recheck = self.policy.evaluate_detailed(&policy_context, &intent);
        self.append(
            run.clone(),
            turn.clone(),
            provenance.clone(),
            recheck.event_payload(),
        )?;
        if record.effect_mode == Some(p::EffectMode::CounterfactualDeny) {
            return self.deny_action(
                run,
                handle,
                record,
                &intent,
                p::ReasonRef("simulation_effect_denied".into()),
                StopReason::ApprovalDenied,
            );
        }
        if recheck.decision == p::PolicyDecision::Deny {
            return self.deny_action(
                run,
                handle,
                record,
                &intent,
                recheck.reason,
                StopReason::ApprovalDenied,
            );
        }
        let capability_recheck = match &self.capability_rechecker {
            Some(rechecker) => rechecker.recheck(&intent, &policy_context),
            None if matches!(
                intent.backend_hint,
                p::BackendKind::Mcp | p::BackendKind::AppApi
            ) =>
            {
                Err(p::Error(
                    "external capability execution has no registry rechecker".into(),
                ))
            }
            None => Ok(()),
        };
        if let Err(error) = capability_recheck {
            self.append(
                run.clone(),
                turn,
                provenance,
                p::EventPayload::ActionDenied(p::ActionDeniedPayload {
                    intent_id: intent.intent_id.clone(),
                    reason: p::ReasonRef(error.to_string()),
                }),
            )?;
            self.append_failure(
                run,
                record,
                p::FailureClass::ExecutionFailure,
                &error.to_string(),
            )?;
            record
                .loop_ctx
                .terminate(p::RunStatus::Aborted, StopReason::RetryExhausted);
            return self.finish_terminal(run, handle, record);
        }
        self.enforce_envelope(&intent, approval.is_some(), record.bound_envelope.as_ref())?;
        if requires_competence_gate(&intent) {
            let ceiling = self.competence.ceiling(
                intent.scope.clone(),
                intent.risk_hint,
                &record.competence_inputs,
            );
            self.append(
                run.clone(),
                turn.clone(),
                provenance.clone(),
                p::EventPayload::CompetenceGateEvaluated(p::CompetenceGateEvaluatedPayload {
                    scope: intent.scope.clone(),
                    risk: intent.risk_hint,
                    max_level: protocol_intervention(ceiling),
                    reads: record.competence_inputs.protocol_reads(),
                }),
            )?;
            if ceiling < required_intervention_level(&intent, &plan.rollback_boundary) {
                return self.deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    p::ReasonRef("competence ceiling downgraded the action below execution".into()),
                    StopReason::ApprovalDenied,
                );
            }
        }
        if let Some(approval) = approval {
            plan = plan.with_approval(approval);
        }
        plan.validate_digest()?;
        if record.remaining_action_budget == Some(0) {
            record.loop_ctx.terminate(
                p::RunStatus::Limited,
                StopReason::BudgetExhausted(forme_loop::BudgetKind::ToolCalls),
            );
            return self.finish_terminal(run, handle, record);
        }
        if let Some(remaining) = &mut record.remaining_action_budget {
            *remaining = remaining.saturating_sub(1);
        }
        record.loop_ctx.mark_action_planned();
        self.append(
            run.clone(),
            turn.clone(),
            provenance.clone(),
            p::EventPayload::ActionPlanned(p::ActionPlannedPayload {
                intent_id: intent.intent_id.clone(),
                plan_digest: plan.digest.clone(),
                backend: plan.backend,
                expected_effect: intent.expected_effect,
                source: intent.source,
                scope: intent.scope.clone(),
                approval_ref: plan.approval_ref.clone(),
                remote_placement: None,
            }),
        )?;
        record.loop_ctx.mark_action_running();
        let store = self.store.clone();
        let event_sequence = self.event_sequence.clone();
        let event_run = run.clone();
        let event_turn = turn.clone();
        let event_provenance = provenance.clone();
        let external_action = is_external_action(&intent);
        let sink = EventSink::with_observer(move |payload| {
            let payload = if external_action {
                stamp_external_payload(payload.clone())
            } else {
                payload.clone()
            };
            let provenance = if external_action && is_external_observation(&payload) {
                untrusted_action_provenance()
            } else {
                event_provenance.clone()
            };
            append_event(
                &store,
                &event_sequence,
                event_run.clone(),
                event_turn.clone(),
                provenance,
                payload,
            )
            .map(|_| ())
        });
        let execution = self
            .backends
            .execute(plan, &sink, handle.cancel_token.clone());
        match execution {
            Ok(result) => self.after_action_result(run, handle, record, result, external_action),
            Err(error) => {
                self.append_failure(
                    run,
                    record,
                    p::FailureClass::ExecutionFailure,
                    &error.to_string(),
                )?;
                if handle.cancelled.load(Ordering::SeqCst) {
                    record
                        .loop_ctx
                        .terminate(p::RunStatus::Aborted, StopReason::UserCancel);
                    self.finish_terminal(run, handle, record)
                } else {
                    record.loop_ctx.continue_after_tool_with_provenance(
                        ResolvedOutcome::Failed(
                            p::FailureEvidenceRef(format!(
                                "execution-failure:{}",
                                intent.intent_id.0
                            )),
                            error.to_string(),
                        ),
                        action_observation_provenance(external_action),
                    )?;
                    self.persist_loop_events(run, record)
                }
            }
        }
    }

    fn after_action_result(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        result: ActionResult,
        external_action: bool,
    ) -> p::Result<()> {
        match result.status {
            ActionStatus::Completed => {
                if !self.verify(run, record)? {
                    record
                        .loop_ctx
                        .terminate(p::RunStatus::Failed, StopReason::VerifyUnfixable);
                    self.finish_terminal(run, handle, record)
                } else if record.mode == RunMode::DirectAction {
                    record
                        .loop_ctx
                        .terminate(p::RunStatus::Complete, StopReason::FinalOutput);
                    self.finish_terminal(run, handle, record)
                } else {
                    record.loop_ctx.continue_after_tool_with_provenance(
                        ResolvedOutcome::Completed(result.result_ref, result.output),
                        action_observation_provenance(external_action),
                    )?;
                    self.persist_loop_events(run, record)
                }
            }
            ActionStatus::Cancelled => {
                record
                    .loop_ctx
                    .terminate(p::RunStatus::Aborted, StopReason::UserCancel);
                self.finish_terminal(run, handle, record)
            }
            ActionStatus::Failed => {
                if record.mode == RunMode::DirectAction {
                    record
                        .loop_ctx
                        .terminate(p::RunStatus::Failed, StopReason::RetryExhausted);
                    self.finish_terminal(run, handle, record)
                } else {
                    record.loop_ctx.continue_after_tool_with_provenance(
                        ResolvedOutcome::Failed(
                            p::FailureEvidenceRef(format!("failure:{}", result.result_ref.0)),
                            result.output,
                        ),
                        action_observation_provenance(external_action),
                    )?;
                    self.persist_loop_events(run, record)
                }
            }
            ActionStatus::Unknown => {
                let receipt = result.external_receipt.ok_or_else(|| {
                    p::Error("unknown action result has no external receipt".into())
                })?;
                if receipt.effect != p::EffectStatus::Unknown
                    || receipt.action.0.trim().is_empty()
                    || receipt.probe_hint.is_none()
                {
                    return Err(p::Error(
                        "unknown action result has no adjudication boundary".into(),
                    ));
                }
                let action = receipt.action;
                let pending = PendingKind::ToolInterrupt(action.clone());
                let waiting =
                    self.append_waiting(run, record, "unknown_action_outcome", &pending)?;
                record.loop_ctx.suspend(pending.clone());
                record.resume_state = Some(ResumeState {
                    schema_version: p::SchemaVersion(1),
                    run: run.clone(),
                    at: LoopState::Suspended(pending.clone()),
                    pending,
                    snapshot_ref: waiting,
                });
                record.recovered_unknown = Some(action);
                handle.changed.notify_all();
                Ok(())
            }
        }
    }

    fn verify(&self, run: &p::RunId, record: &RunRecord) -> p::Result<bool> {
        let passes = record
            .m3_binding
            .as_ref()
            .and_then(|binding| binding.model_scaffold.as_ref())
            .map_or(1, |plan| plan.scaffold.verification_passes);
        for pass in 0..passes {
            if !self.verify_once(run, record, pass, passes)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn verify_once(
        &self,
        run: &p::RunId,
        record: &RunRecord,
        pass: u16,
        total_passes: u16,
    ) -> p::Result<bool> {
        let turn = Some(current_turn(run, record.loop_ctx.turn_index));
        let provenance = internal_provenance(record.request.source);
        let against = p::DoneContractRef(format!("done-contract:{}", run.0));
        let verifier_kind = if total_passes == 1 {
            p::VerifierKind("deterministic".into())
        } else {
            p::VerifierKind(format!("deterministic-m3-pass-{}", pass + 1))
        };
        self.append(
            run.clone(),
            turn.clone(),
            provenance.clone(),
            p::EventPayload::VerificationStarted(p::VerificationStartedPayload {
                verifier_kind: verifier_kind.clone(),
                against: against.clone(),
            }),
        )?;
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let mut evidence = eval::EvidenceBundle::from_events(&events);
        let done = match &record.loop_ctx.effect {
            LoopEffect::Final(output) => {
                evidence = evidence.with_final_output(output.clone());
                eval::DoneContractRef::final_output(against.clone())
            }
            _ => evidence
                .latest_successful_action()
                .map(|result| eval::DoneContractRef::action(against.clone(), result))
                .unwrap_or_else(|| eval::DoneContractRef {
                    schema_version: p::SchemaVersion(1),
                    reference: against.clone(),
                    criteria: vec![eval::DoneCriterion {
                        schema_version: p::SchemaVersion(1),
                        id: "action-completed-event".into(),
                        evidence: eval::EvidenceRequirement::EventObserved(
                            p::EventKind::ActionCompleted,
                        ),
                    }],
                }),
        };
        let outcome = self.verifier.verify(&evidence, &done);
        let (protocol_outcome, passed, failure_detail) = match outcome {
            eval::VerificationOutcome::Pass => (p::VerificationOutcome::Pass, true, None),
            eval::VerificationOutcome::Fail(reference) => (
                p::VerificationOutcome::Fail,
                false,
                Some(format!(
                    "verification failed with evidence {}",
                    reference.reference.0
                )),
            ),
            eval::VerificationOutcome::Unverifiable(reason) => (
                p::VerificationOutcome::Unverifiable(p::ReasonRef(reason.clone())),
                false,
                Some(format!("verification is unverifiable: {reason}")),
            ),
        };
        self.append(
            run.clone(),
            turn,
            provenance,
            p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                verifier_kind,
                outcome: protocol_outcome,
                against,
            }),
        )?;
        if let Some(detail) = failure_detail {
            self.append_failure(run, record, p::FailureClass::VerificationFailure, &detail)?;
        }
        Ok(passed)
    }

    fn enforce_envelope(
        &self,
        intent: &p::ActionIntent,
        approved: bool,
        runtime_envelope: Option<&p::AutonomyEnvelope>,
    ) -> p::Result<()> {
        let (Some(grant), Some(envelope)) = (
            &self.governance.delegation,
            runtime_envelope.or(self.governance.envelope.as_ref()),
        ) else {
            return Err(p::Error(
                "action has no active delegation and autonomy envelope".into(),
            ));
        };
        match self.policy.enforce_envelope(grant, envelope, intent) {
            EnvelopeDecision::Within => Ok(()),
            EnvelopeDecision::NeedsApproval if approved => Ok(()),
            EnvelopeDecision::NeedsApproval => Err(p::Error(
                "action still requires approval at the envelope choke point".into(),
            )),
            EnvelopeDecision::OutOfScope => {
                Err(p::Error("action is outside the delegated envelope".into()))
            }
        }
    }

    fn deny_action(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        intent: &p::ActionIntent,
        reason: p::ReasonRef,
        stop: StopReason,
    ) -> p::Result<()> {
        self.append(
            run.clone(),
            Some(current_turn(run, record.loop_ctx.turn_index)),
            internal_provenance(record.request.source),
            p::EventPayload::ActionDenied(p::ActionDeniedPayload {
                intent_id: intent.intent_id.clone(),
                reason: reason.clone(),
            }),
        )?;
        self.append_failure(run, record, p::FailureClass::TrustFailure, &reason.0)?;
        record.loop_ctx.terminate(p::RunStatus::Aborted, stop);
        self.finish_terminal(run, handle, record)
    }

    fn complete_evolution_simulation(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        intent: &p::ActionIntent,
    ) -> p::Result<()> {
        let simulation = record
            .evolution_simulation
            .take()
            .ok_or_else(|| p::Error("evolution simulation context is missing".into()))?;
        self.append(
            run.clone(),
            Some(current_turn(run, record.loop_ctx.turn_index)),
            internal_provenance(record.request.source),
            p::EventPayload::ActionDenied(p::ActionDeniedPayload {
                intent_id: intent.intent_id.clone(),
                reason: p::ReasonRef("simulation_effect_denied".into()),
            }),
        )?;
        self.evolution_control
            .record_candidate(run.clone(), simulation.candidate.clone())?;
        let against = p::DoneContractRef(format!("done-contract:{}:effect-deny", run.0));
        self.append(
            run.clone(),
            Some(current_turn(run, record.loop_ctx.turn_index)),
            internal_provenance(p::Source::Internal),
            p::EventPayload::VerificationStarted(p::VerificationStartedPayload {
                verifier_kind: p::VerifierKind("effect-deny-simulation".into()),
                against: against.clone(),
            }),
        )?;
        self.append(
            run.clone(),
            Some(current_turn(run, record.loop_ctx.turn_index)),
            internal_provenance(p::Source::Internal),
            p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                verifier_kind: p::VerifierKind("effect-deny-simulation".into()),
                outcome: p::VerificationOutcome::Pass,
                against,
            }),
        )?;
        self.evolution_control.evaluate(
            run.clone(),
            &simulation.candidate,
            simulation.comparison,
        )?;
        record
            .loop_ctx
            .terminate(p::RunStatus::Complete, StopReason::FinalOutput);
        self.finish_terminal(run, handle, record)
    }

    fn append_failure(
        &self,
        run: &p::RunId,
        record: &RunRecord,
        class: p::FailureClass,
        detail: &str,
    ) -> p::Result<()> {
        let failure_ref = p::FailureEvidenceRef(format!(
            "failure:{}:{}:{}",
            run.0,
            record.loop_ctx.turn_index,
            now_nanos()
        ));
        self.append(
            run.clone(),
            Some(current_turn(run, record.loop_ctx.turn_index)),
            internal_provenance(p::Source::Internal),
            p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
                failure_ref,
                class,
                impact: p::Impact::Medium,
                scope: p::Scope(self.config.workspace.0.clone()),
                related_refs: Vec::new(),
                suggested_fix: Some(p::SuggestedFixRef(detail.to_owned())),
            }),
        )?;
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let digest = eval::FailureDigest::from_events(
            p::FailureDigestRef(format!("failure-digest:{}", run.0)),
            &events,
        );
        self.append(
            run.clone(),
            Some(current_turn(run, record.loop_ctx.turn_index)),
            internal_provenance(p::Source::Internal),
            digest.event_payload(),
        )?;
        Ok(())
    }

    fn append_waiting(
        &self,
        run: &p::RunId,
        record: &RunRecord,
        reason: &str,
        pending: &PendingKind,
    ) -> p::Result<p::EventId> {
        self.append(
            run.clone(),
            Some(current_turn(run, record.loop_ctx.turn_index)),
            internal_provenance(record.request.source),
            p::EventPayload::RunWaiting(p::RunWaitingPayload {
                wait_reason: p::WaitReason(reason.into()),
                resume_ref: p::ResumeRef(format!("resume:{pending:?}")),
            }),
        )
    }

    fn finish_terminal(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
    ) -> p::Result<()> {
        let LoopState::Terminal(status) = record.loop_ctx.state else {
            return Err(p::Error(
                "run cannot finish from a nonterminal state".into(),
            ));
        };
        let reason = record
            .loop_ctx
            .stop_reason
            .clone()
            .unwrap_or(StopReason::RetryExhausted)
            .as_protocol();
        let result_ref = if status == p::RunStatus::Complete {
            Some(p::EventId(format!("result:{}", run.0)))
        } else {
            None
        };
        let payload = match status {
            p::RunStatus::Complete => p::EventPayload::RunComplete(p::RunCompletePayload {
                stop_reason: reason.clone(),
                result_ref: result_ref.clone(),
            }),
            p::RunStatus::Aborted => p::EventPayload::RunAborted(p::RunAbortedPayload {
                stop_reason: reason.clone(),
                result_ref: None,
            }),
            p::RunStatus::Failed => p::EventPayload::RunFailed(p::RunFailedPayload {
                stop_reason: reason.clone(),
                result_ref: None,
            }),
            p::RunStatus::Limited => p::EventPayload::RunLimited(p::RunLimitedPayload {
                stop_reason: reason.clone(),
                result_ref: None,
            }),
            _ => {
                return Err(p::Error(
                    "waiting or running is not a terminal status".into(),
                ))
            }
        };
        self.append(
            run.clone(),
            None,
            internal_provenance(record.request.source),
            payload,
        )?;
        record.final_output = record.loop_ctx.final_output().map(str::to_owned);
        record.result = Some(p::RunResult {
            schema_version: p::SchemaVersion(1),
            status,
            stop_reason: reason,
            outputs: record
                .final_output
                .as_ref()
                .map(|_| vec![p::OutputRef(format!("model-output:{}", run.0))])
                .unwrap_or_default(),
            evidence_refs: Vec::new(),
        });
        record.resume_state = None;
        record.pending_action = None;
        handle.changed.notify_all();
        Ok(())
    }

    fn resume_approval(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        grant: forme_approval::ApprovalGrant,
    ) -> p::Result<()> {
        let Some(pending) = record.pending_action.clone() else {
            return Err(p::Error("run has no pending approval action".into()));
        };
        if pending.ticket.0 != grant.approval_id {
            return Err(p::Error("approval grant targets another ticket".into()));
        }
        if requires_one_shot_approval(&pending.intent, &pending.plan.rollback_boundary)
            && grant.granted_scope != GrantScope::OneShot
        {
            record.resume_state = None;
            record.pending_action = None;
            self.deny_action(
                run,
                handle,
                record,
                &pending.intent,
                p::ReasonRef(
                    "high-impact external action requires a one-shot owner approval".into(),
                ),
                StopReason::ApprovalDenied,
            )?;
            return Ok(());
        }
        record
            .approval
            .resolve(pending.ticket.clone(), grant.clone())?;
        let turn = Some(current_turn(run, record.loop_ctx.turn_index));
        let provenance = internal_provenance(record.request.source);
        for payload in record.approval.take_events() {
            self.append(run.clone(), turn.clone(), provenance.clone(), payload)?;
        }
        self.append(
            run.clone(),
            turn,
            provenance,
            p::EventPayload::RunResumed(p::RunResumedPayload {
                wait_reason: p::WaitReason("approval".into()),
                resume_ref: p::ResumeRef(format!("approval:{}", grant.approval_id.0)),
            }),
        )?;
        record.loop_ctx.mark_approval(grant.outcome);
        if grant.outcome != p::ApprovalOutcome::Granted {
            record.resume_state = None;
            record.pending_action = None;
            self.deny_action(
                run,
                handle,
                record,
                &pending.intent,
                p::ReasonRef("approval was denied or expired".into()),
                StopReason::ApprovalDenied,
            )?;
            return Ok(());
        }
        let authorization = record.approval.authorize(&ApprovalAuthorization {
            approval_id: grant.approval_id.clone(),
            session: record.loop_ctx.session.clone(),
            scope: pending.intent.scope.clone(),
            plan_digest: pending.plan.digest.clone(),
            policy_version: self.config.policy_version,
            tool_schema_version: self.config.tool_schema_version,
            now: now_ms(),
            intent: pending.intent.clone(),
        });
        if let Err(error) = authorization {
            record.resume_state = None;
            record.pending_action = None;
            self.deny_action(
                run,
                handle,
                record,
                &pending.intent,
                p::ReasonRef(error.to_string()),
                StopReason::ApprovalDenied,
            )?;
            return Ok(());
        }
        record.resume_state = None;
        record.pending_action = None;
        if let Err(error) = self.execute_action(
            run,
            handle,
            record,
            pending.intent.clone(),
            pending.plan,
            Some(grant.approval_id),
        ) {
            if !record.loop_ctx.state.is_terminal() {
                self.deny_action(
                    run,
                    handle,
                    record,
                    &pending.intent,
                    p::ReasonRef(error.to_string()),
                    StopReason::ApprovalDenied,
                )?;
            }
        }
        Ok(())
    }

    fn resume_unknown(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        action: p::ActionId,
        outcome: ResolvedOutcome,
    ) -> p::Result<()> {
        if record.recovered_unknown.as_ref() != Some(&action) {
            return Err(p::Error("tool adjudication targets another action".into()));
        }
        self.append(
            run.clone(),
            None,
            internal_provenance(p::Source::Internal),
            p::EventPayload::RunResumed(p::RunResumedPayload {
                wait_reason: p::WaitReason("unknown_action_outcome".into()),
                resume_ref: p::ResumeRef(format!("manual-outcome:{}", action.0)),
            }),
        )?;
        let (payload, status, reason) = match outcome {
            ResolvedOutcome::Completed(result_ref, _) => (
                p::EventPayload::ActionCompleted(p::ActionCompletedPayload {
                    intent_id: action,
                    result_ref,
                    receipt: None,
                    remote_receipt: None,
                }),
                p::RunStatus::Complete,
                StopReason::FinalOutput,
            ),
            ResolvedOutcome::Failed(failure_ref, _) => (
                p::EventPayload::ActionFailed(p::ActionFailedPayload {
                    intent_id: action,
                    failure_ref,
                    remote_lease: None,
                }),
                p::RunStatus::Failed,
                StopReason::RetryExhausted,
            ),
            ResolvedOutcome::Cancelled(reason_ref) => (
                p::EventPayload::ActionCancelled(p::ActionCancelledPayload {
                    intent_id: action,
                    reason: reason_ref,
                }),
                p::RunStatus::Aborted,
                StopReason::UserCancel,
            ),
        };
        self.append(
            run.clone(),
            None,
            internal_provenance(p::Source::Internal),
            payload,
        )?;
        record.loop_ctx.terminate(status, reason);
        record.resume_state = None;
        record.recovered_unknown = None;
        self.finish_terminal(run, handle, record)
    }

    fn submit_ingress_internal(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId> {
        self.submit_ingress_internal_as(request, prelude, None)
    }

    fn submit_ingress_internal_as(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
        forced_run: Option<p::RunId>,
    ) -> p::Result<p::RunId> {
        match self.prepare_ingress_internal_as(request, prelude, forced_run)? {
            PreparedSubmission::Existing(run) => Ok(run),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                self.drive_prepared_run(&run, &handle, &session)?;
                Ok(run)
            }
        }
    }

    fn prepare_ingress_internal_as(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
        forced_run: Option<p::RunId>,
    ) -> p::Result<PreparedSubmission> {
        if request.source == p::Source::Replay {
            return Err(p::Error(
                "exact replay requires the dedicated read-only replay audit path".into(),
            ));
        }
        if request.schema_version.0 == 0
            || request.session.0.trim().is_empty()
            || request.input.0.trim().is_empty()
            || prelude
                .iter()
                .any(|event| !event.is_authorized_by(&self.ingress_authority))
        {
            return Err(p::Error(
                "run request or ingress prelude is incomplete".into(),
            ));
        }
        if let Some(key) = &request.idempotency_key {
            if let Some(run) = self
                .idempotency
                .lock()
                .map_err(|_| p::Error("run idempotency state is unavailable".into()))?
                .get(key)
                .cloned()
            {
                return Ok(PreparedSubmission::Existing(run));
            }
        }
        let run = forced_run
            .unwrap_or_else(|| new_run_id(self.run_sequence.fetch_add(1, Ordering::SeqCst)));
        let context = (self.context_factory)(&request, &run, &self.config)?;
        let session = p::SessionId(request.session.0.clone());
        let loop_ctx = forme_loop::RunCtx::new_with_input_provenance(
            session.clone(),
            request.input.clone(),
            context,
            self.config.context_budget,
            self.config.loop_budget.clone(),
            request_provenance(&request),
        )?;
        let record = RunRecord {
            request: request.clone(),
            loop_ctx,
            result: None,
            final_output: None,
            resume_state: None,
            pending_action: None,
            recovered_unknown: None,
            approval: InMemoryApprovalBroker::default(),
            mode: RunMode::Loop,
            direct_intent: None,
            bound_envelope: None,
            effect_mode: None,
            evolution_snapshot: None,
            evolution_snapshot_full: None,
            federation_snapshot: None,
            m3_binding: None,
            remaining_action_budget: action_budget(request.budget.as_ref())?,
            competence_inputs: self.competence_snapshot.clone(),
            skills_prepared: false,
            evolution_simulation: None,
            long_horizon: None,
        };
        let handle = Arc::new(RunHandle::new(record));
        for ingress in prelude {
            self.append(run.clone(), None, ingress.provenance, ingress.payload)?;
        }
        self.append(
            run.clone(),
            None,
            request_provenance(&request),
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: request.source,
                session_ref: session.clone(),
                input_ref: p::InputRef(request.input.0.clone()),
                idempotency_key: request.idempotency_key.clone(),
            }),
        )?;
        self.runs
            .lock()
            .map_err(|_| p::Error("run registry is unavailable".into()))?
            .insert(run.clone(), handle.clone());
        if let Some(key) = request.idempotency_key {
            self.idempotency
                .lock()
                .map_err(|_| p::Error("run idempotency state is unavailable".into()))?
                .insert(key, run.clone());
        }
        Ok(PreparedSubmission::New {
            run,
            handle,
            session,
        })
    }

    fn drive_prepared_run(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        session: &p::SessionId,
    ) -> p::Result<()> {
        let session_lock = self.session_lock(session)?;
        let _session_guard = session_lock
            .lock()
            .map_err(|_| p::Error("session queue is unavailable".into()))?;
        if let Err(error) = self.bind_and_drive(run, handle) {
            self.fail_prepared_run(run, handle, &error)?;
        }
        Ok(())
    }

    fn fail_prepared_run(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        error: &p::Error,
    ) -> p::Result<()> {
        let mut record = handle
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?;
        if record.result.is_none() && !record.loop_ctx.state.is_terminal() {
            self.append_failure(
                run,
                &record,
                p::FailureClass::ExecutionFailure,
                &error.to_string(),
            )?;
            record
                .loop_ctx
                .terminate(p::RunStatus::Failed, StopReason::RetryExhausted);
            self.finish_terminal(run, handle, &mut record)?;
        }
        Ok(())
    }
}

impl AgentHarness for ReactiveHarness {
    fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId> {
        self.submit_ingress_internal(request, Vec::new())
    }

    fn stream_events(&self, run: p::RunId) -> EventStream {
        let events = self
            .store
            .read_run(run)
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        EventStream::new(events)
    }

    fn wait(&self, run: p::RunId) -> p::Result<p::RunResult> {
        let handle = self.run_handle(&run)?;
        let mut record = handle
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?;
        loop {
            if let Some(result) = &record.result {
                return Ok(result.clone());
            }
            if record.loop_ctx.state.is_suspended() {
                return Err(p::Error("run is suspended and requires ResumeInput".into()));
            }
            record = handle
                .changed
                .wait(record)
                .map_err(|_| p::Error("run wait state is unavailable".into()))?;
        }
    }

    fn cancel(&self, run: p::RunId) -> p::Result<()> {
        let handle = self.run_handle(&run)?;
        handle.cancelled.store(true, Ordering::SeqCst);
        handle.cancel_token.cancel();
        if let Ok(mut record) = handle.record.try_lock() {
            if record.loop_ctx.state.is_suspended() && record.result.is_none() {
                record
                    .loop_ctx
                    .terminate(p::RunStatus::Aborted, StopReason::UserCancel);
                self.finish_terminal(&run, &handle, &mut record)?;
            }
        }
        Ok(())
    }

    fn resume(&self, run: p::RunId, input: ResumeInput) -> p::Result<()> {
        let handle = self.run_handle(&run)?;
        let session = {
            let record = handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?;
            record.loop_ctx.session.clone()
        };
        let session_lock = self.session_lock(&session)?;
        let _session_guard = session_lock
            .lock()
            .map_err(|_| p::Error("session queue is unavailable".into()))?;
        let mut record = handle
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?;
        let resume = record
            .resume_state
            .clone()
            .ok_or_else(|| p::Error("run is not suspended".into()))?;
        match (&resume.pending, input) {
            (PendingKind::ApprovalWait(expected), ResumeInput::Approval(grant))
                if expected == &grant.approval_id =>
            {
                self.resume_approval(&run, &handle, &mut record, grant)?;
                if !record.loop_ctx.state.is_terminal() && !record.loop_ctx.state.is_suspended() {
                    drop(record);
                    self.drive(&run, &handle)?;
                }
                Ok(())
            }
            (PendingKind::ToolInterrupt(expected), ResumeInput::ToolOutcome(action, outcome))
                if expected == &action =>
            {
                self.resume_unknown(&run, &handle, &mut record, action, outcome)
            }
            (PendingKind::Handoff(expected), ResumeInput::Handoff(resolution))
                if expected == &resolution.target =>
            {
                self.append(
                    run.clone(),
                    Some(current_turn(&run, record.loop_ctx.turn_index)),
                    internal_provenance(record.request.source),
                    p::EventPayload::HandoffResolved(p::HandoffResolvedPayload {
                        target: resolution.target.clone(),
                        reason: p::ReasonRef(if resolution.accepted {
                            "handoff accepted".into()
                        } else {
                            "handoff rejected".into()
                        }),
                    }),
                )?;
                record.loop_ctx.continue_after_handoff(resolution)?;
                record.resume_state = None;
                self.persist_loop_events(&run, &mut record)?;
                if record.loop_ctx.state.is_terminal() {
                    self.finish_terminal(&run, &handle, &mut record)
                } else {
                    drop(record);
                    self.drive(&run, &handle)
                }
            }
            _ => Err(p::Error(
                "ResumeInput does not match the persisted PendingKind".into(),
            )),
        }
    }

    fn drain(&self, session: p::SessionId) -> p::Result<()> {
        let session_lock = self.session_lock(&session)?;
        let _guard = session_lock
            .lock()
            .map_err(|_| p::Error("session queue is unavailable".into()))?;
        Ok(())
    }
}

impl GatewayControl for ReactiveHarness {
    fn start_run(self: Arc<Self>, request: p::RunRequest) -> p::Result<p::RunId> {
        match self.prepare_ingress_internal_as(request, Vec::new(), None)? {
            PreparedSubmission::Existing(run) => Ok(run),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                let worker = self.clone();
                let worker_run = run.clone();
                let worker_handle = handle.clone();
                let worker_session = session.clone();
                if let Err(error) = std::thread::Builder::new()
                    .name("forme-gateway-run".into())
                    .spawn(move || {
                        if let Err(drive_error) =
                            worker.drive_prepared_run(&worker_run, &worker_handle, &worker_session)
                        {
                            let _ =
                                worker.fail_prepared_run(&worker_run, &worker_handle, &drive_error);
                        }
                    })
                {
                    let spawn_error = p::Error(format!("failed to start accepted run: {error}"));
                    self.fail_prepared_run(&run, &handle, &spawn_error)?;
                    return Err(spawn_error);
                }
                Ok(run)
            }
        }
    }

    fn gateway_profile(&self, surface: p::SurfaceRef) -> p::Result<p::GatewayProfile> {
        let profile = p::GatewayProfile {
            schema_version: p::SchemaVersion(1),
            surface,
            policy: self.config.policy_profile.clone(),
            model: self.config.model_profile.clone(),
            toolset: self.config.toolset_ref.clone(),
            workspace: self.config.workspace.clone(),
        };
        profile.validate()?;
        Ok(profile)
    }

    fn list_runs(&self) -> p::Result<Vec<p::RunSummary>> {
        let mut summaries = self
            .store
            .run_ids()?
            .into_iter()
            .filter_map(|run| {
                let events = self
                    .store
                    .read_run(run.clone())
                    .collect::<p::Result<Vec<_>>>();
                match events {
                    Ok(events)
                        if events
                            .iter()
                            .any(|event| event.kind == p::EventKind::RunAccepted) =>
                    {
                        Some(run_summary_from_events(run, &events))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .collect::<p::Result<Vec<_>>>()?;
        summaries.sort_by(|left, right| right.run.cmp(&left.run));
        Ok(summaries)
    }

    fn stream_event_page(&self, cursor: p::EventCursor) -> p::Result<p::EventPage> {
        cursor.validate()?;
        let events = self
            .store
            .read_run(cursor.run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let snapshot_upper_bound = events
            .last()
            .map(|event| event.stream_seq)
            .ok_or_else(|| p::Error("run event stream was not found".into()))?;
        if cursor.after_stream_seq > snapshot_upper_bound {
            return Err(p::Error("event cursor is ahead of the run snapshot".into()));
        }
        let page = p::EventPage {
            schema_version: p::SchemaVersion(1),
            run: cursor.run,
            after_stream_seq: cursor.after_stream_seq,
            snapshot_upper_bound,
            events: events
                .into_iter()
                .filter(|event| event.stream_seq > cursor.after_stream_seq)
                .collect(),
        };
        page.validate()?;
        Ok(page)
    }

    fn run_summary(&self, run: p::RunId) -> p::Result<p::RunSummary> {
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        run_summary_from_events(run, &events)
    }

    fn pending_approvals(&self, session: p::SessionId) -> p::Result<Vec<p::PendingApproval>> {
        let handles = self
            .runs
            .lock()
            .map_err(|_| p::Error("run registry is unavailable".into()))?
            .iter()
            .map(|(run, handle)| (run.clone(), handle.clone()))
            .collect::<Vec<_>>();
        let mut pending = Vec::new();
        for (run, handle) in handles {
            let record = handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?;
            pending.extend(
                record
                    .approval
                    .pending(ApprovalScope::Session(session.clone()))
                    .into_iter()
                    .map(|request| pending_approval(run.clone(), request)),
            );
        }
        pending.sort_by(|left, right| {
            (&left.run, &left.approval_id).cmp(&(&right.run, &right.approval_id))
        });
        for approval in &pending {
            approval.validate()?;
        }
        Ok(pending)
    }

    fn control(&self, run: p::RunId, control: p::RunControl) -> p::Result<()> {
        match control {
            p::RunControl::ResolveApproval(decision) => {
                decision.validate()?;
                AgentHarness::resume(
                    self,
                    run,
                    ResumeInput::Approval(ApprovalGrant {
                        schema_version: decision.schema_version,
                        approval_id: decision.approval_id,
                        outcome: decision.outcome,
                        granted_scope: GrantScope::OneShot,
                        approver: decision.approver,
                        bound_plan_digest: decision.bound_plan_digest,
                        policy_version: decision.policy_version,
                        tool_schema_version: decision.tool_schema_version,
                        nonce: decision.nonce,
                        use_by: decision.use_by,
                    }),
                )
            }
            p::RunControl::Cancel(request) => {
                request.validate()?;
                AgentHarness::cancel(self, run)
            }
        }
    }

    fn trace_view(&self, run: p::RunId) -> p::Result<p::TraceView> {
        let export = eval::TraceExporter::new(Arc::new(self.store.clone())).export(run.clone())?;
        let snapshot_upper_bound = export
            .events
            .last()
            .map(|event| event.stream_seq)
            .ok_or_else(|| p::Error("run trace was not found".into()))?;
        Ok(p::TraceView {
            schema_version: p::SchemaVersion(1),
            run,
            snapshot_upper_bound,
            events: export.events,
            failure_refs: export
                .failures
                .into_iter()
                .map(|failure| failure.failure_ref)
                .collect(),
            verification_outcomes: export.verification_outcomes,
        })
    }

    fn review_candidate(&self, command: p::CandidateReviewCommand) -> p::Result<()> {
        command.validate()?;
        let initial_events = self
            .store
            .read_run(command.run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let session = run_session(&initial_events)?;
        let session_lock = self.session_lock(&session)?;
        let _session_guard = session_lock
            .lock()
            .map_err(|_| p::Error("session queue is unavailable".into()))?;
        let events = self
            .store
            .read_run(command.run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let candidate = candidate_snapshot(&command.candidate, &events)?;
        if candidate.state != command.expected_state {
            return Err(p::Error("candidate review expected state is stale".into()));
        }
        if !command
            .evidence
            .iter()
            .all(|evidence| evidence_is_resolved(evidence, &candidate, &events))
        {
            return Err(p::Error(
                "candidate review evidence is not in the run trace".into(),
            ));
        }
        let reason = p::ReasonRef(format!(
            "owner review with {} trace evidence reference(s)",
            command.evidence.len()
        ));
        let provenance = p::Provenance {
            source: p::Source::UserTurn,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        };
        match command.decision {
            p::CandidateReviewDecision::Promote
                if matches!(
                    candidate.state,
                    p::CandidateReviewState::Candidate | p::CandidateReviewState::Downgraded
                ) =>
            {
                self.append(
                    command.run,
                    None,
                    provenance,
                    p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                        candidate_id: command.candidate,
                        by: p::DecisionActor::User,
                        reason,
                    }),
                )?;
            }
            p::CandidateReviewDecision::Reject
                if candidate.state == p::CandidateReviewState::Candidate =>
            {
                self.append(
                    command.run,
                    None,
                    provenance,
                    p::EventPayload::CandidateRejected(p::CandidateRejectedPayload {
                        candidate_id: command.candidate,
                        by: p::DecisionActor::User,
                        reason,
                    }),
                )?;
            }
            p::CandidateReviewDecision::Downgrade
                if matches!(
                    candidate.state,
                    p::CandidateReviewState::Candidate | p::CandidateReviewState::Promoted
                ) =>
            {
                self.append(
                    command.run,
                    None,
                    provenance,
                    p::EventPayload::CandidateDowngraded(p::CandidateDowngradedPayload {
                        candidate_id: command.candidate,
                        by: p::DecisionActor::User,
                        reason,
                    }),
                )?;
            }
            p::CandidateReviewDecision::Retract
                if candidate.state == p::CandidateReviewState::Promoted =>
            {
                let retraction = command
                    .retraction
                    .ok_or_else(|| p::Error("candidate retraction is missing".into()))?;
                if retraction.target.0 != candidate.target.0 {
                    return Err(p::Error(
                        "candidate retraction targets another stable object".into(),
                    ));
                }
                let retraction_event = self.append(
                    command.run.clone(),
                    None,
                    provenance,
                    p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                        target_object: retraction.target,
                        evidence_lineage: retraction.lineage,
                    }),
                )?;
                self.append(
                    command.run,
                    None,
                    p::Provenance {
                        source: p::Source::Internal,
                        actor: p::Actor::System,
                        trust_tier: p::TrustTier::VerifiedProcess,
                        caused_by: Some(retraction_event),
                    },
                    p::EventPayload::ReevaluationTaskCreated(p::ReevaluationTaskCreatedPayload {
                        derived_refs: retraction.derived_refs,
                        trigger: p::ReevaluationTriggerRef(format!(
                            "candidate-retraction:{}",
                            command.candidate.0
                        )),
                    }),
                )?;
            }
            _ => return Err(p::Error("candidate review transition is invalid".into())),
        }
        Ok(())
    }
}

impl EvolutionGatewayControl for ReactiveHarness {
    fn evolution_snapshot(&self, scope: p::Scope) -> p::Result<p::EvolutionSnapshot> {
        self.store.snapshot(scope)
    }

    fn record_strategy_candidate(
        &self,
        run: p::RunId,
        candidate: p::StrategyCandidate,
    ) -> p::Result<p::EventId> {
        self.evolution_control.record_candidate(run, candidate)
    }

    fn evaluate_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        comparison: p::EvolutionComparison,
    ) -> p::Result<EvolutionEvaluationResult> {
        self.evolution_control.evaluate(run, candidate, comparison)
    }

    fn promote_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
    ) -> p::Result<p::EventId> {
        self.evolution_control.promote(run, candidate, evaluation)
    }

    fn activate_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        promotion: p::EventId,
        owner_confirmation: Option<p::OwnerControlRef>,
    ) -> p::Result<EvolutionActivationResult> {
        self.evolution_control.activate(
            run,
            aggregate,
            candidate,
            evaluation,
            promotion,
            owner_confirmation,
        )
    }

    fn rollback_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        domain: p::StrategyDomain,
        scope: p::Scope,
        restored: p::StrategyVersionRef,
        triggers: Vec<p::EvidenceRef>,
        in_flight: p::InFlightDisposition,
        owner_confirmation: Option<p::OwnerControlRef>,
    ) -> p::Result<EvolutionActivationResult> {
        self.evolution_control.rollback(
            run,
            aggregate,
            domain,
            scope,
            restored,
            triggers,
            in_flight,
            owner_confirmation,
        )
    }

    fn set_auto_activation_paused(&self, paused: bool) {
        self.evolution_control.set_auto_activation_paused(paused);
    }

    fn auto_activation_paused(&self) -> bool {
        self.evolution_control.auto_activation_paused()
    }
}

impl SchedulerGatewayControl for ReactiveHarness {
    fn schedule(&self, command: p::ScheduleCommand) -> p::Result<p::IntentionId> {
        let validation_at = match command.intention.trigger {
            p::IntentionTrigger::At(at) => at,
            _ => command.envelope.timebox.starts_at,
        };
        self.validate_schedule_runtime(&command, validation_at)?;
        self.scheduler_runtime()?.intentions.schedule(command)
    }

    fn list_jobs(&self) -> p::Result<Vec<p::ScheduledJob>> {
        let mut jobs = self.scheduler_runtime()?.intentions.list()?;
        for job in &mut jobs {
            let run = schedule_run_id(&job.intention.id);
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            if events.is_empty() {
                continue;
            }
            job.run = Some(run.clone());
            job.run_status = run_summary_from_events(run, &events)
                .ok()
                .map(|summary| summary.status);
            job.manual_review = has_unknown_outcome(&events);
        }
        jobs.sort_by(|left, right| left.intention.id.cmp(&right.intention.id));
        Ok(jobs)
    }
}

impl SchedulerService for ReactiveHarness {
    fn tick(&self, now: p::Timestamp) -> p::Result<p::SchedulerTickReport> {
        let scheduler = self.scheduler_runtime()?;
        let claims = scheduler.intentions.claim_due(
            now,
            i64::try_from(scheduler.config.lease.0).unwrap_or(i64::MAX),
            scheduler.config.max_claims_per_tick as usize,
        )?;
        let mut report = p::SchedulerTickReport {
            schema_version: p::SchemaVersion(1),
            at: now,
            claimed: Vec::new(),
            started_runs: Vec::new(),
            deferred: Vec::new(),
            resolved: Vec::new(),
            manual_review: Vec::new(),
            follow_up_runs: Vec::new(),
        };
        for claim in claims {
            let intention = claim.command.intention.id.clone();
            report.claimed.push(intention.clone());
            if self
                .active_sessions
                .lock()
                .map_err(|_| p::Error("foreground activity state is unavailable".into()))?
                .contains(&claim.command.session)
            {
                report.deferred.push(intention);
                continue;
            }
            if let Err(error) = self.validate_schedule_runtime(&claim.command, now) {
                let expired = error.0.contains("timebox");
                scheduler.intentions.resolve(
                    intention.clone(),
                    if expired {
                        p::IntentionOutcome::Expired
                    } else {
                        p::IntentionOutcome::Cancelled
                    },
                )?;
                report.resolved.push(intention);
                continue;
            }
            let run = match self.drive_schedule_claim(&claim, now) {
                Ok(run) => run,
                Err(_) => {
                    report.deferred.push(intention);
                    continue;
                }
            };
            if !report.started_runs.contains(&run) {
                report.started_runs.push(run.clone());
            }
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            if has_unknown_outcome(&events) {
                report.manual_review.push(run);
                continue;
            }
            if events.iter().any(|event| {
                matches!(
                    event.kind,
                    p::EventKind::RunComplete
                        | p::EventKind::RunAborted
                        | p::EventKind::RunFailed
                        | p::EventKind::RunLimited
                        | p::EventKind::RunWaiting
                )
            }) {
                scheduler
                    .intentions
                    .resolve(intention.clone(), p::IntentionOutcome::Done)?;
                report.resolved.push(intention);
            }
        }
        report.follow_up_runs = self.run_failure_followups(now)?;
        Ok(report)
    }

    fn cancel(&self, intention: p::IntentionId, actor: p::Actor) -> p::Result<()> {
        if actor != p::Actor::Owner {
            return Err(p::Error(
                "scheduled intention cancellation requires the owner".into(),
            ));
        }
        self.scheduler_runtime()?
            .intentions
            .resolve(intention.clone(), p::IntentionOutcome::Cancelled)?;
        let run = schedule_run_id(&intention);
        if self
            .runs
            .lock()
            .map_err(|_| p::Error("run registry is unavailable".into()))?
            .contains_key(&run)
        {
            AgentHarness::cancel(self, run)?;
        }
        Ok(())
    }

    fn recover(&self, now: p::Timestamp) -> p::Result<p::RecoveryReport> {
        let recovered_unknown = self.recover_unknown_outcomes()?;
        let scheduler = self.scheduler_runtime()?;
        let mut report = p::RecoveryReport {
            schema_version: p::SchemaVersion(1),
            at: now,
            reclaimable: Vec::new(),
            coalesced: Vec::new(),
            manual_review: recovered_unknown,
        };
        for job in scheduler.intentions.list()? {
            let run = schedule_run_id(&job.intention.id);
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            if has_unknown_outcome(&events) {
                if !report.manual_review.contains(&run) {
                    report.manual_review.push(run);
                }
                continue;
            }
            if events.iter().any(|event| {
                matches!(
                    event.kind,
                    p::EventKind::RunComplete
                        | p::EventKind::RunAborted
                        | p::EventKind::RunFailed
                        | p::EventKind::RunLimited
                        | p::EventKind::RunWaiting
                )
            }) {
                scheduler
                    .intentions
                    .resolve(job.intention.id.clone(), p::IntentionOutcome::Done)?;
                report.coalesced.push(job.intention.id);
            } else if job.intention.state == p::IntentionState::Fired
                && job.lease_until.is_none_or(|lease_until| lease_until <= now)
            {
                report.reclaimable.push(job.intention.id);
            }
        }
        Ok(report)
    }
}

impl ManualEvaluator for ReactiveHarness {
    fn run_case(
        &self,
        case: p::ManualEvalCase,
        profile: p::EvalProfile,
    ) -> p::Result<p::ManualEvalReport> {
        case.validate()?;
        profile.validate()?;
        if let Ok(existing) = self.manual_evals.export(&profile.eval_ref) {
            return if existing.case_ref == case.case_ref && existing.profile == profile {
                Ok(existing)
            } else {
                Err(p::Error(
                    "manual eval ref is already bound to another case or profile".into(),
                ))
            };
        }
        if profile.model != self.config.model_profile
            || profile.policy != self.config.policy_profile
            || profile.toolset != self.config.toolset_ref
            || profile.workspace != self.config.workspace
            || profile.event_schema != p::SchemaVersion(1)
            || case.policy != self.config.policy_profile
            || case.workspace != self.config.workspace
            || !case
                .allowed_capabilities
                .iter()
                .all(|capability| self.governance.visible_capabilities.contains(capability))
        {
            return Err(p::Error(
                "manual eval profile does not match the harness snapshot".into(),
            ));
        }
        let run = if case.kind == p::GoldenTaskKind::BackgroundProactive {
            let now = now_ms();
            let command = background_eval_schedule(&case, &profile, &self.governance, now)?;
            let intention = command.intention.id.clone();
            SchedulerGatewayControl::schedule(self, command)?;
            let tick = SchedulerService::tick(self, now)?;
            tick.started_runs
                .into_iter()
                .find(|run| *run == schedule_run_id(&intention))
                .ok_or_else(|| {
                    p::Error("background eval did not produce its governed schedule run".into())
                })?
        } else {
            AgentHarness::submit_run(self, case.request.clone())?
        };
        let trace = GatewayControl::trace_view(self, run)?;
        let report = eval::evaluate_manual_trace(&case, &profile, &trace)?;
        self.manual_evals.save(report.clone())?;
        Ok(report)
    }

    fn export_report(&self, eval_ref: p::EvalRef) -> p::Result<p::ManualEvalReport> {
        self.manual_evals.export(&eval_ref)
    }
}

impl HarnessIngress for ReactiveHarness {
    fn ingress_authority(&self) -> IngressAuthority {
        self.ingress_authority.clone()
    }

    fn submit_ingress(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId> {
        self.submit_ingress_internal(request, prelude)
    }

    fn resolve_approval(
        &self,
        ticket: ApprovalTicket,
        grant: forme_approval::ApprovalGrant,
    ) -> p::Result<()> {
        if ticket.0 != grant.approval_id {
            return Err(p::Error("approval ticket and grant do not match".into()));
        }
        let runs = self
            .runs
            .lock()
            .map_err(|_| p::Error("run registry is unavailable".into()))?
            .iter()
            .map(|(run, handle)| (run.clone(), handle.clone()))
            .collect::<Vec<_>>();
        for (run, handle) in runs {
            let is_pending = handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?
                .approval
                .pending(ApprovalScope::All)
                .iter()
                .any(|request| request.approval_id == ticket.0);
            if is_pending {
                return AgentHarness::resume(self, run, ResumeInput::Approval(grant));
            }
        }
        Err(p::Error(
            "approval ticket is not pending in this harness".into(),
        ))
    }

    fn append_ingress_events(
        &self,
        run: p::RunId,
        events: Vec<IngressEvent>,
    ) -> p::Result<Vec<p::EventId>> {
        events
            .into_iter()
            .map(|event| {
                if !event.is_authorized_by(&self.ingress_authority) {
                    return Err(p::Error("ingress event is not versioned".into()));
                }
                self.append(run.clone(), None, event.provenance, event.payload)
            })
            .collect()
    }

    fn result_text(&self, run: p::RunId) -> p::Result<Option<String>> {
        self.output_text(run)
    }
}

impl HarnessActionIngress for ReactiveHarness {
    fn submit_action(
        &self,
        request: p::RunRequest,
        intent: p::ActionIntent,
        envelope: p::AutonomyEnvelope,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId> {
        if request.source != intent.source
            || request.idempotency_key.is_none()
            || intent.schema_version.0 == 0
            || envelope.schema_version.0 == 0
            || !is_external_action(&intent)
            || !scope_within(&intent.scope, &envelope.scope)
        {
            return Err(p::Error(
                "direct external action submission is incomplete or unbound".into(),
            ));
        }
        if intent.action_type == p::ActionType::Deliver
            && !delivery_disclosure_is_bound(&request, &intent, &prelude, &self.ingress_authority)
        {
            return Err(p::Error(
                "external delivery requires its exact allowed disclosure decision before submission"
                    .into(),
            ));
        }
        match self.prepare_ingress_internal_as(request, prelude, None)? {
            PreparedSubmission::Existing(run) => Ok(run),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                {
                    let mut record = handle
                        .record
                        .lock()
                        .map_err(|_| p::Error("run state is unavailable".into()))?;
                    record.mode = RunMode::DirectAction;
                    record.direct_intent = Some(intent);
                    record.bound_envelope = Some(envelope.clone());
                    record.remaining_action_budget = action_budget(Some(&envelope.budget))?;
                }
                self.drive_prepared_run(&run, &handle, &session)?;
                Ok(run)
            }
        }
    }
}

impl Observability for ReactiveHarness {
    fn decision_trace(&self, run: p::RunId) -> DecisionTrace {
        let mut event_refs = Vec::new();
        let mut reasons = Vec::new();
        for event in self.store.read_run(run.clone()).filter_map(Result::ok) {
            match &event.payload {
                p::EventPayload::ToolPolicyEvaluated(payload) => {
                    event_refs.push(event.event_id);
                    reasons.push(payload.reason.clone());
                }
                p::EventPayload::DecisionTraceRecorded(payload) => {
                    event_refs.push(event.event_id);
                    reasons.push(p::ReasonRef(payload.rationale.0.clone()));
                }
                p::EventPayload::ActionDenied(payload) => {
                    event_refs.push(event.event_id);
                    reasons.push(payload.reason.clone());
                }
                _ => {}
            }
        }
        DecisionTrace {
            schema_version: p::SchemaVersion(1),
            run,
            event_refs,
            reasons,
        }
    }

    fn state(&self, run: p::RunId) -> RunView {
        let events = self
            .store
            .read_run(run.clone())
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        let session = events.iter().find_map(|event| match &event.payload {
            p::EventPayload::RunAccepted(payload) => Some(payload.session_ref.clone()),
            _ => None,
        });
        let (state, result) = self
            .run_handle(&run)
            .ok()
            .and_then(|handle| {
                handle
                    .record
                    .lock()
                    .ok()
                    .map(|record| (Some(record.loop_ctx.state.clone()), record.result.clone()))
            })
            .unwrap_or((None, None));
        RunView {
            schema_version: p::SchemaVersion(1),
            run,
            session,
            state,
            last_stream_seq: events.last().map(|event| event.stream_seq).unwrap_or(0),
            result,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Suspended,
    Terminal,
}

fn default_context(
    request: &p::RunRequest,
    run: &p::RunId,
    config: &HarnessConfig,
) -> p::Result<forme_context::RunCtx> {
    let scope = p::Scope(config.workspace.0.clone());
    let provenance = internal_provenance(p::Source::Internal);
    Ok(forme_context::RunCtx {
        schema_version: p::SchemaVersion(1),
        run: run.clone(),
        session: p::SessionId(request.session.0.clone()),
        scope: scope.clone(),
        selected_skills: Vec::new(),
        brain_call: false,
        sources: ContextSources::empty(scope, provenance),
    })
}

fn action_only_model_profile() -> p::Result<ModelProfile> {
    Ok(ModelProfile {
        schema_version: p::SchemaVersion(1),
        provider: p::ProviderId("provider:action-only".into()),
        model: "action-only".into(),
        base_url: Url::parse("https://models.invalid/v1")?,
        capability: ModelCapability {
            schema_version: p::SchemaVersion(1),
            context_window: 8_192,
            tool_use: false,
            strength: ModelStrength::Basic,
        },
        cost: Cost {
            schema_version: p::SchemaVersion(1),
            input_microunits_per_million: 0,
            output_microunits_per_million: 0,
        },
        rate_limit: RateLimit {
            schema_version: p::SchemaVersion(1),
            requests_per_minute: 1,
            tokens_per_minute: 1,
        },
        credential_ref: p::CredentialRef("secret:unused-action-only-model".into()),
    })
}

fn approval_request(
    approval_id: p::ApprovalId,
    session: &p::SessionId,
    intent: &p::ActionIntent,
    plan: &ExecutionPlan,
    config: &HarnessConfig,
) -> p::Result<ApprovalRequest> {
    let now = now_ms();
    let ttl = i64::try_from(config.approval_ttl_ms)
        .map_err(|_| p::Error("approval TTL exceeds timestamp range".into()))?;
    Ok(ApprovalRequest {
        schema_version: p::SchemaVersion(1),
        approval_id,
        session: session.clone(),
        action_summary: format!("{:?} action in {}", intent.backend_hint, intent.scope.0),
        risk_level: intent.risk_hint,
        scope: intent.scope.clone(),
        requested_permissions: intent.requested_permissions.clone(),
        affected_resources: affected_resources(intent),
        rollback_boundary: plan.rollback_boundary.clone(),
        expires_at: now.saturating_add(ttl),
        choices: vec![
            p::ApprovalChoice("grant-once".into()),
            p::ApprovalChoice("deny".into()),
        ],
        plan_digest: plan.digest.clone(),
        policy_version: config.policy_version,
        tool_schema_version: config.tool_schema_version,
    })
}

fn affected_resources(intent: &p::ActionIntent) -> Vec<p::ResourceRef> {
    match &intent.parameters {
        p::ActionParameters::Shell { cwd, .. } => cwd
            .iter()
            .map(|cwd| p::ResourceRef(format!("path:{cwd}")))
            .collect(),
        p::ActionParameters::File { path, .. } => {
            vec![p::ResourceRef(format!("path:{path}"))]
        }
        p::ActionParameters::Mcp { server, tool, .. } => {
            vec![p::ResourceRef(format!("mcp:{}:{}", server.0, tool.0))]
        }
        p::ActionParameters::Notification {
            surface, target, ..
        } => vec![
            p::ResourceRef(format!("surface:{}", surface.0)),
            p::ResourceRef(format!("participant:{}", target.0)),
        ],
        p::ActionParameters::Browser(spec) => vec![
            p::ResourceRef(format!("browser-driver:{}", spec.driver.0)),
            p::ResourceRef(format!("url:{}", spec.target_url)),
        ],
        p::ActionParameters::Computer(spec) => vec![
            p::ResourceRef(format!("computer-driver:{}", spec.driver.0)),
            p::ResourceRef(format!("surface:{}", spec.surface.0)),
        ],
        p::ActionParameters::Pty(spec) => vec![
            p::ResourceRef(format!("program:{}", spec.program)),
            p::ResourceRef(format!("path:{}", spec.cwd)),
        ],
        p::ActionParameters::AppApi(spec) => {
            let mut resources = vec![
                p::ResourceRef(format!("connector:{}", spec.connector.0)),
                p::ResourceRef(format!("endpoint:{}", spec.endpoint)),
            ];
            if let Some(participant) = &spec.participant {
                resources.push(p::ResourceRef(format!("participant:{}", participant.0)));
            }
            resources
        }
        p::ActionParameters::Remote(spec) => vec![
            p::ResourceRef(format!("federated-peer:{}", spec.placement.executor.0)),
            p::ResourceRef(format!("peer-grant:{}", spec.placement.peer_grant.0)),
        ],
    }
}

fn pending_approval(run: p::RunId, request: ApprovalRequest) -> p::PendingApproval {
    p::PendingApproval {
        schema_version: request.schema_version,
        run,
        session: request.session,
        approval_id: request.approval_id,
        action_summary: request.action_summary,
        risk_level: request.risk_level,
        scope: request.scope,
        requested_permissions: request.requested_permissions,
        affected_resources: request.affected_resources,
        rollback_boundary: request.rollback_boundary,
        expires_at: request.expires_at,
        choices: request.choices,
        plan_digest: request.plan_digest,
        policy_version: request.policy_version,
        tool_schema_version: request.tool_schema_version,
    }
}

fn run_summary_from_events(run: p::RunId, events: &[p::Event]) -> p::Result<p::RunSummary> {
    let first = events
        .first()
        .ok_or_else(|| p::Error("run event stream was not found".into()))?;
    if events
        .iter()
        .any(|event| event.run_id != run || event.stream_seq == 0)
    {
        return Err(p::Error(
            "run event stream contains an invalid boundary".into(),
        ));
    }
    let mut source = None;
    let mut session = None;
    let mut workspace = None;
    let mut status = p::RunStatus::Accepted;
    let mut terminal = None;
    let mut evidence_refs = Vec::new();
    for event in events {
        match &event.payload {
            p::EventPayload::RunAccepted(payload) => {
                source = Some(payload.source);
                session = Some(payload.session_ref.clone());
                status = p::RunStatus::Accepted;
            }
            p::EventPayload::SessionBound(payload) => {
                workspace = Some(payload.workspace.clone());
                status = p::RunStatus::Running;
            }
            p::EventPayload::RunWaiting(_) => status = p::RunStatus::Waiting,
            p::EventPayload::RunResumed(_) => status = p::RunStatus::Running,
            p::EventPayload::RunComplete(payload) => {
                status = p::RunStatus::Complete;
                terminal = Some((status, payload.stop_reason.clone()));
            }
            p::EventPayload::RunAborted(payload) => {
                status = p::RunStatus::Aborted;
                terminal = Some((status, payload.stop_reason.clone()));
            }
            p::EventPayload::RunFailed(payload) => {
                status = p::RunStatus::Failed;
                terminal = Some((status, payload.stop_reason.clone()));
            }
            p::EventPayload::RunLimited(payload) => {
                status = p::RunStatus::Limited;
                terminal = Some((status, payload.stop_reason.clone()));
            }
            p::EventPayload::FailureEvidenceRecorded(_) => {
                evidence_refs.push(event.event_id.clone());
            }
            _ if terminal.is_none() && event.stream_seq > first.stream_seq => {
                status = p::RunStatus::Running;
            }
            _ => {}
        }
    }
    let source = source.ok_or_else(|| p::Error("run is missing RunAccepted".into()))?;
    let session = session.ok_or_else(|| p::Error("run is missing its session binding".into()))?;
    Ok(p::RunSummary {
        schema_version: p::SchemaVersion(1),
        run,
        source,
        session,
        workspace,
        status,
        last_stream_seq: events.last().map(|event| event.stream_seq).unwrap_or(0),
        result: terminal.map(|(status, stop_reason)| p::RunResult {
            schema_version: p::SchemaVersion(1),
            status,
            stop_reason,
            outputs: Vec::new(),
            evidence_refs,
        }),
    })
}

fn run_session(events: &[p::Event]) -> p::Result<p::SessionId> {
    events
        .iter()
        .find_map(|event| match &event.payload {
            p::EventPayload::RunAccepted(payload) => Some(payload.session_ref.clone()),
            _ => None,
        })
        .ok_or_else(|| p::Error("run is missing its session binding".into()))
}

struct CandidateSnapshot {
    state: p::CandidateReviewState,
    target: p::CandidateTargetRef,
    evidence: Vec<p::EvidenceRef>,
}

fn candidate_snapshot(
    candidate: &p::CandidateId,
    events: &[p::Event],
) -> p::Result<CandidateSnapshot> {
    let mut snapshot: Option<CandidateSnapshot> = None;
    for event in events {
        match &event.payload {
            p::EventPayload::CandidateCreated(payload) if &payload.candidate_id == candidate => {
                if snapshot.is_some() {
                    return Err(p::Error("candidate was created more than once".into()));
                }
                snapshot = Some(CandidateSnapshot {
                    state: p::CandidateReviewState::Candidate,
                    target: payload.target.clone(),
                    evidence: payload.evidence_refs.clone(),
                });
            }
            p::EventPayload::CandidatePromoted(payload) if &payload.candidate_id == candidate => {
                candidate_state_mut(&mut snapshot)?.state = p::CandidateReviewState::Promoted;
            }
            p::EventPayload::CandidateRejected(payload) if &payload.candidate_id == candidate => {
                candidate_state_mut(&mut snapshot)?.state = p::CandidateReviewState::Rejected;
            }
            p::EventPayload::CandidateDowngraded(payload) if &payload.candidate_id == candidate => {
                candidate_state_mut(&mut snapshot)?.state = p::CandidateReviewState::Downgraded;
            }
            p::EventPayload::CandidateDecayed(payload) if &payload.candidate_id == candidate => {
                candidate_state_mut(&mut snapshot)?.state = p::CandidateReviewState::Decayed;
            }
            p::EventPayload::RetractionEvent(payload)
                if snapshot
                    .as_ref()
                    .is_some_and(|current| current.target.0 == payload.target_object.0) =>
            {
                candidate_state_mut(&mut snapshot)?.state = p::CandidateReviewState::Retracted;
            }
            _ => {}
        }
    }
    snapshot.ok_or_else(|| p::Error("candidate was not found in the run trace".into()))
}

fn candidate_state_mut(
    snapshot: &mut Option<CandidateSnapshot>,
) -> p::Result<&mut CandidateSnapshot> {
    snapshot
        .as_mut()
        .ok_or_else(|| p::Error("candidate transition precedes its creation".into()))
}

fn evidence_is_resolved(
    evidence: &p::EvidenceRef,
    candidate: &CandidateSnapshot,
    events: &[p::Event],
) -> bool {
    candidate.evidence.contains(evidence)
        || events.iter().any(|event| event.event_id.0 == evidence.0)
        || events.iter().any(|event| match &event.payload {
            p::EventPayload::FailureEvidenceRecorded(payload) => {
                payload.failure_ref.0 == evidence.0
            }
            _ => false,
        })
}

fn requires_competence_gate(intent: &p::ActionIntent) -> bool {
    matches!(intent.source, p::Source::ProactiveJob | p::Source::Schedule)
        || intent.expected_effect == p::ExpectedEffect::Outward
}

fn required_intervention_level(
    intent: &p::ActionIntent,
    rollback_boundary: &p::RollbackBoundary,
) -> cognition::InterventionLevel {
    if requires_one_shot_approval(intent, rollback_boundary) {
        cognition::InterventionLevel::L5HighImpact
    } else if intent.backend_hint == p::BackendKind::Notification
        && intent.risk_hint == p::Risk::Low
    {
        cognition::InterventionLevel::L1Suggest
    } else {
        cognition::InterventionLevel::L3ActWithApproval
    }
}

fn is_external_action(intent: &p::ActionIntent) -> bool {
    matches!(
        intent.backend_hint,
        p::BackendKind::Browser
            | p::BackendKind::Computer
            | p::BackendKind::Pty
            | p::BackendKind::AppApi
    ) || (intent.expected_effect == p::ExpectedEffect::Outward
        && !is_owner_local_notification(intent))
}

fn delivery_disclosure_is_bound(
    request: &p::RunRequest,
    intent: &p::ActionIntent,
    prelude: &[IngressEvent],
    authority: &IngressAuthority,
) -> bool {
    let p::ActionParameters::AppApi(spec) = &intent.parameters else {
        return false;
    };
    let (Some(expected_request), Some(expected_representation), Some(expected_participant)) = (
        &spec.disclosure_request,
        spec.representation,
        &spec.participant,
    ) else {
        return false;
    };
    let p::AppApiOperation::Mutation {
        body: Some(p::ExternalInput::Content(expected_content)),
        ..
    } = &spec.operation
    else {
        return false;
    };
    prelude.iter().any(|event| {
        event.is_authorized_by(authority)
            && event.provenance().source == p::Source::Communication
            && event.provenance().actor == p::Actor::System
            && event.provenance().trust_tier == p::TrustTier::VerifiedProcess
            && matches!(
                event.payload(),
                p::EventPayload::DisclosurePolicyApplied(disclosure)
                    if &disclosure.request == expected_request
                        && disclosure.representation == expected_representation
                        && matches!(
                            disclosure.outcome,
                            p::DisclosureOutcome::Answer | p::DisclosureOutcome::Approve
                        )
                        && disclosure.binding.as_ref().is_some_and(|binding| {
                            binding.schema_version.0 > 0
                                && binding.session.0 == request.session.0
                                && &binding.participant == expected_participant
                                && binding.purpose.0 == intent.goal.0
                                && &binding.content_ref == expected_content
                                && !binding.category.trim().is_empty()
                                && (!(binding.sensitive
                                    || !binding.confirmed
                                    || binding.high_impact)
                                    || intent.risk_hint == p::Risk::High)
                        })
            )
    })
}

fn is_owner_local_notification(intent: &p::ActionIntent) -> bool {
    matches!(
        &intent.parameters,
        p::ActionParameters::Notification {
            surface,
            target,
            ..
        } if intent.backend_hint == p::BackendKind::Notification
            && surface.0.starts_with("surface:local")
            && target.0 == "owner"
    )
}

fn requires_one_shot_approval(
    intent: &p::ActionIntent,
    rollback_boundary: &p::RollbackBoundary,
) -> bool {
    let rollback = rollback_boundary.0.trim().to_ascii_lowercase();
    is_external_action(intent)
        && (intent.risk_hint == p::Risk::High
            || intent.action_type == p::ActionType::ExternalCommit
            || rollback.is_empty()
            || rollback == "none"
            || rollback.contains("not-retractable")
            || rollback.contains("unknown"))
}

fn external_policy_floor(
    intent: &p::ActionIntent,
    rollback_boundary: Option<&p::RollbackBoundary>,
    policy: p::PolicyDecision,
    envelope: Option<&p::AutonomyEnvelope>,
    inputs: &cognition::CompetenceInputs,
    competence: &(dyn cognition::CompetenceGate + Send + Sync),
) -> p::PolicyDecision {
    if policy == p::PolicyDecision::Deny || !is_external_action(intent) {
        return policy;
    }
    let Some(rollback_boundary) = rollback_boundary else {
        return p::PolicyDecision::Ask;
    };
    if requires_one_shot_approval(intent, rollback_boundary) {
        return p::PolicyDecision::Ask;
    }
    let evidence_present =
        !inputs.capability_evidence.is_empty() || !inputs.verification.is_empty();
    let narrow_scope = !intent.scope.0.trim().is_empty()
        && intent.scope.0 != "*"
        && !intent.scope.0.eq_ignore_ascii_case("all");
    let explicit_l4 = envelope.is_some_and(|envelope| {
        envelope.approval_rule == p::ApprovalRule::Allow
            && envelope.risk_limit == p::Risk::Low
            && envelope.rollback.required
            && envelope.rollback.boundary.as_ref() == Some(rollback_boundary)
            && scope_within(&intent.scope, &envelope.scope)
            && envelope
                .capability
                .capabilities
                .contains(&intent.capability_ref)
            && intent
                .requested_permissions
                .iter()
                .all(|permission| envelope.capability.permissions.contains(permission))
    });
    let ceiling = competence.ceiling(intent.scope.clone(), intent.risk_hint, inputs);
    if policy == p::PolicyDecision::Allow
        && intent.risk_hint == p::Risk::Low
        && evidence_present
        && narrow_scope
        && explicit_l4
        && ceiling >= cognition::InterventionLevel::L4Autonomous
    {
        p::PolicyDecision::Allow
    } else {
        p::PolicyDecision::Ask
    }
}

fn untrusted_ingress_policy_floor(
    intent: &p::ActionIntent,
    policy: p::PolicyDecision,
) -> p::PolicyDecision {
    if intent.source == p::Source::Communication && policy == p::PolicyDecision::Allow {
        p::PolicyDecision::Ask
    } else {
        policy
    }
}

fn validate_competence_snapshot(snapshot: &cognition::CompetenceInputs) -> p::Result<()> {
    let references_are_present =
        snapshot
            .capability_evidence
            .iter()
            .all(|item| item.schema_version.0 > 0 && !item.reference.0.trim().is_empty())
            && snapshot
                .verification
                .iter()
                .all(|item| item.schema_version.0 > 0 && !item.reference.0.trim().is_empty())
            && snapshot
                .failure
                .iter()
                .all(|item| item.schema_version.0 > 0 && !item.reference.0.trim().is_empty())
            && snapshot.map_confidence.as_ref().is_none_or(|item| {
                item.schema_version.0 > 0 && !item.reference.0.trim().is_empty()
            })
            && snapshot.self_model.as_ref().is_none_or(|item| {
                item.schema_version.0 > 0 && !item.reference.0.trim().is_empty()
            })
            && snapshot.trust.as_ref().is_none_or(|item| {
                item.schema_version.0 > 0 && !item.reference.0.trim().is_empty()
            });
    if snapshot.schema_version.0 == 0 || !references_are_present {
        return Err(p::Error(
            "competence evidence snapshot is incomplete".into(),
        ));
    }
    Ok(())
}

fn is_external_observation(payload: &p::EventPayload) -> bool {
    matches!(
        payload,
        p::EventPayload::ActionOutputDelta(_)
            | p::EventPayload::ActionCompleted(_)
            | p::EventPayload::ActionFailed(_)
            | p::EventPayload::ActionCancelled(_)
            | p::EventPayload::ActionOutcomeUnknown(_)
            | p::EventPayload::CapabilityEvidenceRecorded(_)
    )
}

fn untrusted_action_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::Untrusted,
        caused_by: None,
    }
}

fn action_observation_provenance(external_action: bool) -> p::Provenance {
    if external_action {
        untrusted_action_provenance()
    } else {
        internal_provenance(p::Source::Internal)
    }
}

fn stamp_external_payload(payload: p::EventPayload) -> p::EventPayload {
    match payload {
        p::EventPayload::ActionOutputDelta(mut output) => {
            output.trust = p::TrustTier::Untrusted;
            p::EventPayload::ActionOutputDelta(output)
        }
        p::EventPayload::ActionCompleted(mut completed) => {
            if let Some(receipt) = &mut completed.receipt {
                receipt.trust = p::TrustTier::Untrusted;
            }
            p::EventPayload::ActionCompleted(completed)
        }
        other => other,
    }
}

fn protocol_intervention(level: cognition::InterventionLevel) -> p::InterventionLevel {
    match level {
        cognition::InterventionLevel::L0Observe => p::InterventionLevel::L0Observe,
        cognition::InterventionLevel::L1Suggest => p::InterventionLevel::L1Suggest,
        cognition::InterventionLevel::L2Prepare => p::InterventionLevel::L2Prepare,
        cognition::InterventionLevel::L3ActWithApproval => p::InterventionLevel::L3ActWithApproval,
        cognition::InterventionLevel::L4Autonomous => p::InterventionLevel::L4ActAutonomously,
        cognition::InterventionLevel::L5HighImpact => p::InterventionLevel::L5HighImpact,
    }
}

fn protocol_impulse_source(source: cognition::ImpulseSource) -> p::ImpulseSource {
    match source {
        cognition::ImpulseSource::Gap => p::ImpulseSource::Gap,
        cognition::ImpulseSource::Change => p::ImpulseSource::Change,
        cognition::ImpulseSource::Tension => p::ImpulseSource::Tension,
        cognition::ImpulseSource::Association => p::ImpulseSource::Association,
        cognition::ImpulseSource::Pressure => p::ImpulseSource::Pressure,
        cognition::ImpulseSource::Commitment => p::ImpulseSource::Commitment,
    }
}

fn protocol_reach(reach: cognition::Reach) -> p::Reach {
    p::Reach(match reach {
        cognition::Reach::Internalize => 0,
        cognition::Reach::ActIndependently => 1,
        cognition::Reach::Collaborate => 2,
        cognition::Reach::ExternalProxy => 3,
    })
}

fn protocol_delivery_mode(delivery: cognition::DeliveryMode) -> p::DeliveryMode {
    match delivery {
        cognition::DeliveryMode::Hitchhike => p::DeliveryMode::Hitchhike,
        cognition::DeliveryMode::Interrupt => p::DeliveryMode::Interrupt,
        cognition::DeliveryMode::Digest => p::DeliveryMode::Digest,
        cognition::DeliveryMode::Internal => p::DeliveryMode::Internal,
    }
}

fn risk_for_level(level: cognition::InterventionLevel) -> p::Risk {
    match level {
        cognition::InterventionLevel::L0Observe | cognition::InterventionLevel::L1Suggest => {
            p::Risk::Low
        }
        cognition::InterventionLevel::L2Prepare
        | cognition::InterventionLevel::L3ActWithApproval => p::Risk::Medium,
        cognition::InterventionLevel::L4Autonomous | cognition::InterventionLevel::L5HighImpact => {
            p::Risk::High
        }
    }
}

fn coordination_events(
    frame: &coordination::GoalFrame,
    plan: &coordination::ResourcePlan,
    done: &coordination::DoneContract,
    envelope: &p::AutonomyEnvelope,
    trace: &coordination::DecisionTrace,
    evolution_snapshot: Option<p::EvolutionSnapshotRef>,
    federation_snapshot: Option<p::FederationSnapshotRef>,
) -> Vec<p::EventPayload> {
    vec![
        p::EventPayload::GoalFramed(p::GoalFramedPayload {
            goal_frame: frame.reference.clone(),
            long_term: None,
        }),
        p::EventPayload::ResourcePlanned(p::ResourcePlannedPayload {
            plan: plan.reference.clone(),
        }),
        p::EventPayload::DoneContractSet(p::DoneContractSetPayload {
            contract: done.reference.clone(),
        }),
        p::EventPayload::AutonomyEnvelopeSet(p::AutonomyEnvelopeSetPayload {
            envelope: p::AutonomyEnvelopeRef(format!(
                "envelope:{}:{}",
                frame.goal.reference.0, envelope.scope.0
            )),
        }),
        p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
            trace_ref: trace.reference.clone(),
            refs: trace.refs.clone(),
            rationale: trace.rationale.clone(),
            workspace_snapshot: trace.workspace_snapshot.clone(),
            resource_graph_snapshot: trace.resource_graph_snapshot.clone(),
            evolution_snapshot,
            federation_snapshot,
        }),
    ]
}

fn append_event(
    store: &SqliteEventStore,
    sequence: &AtomicU64,
    run: p::RunId,
    turn: Option<p::TurnId>,
    provenance: p::Provenance,
    payload: p::EventPayload,
) -> p::Result<p::EventId> {
    let id = p::EventId(format!(
        "harness:{}:{}",
        now_nanos(),
        sequence.fetch_add(1, Ordering::SeqCst)
    ));
    store.append(p::Event::new(
        id,
        run,
        turn,
        payload,
        p::SchemaVersion(1),
        now_ms(),
        provenance,
    ))
}

fn request_provenance(request: &p::RunRequest) -> p::Provenance {
    match request.source {
        p::Source::UserTurn => p::Provenance {
            source: request.source,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
        p::Source::Communication => p::Provenance {
            source: request.source,
            actor: p::Actor::External(p::ParticipantId("gateway-external".into())),
            trust_tier: p::TrustTier::Untrusted,
            caused_by: None,
        },
        _ => internal_provenance(request.source),
    }
}

fn internal_provenance(source: p::Source) -> p::Provenance {
    p::Provenance {
        source,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn current_turn(run: &p::RunId, index: u32) -> p::TurnId {
    p::TurnId(format!("turn:{}:{index}", run.0))
}

fn protocol_budget_units(budget: &p::Budget) -> p::Result<u64> {
    let raw = budget.0.trim();
    let raw = raw.strip_prefix("units:").unwrap_or(raw);
    raw.parse::<u64>()
        .map_err(|_| p::Error("budget must be an integer or units:<integer>".into()))
}

fn action_budget(budget: Option<&p::Budget>) -> p::Result<Option<u64>> {
    budget.map(protocol_budget_units).transpose()
}

fn schedule_run_id(intention: &p::IntentionId) -> p::RunId {
    p::RunId(format!("run:schedule:{}", intention.0))
}

fn scheduled_request(command: &p::ScheduleCommand) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::Schedule,
        session: p::SessionRef(command.session.0.clone()),
        agent_profile: p::AgentProfileRef("agent:scheduled".into()),
        input: p::RunInput(command.intention.seed.0.clone()),
        budget: Some(command.budget.clone()),
        idempotency_key: Some(p::IdempotencyKey(format!(
            "intention:{}",
            command.intention.id.0
        ))),
    }
}

fn background_eval_schedule(
    case: &p::ManualEvalCase,
    profile: &p::EvalProfile,
    governance: &GovernanceConfig,
    now: p::Timestamp,
) -> p::Result<p::ScheduleCommand> {
    let global = governance
        .envelope
        .as_ref()
        .ok_or_else(|| p::Error("background eval requires an active autonomy envelope".into()))?;
    if global.approval_rule != p::ApprovalRule::Allow
        || !case
            .allowed_capabilities
            .iter()
            .any(|capability| capability.0.contains("notification"))
    {
        return Err(p::Error(
            "background eval requires allowed local notification delivery".into(),
        ));
    }
    let expires_at = now.saturating_add(60_000).min(global.timebox.expires_at);
    if expires_at <= now {
        return Err(p::Error(
            "background eval autonomy envelope has already expired".into(),
        ));
    }
    let budget = case
        .request
        .budget
        .clone()
        .unwrap_or_else(|| p::Budget("units:1".into()));
    let envelope = p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope(case.workspace.0.clone()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: case.allowed_capabilities.clone(),
            permissions: global.capability.permissions.clone(),
        },
        action_type: vec![p::ActionType::Deliver],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Allow,
        budget: budget.clone(),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: now,
            expires_at,
            max_turns: global.timebox.max_turns.min(2),
        },
        rollback: global.rollback.clone(),
    };
    let command = p::ScheduleCommand {
        schema_version: p::SchemaVersion(1),
        intention: p::ProspectiveIntention {
            schema_version: p::SchemaVersion(1),
            id: p::IntentionId(format!("intention:{}", profile.eval_ref.0)),
            source: p::IntentionSource::Commitment,
            trigger: p::IntentionTrigger::At(now),
            state: p::IntentionState::Pending,
            seed: p::SeedRef(case.request.input.0.clone()),
            provenance: p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            expires_at: Some(expires_at),
        },
        session: p::SessionId(case.request.session.0.clone()),
        envelope,
        budget,
    };
    command.validate()?;
    Ok(command)
}

fn configure_scheduled_record(
    record: &mut RunRecord,
    claim: &p::ScheduleClaim,
    now: p::Timestamp,
) -> p::Result<()> {
    record.bound_envelope = Some(claim.command.envelope.clone());
    record.remaining_action_budget = action_budget(Some(&claim.command.budget))?;
    if claim.command.intention.source == p::IntentionSource::Commitment {
        record.mode = RunMode::DirectAction;
        record.direct_intent = Some(notification_intent(&claim.command, now)?);
    }
    Ok(())
}

fn notification_intent(
    command: &p::ScheduleCommand,
    now: p::Timestamp,
) -> p::Result<p::ActionIntent> {
    if !command
        .envelope
        .action_type
        .contains(&p::ActionType::Deliver)
    {
        return Err(p::Error(
            "commitment reminder envelope does not permit delivery".into(),
        ));
    }
    let capability = command
        .envelope
        .capability
        .capabilities
        .iter()
        .find(|capability| capability.0.contains("notification"))
        .cloned()
        .ok_or_else(|| {
            p::Error("commitment reminder has no local notification capability".into())
        })?;
    Ok(p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId(format!("notification:{}", command.intention.id.0)),
        source: p::Source::Schedule,
        goal: p::GoalRef(command.intention.seed.0.clone()),
        backend_hint: p::BackendKind::Notification,
        capability_ref: capability,
        action_type: p::ActionType::Deliver,
        scope: command.envelope.scope.clone(),
        risk_hint: command.envelope.risk_limit,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("notification-not-retractable".into()),
        parameters: p::ActionParameters::Notification {
            surface: p::SurfaceRef("surface:local-notification".into()),
            target: p::ParticipantId("owner".into()),
            title: "Scheduled reminder".into(),
            body_ref: p::ContentRef(format!("intention:{}", command.intention.id.0)),
        },
        requested_permissions: command.envelope.capability.permissions.clone(),
        requested_at: now,
        estimated_output_bytes: 256,
        estimated_duration: p::DurationMs(5_000),
    })
}

fn has_unknown_outcome(events: &[p::Event]) -> bool {
    let mut unknown = BTreeSet::new();
    for event in events {
        match &event.payload {
            p::EventPayload::ActionOutcomeUnknown(payload) => {
                unknown.insert(payload.intent_id.clone());
            }
            p::EventPayload::ActionCompleted(payload) => {
                unknown.remove(&payload.intent_id);
            }
            p::EventPayload::ActionFailed(payload) => {
                unknown.remove(&payload.intent_id);
            }
            p::EventPayload::ActionCancelled(payload) => {
                unknown.remove(&payload.intent_id);
            }
            p::EventPayload::ActionDenied(payload) => {
                unknown.remove(&payload.intent_id);
            }
            _ => {}
        }
    }
    !unknown.is_empty()
}

fn scope_within(child: &p::Scope, parent: &p::Scope) -> bool {
    child == parent
        || child.0.strip_prefix(&parent.0).is_some_and(|suffix| {
            suffix.starts_with(':') || suffix.starts_with('/') || suffix.starts_with('\\')
        })
}

fn local_notification_governance(
    workspace: &p::WorkspaceRef,
    now: p::Timestamp,
) -> GovernanceConfig {
    let scope = p::Scope(workspace.0.clone());
    let capability = p::CapabilityRef("capability:local-notification".into());
    let permission = p::PermissionRef("permission:local-notification".into());
    let envelope = p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: scope.clone(),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![capability.clone()],
            permissions: vec![permission.clone()],
        },
        action_type: vec![p::ActionType::Deliver],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("units:100000".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: now,
            expires_at: now.saturating_add(365 * 24 * 60 * 60 * 1_000),
            max_turns: u32::MAX,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    };
    GovernanceConfig {
        schema_version: p::SchemaVersion(1),
        layers: vec![PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::User,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::Notification),
                    capability: Some(capability.clone()),
                    action_type: Some(p::ActionType::Deliver),
                    parameters: ArgMatcher::Any,
                },
                effect: p::PolicyDecision::Allow,
                scope: scope.clone(),
            }],
        }],
        visible_capabilities: vec![capability],
        granted_permissions: vec![permission],
        allowed_scopes: vec![scope],
        shell_allowlist: Vec::new(),
        file_roots: Vec::new(),
        mcp_allowlist: Vec::new(),
        external: forme_policy::ExternalPolicyLimits::default(),
        network_allowed: false,
        sandbox_available: false,
        delegation: Some(DelegationGrant {
            schema_version: p::SchemaVersion(1),
            subject: DelegationSubject::Agent,
            envelope: envelope.clone(),
            granted_by: p::Actor::Owner,
            audit_ref: p::EventId("grant:local-notification".into()),
        }),
        envelope: Some(envelope),
    }
}

impl FederationGatewayControl for ReactiveHarness {
    fn federation_snapshot(&self, scope: p::Scope) -> p::Result<p::FederationSnapshot> {
        self.federation.snapshot(scope)
    }

    fn federated_peer(
        &self,
        peer: &p::FederatedPeerRef,
    ) -> p::Result<Option<p::FederatedPeerState>> {
        self.federation.peer(peer)
    }

    fn register_federated_peer(
        &self,
        run: p::RunId,
        grant: p::FederatedPeerGrant,
        previous: Option<p::FederatedPeerGrantRef>,
        expected: p::FederationAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        self.federation
            .register_peer(run, grant, previous, expected, owner)
    }

    fn revoke_federated_peer(
        &self,
        run: p::RunId,
        peer: p::FederatedPeerRef,
        grant: p::FederatedPeerGrantRef,
        in_flight: p::InFlightDisposition,
        expected: p::FederationAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        self.federation
            .revoke_peer(run, peer, grant, in_flight, expected, owner)
    }

    fn apply_federated_owner_command(
        &self,
        envelope: p::FederatedControlEnvelope,
        command: p::FederatedOwnerCommand,
        now: p::Timestamp,
    ) -> p::Result<FederationControlResult> {
        self.federation.apply_owner_command(envelope, command, now)
    }

    fn federated_lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteExecutionLease>> {
        self.federation.lease(lease)
    }

    fn federated_retention_state(
        &self,
        request: &p::RetentionRequestRef,
    ) -> p::Result<Option<m4::RetentionState>> {
        self.federation.retention_state(request)
    }

    fn recover_federated_remote_action(
        &self,
        run: &p::RunId,
    ) -> p::Result<m4::RemoteRecoveryResult> {
        self.federation.recover_remote_action(run, None)
    }

    fn acknowledge_federated_replication(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        batch: &p::ReplicationBatch,
        ack: p::ReplicationAck,
        expected: p::FederationAggregateVersion,
        now: p::Timestamp,
    ) -> p::Result<p::ExpectedAppend> {
        self.federation
            .acknowledge_replication(authenticated_peer, batch, ack, expected, now)
    }

    fn accept_federated_retention_receipt(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        receipt: p::FederatedRetentionReceipt,
        now: p::Timestamp,
    ) -> p::Result<()> {
        self.federation
            .accept_retention_receipt(authenticated_peer, receipt, now)
    }

    fn accept_federated_device_signal(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        signal: p::FederatedDeviceSignal,
        now: p::Timestamp,
    ) -> p::Result<bool> {
        self.federation
            .accept_device_signal(authenticated_peer, signal, now)
    }
}

impl FederationActionGateway for ReactiveHarness {
    fn submit_federated_remote_action(
        &self,
        run: p::RunId,
        request: p::RunRequest,
        intent: p::ActionIntent,
    ) -> p::Result<RemoteActionSubmission> {
        self.federation.prepare_remote_action(
            run,
            request,
            intent,
            &self.governance,
            &self.config,
            self.competence.as_ref(),
            &self.competence_snapshot,
        )
    }
}

fn new_run_id(sequence: u64) -> p::RunId {
    p::RunId(format!("run:{}:{sequence}", now_nanos()))
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn required_env(key: &str) -> p::Result<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| p::Error(format!("required configuration {key} is missing")))
}

fn optional_env_u32(key: &str, fallback: u32) -> p::Result<u32> {
    match std::env::var(key) {
        Ok(value) => value
            .parse()
            .map_err(|_| p::Error(format!("configuration {key} must be an unsigned integer"))),
        Err(_) => Ok(fallback),
    }
}

fn optional_env_bool(key: &str, fallback: bool) -> p::Result<bool> {
    match std::env::var(key) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(p::Error(format!("{key} must be a boolean"))),
        },
        Err(std::env::VarError::NotPresent) => Ok(fallback),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(p::Error(format!("{key} is not valid UTF-8")))
        }
    }
}
