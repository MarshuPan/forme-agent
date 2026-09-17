use std::{fmt::Debug, str::FromStr};

use forme_protocol::*;
use serde::{de::DeserializeOwned, Serialize};

macro_rules! text {
    ($type:ident, $value:literal) => {
        $type($value.into())
    };
}

const M2_EVENT_KIND_PREFIX: [EventKind; 86] = [
    EventKind::RunAccepted,
    EventKind::SessionBound,
    EventKind::RunComplete,
    EventKind::RunAborted,
    EventKind::RunFailed,
    EventKind::RunLimited,
    EventKind::RunWaiting,
    EventKind::RunResumed,
    EventKind::TurnStarted,
    EventKind::TurnComplete,
    EventKind::ContextBuildStarted,
    EventKind::ContextBuildFinished,
    EventKind::CompactionStarted,
    EventKind::CompactionFinished,
    EventKind::ModelCallStarted,
    EventKind::ModelCallDelta,
    EventKind::ModelCallFinished,
    EventKind::OutputClassified,
    EventKind::ToolCallProposed,
    EventKind::ToolPolicyEvaluated,
    EventKind::ApprovalRequested,
    EventKind::ApprovalResolved,
    EventKind::HandoffRequested,
    EventKind::HandoffResolved,
    EventKind::ActionPlanned,
    EventKind::ActionStarted,
    EventKind::ActionOutputDelta,
    EventKind::ActionCompleted,
    EventKind::ActionFailed,
    EventKind::ActionDenied,
    EventKind::ActionCancelled,
    EventKind::ActionOutcomeUnknown,
    EventKind::VerificationStarted,
    EventKind::VerificationFinished,
    EventKind::FailureEvidenceRecorded,
    EventKind::FailureDigestUpdated,
    EventKind::CandidateCreated,
    EventKind::CandidateConflictDetected,
    EventKind::CandidatePromoted,
    EventKind::CandidateRejected,
    EventKind::CandidateDowngraded,
    EventKind::CandidateDecayed,
    EventKind::RetractionEvent,
    EventKind::RevocationEvent,
    EventKind::ReevaluationTaskCreated,
    EventKind::ObservationRecorded,
    EventKind::OpportunityDetected,
    EventKind::ValueGateEvaluated,
    EventKind::CompetenceGateEvaluated,
    EventKind::ImpulseRaised,
    EventKind::ReflectionProduced,
    EventKind::ProactiveProposalEmitted,
    EventKind::ProactiveProposalResolved,
    EventKind::ProspectiveIntentionCreated,
    EventKind::ProspectiveIntentionResolved,
    EventKind::GoalFramed,
    EventKind::ResourcePlanned,
    EventKind::DoneContractSet,
    EventKind::AutonomyEnvelopeSet,
    EventKind::DecisionTraceRecorded,
    EventKind::OrchestrationRouteCreated,
    EventKind::SubagentSpawned,
    EventKind::SubagentResultReturned,
    EventKind::CapabilityIndexed,
    EventKind::ToolsetResolved,
    EventKind::McpDiscovered,
    EventKind::McpCallEvent,
    EventKind::SkillMetadataExposed,
    EventKind::SkillBodyLoaded,
    EventKind::PluginContributionRegistered,
    EventKind::PluginToggled,
    EventKind::CapabilityEvidenceRecorded,
    EventKind::CommunicationEventReceived,
    EventKind::CommunicationSessionOpened,
    EventKind::CommunicationSessionTerminated,
    EventKind::ExternalCommunicationGranted,
    EventKind::DisclosurePolicyApplied,
    EventKind::CommunicationProposalEmitted,
    EventKind::MemoryNodeAppended,
    EventKind::MemoryEdgeAppended,
    EventKind::MemoryMaintenanceApplied,
    EventKind::UserAttributeCandidateCreated,
    EventKind::ImportedHistoricalEvidenceRecorded,
    EventKind::CognitiveMapUpdateProposed,
    EventKind::ConfigDoctorReport,
    EventKind::ComplianceCheckResult,
];

fn provenance() -> Provenance {
    Provenance {
        source: Source::UserTurn,
        actor: Actor::Owner,
        trust_tier: TrustTier::OwnerInput,
        caused_by: None,
    }
}

fn competence_inputs() -> CompetenceInputs {
    CompetenceInputs {
        map_confidence: Some(text!(MapConfidenceRef, "map-confidence-1")),
        agent_self_model: Some(text!(AgentSelfModelRef, "self-model-1")),
        capability_evidence: vec![text!(CapabilityEvidenceRef, "capability-evidence-1")],
        trust_profile: Some(text!(TrustProfileRef, "trust-profile-1")),
        failure_evidence: vec![text!(FailureEvidenceRef, "failure-evidence-1")],
        verification_evidence: vec![text!(EvidenceRef, "verification-evidence-1")],
    }
}

fn decision_refs() -> DecisionRefs {
    DecisionRefs {
        map: Some(text!(JudgmentFrameRef, "judgment-frame-1")),
        user: Some(text!(UserAttributeRef, "user-attribute-1")),
        agent_self: Some(text!(AgentSelfModelRef, "self-model-1")),
        trust: Some(text!(TrustProfileRef, "trust-profile-1")),
        failure: vec![text!(FailureEvidenceRef, "failure-evidence-1")],
    }
}

fn evolution_version(value: u64) -> EvolutionAggregateVersion {
    EvolutionAggregateVersion {
        schema_version: SchemaVersion(1),
        aggregate: text!(EvolutionAggregateRef, "evolution:owner:workspace-1"),
        value,
    }
}

fn strategy_activation() -> StrategyActivation {
    StrategyActivation {
        schema_version: SchemaVersion(1),
        aggregate: text!(EvolutionAggregateRef, "evolution:owner:workspace-1"),
        domain: StrategyDomain::Loop,
        scope: text!(Scope, "workspace-1"),
        from: Some(text!(StrategyVersionRef, "loop:v1")),
        to: text!(StrategyVersionRef, "loop:v2"),
        spec_ref: text!(ContentRef, "content:loop:v2"),
        spec_digest: text!(SchemaDigest, "digest:loop:v2"),
        evaluation: text!(EvolutionEvaluationRef, "evaluation:loop:v2"),
        promotion: text!(EventId, "event:promotion:loop:v2"),
        owner_confirmation: None,
        impact: EvolutionImpact::Cautious,
        expected_version: evolution_version(0),
        committed_version: evolution_version(1),
    }
}

