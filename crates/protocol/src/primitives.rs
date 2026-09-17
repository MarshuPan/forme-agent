use core::fmt;

use serde::{Deserialize, Serialize};

macro_rules! string_newtypes {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(
                Debug,
                Clone,
                PartialEq,
                Eq,
                Hash,
                PartialOrd,
                Ord,
                Serialize,
                Deserialize,
            )]
            pub struct $name(pub String);
        )+
    };
}

string_newtypes!(
    EventId,
    RunId,
    SessionId,
    TurnId,
    ActionId,
    ApprovalId,
    CandidateId,
    IntentionId,
    NodeId,
    EdgeId,
    ProviderId,
    ParticipantId,
    WorkspaceId,
    ModelCallId,
    ToolCallId,
    InputRef,
    IdempotencyKey,
    PolicyProfileRef,
    ModelProfileRef,
    ToolsetRef,
    WorkspaceRef,
    AgentProfileRef,
    SessionRef,
    OutputRef,
    ContextSliceRef,
    LineageRef,
    PreservedRef,
    WaitReason,
    ResumeRef,
    FinishReason,
    RuleSourceRef,
    ReasonRef,
    ActionSummary,
    RollbackBoundary,
    ApprovalChoice,
    ApprovalGrantRef,
    PermissionRef,
    HandoffTargetRef,
    ActionResultRef,
    FailureEvidenceRef,
    ProbeHintRef,
    VerifierKind,
    DoneContractRef,
    FailureDigestRef,
    DigestSummaryRef,
    EvidenceRef,
    CandidateTargetRef,
    ConflictKind,
    ObjectRef,
    ReevaluationTriggerRef,
    GrantRef,
    SeedRef,
    GateDecision,
    ProposalKind,
    ProposalRef,
    FeedbackRef,
    IntentionTriggerRef,
    GoalFrameRef,
    ResourcePlanRef,
    AutonomyEnvelopeRef,
    MapConfidenceRef,
    AgentSelfModelRef,
    TrustProfileRef,
    Rationale,
    AgentWorkspaceSnapshotRef,
    DecisionTraceRef,
    OrchestrationPatternRef,
    ExecutionRouteRef,
    PermissionProfileRef,
    RoleRef,
    SummaryRef,
    ResultRef,
    CapabilitySourceRef,
    McpServerRef,
    McpToolRef,
    ToolRef,
    ResourceRef,
    McpErrorClass,
    SkillRef,
    SkillDescriptorRef,
    SkillTriggerRef,
    PluginManifestRef,
    PluginRef,
    PluginContributionRef,
    CapabilityRef,
    HookRef,
    CapabilityEvidenceRef,
    CapabilityOutcome,
    Reliability,
    CarrierRef,
    ChannelAdapterRef,
    PurposeRef,
    TerminationReason,
    DisclosurePolicyRef,
    TranscriptPolicyRef,
    DisclosureRequestRef,
    CommunicationProposalRef,
    CommunicationSessionId,
    ContentRef,
    MemoryNodeRef,
    MemoryEdgeRef,
    MemoryNodeType,
    MemoryEdgeType,
    MemoryDeltasRef,
    UserAttributeRef,
    UserAttributeValueRef,
    HistoricalSourceRef,
    JudgmentFrameRef,
    QualityModelRef,
    BlindSpotModelRef,
    MapUpdateKind,
    ConfigFindingRef,
    ComplianceFindingRef,
    SuggestedFixRef,
    StopReason,
    Budget,
    MemoryScope,
    RunInput,
    Constraint,
    PlanDigest,
    SchemaDigest,
    Nonce,
    VerifiedPrincipal,
    Scope,
    GoalRef,
    CredentialRef,
    VerifyRef,
    SurfaceRef,
    EvalRef,
    EvalCaseRef,
    RubricRef,
    ReplaySnapshotRef,
    ReplayBundleRef,
    ExactReplayReportRef,
    EvaluationCaseRef,
    EvaluationProfileRef,
    EvolutionEvaluationRef,
    EvolutionSnapshotRef,
    EvolutionAggregateRef,
    StrategyVersionRef,
    LoopSpecRef,
    DriverProfileRef,
    OwnerControlRef,
    InvariantResultRef,
    ActiveStrategyId,
    ResourceGraphSnapshotRef,
    CapabilityGapRef,
    CapabilityUpdateProposalRef,
    GoalCheckpointRef,
    ManagedPluginPolicyRef,
    ManagedPluginSourceRef,
    PluginSignatureRef,
    ManagedPluginSnapshotRef,
    SyncPeerRef,
    SyncBatchId,
    RedactionPolicyRef,
    FederationAggregateRef,
    FederatedPeerRef,
    TransportIdentityDigest,
    FederatedPeerGrantRef,
    FederationSnapshotRef,
    AuthorityRef,
    ExecutorCredentialSlotRef,
    ExecutorProfileRef,
    RemotePlacementPlanRef,
    RemoteExecutionLeaseRef,
    RemoteDispatchId,
    RemoteDriverReceiptRef,
    RemoteExecutionReceiptRef,
    RemoteWireRequestRef,
    ReplicationBatchRef,
    FederatedSessionRef,
    ReplicaProjectionDigestRef,
    RetentionRequestRef,
    RetentionReceiptRef,
    PlacementDecisionRef,
    FederatedCheckpointArtifactRef,
    FederatedDeviceSignalRef,
    CapabilityPackageRef,
    CapabilityReleaseRef,
    CapabilityPublisherRef,
    CapabilityPublisherGrantRef,
    CapabilityAdmissionRef,
    CapabilityPolicyRef,
    CapabilityInstallPlanRef,
    CapabilityDistributionReceiptRef,
    EcosystemAggregateRef,
);

