use core::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::*;

macro_rules! define_events {
    (
        $(
            $kind:ident => $payload:ident {
                $($(#[$field_meta:meta])* $field:ident: $field_type:ty),* $(,)?
            }
        ),+ $(,)?
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum EventKind {
            $($kind),+
        }

        impl EventKind {
            pub const ALL: [EventKind; 99] = [$(EventKind::$kind),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(EventKind::$kind => stringify!($kind)),+
                }
            }
        }

        impl FromStr for EventKind {
            type Err = Error;

            fn from_str(value: &str) -> Result<Self> {
                match value {
                    $(stringify!($kind) => Ok(EventKind::$kind)),+,
                    _ => Err(Error(format!("unknown event kind: {value}"))),
                }
            }
        }

        impl fmt::Display for EventKind {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        $(
            #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
            pub struct $payload {
                $($(#[$field_meta])* pub $field: $field_type),*
            }
        )+

        // EventPayload is a frozen additive protocol surface; boxing one variant would change it.
        #[allow(clippy::large_enum_variant)]
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub enum EventPayload {
            $($kind($payload)),+
        }

        impl EventPayload {
            pub const fn kind(&self) -> EventKind {
                match self {
                    $(EventPayload::$kind(_) => EventKind::$kind),+
                }
            }
        }
    };
}

// Architecture/03 section 2.1.1 is declared once here. The exact-length ALL
// array and generated payload mapping make taxonomy drift a compile-time error.
define_events! {
    // A. Run / Session lifecycle
    RunAccepted => RunAcceptedPayload {
        source: Source,
        session_ref: SessionId,
        input_ref: InputRef,
        idempotency_key: Option<IdempotencyKey>,
    },
    SessionBound => SessionBoundPayload {
        policy_profile: PolicyProfileRef,
        model_profile: ModelProfileRef,
        toolset_ref: ToolsetRef,
        workspace: WorkspaceRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effect_mode: Option<EffectMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evolution_snapshot: Option<EvolutionSnapshotRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        federation_snapshot: Option<FederationSnapshotRef>,
    },
    RunComplete => RunCompletePayload {
        stop_reason: StopReason,
        result_ref: Option<EventId>,
    },
    RunAborted => RunAbortedPayload {
        stop_reason: StopReason,
        result_ref: Option<EventId>,
    },
    RunFailed => RunFailedPayload {
        stop_reason: StopReason,
        result_ref: Option<EventId>,
    },
    RunLimited => RunLimitedPayload {
        stop_reason: StopReason,
        result_ref: Option<EventId>,
    },
    RunWaiting => RunWaitingPayload {
        wait_reason: WaitReason,
        resume_ref: ResumeRef,
    },
    RunResumed => RunResumedPayload {
        wait_reason: WaitReason,
        resume_ref: ResumeRef,
    },

    // B. Turn / Context
    TurnStarted => TurnStartedPayload { turn_index: u32 },
    TurnComplete => TurnCompletePayload { turn_index: u32 },
    ContextBuildStarted => ContextBuildStartedPayload {
        sources: Vec<ContextSource>,
        slice_refs: Vec<ContextSliceRef>,
    },
    ContextBuildFinished => ContextBuildFinishedPayload {
        sources: Vec<ContextSource>,
        slice_refs: Vec<ContextSliceRef>,
    },
    CompactionStarted => CompactionStartedPayload {
        lineage_ref: LineageRef,
        preserved_refs: Vec<PreservedRef>,
    },
    CompactionFinished => CompactionFinishedPayload {
        lineage_ref: LineageRef,
        preserved_refs: Vec<PreservedRef>,
        summary_ref: Option<SummaryRef>,
    },

    // C. Model
    ModelCallStarted => ModelCallStartedPayload {
        call_id: ModelCallId,
        model_profile: ModelProfileRef,
    },
    ModelCallDelta => ModelCallDeltaPayload {
        call_id: ModelCallId,
        delta: String,
    },
    ModelCallFinished => ModelCallFinishedPayload {
        call_id: ModelCallId,
        model_profile: ModelProfileRef,
        usage: ModelUsage,
        finish_reason: FinishReason,
    },
    OutputClassified => OutputClassifiedPayload { kind: OutputKind },

    // D. Tool / Policy / Approval / Handoff
    ToolCallProposed => ToolCallProposedPayload {
        call_id: ToolCallId,
        tool: ToolRef,
        args: serde_json::Value,
    },
    ToolPolicyEvaluated => ToolPolicyEvaluatedPayload {
        decision: PolicyDecision,
        rule_source: RuleSourceRef,
        reason: ReasonRef,
    },
    ApprovalRequested => ApprovalRequestedPayload {
        approval_id: ApprovalId,
        action_summary: ActionSummary,
        risk: Risk,
        scope: Scope,
        rollback_boundary: RollbackBoundary,
        expires_at: Timestamp,
        choices: Vec<ApprovalChoice>,
        requested_permissions: Vec<PermissionRef>,
        affected_resources: Vec<ResourceRef>,
    },
    ApprovalResolved => ApprovalResolvedPayload {
        approval_id: ApprovalId,
        outcome: ApprovalOutcome,
        grant_ref: Option<ApprovalGrantRef>,
    },
    HandoffRequested => HandoffRequestedPayload {
        target: HandoffTargetRef,
        reason: ReasonRef,
    },
    HandoffResolved => HandoffResolvedPayload {
        target: HandoffTargetRef,
        reason: ReasonRef,
    },

    // E. Action / Execution
    ActionPlanned => ActionPlannedPayload {
        intent_id: ActionId,
        plan_digest: PlanDigest,
        backend: BackendKind,
        expected_effect: ExpectedEffect,
        source: Source,
        scope: Scope,
        approval_ref: Option<ApprovalId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_placement: Option<RemotePlacementPlanRef>,
    },
    ActionStarted => ActionStartedPayload {
        intent_id: ActionId,
        backend: BackendKind,
        scope: Scope,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_lease: Option<RemoteExecutionLeaseRef>,
    },
    ActionOutputDelta => ActionOutputDeltaPayload {
        intent_id: ActionId,
        backend: BackendKind,
        scope: Scope,
        delta: String,
        truncated: bool,
        #[serde(default)]
        trust: TrustTier,
        #[serde(default)]
        content_ref: Option<ContentRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_lease: Option<RemoteExecutionLeaseRef>,
    },
    ActionCompleted => ActionCompletedPayload {
        intent_id: ActionId,
        result_ref: ActionResultRef,
        #[serde(default)]
        receipt: Option<ExternalActionReceipt>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_receipt: Option<RemoteExecutionReceiptRef>,
    },
    ActionFailed => ActionFailedPayload {
        intent_id: ActionId,
        failure_ref: FailureEvidenceRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_lease: Option<RemoteExecutionLeaseRef>,
    },
    ActionDenied => ActionDeniedPayload {
        intent_id: ActionId,
        reason: ReasonRef,
    },
    ActionCancelled => ActionCancelledPayload {
        intent_id: ActionId,
        reason: ReasonRef,
    },
    ActionOutcomeUnknown => ActionOutcomeUnknownPayload {
        intent_id: ActionId,
        probe_hint: ProbeHintRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_lease: Option<RemoteExecutionLeaseRef>,
    },

    // F. Verification
    VerificationStarted => VerificationStartedPayload {
        verifier_kind: VerifierKind,
        against: DoneContractRef,
    },
    VerificationFinished => VerificationFinishedPayload {
        verifier_kind: VerifierKind,
        outcome: VerificationOutcome,
        against: DoneContractRef,
    },

    // G. Failure
    FailureEvidenceRecorded => FailureEvidenceRecordedPayload {
        failure_ref: FailureEvidenceRef,
        class: FailureClass,
        impact: Impact,
        scope: Scope,
        related_refs: Vec<EvidenceRef>,
        suggested_fix: Option<SuggestedFixRef>,
    },
    FailureDigestUpdated => FailureDigestUpdatedPayload {
        digest_ref: FailureDigestRef,
        members: Vec<FailureEvidenceRef>,
        summary: DigestSummaryRef,
    },

    // H. Candidate / Evolution / Retraction
    CandidateCreated => CandidateCreatedPayload {
        candidate_id: CandidateId,
        target: CandidateTargetRef,
        evidence_refs: Vec<EvidenceRef>,
        confidence: Confidence,
        provenance: Provenance,
        target_tier: StabilityTier,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capability_update: Option<CapabilityUpdateProposal>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        strategy_candidate: Option<StrategyCandidate>,
    },
    CandidateConflictDetected => CandidateConflictDetectedPayload {
        candidate_id: CandidateId,
        conflict_with: CandidateId,
        kind: ConflictKind,
    },
    CandidatePromoted => CandidatePromotedPayload {
        candidate_id: CandidateId,
        by: DecisionActor,
        reason: ReasonRef,
    },
    CandidateRejected => CandidateRejectedPayload {
        candidate_id: CandidateId,
        by: DecisionActor,
        reason: ReasonRef,
    },
    CandidateDowngraded => CandidateDowngradedPayload {
        candidate_id: CandidateId,
        by: DecisionActor,
        reason: ReasonRef,
    },
    CandidateDecayed => CandidateDecayedPayload {
        candidate_id: CandidateId,
        by: DecisionActor,
        reason: ReasonRef,
    },
    RetractionEvent => RetractionEventPayload {
        target_object: ObjectRef,
        evidence_lineage: LineageRef,
    },
    RevocationEvent => RevocationEventPayload {
        target_object: ObjectRef,
        evidence_lineage: LineageRef,
    },
    ReevaluationTaskCreated => ReevaluationTaskCreatedPayload {
        derived_refs: Vec<ObjectRef>,
        trigger: ReevaluationTriggerRef,
    },

    // I. Cognitive / Proactive
    ObservationRecorded => ObservationRecordedPayload {
        source: Source,
        scope: Scope,
        grant_ref: Option<GrantRef>,
    },
    OpportunityDetected => OpportunityDetectedPayload {
        seed: Vec<NodeId>,
        activation_shape: Option<ActivationShape>,
    },
    ValueGateEvaluated => ValueGateEvaluatedPayload {
        decision: GateDecision,
        reason: ReasonRef,
    },
    CompetenceGateEvaluated => CompetenceGateEvaluatedPayload {
        scope: Scope,
        risk: Risk,
        max_level: InterventionLevel,
        reads: CompetenceInputs,
    },
    ImpulseRaised => ImpulseRaisedPayload {
        source: ImpulseSource,
        reach: Reach,
        seed: Vec<NodeId>,
    },
    ReflectionProduced => ReflectionProducedPayload {
        inputs: Vec<EvidenceRef>,
        candidate_refs: Vec<CandidateId>,
    },
    ProactiveProposalEmitted => ProactiveProposalEmittedPayload {
        proposal_ref: ProposalRef,
        proposal_kind: ProposalKind,
        level: InterventionLevel,
        guard: EmissionGuard,
        #[serde(default = "default_proactive_delivery")]
        delivery: DeliveryMode,
        #[serde(default)]
        attention_cost: u32,
    },
    ProactiveProposalResolved => ProactiveProposalResolvedPayload {
        proposal_ref: ProposalRef,
        outcome: ProposalOutcome,
        feedback: Option<FeedbackRef>,
    },
    ProspectiveIntentionCreated => ProspectiveIntentionCreatedPayload {
        intention_id: IntentionId,
        source: IntentionSource,
        trigger: IntentionTriggerRef,
        #[serde(default)]
        schedule: Option<ScheduleBinding>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal_frame: Option<GoalFrameRef>,
    },
    ProspectiveIntentionResolved => ProspectiveIntentionResolvedPayload {
        intention_id: IntentionId,
        outcome: IntentionOutcome,
    },

    // J. Coordination / Orchestration / Subagent
    GoalFramed => GoalFramedPayload {
        goal_frame: GoalFrameRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        long_term: Option<LongTermGoal>,
    },
    ResourcePlanned => ResourcePlannedPayload { plan: ResourcePlanRef },
    DoneContractSet => DoneContractSetPayload { contract: DoneContractRef },
    AutonomyEnvelopeSet => AutonomyEnvelopeSetPayload { envelope: AutonomyEnvelopeRef },
    DecisionTraceRecorded => DecisionTraceRecordedPayload {
        trace_ref: DecisionTraceRef,
        refs: DecisionRefs,
        rationale: Rationale,
        workspace_snapshot: AgentWorkspaceSnapshotRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resource_graph_snapshot: Option<ResourceGraphSnapshotRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evolution_snapshot: Option<EvolutionSnapshotRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        federation_snapshot: Option<FederationSnapshotRef>,
    },
    OrchestrationRouteCreated => OrchestrationRouteCreatedPayload {
        pattern_ref: Option<OrchestrationPatternRef>,
        route: ExecutionRouteRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal_frame: Option<GoalFrameRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint: Option<GoalCheckpoint>,
    },
    SubagentSpawned => SubagentSpawnedPayload {
        child_run: RunId,
        role: RoleRef,
        toolset_ref: ToolsetRef,
        model_profile: ModelProfileRef,
        permission: PermissionProfileRef,
        budget: Budget,
    },
    SubagentResultReturned => SubagentResultReturnedPayload {
        child_run: RunId,
        summary: SummaryRef,
        result_ref: ResultRef,
        status: RunStatus,
    },

    // K. Capability
    CapabilityIndexed => CapabilityIndexedPayload {
        capability: CapabilityRef,
        sources: Vec<CapabilitySourceRef>,
    },
    ToolsetResolved => ToolsetResolvedPayload {
        toolset_ref: ToolsetRef,
        sources: Vec<CapabilitySourceRef>,
    },
    McpDiscovered => McpDiscoveredPayload {
        server: McpServerRef,
        tools: Vec<ToolRef>,
        resources: Vec<ResourceRef>,
    },
    McpCallEvent => McpCallEventPayload {
        server: McpServerRef,
        tool: ToolRef,
        timeout: bool,
        error_class: Option<McpErrorClass>,
    },
    SkillMetadataExposed => SkillMetadataExposedPayload {
        skills: Vec<SkillDescriptorRef>,
        scope: Scope,
        version: Version,
        trust: TrustTier,
    },
    SkillBodyLoaded => SkillBodyLoadedPayload {
        skill: SkillRef,
        trigger: SkillTriggerRef,
        scope: Scope,
        version: Version,
        trust: TrustTier,
    },
    PluginContributionRegistered => PluginContributionRegisteredPayload {
        manifest: PluginManifestRef,
        contributions: Vec<PluginContributionRef>,
        enabled: bool,
        trust: TrustTier,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        managed_snapshot: Option<ManagedPluginSnapshotRef>,
    },
    PluginToggled => PluginToggledPayload {
        plugin: PluginRef,
        enabled: bool,
        trust: TrustTier,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        managed_policy: Option<ManagedPluginPolicyRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        managed_snapshot: Option<ManagedPluginSnapshotRef>,
    },
    CapabilityEvidenceRecorded => CapabilityEvidenceRecordedPayload {
        capability: CapabilityRef,
        outcome: CapabilityOutcome,
        reliability: Reliability,
    },

    // L. Communication
    CommunicationEventReceived => CommunicationEventReceivedPayload {
        modality: Modality,
        carrier: CarrierRef,
        channel_adapter: ChannelAdapterRef,
        participant: ParticipantId,
        scope: Scope,
        session_ref: Option<CommunicationSessionId>,
        content_ref: Option<ContentRef>,
    },
    CommunicationSessionOpened => CommunicationSessionOpenedPayload {
        session_id: CommunicationSessionId,
        participant: ParticipantId,
        purpose: PurposeRef,
        ttl: DurationMs,
        budget: Budget,
    },
    CommunicationSessionTerminated => CommunicationSessionTerminatedPayload {
        session_id: CommunicationSessionId,
        termination_reason: TerminationReason,
    },
    ExternalCommunicationGranted => ExternalCommunicationGrantedPayload {
        grant_ref: GrantRef,
        purpose: PurposeRef,
        disclosure: DisclosurePolicyRef,
        ttl: DurationMs,
        budget: Budget,
        transcript_policy: TranscriptPolicyRef,
    },
    DisclosurePolicyApplied => DisclosurePolicyAppliedPayload {
        request: DisclosureRequestRef,
        outcome: DisclosureOutcome,
        representation: Representation,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binding: Option<DisclosureBinding>,
    },
    CommunicationProposalEmitted => CommunicationProposalEmittedPayload {
        proposal_ref: CommunicationProposalRef,
        level: InterventionLevel,
    },

    // M. Memory
    MemoryNodeAppended => MemoryNodeAppendedPayload {
        node_id: NodeId,
        kind: MemoryNodeType,
        content_ref: ContentRef,
        tier: StabilityTier,
        confidence: Confidence,
        scope: Scope,
        resting_activation: RestingActivation,
        recency: Recency,
    },
    MemoryEdgeAppended => MemoryEdgeAppendedPayload {
        edge_id: EdgeId,
        from: NodeId,
        to: NodeId,
        kind: MemoryEdgeType,
        weight: Weight,
    },
    MemoryMaintenanceApplied => MemoryMaintenanceAppliedPayload { deltas_ref: MemoryDeltasRef },
    UserAttributeCandidateCreated => UserAttributeCandidateCreatedPayload {
        candidate_id: CandidateId,
        attribute: UserAttributeRef,
        value: UserAttributeValueRef,
        evidence: Vec<EvidenceRef>,
        confidence: Confidence,
        first_at: Timestamp,
        last_at: Timestamp,
        stability: StabilityTier,
        scope: Scope,
        conflicts: Vec<CandidateId>,
        feedback: Vec<FeedbackRef>,
    },
    ImportedHistoricalEvidenceRecorded => ImportedHistoricalEvidenceRecordedPayload {
        source: HistoricalSourceRef,
        low_weight: RequiredTrue,
        bootstrap_only: RequiredTrue,
    },
    CognitiveMapUpdateProposed => CognitiveMapUpdateProposedPayload {
        candidate_id: CandidateId,
        kind: MapUpdateKind,
        evidence: Vec<EvidenceRef>,
        frame: Option<JudgmentFrameRef>,
        quality: Option<QualityModelRef>,
        blindspot: Option<BlindSpotModelRef>,
        resource: Option<ResourceRef>,
        confidence: Confidence,
    },

    // N. Config / Compliance
    ConfigDoctorReport => ConfigDoctorReportPayload {
        checks: Vec<ConfigCheck>,
        findings: Vec<ConfigFindingRef>,
    },
    ComplianceCheckResult => ComplianceCheckResultPayload {
        scope: ComplianceScope,
        outcome: ComplianceOutcome,
        blocking: bool,
        findings: Vec<ComplianceFindingRef>,
    },

    // O. M3 Evolution Control (append-only after the frozen M2 prefix)
    EvolutionEvaluationRecorded => EvolutionEvaluationRecordedPayload {
        evaluation: EvolutionEvaluationRef,
        baseline: StrategyVersionRef,
        candidate: StrategyVersionRef,
        verdict: EvaluationVerdict,
        hard_invariants: Vec<InvariantResultRef>,
        ground_truth: Vec<EvidenceRef>,
    },
    StrategyActivated => StrategyActivatedPayload {
        activation: StrategyActivation,
        active_snapshot: EvolutionSnapshotRef,
    },
    StrategyRolledBack => StrategyRolledBackPayload {
        rollback: StrategyRollback,
        active_snapshot: EvolutionSnapshotRef,
    },

    // P. M4 Federation Control (append-only after the frozen M3 prefix)
    FederatedPeerRegistered => FederatedPeerRegisteredPayload {
        grant: FederatedPeerGrant,
        previous: Option<FederatedPeerGrantRef>,
        committed_version: FederationAggregateVersion,
    },
    FederatedPeerRevoked => FederatedPeerRevokedPayload {
        peer: FederatedPeerRef,
        revoked_grant: FederatedPeerGrantRef,
        new_authority_epoch: AuthorityEpoch,
        in_flight: InFlightDisposition,
        committed_version: FederationAggregateVersion,
    },
    RemoteExecutionLeaseChanged => RemoteExecutionLeaseChangedPayload {
        lease: RemoteExecutionLease,
        reason: ReasonRef,
        committed_version: FederationAggregateVersion,
    },
    ReplicationCheckpointAdvanced => ReplicationCheckpointAdvancedPayload {
        peer: FederatedPeerRef,
        aggregate: RunId,
        from_stream_seq: u64,
        to_stream_seq: u64,
        batch_digest: SchemaDigest,
        redaction: RedactionPolicyRef,
        authority_epoch: AuthorityEpoch,
        committed_version: FederationAggregateVersion,
    },

    // Q. M5 Capability Ecosystem (append-only after the frozen M4 prefix)
    CapabilityPublisherChanged => CapabilityPublisherChangedPayload {
        grant: CapabilityPublisherGrant,
        previous: Option<CapabilityPublisherGrantRef>,
        committed_version: EcosystemAggregateVersion,
    },
    CapabilityPackageAdmitted => CapabilityPackageAdmittedPayload {
        admission: CapabilityPackageAdmission,
        committed_version: EcosystemAggregateVersion,
    },
    CapabilityPackageStateChanged => CapabilityPackageStateChangedPayload {
        change: CapabilityPackageStateChange,
        committed_version: EcosystemAggregateVersion,
    },
    CapabilityPackageDistributionRecorded => CapabilityPackageDistributionRecordedPayload {
        receipt: CapabilityPackageDistributionReceipt,
        committed_version: EcosystemAggregateVersion,
    },

    // R. V1 Core Brain Closure (append-only after the frozen M5 prefix)
    WorkspaceCharterChanged => WorkspaceCharterChangedPayload {
        charter: WorkspaceCharterRecord,
        expected_version: u64,
        committed_version: u64,
    },
    DataLifecycleApplied => DataLifecycleAppliedPayload {
        receipt: DataLifecycleReceipt,
        expected_version: u64,
        committed_version: u64,
    },
}

fn default_proactive_delivery() -> DeliveryMode {
    DeliveryMode::Hitchhike
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event_id: EventId,
    pub run_id: RunId,
    pub stream_seq: u64,
    pub turn_id: Option<TurnId>,
    pub kind: EventKind,
    pub payload: EventPayload,
    pub schema_version: SchemaVersion,
    pub ts_unix_ms: i64,
    pub provenance: Provenance,
}

impl Event {
    pub fn new(
        event_id: EventId,
        run_id: RunId,
        turn_id: Option<TurnId>,
        payload: EventPayload,
        schema_version: SchemaVersion,
        ts_unix_ms: i64,
        provenance: Provenance,
    ) -> Self {
        let kind = payload.kind();
        Self {
            event_id,
            run_id,
            stream_seq: 0,
            turn_id,
            kind,
            payload,
            schema_version,
            ts_unix_ms,
            provenance,
        }
    }

    pub fn validate_payload_kind(&self) -> Result<()> {
        let payload_kind = self.payload.kind();
        if self.kind == payload_kind {
            Ok(())
        } else {
            Err(Error(format!(
                "event kind {} does not match payload kind {}",
                self.kind, payload_kind
            )))
        }
    }
}