fn strategy_rollback() -> StrategyRollback {
    StrategyRollback {
        schema_version: SchemaVersion(1),
        aggregate: text!(EvolutionAggregateRef, "evolution:owner:workspace-1"),
        domain: StrategyDomain::Loop,
        scope: text!(Scope, "workspace-1"),
        failed: text!(StrategyVersionRef, "loop:v2"),
        restored: text!(StrategyVersionRef, "loop:v1"),
        restored_spec_ref: text!(ContentRef, "content:loop:v1"),
        restored_spec_digest: text!(SchemaDigest, "digest:loop:v1"),
        triggers: vec![text!(EvidenceRef, "evidence:regression:1")],
        expected_version: evolution_version(1),
        committed_version: evolution_version(2),
        in_flight: InFlightDisposition::KeepPinned,
        external_effects_reverted: HistoricalFalse,
    }
}

fn all_payload_fixtures() -> Vec<EventPayload> {
    vec![
        EventPayload::RunAccepted(RunAcceptedPayload {
            source: Source::UserTurn,
            session_ref: text!(SessionId, "session-1"),
            input_ref: text!(InputRef, "input-1"),
            idempotency_key: Some(text!(IdempotencyKey, "request-1")),
        }),
        EventPayload::SessionBound(SessionBoundPayload {
            policy_profile: text!(PolicyProfileRef, "policy-1"),
            model_profile: text!(ModelProfileRef, "model-1"),
            toolset_ref: text!(ToolsetRef, "toolset-1"),
            workspace: text!(WorkspaceRef, "workspace-1"),
            effect_mode: None,
            evolution_snapshot: None,
            federation_snapshot: None,
        }),
        EventPayload::RunComplete(RunCompletePayload {
            stop_reason: text!(StopReason, "complete"),
            result_ref: Some(text!(EventId, "event-result-1")),
        }),
        EventPayload::RunAborted(RunAbortedPayload {
            stop_reason: text!(StopReason, "aborted"),
            result_ref: None,
        }),
        EventPayload::RunFailed(RunFailedPayload {
            stop_reason: text!(StopReason, "failed"),
            result_ref: Some(text!(EventId, "event-result-2")),
        }),
        EventPayload::RunLimited(RunLimitedPayload {
            stop_reason: text!(StopReason, "limited"),
            result_ref: None,
        }),
        EventPayload::RunWaiting(RunWaitingPayload {
            wait_reason: text!(WaitReason, "approval"),
            resume_ref: text!(ResumeRef, "resume-1"),
        }),
        EventPayload::RunResumed(RunResumedPayload {
            wait_reason: text!(WaitReason, "approval"),
            resume_ref: text!(ResumeRef, "resume-1"),
        }),
        EventPayload::TurnStarted(TurnStartedPayload { turn_index: 0 }),
        EventPayload::TurnComplete(TurnCompletePayload { turn_index: 0 }),
        EventPayload::ContextBuildStarted(ContextBuildStartedPayload {
            sources: vec![ContextSource::Rules],
            slice_refs: vec![text!(ContextSliceRef, "slice-1")],
        }),
        EventPayload::ContextBuildFinished(ContextBuildFinishedPayload {
            sources: vec![ContextSource::History],
            slice_refs: vec![text!(ContextSliceRef, "slice-2")],
        }),
        EventPayload::CompactionStarted(CompactionStartedPayload {
            lineage_ref: text!(LineageRef, "lineage-1"),
            preserved_refs: vec![text!(PreservedRef, "preserved-1")],
        }),
        EventPayload::CompactionFinished(CompactionFinishedPayload {
            lineage_ref: text!(LineageRef, "lineage-1"),
            preserved_refs: vec![text!(PreservedRef, "preserved-1")],
            summary_ref: Some(text!(SummaryRef, "summary-1")),
        }),
        EventPayload::ModelCallStarted(ModelCallStartedPayload {
            call_id: text!(ModelCallId, "model-call-1"),
            model_profile: text!(ModelProfileRef, "model-1"),
        }),
        EventPayload::ModelCallDelta(ModelCallDeltaPayload {
            call_id: text!(ModelCallId, "model-call-1"),
            delta: "typed delta".into(),
        }),
        EventPayload::ModelCallFinished(ModelCallFinishedPayload {
            call_id: text!(ModelCallId, "model-call-1"),
            model_profile: text!(ModelProfileRef, "model-1"),
            usage: ModelUsage {
                input_tokens: 10,
                output_tokens: 20,
            },
            finish_reason: text!(FinishReason, "stop"),
        }),
        EventPayload::OutputClassified(OutputClassifiedPayload {
            kind: OutputKind::Final,
        }),
        EventPayload::ToolCallProposed(ToolCallProposedPayload {
            call_id: text!(ToolCallId, "tool-call-1"),
            tool: text!(ToolRef, "filesystem.read"),
            args: serde_json::json!({"path": "README.md"}),
        }),
        EventPayload::ToolPolicyEvaluated(ToolPolicyEvaluatedPayload {
            decision: PolicyDecision::Ask,
            rule_source: text!(RuleSourceRef, "rule-1"),
            reason: text!(ReasonRef, "approval-required"),
        }),
        EventPayload::ApprovalRequested(ApprovalRequestedPayload {
            approval_id: text!(ApprovalId, "approval-1"),
            action_summary: text!(ActionSummary, "write file"),
            risk: Risk::Medium,
            scope: text!(Scope, "workspace"),
            rollback_boundary: text!(RollbackBoundary, "single-file"),
            expires_at: 1_700_000_060_000,
            choices: vec![text!(ApprovalChoice, "allow-once")],
            requested_permissions: vec![text!(PermissionRef, "file-write")],
            affected_resources: vec![text!(ResourceRef, "README.md")],
        }),
        EventPayload::ApprovalResolved(ApprovalResolvedPayload {
            approval_id: text!(ApprovalId, "approval-1"),
            outcome: ApprovalOutcome::Granted,
            grant_ref: Some(text!(ApprovalGrantRef, "grant-1")),
        }),
        EventPayload::HandoffRequested(HandoffRequestedPayload {
            target: text!(HandoffTargetRef, "human"),
            reason: text!(ReasonRef, "needs-context"),
        }),
        EventPayload::HandoffResolved(HandoffResolvedPayload {
            target: text!(HandoffTargetRef, "human"),
            reason: text!(ReasonRef, "context-supplied"),
        }),
        EventPayload::ActionPlanned(ActionPlannedPayload {
            intent_id: text!(ActionId, "action-1"),
            plan_digest: text!(PlanDigest, "digest-1"),
            backend: BackendKind::File,
            expected_effect: ExpectedEffect::Outward,
            source: Source::UserTurn,
            scope: text!(Scope, "workspace"),
            approval_ref: Some(text!(ApprovalId, "approval-1")),
            remote_placement: None,
        }),
        EventPayload::ActionStarted(ActionStartedPayload {
            intent_id: text!(ActionId, "action-1"),
            backend: BackendKind::File,
            scope: text!(Scope, "workspace"),
            remote_lease: None,
        }),
        EventPayload::ActionOutputDelta(ActionOutputDeltaPayload {
            intent_id: text!(ActionId, "action-1"),
            backend: BackendKind::File,
            scope: text!(Scope, "workspace"),
            delta: "wrote bytes".into(),
            truncated: false,
            trust: TrustTier::Untrusted,
            content_ref: None,
            remote_lease: None,
        }),
        EventPayload::ActionCompleted(ActionCompletedPayload {
            intent_id: text!(ActionId, "action-1"),
            result_ref: text!(ActionResultRef, "action-result-1"),
            receipt: None,
            remote_receipt: None,
        }),
        EventPayload::ActionFailed(ActionFailedPayload {
            intent_id: text!(ActionId, "action-2"),
            failure_ref: text!(FailureEvidenceRef, "failure-1"),
            remote_lease: None,
        }),
        EventPayload::ActionDenied(ActionDeniedPayload {
            intent_id: text!(ActionId, "action-3"),
            reason: text!(ReasonRef, "denied"),
        }),
        EventPayload::ActionCancelled(ActionCancelledPayload {
            intent_id: text!(ActionId, "action-4"),
            reason: text!(ReasonRef, "cancelled"),
        }),
        EventPayload::ActionOutcomeUnknown(ActionOutcomeUnknownPayload {
            intent_id: text!(ActionId, "action-5"),
            probe_hint: text!(ProbeHintRef, "check-file"),
            remote_lease: None,
        }),
        EventPayload::VerificationStarted(VerificationStartedPayload {
            verifier_kind: text!(VerifierKind, "deterministic"),
            against: text!(DoneContractRef, "done-contract-1"),
        }),
        EventPayload::VerificationFinished(VerificationFinishedPayload {
            verifier_kind: text!(VerifierKind, "deterministic"),
            outcome: VerificationOutcome::Unverifiable(text!(ReasonRef, "offline")),
            against: text!(DoneContractRef, "done-contract-1"),
        }),
        EventPayload::FailureEvidenceRecorded(FailureEvidenceRecordedPayload {
            failure_ref: text!(FailureEvidenceRef, "failure-1"),
            class: FailureClass::ExecutionFailure,
            impact: Impact::Medium,
            scope: text!(Scope, "workspace"),
            related_refs: vec![text!(EvidenceRef, "evidence-1")],
            suggested_fix: Some(text!(SuggestedFixRef, "retry-with-probe")),
        }),
        EventPayload::FailureDigestUpdated(FailureDigestUpdatedPayload {
            digest_ref: text!(FailureDigestRef, "failure-digest-1"),
            members: vec![text!(FailureEvidenceRef, "failure-1")],
            summary: text!(DigestSummaryRef, "summary-1"),
        }),
        EventPayload::CandidateCreated(CandidateCreatedPayload {
            candidate_id: text!(CandidateId, "candidate-1"),
            target: text!(CandidateTargetRef, "memory-node-1"),
            evidence_refs: vec![text!(EvidenceRef, "evidence-1")],
            confidence: Confidence(0.8),
            provenance: provenance(),
            target_tier: StabilityTier::Working,
            capability_update: None,
            strategy_candidate: None,
        }),
        EventPayload::CandidateConflictDetected(CandidateConflictDetectedPayload {
            candidate_id: text!(CandidateId, "candidate-1"),
            conflict_with: text!(CandidateId, "candidate-2"),
            kind: text!(ConflictKind, "contradiction"),
        }),
        EventPayload::CandidatePromoted(CandidatePromotedPayload {
            candidate_id: text!(CandidateId, "candidate-1"),
            by: DecisionActor::User,
            reason: text!(ReasonRef, "confirmed"),
        }),
        EventPayload::CandidateRejected(CandidateRejectedPayload {
            candidate_id: text!(CandidateId, "candidate-2"),
            by: DecisionActor::User,
            reason: text!(ReasonRef, "incorrect"),
        }),
        EventPayload::CandidateDowngraded(CandidateDowngradedPayload {
            candidate_id: text!(CandidateId, "candidate-3"),
            by: DecisionActor::Auto,
            reason: text!(ReasonRef, "stale"),
        }),
        EventPayload::CandidateDecayed(CandidateDecayedPayload {
            candidate_id: text!(CandidateId, "candidate-4"),
            by: DecisionActor::Auto,
            reason: text!(ReasonRef, "expired"),
        }),
        EventPayload::RetractionEvent(RetractionEventPayload {
            target_object: text!(ObjectRef, "object-1"),
            evidence_lineage: text!(LineageRef, "lineage-1"),
        }),
        EventPayload::RevocationEvent(RevocationEventPayload {
            target_object: text!(ObjectRef, "object-2"),
            evidence_lineage: text!(LineageRef, "lineage-2"),
        }),
        EventPayload::ReevaluationTaskCreated(ReevaluationTaskCreatedPayload {
            derived_refs: vec![text!(ObjectRef, "object-3")],
            trigger: text!(ReevaluationTriggerRef, "retraction"),
        }),
        EventPayload::ObservationRecorded(ObservationRecordedPayload {
            source: Source::ProactiveJob,
            scope: text!(Scope, "workspace"),
            grant_ref: Some(text!(GrantRef, "grant-1")),
        }),
        EventPayload::OpportunityDetected(OpportunityDetectedPayload {
            seed: vec![text!(NodeId, "node-1")],
            activation_shape: Some(ActivationShape::Gap),
        }),
        EventPayload::ValueGateEvaluated(ValueGateEvaluatedPayload {
            decision: text!(GateDecision, "pass"),
            reason: text!(ReasonRef, "useful"),
        }),
        EventPayload::CompetenceGateEvaluated(CompetenceGateEvaluatedPayload {
            scope: text!(Scope, "workspace"),
            risk: Risk::Low,
            max_level: InterventionLevel::L2Prepare,
            reads: competence_inputs(),
        }),
        EventPayload::ImpulseRaised(ImpulseRaisedPayload {
            source: ImpulseSource::Gap,
            reach: Reach(1),
            seed: vec![text!(NodeId, "node-1")],
        }),
        EventPayload::ReflectionProduced(ReflectionProducedPayload {
            inputs: vec![text!(EvidenceRef, "evidence-1")],
            candidate_refs: vec![text!(CandidateId, "candidate-1")],
        }),
        EventPayload::ProactiveProposalEmitted(ProactiveProposalEmittedPayload {
            proposal_ref: text!(ProposalRef, "proposal-1"),
            proposal_kind: text!(ProposalKind, "suggestion"),
            level: InterventionLevel::L1Suggest,
            guard: EmissionGuard {
                value_gate_passed: true,
                competence_gate_passed: true,
                policy_and_envelope_passed: true,
            },
            delivery: DeliveryMode::Hitchhike,
            attention_cost: 0,
        }),
        EventPayload::ProactiveProposalResolved(ProactiveProposalResolvedPayload {
            proposal_ref: text!(ProposalRef, "proposal-1"),
            outcome: ProposalOutcome::Adopt,
            feedback: Some(text!(FeedbackRef, "feedback-1")),
        }),
        EventPayload::ProspectiveIntentionCreated(ProspectiveIntentionCreatedPayload {
            intention_id: text!(IntentionId, "intention-1"),
            source: IntentionSource::Commitment,
            trigger: text!(IntentionTriggerRef, "tomorrow"),
            schedule: None,
            goal_frame: None,
        }),
        EventPayload::ProspectiveIntentionResolved(ProspectiveIntentionResolvedPayload {
            intention_id: text!(IntentionId, "intention-1"),
            outcome: IntentionOutcome::Done,
        }),
        EventPayload::GoalFramed(GoalFramedPayload {
            goal_frame: text!(GoalFrameRef, "goal-frame-1"),
            long_term: None,
        }),
        EventPayload::ResourcePlanned(ResourcePlannedPayload {
            plan: text!(ResourcePlanRef, "resource-plan-1"),
        }),
        EventPayload::DoneContractSet(DoneContractSetPayload {
            contract: text!(DoneContractRef, "done-contract-1"),
        }),
        EventPayload::AutonomyEnvelopeSet(AutonomyEnvelopeSetPayload {
            envelope: text!(AutonomyEnvelopeRef, "autonomy-envelope-1"),
        }),
        EventPayload::DecisionTraceRecorded(DecisionTraceRecordedPayload {
            trace_ref: text!(DecisionTraceRef, "decision-trace-1"),
            refs: decision_refs(),
            rationale: text!(Rationale, "rationale-1"),
            workspace_snapshot: text!(AgentWorkspaceSnapshotRef, "workspace-snapshot-1"),
            resource_graph_snapshot: None,
            evolution_snapshot: None,
            federation_snapshot: None,
        }),
        EventPayload::OrchestrationRouteCreated(OrchestrationRouteCreatedPayload {
            pattern_ref: Some(text!(OrchestrationPatternRef, "pattern-1")),
            route: text!(ExecutionRouteRef, "route-1"),
            goal_frame: None,
            checkpoint: None,
        }),
        EventPayload::SubagentSpawned(SubagentSpawnedPayload {
            child_run: text!(RunId, "child-run-1"),
            role: text!(RoleRef, "researcher"),
            toolset_ref: text!(ToolsetRef, "toolset-1"),
            model_profile: text!(ModelProfileRef, "model-1"),
            permission: text!(PermissionProfileRef, "permission-profile-1"),
            budget: text!(Budget, "subagent-budget"),
        }),
        EventPayload::SubagentResultReturned(SubagentResultReturnedPayload {
            child_run: text!(RunId, "child-run-1"),
            summary: text!(SummaryRef, "summary-1"),
            result_ref: text!(ResultRef, "result-1"),
            status: RunStatus::Complete,
        }),
        EventPayload::CapabilityIndexed(CapabilityIndexedPayload {
            capability: text!(CapabilityRef, "capability-1"),
            sources: vec![text!(CapabilitySourceRef, "source-1")],
        }),
        EventPayload::ToolsetResolved(ToolsetResolvedPayload {
            toolset_ref: text!(ToolsetRef, "toolset-1"),
            sources: vec![text!(CapabilitySourceRef, "source-1")],
        }),
        EventPayload::McpDiscovered(McpDiscoveredPayload {
            server: text!(McpServerRef, "server-1"),
            tools: vec![text!(ToolRef, "tool-1")],
            resources: vec![text!(ResourceRef, "resource-1")],
        }),
        EventPayload::McpCallEvent(McpCallEventPayload {
            server: text!(McpServerRef, "server-1"),
            tool: text!(ToolRef, "tool-1"),
            timeout: false,
            error_class: None,
        }),
        EventPayload::SkillMetadataExposed(SkillMetadataExposedPayload {
            skills: vec![text!(SkillDescriptorRef, "skill-1")],
            scope: text!(Scope, "workspace"),
            version: Version(1),
            trust: TrustTier::ApprovedSource,
        }),
        EventPayload::SkillBodyLoaded(SkillBodyLoadedPayload {
            skill: text!(SkillRef, "skill-1"),
            trigger: text!(SkillTriggerRef, "explicit"),
            scope: text!(Scope, "workspace"),
            version: Version(1),
            trust: TrustTier::ApprovedSource,
        }),
        EventPayload::PluginContributionRegistered(PluginContributionRegisteredPayload {
            manifest: text!(PluginManifestRef, "manifest-1"),
            contributions: vec![text!(PluginContributionRef, "contribution-1")],
            enabled: true,
            trust: TrustTier::ApprovedSource,
            managed_snapshot: None,
        }),
        EventPayload::PluginToggled(PluginToggledPayload {
            plugin: text!(PluginRef, "plugin-1"),
            enabled: true,
            trust: TrustTier::ApprovedSource,
            managed_policy: None,
            managed_snapshot: None,
        }),
        EventPayload::CapabilityEvidenceRecorded(CapabilityEvidenceRecordedPayload {
            capability: text!(CapabilityRef, "capability-1"),
            outcome: text!(CapabilityOutcome, "success"),
            reliability: text!(Reliability, "observed"),
        }),
        EventPayload::CommunicationEventReceived(CommunicationEventReceivedPayload {
            modality: Modality::Text,
            carrier: text!(CarrierRef, "carrier-1"),
            channel_adapter: text!(ChannelAdapterRef, "adapter-1"),
            participant: text!(ParticipantId, "participant-1"),
            scope: text!(Scope, "channel"),
            session_ref: Some(text!(CommunicationSessionId, "communication-session-1")),
            content_ref: Some(text!(ContentRef, "content-1")),
        }),
        EventPayload::CommunicationSessionOpened(CommunicationSessionOpenedPayload {
            session_id: text!(CommunicationSessionId, "communication-session-1"),
            participant: text!(ParticipantId, "participant-1"),
            purpose: text!(PurposeRef, "support"),
            ttl: DurationMs(60_000),
            budget: text!(Budget, "communication-budget"),
        }),
        EventPayload::CommunicationSessionTerminated(CommunicationSessionTerminatedPayload {
            session_id: text!(CommunicationSessionId, "communication-session-1"),
            termination_reason: text!(TerminationReason, "complete"),
        }),
        EventPayload::ExternalCommunicationGranted(ExternalCommunicationGrantedPayload {
            grant_ref: text!(GrantRef, "grant-1"),
            purpose: text!(PurposeRef, "support"),
            disclosure: text!(DisclosurePolicyRef, "minimal"),
            ttl: DurationMs(60_000),
            budget: text!(Budget, "communication-budget"),
            transcript_policy: text!(TranscriptPolicyRef, "retain-none"),
        }),
        EventPayload::DisclosurePolicyApplied(DisclosurePolicyAppliedPayload {
            request: text!(DisclosureRequestRef, "disclosure-request-1"),
            outcome: DisclosureOutcome::Answer,
            representation: Representation::Agent,
            binding: Some(DisclosureBinding {
                schema_version: SchemaVersion(1),
                session: text!(CommunicationSessionId, "communication-session-1"),
                participant: text!(ParticipantId, "participant-1"),
                purpose: text!(PurposeRef, "bounded-purpose"),
                content_ref: text!(ContentRef, "content-1"),
                category: "public".into(),
                sensitive: false,
                confirmed: true,
                high_impact: false,
            }),
        }),
        EventPayload::CommunicationProposalEmitted(CommunicationProposalEmittedPayload {
            proposal_ref: text!(CommunicationProposalRef, "communication-proposal-1"),
            level: InterventionLevel::L1Suggest,
        }),
        EventPayload::MemoryNodeAppended(MemoryNodeAppendedPayload {
            node_id: text!(NodeId, "node-1"),
            kind: text!(MemoryNodeType, "observation"),
            content_ref: text!(ContentRef, "content-1"),
            tier: StabilityTier::Working,
            confidence: Confidence(0.7),
            scope: text!(Scope, "owner"),
            resting_activation: RestingActivation(0.25),
            recency: Recency(1_700_000_000_000),
        }),
        EventPayload::MemoryEdgeAppended(MemoryEdgeAppendedPayload {
            edge_id: text!(EdgeId, "edge-1"),
            from: text!(NodeId, "node-1"),
            to: text!(NodeId, "node-2"),
            kind: text!(MemoryEdgeType, "supports"),
            weight: Weight(0.8),
        }),
        EventPayload::MemoryMaintenanceApplied(MemoryMaintenanceAppliedPayload {
            deltas_ref: text!(MemoryDeltasRef, "memory-deltas-1"),
        }),
        EventPayload::UserAttributeCandidateCreated(UserAttributeCandidateCreatedPayload {
            candidate_id: text!(CandidateId, "candidate-5"),
            attribute: text!(UserAttributeRef, "preferred-language"),
            value: text!(UserAttributeValueRef, "zh-CN"),
            evidence: vec![text!(EvidenceRef, "evidence-1")],
            confidence: Confidence(0.9),
            first_at: 1_700_000_000_000,
            last_at: 1_700_000_010_000,
            stability: StabilityTier::Working,
            scope: text!(Scope, "owner"),
            conflicts: vec![text!(CandidateId, "candidate-6")],
            feedback: vec![text!(FeedbackRef, "feedback-1")],
        }),
        EventPayload::ImportedHistoricalEvidenceRecorded(
            ImportedHistoricalEvidenceRecordedPayload {
                source: text!(HistoricalSourceRef, "archive-1"),
                low_weight: RequiredTrue,
                bootstrap_only: RequiredTrue,
            },
        ),
        EventPayload::CognitiveMapUpdateProposed(CognitiveMapUpdateProposedPayload {
            candidate_id: text!(CandidateId, "candidate-7"),
            kind: text!(MapUpdateKind, "judgment-frame"),
            evidence: vec![text!(EvidenceRef, "evidence-1")],
            frame: Some(text!(JudgmentFrameRef, "judgment-frame-1")),
            quality: None,
            blindspot: None,
            resource: None,
            confidence: Confidence(0.75),
        }),
        EventPayload::ConfigDoctorReport(ConfigDoctorReportPayload {
            checks: vec![ConfigCheck::Provider],
            findings: vec![text!(ConfigFindingRef, "config-finding-1")],
        }),
        EventPayload::ComplianceCheckResult(ComplianceCheckResultPayload {
            scope: ComplianceScope::Copy,
            outcome: ComplianceOutcome::Pass,
            blocking: false,
            findings: vec![text!(ComplianceFindingRef, "compliance-finding-1")],
        }),
        EventPayload::EvolutionEvaluationRecorded(EvolutionEvaluationRecordedPayload {
            evaluation: text!(EvolutionEvaluationRef, "evaluation:loop:v2"),
            baseline: text!(StrategyVersionRef, "loop:v1"),
            candidate: text!(StrategyVersionRef, "loop:v2"),
            verdict: EvaluationVerdict::Pass,
            hard_invariants: vec![text!(InvariantResultRef, "invariant:harness-first")],
            ground_truth: vec![text!(EvidenceRef, "evidence:verification:1")],
        }),
        EventPayload::StrategyActivated(StrategyActivatedPayload {
            activation: strategy_activation(),
            active_snapshot: text!(EvolutionSnapshotRef, "snapshot:evolution:1"),
        }),
        EventPayload::StrategyRolledBack(StrategyRolledBackPayload {
            rollback: strategy_rollback(),
            active_snapshot: text!(EvolutionSnapshotRef, "snapshot:evolution:2"),
        }),
        EventPayload::FederatedPeerRegistered(FederatedPeerRegisteredPayload {
            grant: FederatedPeerGrant {
                schema_version: SchemaVersion(1),
                peer: text!(FederatedPeerRef, "peer:executor:1"),
                owner: text!(VerifiedPrincipal, "owner:1"),
                roles: vec![FederatedPeerRole::Executor],
                scopes: vec![text!(Scope, "workspace")],
                capabilities: vec![text!(CapabilityRef, "file.write")],
                transport_identity: text!(TransportIdentityDigest, "sha256:identity"),
                authority_epoch: AuthorityEpoch(1),
                grant_version: PeerGrantVersion(1),
                expires_at: 1_800_000_000_000,
                created_by: text!(OwnerControlRef, "owner-control:1"),
            },
            previous: None,
            committed_version: FederationAggregateVersion {
                schema_version: SchemaVersion(1),
                aggregate: text!(FederationAggregateRef, "federation"),
                version: 1,
            },
        }),
        EventPayload::FederatedPeerRevoked(FederatedPeerRevokedPayload {
            peer: text!(FederatedPeerRef, "peer:executor:1"),
            revoked_grant: text!(FederatedPeerGrantRef, "grant:executor:1"),
            new_authority_epoch: AuthorityEpoch(2),
            in_flight: InFlightDisposition::WaitForOwner,
            committed_version: FederationAggregateVersion {
                schema_version: SchemaVersion(1),
                aggregate: text!(FederationAggregateRef, "federation"),
                version: 2,
            },
        }),
        EventPayload::RemoteExecutionLeaseChanged(RemoteExecutionLeaseChangedPayload {
            lease: RemoteExecutionLease {
                schema_version: SchemaVersion(1),
                lease: text!(RemoteExecutionLeaseRef, "lease:1"),
                dispatch: text!(RemoteDispatchId, "dispatch:1"),
                intent: text!(ActionId, "action:remote:1"),
                plan_digest: text!(PlanDigest, "sha256:plan"),
                placement: text!(RemotePlacementPlanRef, "sha256:placement"),
                executor: text!(FederatedPeerRef, "peer:executor:1"),
                peer_grant: text!(FederatedPeerGrantRef, "grant:executor:1"),
                grant_version: PeerGrantVersion(1),
                authority_epoch: AuthorityEpoch(1),
                fence: FenceToken(1),
                expires_at: 1_800_000_000_000,
                state: RemoteLeaseState::Acquired,
            },
            reason: text!(ReasonRef, "approved"),
            committed_version: FederationAggregateVersion {
                schema_version: SchemaVersion(1),
                aggregate: text!(FederationAggregateRef, "federation"),
                version: 3,
            },
        }),
        EventPayload::ReplicationCheckpointAdvanced(ReplicationCheckpointAdvancedPayload {
            peer: text!(FederatedPeerRef, "peer:replica:1"),
            aggregate: text!(RunId, "run-1"),
            from_stream_seq: 0,
            to_stream_seq: 1,
            batch_digest: text!(SchemaDigest, "sha256:batch"),
            redaction: text!(RedactionPolicyRef, "replica-safe"),
            authority_epoch: AuthorityEpoch(1),
            committed_version: FederationAggregateVersion {
                schema_version: SchemaVersion(1),
                aggregate: text!(FederationAggregateRef, "federation"),
                version: 4,
            },
        }),
        EventPayload::CapabilityPublisherChanged(CapabilityPublisherChangedPayload {
            grant: CapabilityPublisherGrant {
                schema_version: SchemaVersion(1),
                reference: text!(CapabilityPublisherGrantRef, "publisher-grant:1"),
                publisher: text!(CapabilityPublisherRef, "publisher:1"),
                public_key_digest: text!(SchemaDigest, "sha256:key"),
                allowed_kinds: vec![CapabilityPackageKind::Skill],
                scope: text!(Scope, "workspace"),
                expires_at: 1_800_000_000_000,
                version: Version(1),
                status: CapabilityPublisherStatus::Active,
            },
            previous: None,
            committed_version: EcosystemAggregateVersion {
                schema_version: SchemaVersion(1),
                value: 1,
            },
        }),
        EventPayload::CapabilityPackageAdmitted(CapabilityPackageAdmittedPayload {
            admission: CapabilityPackageAdmission {
                schema_version: SchemaVersion(1),
                reference: text!(CapabilityAdmissionRef, "admission:1"),
                package: text!(CapabilityPackageRef, "package:1"),
                release: text!(CapabilityReleaseRef, "release:1"),
                package_digest: text!(SchemaDigest, "sha256:package"),
                publisher_grant: text!(CapabilityPublisherGrantRef, "publisher-grant:1"),
                publisher_version: Version(1),
                policy: text!(CapabilityPolicyRef, "policy:ecosystem"),
                policy_version: Version(1),
                checks: Vec::new(),
                dependencies: Vec::new(),
                admitted_at: 1_700_000_000_000,
            },
            committed_version: EcosystemAggregateVersion {
                schema_version: SchemaVersion(1),
                value: 2,
            },
        }),
        EventPayload::CapabilityPackageStateChanged(CapabilityPackageStateChangedPayload {
            change: CapabilityPackageStateChange {
                schema_version: SchemaVersion(1),
                plan: text!(CapabilityInstallPlanRef, "plan:1"),
                approval: text!(ApprovalId, "approval:1"),
                package: text!(CapabilityPackageRef, "package:1"),
                release: text!(CapabilityReleaseRef, "release:1"),
                from: CapabilityLifecycleState::Admitted,
                to: CapabilityLifecycleState::Installed,
                active_generation: 1,
                reason: text!(ReasonRef, "owner-approved"),
                external_effects_reverted: false,
            },
            committed_version: EcosystemAggregateVersion {
                schema_version: SchemaVersion(1),
                value: 3,
            },
        }),
        EventPayload::CapabilityPackageDistributionRecorded(
            CapabilityPackageDistributionRecordedPayload {
                receipt: CapabilityPackageDistributionReceipt {
                    schema_version: SchemaVersion(1),
                    reference: text!(CapabilityDistributionReceiptRef, "distribution:1"),
                    package: text!(CapabilityPackageRef, "package:1"),
                    release: text!(CapabilityReleaseRef, "release:1"),
                    package_digest: text!(SchemaDigest, "sha256:package"),
                    peer: text!(FederatedPeerRef, "peer:executor:1"),
                    peer_grant: text!(FederatedPeerGrantRef, "grant:executor:1"),
                    authority_epoch: AuthorityEpoch(1),
                    plan_digest: text!(PlanDigest, "sha256:plan"),
                    lease: text!(RemoteExecutionLeaseRef, "lease:1"),
                    fence_token: 1,
                    installed_generation: 1,
                    ground_truth: text!(EvidenceRef, "ground-truth:1"),
                    verified: RequiredTrue,
                },
                committed_version: EcosystemAggregateVersion {
                    schema_version: SchemaVersion(1),
                    value: 4,
                },
            },
        ),
        EventPayload::WorkspaceCharterChanged(WorkspaceCharterChangedPayload {
            charter: WorkspaceCharterRecord {
                schema_version: SchemaVersion(1),
                workspace: text!(WorkspaceRef, "workspace:1"),
                version: 1,
                goals: vec![text!(GoalRef, "goal:1")],
                constraints: vec![text!(Constraint, "constraint:1")],
                prohibitions: vec![text!(Constraint, "prohibition:1")],
                done_contract: Some(text!(DoneContractRef, "done:1")),
                review_cadence: Some(DurationMs(1_000)),
                actor: Actor::Owner,
                digest: text!(SchemaDigest, "sha256:charter"),
            },
            expected_version: 0,
            committed_version: 1,
        }),
        EventPayload::DataLifecycleApplied(DataLifecycleAppliedPayload {
            receipt: DataLifecycleReceipt {
                schema_version: SchemaVersion(1),
                aggregate: text!(RunId, "run:lifecycle"),
                operation: DataLifecycleOperation::Delete,
                scope: text!(Scope, "workspace:1"),
                subject_digest: text!(SchemaDigest, "sha256:subject"),
                cleaned_projections: vec![text!(ProjectionRef, "projection:1")],
                destroyed_key_digests: vec![text!(SchemaDigest, "sha256:key")],
                remote_disposition: RemoteDeletionDisposition::NotApplicable,
                evidence: vec![text!(EvidenceRef, "evidence:1")],
            },
            expected_version: 0,
            committed_version: 1,
        }),
    ]
}