// M2 canonicalizes the name while retaining the existing wire representation.
pub use CredentialRef as SecretRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SchemaVersion(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version(pub u32);

/// Monotonic fencing generation owned by the authority.  It is deliberately
/// not a timestamp, process id, or network term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthorityEpoch(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PeerGrantVersion(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FenceToken(pub u64);

impl AuthorityEpoch {
    pub const fn initial() -> Self {
        Self(0)
    }

    pub fn next(self) -> Result<Self> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| Error("authority epoch is exhausted".into()))
    }
}

impl PeerGrantVersion {
    pub fn validate(self) -> Result<()> {
        if self.0 == 0 {
            return Err(Error("peer grant version must be non-zero".into()));
        }
        Ok(())
    }
}

impl FenceToken {
    pub fn validate(self) -> Result<()> {
        if self.0 == 0 {
            return Err(Error("fence token must be non-zero".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DurationMs(pub u64);

macro_rules! finite_f32_newtypes {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq)]
            pub struct $name(pub f32);

            impl $name {
                pub fn new(value: f32) -> Result<Self> {
                    if value.is_finite() {
                        Ok(Self(value))
                    } else {
                        Err(Error(format!("{} must be finite", stringify!($name))))
                    }
                }

                pub const fn get(self) -> f32 {
                    self.0
                }
            }

            impl Serialize for $name {
                fn serialize<S>(
                    &self,
                    serializer: S,
                ) -> core::result::Result<S::Ok, S::Error>
                where
                    S: serde::Serializer,
                {
                    if !self.0.is_finite() {
                        return Err(serde::ser::Error::custom(format!(
                            "{} must be finite",
                            stringify!($name)
                        )));
                    }
                    serializer.serialize_f32(self.0)
                }
            }

            impl<'de> Deserialize<'de> for $name {
                fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
                where
                    D: serde::Deserializer<'de>,
                {
                    let value = f32::deserialize(deserializer)?;
                    Self::new(value).map_err(serde::de::Error::custom)
                }
            }
        )+
    };
}

finite_f32_newtypes!(Confidence, Weight, RestingActivation);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Recency(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RequiredTrue;

impl Serialize for RequiredTrue {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for RequiredTrue {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if bool::deserialize(deserializer)? {
            Ok(Self)
        } else {
            Err(serde::de::Error::custom("expected true"))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Reach(pub u32);

pub type Timestamp = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    UserTurn,
    ProactiveJob,
    Schedule,
    Subagent,
    Communication,
    Internal,
    Replay,
    Simulation,
    OwnerControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Actor {
    Owner,
    Agent,
    Subagent(RunId),
    External(ParticipantId),
    System,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum TrustTier {
    #[default]
    Untrusted,
    ApprovedSource,
    VerifiedProcess,
    OwnerInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: Source,
    pub actor: Actor,
    pub trust_tier: TrustTier,
    pub caused_by: Option<EventId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunStatus {
    Accepted,
    Running,
    Waiting,
    Complete,
    Aborted,
    Failed,
    Limited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContextSource {
    Rules,
    History,
    MemorySummary,
    SkillsMetadata,
    ToolSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutputKind {
    Final,
    Tool,
    Handoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyDecision {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApprovalOutcome {
    Granted,
    Denied,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BackendKind {
    Shell,
    File,
    Mcp,
    Notification,
    Browser,
    Computer,
    Pty,
    AppApi,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionType {
    Observe,
    Analyze,
    Prepare,
    Write,
    Execute,
    Deliver,
    ExternalCommit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApprovalRule {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileOperation {
    Read,
    Write,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum McpTransport {
    Stdio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpStdioSpec {
    pub schema_version: SchemaVersion,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExpectedEffect {
    Internal,
    Outward,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VerificationOutcome {
    Pass,
    Fail,
    Unverifiable(ReasonRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailureClass {
    GoalFramingFailure,
    ContextFailure,
    CognitiveMapFailure,
    ResourceSelectionFailure,
    ExecutionFailure,
    VerificationFailure,
    TrustFailure,
    ProactivityFailure,
    LearningFailure,
    HandoffFailure,
    SelfEvalTrap,
    SafetyPolicyFailure,
    MemoryMisevolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Impact {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StabilityTier {
    Fixed,
    Constitutional,
    Stable,
    Working,
    Session,
    Ephemeral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecisionActor {
    Auto,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActivationShape {
    Association,
    Change,
    Pressure,
    Gap,
    Tension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InterventionLevel {
    L0Observe,
    L1Suggest,
    L2Prepare,
    L3ActWithApproval,
    L4ActAutonomously,
    L5HighImpact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImpulseSource {
    Gap,
    Change,
    Tension,
    Association,
    Pressure,
    Commitment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EmissionGuard {
    pub value_gate_passed: bool,
    pub competence_gate_passed: bool,
    pub policy_and_envelope_passed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProposalOutcome {
    Adopt,
    Reject,
    Defer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeliveryMode {
    Hitchhike,
    Interrupt,
    Digest,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntentionSource {
    Commitment,
    DeferredProposal,
    SelfGenerated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntentionOutcome {
    Fired,
    Done,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompetenceInputs {
    pub map_confidence: Option<MapConfidenceRef>,
    pub agent_self_model: Option<AgentSelfModelRef>,
    pub capability_evidence: Vec<CapabilityEvidenceRef>,
    pub trust_profile: Option<TrustProfileRef>,
    pub failure_evidence: Vec<FailureEvidenceRef>,
    #[serde(default)]
    pub verification_evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionRefs {
    pub map: Option<JudgmentFrameRef>,
    pub user: Option<UserAttributeRef>,
    pub agent_self: Option<AgentSelfModelRef>,
    pub trust: Option<TrustProfileRef>,
    pub failure: Vec<FailureEvidenceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Modality {
    Text,
    Voice,
    Image,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DisclosureOutcome {
    Answer,
    Blur,
    Approve,
    Refuse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Representation {
    Agent,
    AgentRepresentingOwner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisclosureBinding {
    pub schema_version: SchemaVersion,
    pub session: CommunicationSessionId,
    pub participant: ParticipantId,
    pub purpose: PurposeRef,
    pub content_ref: ContentRef,
    pub category: String,
    pub sensitive: bool,
    pub confirmed: bool,
    pub high_impact: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConfigCheck {
    Provider,
    Credential,
    Capability,
    Context,
    Mcp,
    Plugin,
    FileSystem,
    Shell,
    Scheduler,
    Notification,
    Browser,
    Computer,
    Pty,
    Connector,
    Sync,
    Evolution,
    ReleaseAudit,
    Federation,
    Ecosystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComplianceScope {
    Upstream,
    License,
    Copy,
    Dependency,
    Notice,
    ReleaseTree,
    Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComplianceOutcome {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExternalInput {
    Literal(String),
    Content(ContentRef),
    Secret(SecretRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserActionSpec {
    pub schema_version: SchemaVersion,
    pub driver: ProviderId,
    pub target_url: String,
    pub allowed_origins: Vec<String>,
    pub operation: BrowserOperation,
    pub artifact_scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrowserOperation {
    Navigate,
    ReadText {
        selector: Option<String>,
    },
    Click {
        selector: String,
    },
    Type {
        selector: String,
        input: ExternalInput,
    },
    Screenshot {
        full_page: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinateBounds {
    pub schema_version: SchemaVersion,
    pub min_x: i32,
    pub min_y: i32,
    pub max_x_exclusive: i32,
    pub max_y_exclusive: i32,
}

impl CoordinateBounds {
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.min_x && x < self.max_x_exclusive && y >= self.min_y && y < self.max_y_exclusive
    }

    pub fn validate(self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.min_x >= self.max_x_exclusive
            || self.min_y >= self.max_y_exclusive
        {
            return Err(Error("computer coordinate bounds are invalid".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyCode {
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputerActionSpec {
    pub schema_version: SchemaVersion,
    pub driver: ProviderId,
    pub surface: SurfaceRef,
    pub bounds: CoordinateBounds,
    pub operation: ComputerOperation,
    pub artifact_scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputerOperation {
    Move {
        x: i32,
        y: i32,
    },
    Click {
        x: i32,
        y: i32,
        button: PointerButton,
    },
    Type {
        input: ExternalInput,
    },
    Key {
        key: KeyCode,
    },
    Scroll {
        dx: i32,
        dy: i32,
    },
    Screenshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtyActionSpec {
    pub schema_version: SchemaVersion,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub input: Option<ExternalInput>,
    pub environment: Vec<SecretBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretBinding {
    pub schema_version: SchemaVersion,
    pub name: String,
    pub value: SecretRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppApiMutationMethod {
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppApiOperation {
    Read,
    Mutation {
        method: AppApiMutationMethod,
        body: Option<ExternalInput>,
        idempotency_key: IdempotencyKey,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppApiActionSpec {
    pub schema_version: SchemaVersion,
    pub connector: ProviderId,
    pub endpoint: String,
    pub schema_digest: SchemaDigest,
    pub credential: Option<SecretRef>,
    pub operation: AppApiOperation,
    pub timeout: DurationMs,
    pub participant: Option<ParticipantId>,
    pub representation: Option<Representation>,
    pub disclosure_request: Option<DisclosureRequestRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectStatus {
    Observed,
    Committed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalActionReceipt {
    pub schema_version: SchemaVersion,
    pub action: ActionId,
    pub content_ref: Option<ContentRef>,
    pub content_digest: Option<SchemaDigest>,
    pub trust: TrustTier,
    pub effect: EffectStatus,
    pub probe_hint: Option<ProbeHintRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionParameters {
    Shell {
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        network: bool,
    },
    File {
        operation: FileOperation,
        path: String,
        content: Option<Vec<u8>>,
    },
    Mcp {
        server: McpServerRef,
        tool: ToolRef,
        arguments: serde_json::Value,
        #[serde(default)]
        schema_digest: Option<SchemaDigest>,
        transport: McpTransport,
        stdio: McpStdioSpec,
        timeout: DurationMs,
    },
    Notification {
        surface: SurfaceRef,
        target: ParticipantId,
        title: String,
        body_ref: ContentRef,
    },
    Browser(BrowserActionSpec),
    Computer(ComputerActionSpec),
    Pty(PtyActionSpec),
    AppApi(AppApiActionSpec),
    Remote(Box<crate::RemoteActionSpec>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionIntent {
    pub schema_version: SchemaVersion,
    pub intent_id: ActionId,
    pub source: Source,
    pub goal: GoalRef,
    pub backend_hint: BackendKind,
    pub capability_ref: CapabilityRef,
    pub action_type: ActionType,
    pub scope: Scope,
    pub risk_hint: Risk,
    pub expected_effect: ExpectedEffect,
    pub rollback_expectation: RollbackBoundary,
    pub parameters: ActionParameters,
    pub requested_permissions: Vec<PermissionRef>,
    pub requested_at: Timestamp,
    pub estimated_output_bytes: u64,
    pub estimated_duration: DurationMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateUpdate {
    pub schema_version: SchemaVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub schema_version: SchemaVersion,
    pub capabilities: Vec<CapabilityRef>,
    pub permissions: Vec<PermissionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timebox {
    pub schema_version: SchemaVersion,
    pub starts_at: Timestamp,
    pub expires_at: Timestamp,
    pub max_turns: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackReq {
    pub schema_version: SchemaVersion,
    pub required: bool,
    pub boundary: Option<RollbackBoundary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEvidence {
    pub schema_version: SchemaVersion,
    pub capability: CapabilityRef,
    pub outcome: CapabilityOutcome,
    pub reliability: Reliability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutonomyEnvelope {
    pub schema_version: SchemaVersion,
    pub scope: Scope,
    pub capability: CapabilitySet,
    pub action_type: Vec<ActionType>,
    pub risk_limit: Risk,
    pub approval_rule: ApprovalRule,
    pub budget: Budget,
    pub timebox: Timebox,
    pub rollback: RollbackReq,
}
