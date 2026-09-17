use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ResourceKind {
    Tool,
    Skill,
    Model,
    Memory,
    Source,
    Backend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceEvidenceOutcome {
    Pass,
    Fail,
    Unverifiable,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceScore {
    pub schema_version: SchemaVersion,
    pub passed: u32,
    pub failed: u32,
    pub unverifiable: u32,
    pub latest_at: Timestamp,
}

impl ResourceScore {
    pub fn rank(&self) -> i64 {
        i64::from(self.passed) * 4 - i64::from(self.failed) * 6 - i64::from(self.unverifiable) * 2
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceNode {
    pub schema_version: SchemaVersion,
    pub resource: ResourceRef,
    pub kind: ResourceKind,
    pub scope: Scope,
    pub available: bool,
    pub score: ResourceScore,
    pub evidence_refs: Vec<EventId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ResourceRelation {
    Provides,
    UsedBy,
    Supports,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEdge {
    pub schema_version: SchemaVersion,
    pub from: ResourceRef,
    pub to: ResourceRef,
    pub relation: ResourceRelation,
    pub scope: Scope,
    pub fresh_at: Timestamp,
    pub evidence_refs: Vec<EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregateVersion {
    pub schema_version: SchemaVersion,
    pub aggregate: RunId,
    pub value: u64,
}

impl AggregateVersion {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.aggregate.0.trim().is_empty() {
            return Err(Error("aggregate version is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceGraphSnapshot {
    pub schema_version: SchemaVersion,
    pub reference: ResourceGraphSnapshotRef,
    pub aggregate_versions: Vec<AggregateVersion>,
    pub nodes: Vec<ResourceNode>,
    pub edges: Vec<ResourceEdge>,
    pub evidence_refs: Vec<EventId>,
    pub built_at: Timestamp,
}

impl ResourceGraphSnapshot {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.reference.0.trim().is_empty()
            || self.aggregate_versions.is_empty()
            || self
                .aggregate_versions
                .iter()
                .any(|version| version.validate().is_err())
        {
            return Err(Error("resource graph snapshot is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongTermGoal {
    pub schema_version: SchemaVersion,
    pub goal_frame: GoalFrameRef,
    pub scope: Scope,
    pub situation_digest: SchemaDigest,
    pub budget: Budget,
    pub expires_at: Timestamp,
}

impl LongTermGoal {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.goal_frame.0.trim().is_empty()
            || self.scope.0.trim().is_empty()
            || self.situation_digest.0.trim().is_empty()
            || self.budget.0.trim().is_empty()
            || self.expires_at <= 0
        {
            return Err(Error("long-term goal is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalCheckpoint {
    pub schema_version: SchemaVersion,
    pub reference: GoalCheckpointRef,
    pub goal_frame: GoalFrameRef,
    pub intention: IntentionId,
    pub route: ExecutionRouteRef,
    pub artifact: ContentRef,
    pub situation_digest: SchemaDigest,
    pub evidence_refs: Vec<EventId>,
    pub created_at: Timestamp,
}

impl GoalCheckpoint {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.reference.0.trim().is_empty()
            || self.goal_frame.0.trim().is_empty()
            || self.intention.0.trim().is_empty()
            || self.route.0.trim().is_empty()
            || self.artifact.0.trim().is_empty()
            || self.situation_digest.0.trim().is_empty()
            || self.evidence_refs.is_empty()
            || self.created_at <= 0
        {
            return Err(Error("goal checkpoint is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalLineageSnapshot {
    pub schema_version: SchemaVersion,
    pub goal: LongTermGoal,
    pub intentions: Vec<IntentionId>,
    pub resolutions: Vec<GoalIntentionResolution>,
    pub routes: Vec<ExecutionRouteRef>,
    pub checkpoints: Vec<GoalCheckpoint>,
    pub aggregate_versions: Vec<AggregateVersion>,
    pub cancelled: bool,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalIntentionResolution {
    pub schema_version: SchemaVersion,
    pub intention: IntentionId,
    pub outcome: IntentionOutcome,
    pub event_ref: EventId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityResultEvidence {
    pub schema_version: SchemaVersion,
    pub evidence_ref: EvidenceRef,
    pub outcome: ResourceEvidenceOutcome,
    pub observed_at: Timestamp,
    pub owner_feedback: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityGap {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityGapRef,
    pub capability: CapabilityRef,
    pub scope: Scope,
    pub result_evidence: Vec<CapabilityResultEvidence>,
    pub self_confidence: Option<Confidence>,
    pub ceiling: InterventionLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityUpdateProposal {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityUpdateProposalRef,
    pub candidate_id: CandidateId,
    pub gap: CapabilityGap,
    pub requested_envelope: AutonomyEnvelope,
    pub evidence_refs: Vec<EvidenceRef>,
}

impl CapabilityUpdateProposal {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.reference.0.trim().is_empty()
            || self.candidate_id.0.trim().is_empty()
            || self.gap.schema_version.0 == 0
            || self.gap.reference.0.trim().is_empty()
            || self.gap.capability.0.trim().is_empty()
            || self.gap.scope.0.trim().is_empty()
            || self.gap.result_evidence.is_empty()
            || self.requested_envelope.schema_version.0 == 0
            || self.evidence_refs.is_empty()
        {
            return Err(Error("capability update proposal is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NarrowCapabilityGrant {
    pub schema_version: SchemaVersion,
    pub proposal: CapabilityUpdateProposalRef,
    pub envelope: AutonomyEnvelope,
    pub approved_by: VerifiedPrincipal,
    pub approved_at: Timestamp,
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedPluginBinding {
    pub schema_version: SchemaVersion,
    pub source: ManagedPluginSourceRef,
    pub source_digest: SchemaDigest,
    pub manifest_digest: SchemaDigest,
    pub signature: PluginSignatureRef,
}

impl ManagedPluginBinding {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.source.0.trim().is_empty()
            || self.source_digest.0.trim().is_empty()
            || self.manifest_digest.0.trim().is_empty()
            || self.signature.0.trim().is_empty()
        {
            return Err(Error("managed plugin binding is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedPluginPolicy {
    pub schema_version: SchemaVersion,
    pub reference: ManagedPluginPolicyRef,
    pub version: Version,
    pub allowed_plugins: Vec<PluginRef>,
    pub denied_plugins: Vec<PluginRef>,
    pub allowed_sources: Vec<ManagedPluginSourceRef>,
    pub revoked_manifests: Vec<SchemaDigest>,
    pub require_signature: bool,
}

impl ManagedPluginPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.reference.0.trim().is_empty()
            || self.version.0 == 0
            || self.allowed_sources.is_empty()
            || !self.require_signature
        {
            return Err(Error("managed plugin policy is incomplete".into()));
        }
        Ok(())
    }

    pub fn allows(&self, plugin: &PluginRef, binding: &ManagedPluginBinding) -> bool {
        !self.denied_plugins.contains(plugin)
            && self.allowed_plugins.contains(plugin)
            && self.allowed_sources.contains(&binding.source)
            && !self.revoked_manifests.contains(&binding.manifest_digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedPluginSnapshot {
    pub schema_version: SchemaVersion,
    pub reference: ManagedPluginSnapshotRef,
    pub policy: ManagedPluginPolicyRef,
    pub policy_version: Version,
    pub plugin: PluginRef,
    pub source: ManagedPluginSourceRef,
    pub manifest: PluginManifestRef,
    pub manifest_digest: SchemaDigest,
    pub generation: u64,
}

impl ManagedPluginSnapshot {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.reference.0.trim().is_empty()
            || self.policy.0.trim().is_empty()
            || self.policy_version.0 == 0
            || self.plugin.0.trim().is_empty()
            || self.source.0.trim().is_empty()
            || self.manifest.0.trim().is_empty()
            || self.manifest_digest.0.trim().is_empty()
            || self.generation == 0
        {
            return Err(Error("managed plugin snapshot is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryTemperature {
    Hot,
    Cold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryRetentionState {
    Active,
    Redacted,
    Expired,
    Tombstoned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryTierEntry {
    pub schema_version: SchemaVersion,
    pub event_id: EventId,
    pub aggregate: RunId,
    pub stream_seq: u64,
    pub scope: Option<Scope>,
    pub temperature: MemoryTemperature,
    pub retention: MemoryRetentionState,
    pub content_ref: Option<ContentRef>,
    pub occurred_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotColdMemorySnapshot {
    pub schema_version: SchemaVersion,
    pub aggregate_versions: Vec<AggregateVersion>,
    pub entries: Vec<MemoryTierEntry>,
    pub built_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncPeer {
    pub schema_version: SchemaVersion,
    pub reference: SyncPeerRef,
    pub owner: VerifiedPrincipal,
    pub allowed_scopes: Vec<Scope>,
}

impl SyncPeer {
    pub fn validate(&self) -> Result<()> {
        let unique_scopes = self.allowed_scopes.iter().collect::<BTreeSet<_>>();
        if self.schema_version.0 == 0
            || self.reference.0.trim().is_empty()
            || self.owner.0.trim().is_empty()
            || self.allowed_scopes.is_empty()
            || self
                .allowed_scopes
                .iter()
                .any(|scope| scope.0.trim().is_empty())
            || unique_scopes.len() != self.allowed_scopes.len()
        {
            return Err(Error("sync peer is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRedactionPolicy {
    pub schema_version: SchemaVersion,
    pub reference: RedactionPolicyRef,
    pub redact_raw_content: bool,
    pub forbid_secret_refs: bool,
    pub forbidden_keys: Vec<String>,
    pub forbidden_value_markers: Vec<String>,
}

impl SyncRedactionPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.reference.0.trim().is_empty()
            || !self.redact_raw_content
            || !self.forbid_secret_refs
        {
            return Err(Error("sync redaction policy is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncWriteBatch {
    pub schema_version: SchemaVersion,
    pub batch_id: SyncBatchId,
    pub peer: SyncPeer,
    pub aggregate: RunId,
    pub expected_version: AggregateVersion,
    pub events: Vec<Event>,
}

impl SyncWriteBatch {
    pub fn validate(&self) -> Result<()> {
        self.peer.validate()?;
        self.expected_version.validate()?;
        let unique_events = self
            .events
            .iter()
            .map(|event| &event.event_id)
            .collect::<BTreeSet<_>>();
        let expected_first = self.expected_version.value.checked_add(1);
        if self.schema_version.0 == 0
            || self.batch_id.0.trim().is_empty()
            || self.aggregate.0.trim().is_empty()
            || self.events.is_empty()
            || self.expected_version.aggregate != self.aggregate
            || self
                .events
                .iter()
                .any(|event| event.run_id != self.aggregate)
            || unique_events.len() != self.events.len()
            || expected_first.is_none()
            || self.events.iter().enumerate().any(|(index, event)| {
                let expected = expected_first
                    .and_then(|first| first.checked_add(index as u64))
                    .unwrap_or(0);
                event.stream_seq != expected
                    || event.schema_version.0 == 0
                    || event.validate_payload_kind().is_err()
            })
        {
            return Err(Error("sync write batch is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncExportRequest {
    pub schema_version: SchemaVersion,
    pub peer: SyncPeer,
    pub aggregate: RunId,
    pub after_version: AggregateVersion,
    pub limit: u32,
    pub redaction: SyncRedactionPolicy,
}

impl SyncExportRequest {
    pub fn validate(&self) -> Result<()> {
        self.peer.validate()?;
        self.after_version.validate()?;
        self.redaction.validate()?;
        if self.schema_version.0 == 0
            || self.aggregate.0.trim().is_empty()
            || self.after_version.aggregate != self.aggregate
            || self.limit == 0
        {
            return Err(Error("sync export request is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SyncTransferPayload {
    Full(Box<EventPayload>),
    Redacted {
        kind: EventKind,
        reason: ReasonRef,
        digest: SchemaDigest,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncTransferEvent {
    pub schema_version: SchemaVersion,
    pub event_id: EventId,
    pub aggregate: RunId,
    pub source_stream_seq: u64,
    pub turn_id: Option<TurnId>,
    pub kind: EventKind,
    pub payload: SyncTransferPayload,
    pub event_schema_version: SchemaVersion,
    pub ts_unix_ms: Timestamp,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncTransferBatch {
    pub schema_version: SchemaVersion,
    pub batch_id: SyncBatchId,
    pub peer: SyncPeerRef,
    pub aggregate: RunId,
    pub from_version: u64,
    pub to_version: u64,
    pub events: Vec<SyncTransferEvent>,
}

impl SyncTransferBatch {
    pub fn into_authoritative_write(self, peer: SyncPeer) -> Result<SyncWriteBatch> {
        peer.validate()?;
        if self.schema_version.0 == 0
            || self.batch_id.0.trim().is_empty()
            || self.peer != peer.reference
            || self.aggregate.0.trim().is_empty()
            || self.events.is_empty()
            || self.from_version >= self.to_version
            || self.events.last().map(|event| event.source_stream_seq) != Some(self.to_version)
        {
            return Err(Error("sync transfer batch is incomplete".into()));
        }

        let mut events = Vec::with_capacity(self.events.len());
        for (index, transfer) in self.events.into_iter().enumerate() {
            let expected_sequence = self
                .from_version
                .checked_add(index as u64 + 1)
                .ok_or_else(|| Error("sync transfer sequence is exhausted".into()))?;
            if transfer.schema_version.0 == 0
                || transfer.aggregate != self.aggregate
                || transfer.source_stream_seq != expected_sequence
                || transfer.event_schema_version.0 == 0
            {
                return Err(Error("sync transfer event is not contiguous".into()));
            }
            let payload = match transfer.payload {
                SyncTransferPayload::Full(payload) if payload.kind() == transfer.kind => *payload,
                SyncTransferPayload::Full(_) => {
                    return Err(Error("sync transfer payload kind does not match".into()))
                }
                SyncTransferPayload::Redacted { .. } => {
                    return Err(Error(
                        "redacted sync transfer cannot become an authoritative event".into(),
                    ))
                }
            };
            let mut event = Event::new(
                transfer.event_id,
                transfer.aggregate,
                transfer.turn_id,
                payload,
                transfer.event_schema_version,
                transfer.ts_unix_ms,
                transfer.provenance,
            );
            event.stream_seq = transfer.source_stream_seq;
            events.push(event);
        }

        let batch = SyncWriteBatch {
            schema_version: self.schema_version,
            batch_id: self.batch_id,
            peer,
            aggregate: self.aggregate.clone(),
            expected_version: AggregateVersion {
                schema_version: SchemaVersion(1),
                aggregate: self.aggregate,
                value: self.from_version,
            },
            events,
        };
        batch.validate()?;
        Ok(batch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SyncApplyStatus {
    Applied,
    Duplicate,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncApplyReport {
    pub schema_version: SchemaVersion,
    pub batch_id: SyncBatchId,
    pub status: SyncApplyStatus,
    pub expected_version: u64,
    pub actual_version: u64,
    pub resulting_version: u64,
    pub applied_events: Vec<EventId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExpectedAppendStatus {
    Applied,
    Duplicate,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedAppend {
    pub schema_version: SchemaVersion,
    pub status: ExpectedAppendStatus,
    pub expected_version: u64,
    pub actual_version: u64,
    pub resulting_version: u64,
    pub event_id: Option<EventId>,
}