fn fixture_event(payload: EventPayload) -> Event {
    Event::new(
        text!(EventId, "event-1"),
        text!(RunId, "run-1"),
        Some(text!(TurnId, "turn-1")),
        payload,
        SchemaVersion(1),
        1_700_000_000_000,
        provenance(),
    )
}

fn assert_serde_round_trip<T>(value: T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let bytes = serde_json::to_vec(&value).unwrap();
    assert_eq!(serde_json::from_slice::<T>(&bytes).unwrap(), value);
}

#[test]
fn taxonomy_matches_the_fixed_contract_snapshot() {
    assert_eq!(&EventKind::ALL[..86], M2_EVENT_KIND_PREFIX.as_slice());
    assert_eq!(
        &EventKind::ALL[86..89],
        &[
            EventKind::EvolutionEvaluationRecorded,
            EventKind::StrategyActivated,
            EventKind::StrategyRolledBack,
        ]
    );
    assert_eq!(
        &EventKind::ALL[89..93],
        &[
            EventKind::FederatedPeerRegistered,
            EventKind::FederatedPeerRevoked,
            EventKind::RemoteExecutionLeaseChanged,
            EventKind::ReplicationCheckpointAdvanced,
        ]
    );
    assert_eq!(
        &EventKind::ALL[93..97],
        &[
            EventKind::CapabilityPublisherChanged,
            EventKind::CapabilityPackageAdmitted,
            EventKind::CapabilityPackageStateChanged,
            EventKind::CapabilityPackageDistributionRecorded,
        ]
    );
    assert_eq!(
        &EventKind::ALL[97..],
        &[
            EventKind::WorkspaceCharterChanged,
            EventKind::DataLifecycleApplied,
        ]
    );
}

#[test]
fn event_kind_strings_round_trip_for_the_complete_taxonomy() {
    for kind in EventKind::ALL {
        assert_eq!(EventKind::from_str(kind.as_str()).unwrap(), kind);
    }
    assert!(EventKind::from_str("NotAnEvent").is_err());
}

#[test]
fn all_typed_payloads_match_the_taxonomy_and_round_trip() {
    let payloads = all_payload_fixtures();
    let kinds: Vec<_> = payloads.iter().map(EventPayload::kind).collect();

    assert_eq!(payloads.len(), 99);
    assert_eq!(kinds.as_slice(), EventKind::ALL.as_slice());

    for payload in payloads {
        assert_serde_round_trip(payload);
    }
}

#[test]
fn event_constructor_derives_kind_and_leaves_sequence_unassigned() {
    let event = fixture_event(EventPayload::TurnStarted(TurnStartedPayload {
        turn_index: 0,
    }));
    assert_eq!(event.kind, EventKind::TurnStarted);
    assert_eq!(event.stream_seq, 0);
    assert!(event.validate_payload_kind().is_ok());
}

#[test]
fn deserialized_envelope_rejects_a_kind_payload_mismatch() {
    let event = fixture_event(EventPayload::TurnStarted(TurnStartedPayload {
        turn_index: 0,
    }));
    let mut encoded = serde_json::to_value(event).unwrap();
    encoded["kind"] = serde_json::json!("RunAccepted");

    let decoded: Event = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.kind, EventKind::RunAccepted);
    assert_eq!(decoded.payload.kind(), EventKind::TurnStarted);
    assert!(decoded.validate_payload_kind().is_err());
}

#[test]
fn lifecycle_objects_round_trip_without_losing_schema_versions() {
    let version = SchemaVersion(1);
    let charter = WorkspaceCharter {
        schema_version: version,
        constraints: vec![text!(Constraint, "stay-inside-workspace")],
    };
    let workspace = Workspace {
        schema_version: version,
        workspace_id: text!(WorkspaceId, "workspace-1"),
        charter: Some(charter.clone()),
    };
    let run = Run {
        schema_version: version,
        run_id: text!(RunId, "run-1"),
        source: Source::UserTurn,
        status: RunStatus::Accepted,
        budget: text!(Budget, "m0-default"),
        stop_reason: None,
        result_ref: None,
    };
    let session = Session {
        schema_version: version,
        session_id: text!(SessionId, "session-1"),
        workspace: text!(WorkspaceRef, "workspace-1"),
        agent_profile: text!(AgentProfileRef, "agent-1"),
        policy_profile: text!(PolicyProfileRef, "policy-1"),
        model_profile: text!(ModelProfileRef, "model-1"),
        memory_scope: text!(MemoryScope, "owner"),
    };
    let turn = Turn {
        schema_version: version,
        turn_id: text!(TurnId, "turn-1"),
        run_id: run.run_id.clone(),
        index: 0,
    };
    let request = RunRequest {
        schema_version: version,
        source: Source::UserTurn,
        session: text!(SessionRef, "session-1"),
        agent_profile: text!(AgentProfileRef, "agent-1"),
        input: text!(RunInput, "input-1"),
        budget: Some(text!(Budget, "m0-default")),
        idempotency_key: Some(text!(IdempotencyKey, "request-1")),
    };
    let result = RunResult {
        schema_version: version,
        status: RunStatus::Complete,
        stop_reason: text!(StopReason, "completed"),
        outputs: vec![text!(OutputRef, "output-1")],
        evidence_refs: vec![text!(EventId, "event-1")],
    };

    assert_serde_round_trip(charter);
    assert_serde_round_trip(workspace);
    assert_serde_round_trip(run);
    assert_serde_round_trip(session);
    assert_serde_round_trip(turn);
    assert_serde_round_trip(request);
    assert_serde_round_trip(result);
}

#[test]
fn historical_evidence_flags_serialize_true_and_reject_false() {
    let payload = EventPayload::ImportedHistoricalEvidenceRecorded(
        ImportedHistoricalEvidenceRecordedPayload {
            source: text!(HistoricalSourceRef, "archive-1"),
            low_weight: RequiredTrue,
            bootstrap_only: RequiredTrue,
        },
    );
    let encoded = serde_json::to_value(payload).unwrap();
    assert_eq!(
        encoded["ImportedHistoricalEvidenceRecorded"]["low_weight"],
        true
    );
    assert_eq!(
        encoded["ImportedHistoricalEvidenceRecorded"]["bootstrap_only"],
        true
    );

    let low_weight_false = serde_json::json!({
        "ImportedHistoricalEvidenceRecorded": {
            "source": "archive-1",
            "low_weight": false,
            "bootstrap_only": true
        }
    });
    let bootstrap_only_false = serde_json::json!({
        "ImportedHistoricalEvidenceRecorded": {
            "source": "archive-1",
            "low_weight": true,
            "bootstrap_only": false
        }
    });

    assert!(serde_json::from_value::<EventPayload>(low_weight_false).is_err());
    assert!(serde_json::from_value::<EventPayload>(bootstrap_only_false).is_err());
}

#[test]
fn competence_inputs_keep_capability_and_failure_evidence_distinct() {
    let inputs = CompetenceInputs {
        map_confidence: None,
        agent_self_model: None,
        capability_evidence: vec![text!(CapabilityEvidenceRef, "capability-evidence-1")],
        trust_profile: None,
        failure_evidence: vec![text!(FailureEvidenceRef, "failure-evidence-1")],
        verification_evidence: vec![text!(EvidenceRef, "verification-evidence-1")],
    };

    let encoded = serde_json::to_vec(&inputs).unwrap();
    assert_eq!(
        serde_json::from_slice::<CompetenceInputs>(&encoded).unwrap(),
        inputs
    );

    let legacy = serde_json::json!({
        "map_confidence": null,
        "agent_self_model": null,
        "capability_evidence": [],
        "trust_profile": null,
        "failure_evidence": []
    });
    assert!(serde_json::from_value::<CompetenceInputs>(legacy)
        .unwrap()
        .verification_evidence
        .is_empty());
}

#[test]
fn m1_b_additive_payload_fields_keep_m0_events_readable() {
    let legacy = serde_json::json!({
        "ProactiveProposalEmitted": {
            "proposal_ref": "proposal:legacy",
            "proposal_kind": "communication",
            "level": "L1Suggest",
            "guard": {
                "value_gate_passed": true,
                "competence_gate_passed": true,
                "policy_and_envelope_passed": true
            }
        }
    });
    let decoded = serde_json::from_value::<EventPayload>(legacy).unwrap();
    let EventPayload::ProactiveProposalEmitted(payload) = decoded else {
        unreachable!()
    };
    assert_eq!(payload.delivery, DeliveryMode::Hitchhike);
    assert_eq!(payload.attention_cost, 0);

    let legacy = serde_json::json!({
        "ProspectiveIntentionCreated": {
            "intention_id": "intention:legacy",
            "source": "Commitment",
            "trigger": "at:100"
        }
    });
    let decoded = serde_json::from_value::<EventPayload>(legacy).unwrap();
    let EventPayload::ProspectiveIntentionCreated(payload) = decoded else {
        unreachable!()
    };
    assert_eq!(payload.schedule, None);
}

#[test]
fn floating_protocol_primitives_reject_non_finite_values() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(Confidence::new(value).is_err());
        assert!(Weight::new(value).is_err());
        assert!(RestingActivation::new(value).is_err());

        assert!(serde_json::to_vec(&Confidence(value)).is_err());
        assert!(serde_json::to_vec(&Weight(value)).is_err());
        assert!(serde_json::to_vec(&RestingActivation(value)).is_err());
    }

    assert_eq!(Confidence::new(0.5).unwrap(), Confidence(0.5));
    assert_eq!(Weight::new(-0.25).unwrap(), Weight(-0.25));
    assert_eq!(RestingActivation::new(1.0).unwrap(), RestingActivation(1.0));
}
