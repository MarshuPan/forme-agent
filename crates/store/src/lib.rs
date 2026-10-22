//! forme-store - append-only event log, projections, and replay contracts.
#![forbid(unsafe_code)]

mod at_rest;
mod backup;
mod brain;
mod charter;
mod checksum;
mod content;
mod cursor;
mod data_catalog;
mod file_keys;
mod offline_access;
mod offline_backup;
mod offline_validation;
mod orchestration;
mod projection;
mod replay;
mod replica;
mod restoration_fence;
mod schema;
mod sqlite;
mod store_access;

use forme_protocol as p;

pub use at_rest::{FileContentKeyResolver, StoreContentProtection};
pub use backup::{BackupManifest, BackupService, BackupVerification, RestoreReport};
pub use brain::{
    AuthorityLeaseGuard, AuthorityWriterLease, BrainAggregateKind, BrainAuthorityBinding,
    BrainAuthorityClaim, BrainAuthorityMetadata, BrainAuthorityStore, CognitiveTickClaim,
    CognitiveTickClaimRequest, CognitiveTickLedger, CognitiveTickWindowState, TickEventCursor,
    TickEventRange, BRAIN_AUTHORITY_SCHEMA_VERSION, COGNITIVE_TICK_SCHEMA_VERSION,
};
pub use content::{ContentKeyResolver, ProtectedContentService};
pub use cursor::EventCursor;
pub use offline_backup::{
    OfflineBrainIdentity, OfflineBrainSource, OfflineRestorePlan, OfflineRestoreReceipt,
};
pub use projection::{
    SearchHit, SessionState, SessionStateProjection, Transcript, TranscriptEntry,
    TranscriptProjection,
};
pub use replay::{
    ProjectionDiff, ProjectionName, ProjectionPath, ProjectionValue, ReplayReport,
    SessionStatePath, TranscriptPath,
};
pub use replica::ReplicaSqliteStore;
pub use restoration_fence::RestoredExecutionFence;
pub use schema::{PayloadType, SchemaSnapshot, Upcaster};
pub use sqlite::lifecycle::{
    DataLifecyclePlan, DataLifecycleRequest, DataLifecycleService, LifecycleObject,
    ProtectedContentRecord, DATA_LIFECYCLE_STORE_SCHEMA_VERSION, PROTECTED_CONTENT_PROJECTIONS,
};
pub use sqlite::migration::{
    LegacyMigrationPlan, LegacyMigrationReceipt, LegacyPlaintextMigration, SourceBackupDisposition,
    LEGACY_MIGRATION_SCHEMA_VERSION,
};
pub use sqlite::{DomainClaimProbe, SqliteEventStore, StoreOptions};
pub use store_access::StoreAccessGuard;

pub const APPROVAL_CONSUMPTION_SCHEMA_VERSION: p::SchemaVersion = p::SchemaVersion(1);

pub trait Projection {
    type State;

    fn empty() -> Self::State;
    fn apply(state: &mut Self::State, event: &p::Event);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionScope {
    Run(p::RunId),
    All,
}

impl ProjectionScope {
    pub fn run(run_id: p::RunId) -> Self {
        Self::Run(run_id)
    }
}

pub trait EventStore {
    /// Sole write path. Single writer assigns per-run monotonic stream_seq (ordering authority).
    /// Idempotent: event_id dedup + command/action idempotency keys (prd/02 section 4/5).
    fn append(&self, event: p::Event) -> p::Result<p::EventId>;
    fn read_run(&self, run: p::RunId) -> EventCursor;
    fn project<P: Projection>(&self, scope: ProjectionScope) -> p::Result<P::State>;
    fn replay(&self, run: p::RunId, at: SchemaSnapshot) -> p::Result<ReplayReport>;
}

/// Durable one-shot approval consumption fact.  This is authority companion
/// metadata: it prevents a granted nonce from being reused across process
/// restarts without adding another business event kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalConsumptionClaim {
    pub schema_version: p::SchemaVersion,
    pub approval_id: p::ApprovalId,
    pub approver: p::VerifiedPrincipal,
    pub nonce: p::Nonce,
    pub plan_digest: p::PlanDigest,
    pub normalized_action_digest: p::SchemaDigest,
    pub affected_resources: Vec<p::ResourceRef>,
    pub policy_version: p::Version,
    pub tool_schema_version: p::Version,
    pub now: p::Timestamp,
}

impl ApprovalConsumptionClaim {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != APPROVAL_CONSUMPTION_SCHEMA_VERSION {
            return Err(p::Error(format!(
                "approval consumption schema version {} is unsupported",
                self.schema_version.0
            )));
        }
        if self.approval_id.0.trim().is_empty()
            || self.approver.0.trim().is_empty()
            || self.nonce.0.trim().is_empty()
            || self.plan_digest.0.trim().is_empty()
            || self.normalized_action_digest.0.trim().is_empty()
        {
            return Err(p::Error(
                "approval consumption binding is incomplete".into(),
            ));
        }
        if self.nonce.0.len() > 256 {
            return Err(p::Error(
                "approval consumption nonce exceeds its bound".into(),
            ));
        }
        p::sha256_digest_bytes(&p::SchemaDigest(self.plan_digest.0.clone()))
            .map_err(|_| p::Error("approval consumption plan digest is not canonical".into()))?;
        p::sha256_digest_bytes(&self.normalized_action_digest)
            .map_err(|_| p::Error("approval consumption action digest is not canonical".into()))?;
        if self.policy_version.0 == 0 || self.tool_schema_version.0 == 0 {
            return Err(p::Error(
                "approval consumption versions must be non-zero".into(),
            ));
        }
        if self.now < 0 {
            return Err(p::Error(
                "approval consumption claim time cannot be negative".into(),
            ));
        }
        if self
            .affected_resources
            .iter()
            .any(|resource| resource.0.trim().is_empty())
        {
            return Err(p::Error(
                "approval consumption resources cannot be empty".into(),
            ));
        }
        if p::normalize_resource_set(&self.affected_resources) != self.affected_resources {
            return Err(p::Error(
                "approval consumption resources are not canonical".into(),
            ));
        }
        Ok(())
    }
}

/// The durable one-shot gate used immediately before an approved driver call.
pub trait ApprovalConsumptionLedger {
    /// `true` means this nonce was claimed for the first time; `false` means
    /// an exactly equivalent retry observed the already-consumed claim.
    fn claim_approval(&self, claim: &ApprovalConsumptionClaim) -> p::Result<bool>;
}

pub trait VersionedEventStore: EventStore {
    fn aggregate_version(&self, aggregate: p::RunId) -> p::Result<p::AggregateVersion>;
    fn append_expected(
        &self,
        event: p::Event,
        expected: p::AggregateVersion,
    ) -> p::Result<p::ExpectedAppend>;
    fn apply_sync_batch(&self, batch: p::SyncWriteBatch) -> p::Result<p::SyncApplyReport>;
    fn export_sync_batch(&self, request: p::SyncExportRequest) -> p::Result<p::SyncTransferBatch>;
}

pub trait EvolutionProjection {
    fn active(
        &self,
        domain: p::StrategyDomain,
        scope: p::Scope,
    ) -> p::Result<Option<p::ActiveStrategyRef>>;
    fn snapshot(&self, scope: p::Scope) -> p::Result<p::EvolutionSnapshot>;
}

pub trait EvolutionEventStore: EventStore {
    fn evolution_version(
        &self,
        aggregate: &p::EvolutionAggregateRef,
    ) -> p::Result<p::EvolutionAggregateVersion>;
    fn append_evolution_expected(
        &self,
        event: p::Event,
        aggregate: &p::EvolutionAggregateRef,
        expected: p::EvolutionAggregateVersion,
    ) -> p::Result<p::ExpectedAppend>;
}

/// Read-only authority projection used by the Harness when it binds a run or
/// performs the final pre-dispatch recheck.
pub trait FederationProjection {
    fn snapshot(&self, scope: p::Scope) -> p::Result<p::FederationSnapshot>;
    fn peer(&self, peer: &p::FederatedPeerRef) -> p::Result<Option<p::FederatedPeerState>>;
    fn lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteExecutionLease>>;
    fn checkpoint(
        &self,
        peer: &p::FederatedPeerRef,
        aggregate: &p::RunId,
    ) -> p::Result<p::ReplicationCursor>;
}

/// The sole M4 write path.  It is separate from run and evolution CAS so the
/// aggregate domains cannot be mixed by accident.
pub trait FederationEventStore: EventStore {
    fn federation_version(
        &self,
        aggregate: &p::FederationAggregateRef,
    ) -> p::Result<p::FederationAggregateVersion>;
    fn append_federation_expected(
        &self,
        event: p::Event,
        aggregate: &p::FederationAggregateRef,
        expected: p::FederationAggregateVersion,
    ) -> p::Result<p::ExpectedAppend>;
    fn export_replication(
        &self,
        request: p::ReplicationExportRequest,
    ) -> p::Result<p::ReplicationBatch>;
    fn acknowledge_replication(
        &self,
        event: p::Event,
        ack: p::ReplicationAck,
        expected: p::FederationAggregateVersion,
    ) -> p::Result<p::ExpectedAppend>;
}

/// Durable one-shot dispatch gate. The Harness claims an acquired lease here
/// before the first network byte is sent. An already-attempted claim may only
/// be probed or resolved from the original receipt.
pub trait RemoteDispatchLedger {
    fn claim_remote_dispatch(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
        plan: &p::PlanDigest,
        authority_epoch: p::AuthorityEpoch,
    ) -> p::Result<p::RemoteDispatchClaim>;
    fn remote_dispatch_claim(
        &self,
        dispatch: &p::RemoteDispatchId,
    ) -> p::Result<Option<p::RemoteDispatchClaim>>;
}

/// Authority-local, restart-durable federation runtime state. These records
/// bind replay prevention and recovery to existing events, leases, and typed
/// artifacts; they never allocate stream_seq or grant peer authority.
pub trait FederationRuntimeLedger {
    fn claim_control_nonce(
        &self,
        peer: &p::FederatedPeerRef,
        nonce: &p::Nonce,
        digest: &p::SchemaDigest,
    ) -> p::Result<bool>;
    fn claim_device_signal(&self, signal: &p::FederatedDeviceSignal) -> p::Result<bool>;
    fn record_federated_checkpoint(
        &self,
        source: &p::RunId,
        checkpoint: &p::FederatedCheckpointArtifact,
    ) -> p::Result<bool>;
    fn federated_checkpoint_artifact(
        &self,
        checkpoint: &p::FederatedCheckpointArtifactRef,
    ) -> p::Result<Option<(p::RunId, p::FederatedCheckpointArtifact)>>;
    fn record_federated_handoff(&self, handoff: &p::FederatedHandoffPlan) -> p::Result<bool>;
    fn federated_handoff_for_run(
        &self,
        run: &p::RunId,
    ) -> p::Result<Option<p::FederatedHandoffPlan>>;
    fn record_retention_request(&self, request: &p::FederatedRetentionRequest) -> p::Result<bool>;
    fn federated_retention_state(
        &self,
        request: &p::RetentionRequestRef,
    ) -> p::Result<Option<p::FederatedRetentionState>>;
    fn accept_retention_receipt(
        &self,
        receipt: &p::FederatedRetentionReceipt,
    ) -> p::Result<p::FederatedRetentionState>;
    fn save_remote_recovery(&self, recovery: &p::RemoteActionRecoveryRecord) -> p::Result<()>;
    fn remote_recovery(&self, run: &p::RunId) -> p::Result<Option<p::RemoteActionRecoveryRecord>>;
    fn remote_recovery_for_lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteActionRecoveryRecord>>;
}

/// Read-only M5 authority projection. Admission, lifecycle, and distribution
/// remain separate facts and are never inferred from one another.
pub trait EcosystemProjection {
    fn publisher(
        &self,
        publisher: &p::CapabilityPublisherRef,
    ) -> p::Result<Option<p::CapabilityPublisherGrant>>;
    fn admission(
        &self,
        release: &p::CapabilityReleaseRef,
    ) -> p::Result<Option<p::CapabilityPackageAdmission>>;
    fn package_state(
        &self,
        package: &p::CapabilityPackageRef,
    ) -> p::Result<Option<p::CapabilityPackageState>>;
    fn snapshot(&self, scope: p::Scope) -> p::Result<p::CapabilityEcosystemSnapshot>;
}

/// The sole M5 ecosystem write path. It deliberately does not share aggregate
/// versions with run, evolution, or federation state.
pub trait EcosystemEventStore: EventStore {
    fn ecosystem_version(
        &self,
        aggregate: &p::EcosystemAggregateRef,
    ) -> p::Result<p::EcosystemAggregateVersion>;
    fn append_ecosystem_expected(
        &self,
        event: p::Event,
        aggregate: &p::EcosystemAggregateRef,
        expected: p::EcosystemAggregateVersion,
    ) -> p::Result<p::ExpectedAppend>;
}

/// Restart-durable replay and distribution ledger. These records do not append
/// events or grant package authority.
pub trait EcosystemRuntimeLedger {
    fn claim_ecosystem_nonce(&self, nonce: &p::Nonce, plan: &p::PlanDigest) -> p::Result<bool>;
    fn record_distribution_attempt(
        &self,
        receipt: &p::CapabilityPackageDistributionReceipt,
    ) -> p::Result<bool>;
    fn distribution_receipt(
        &self,
        reference: &p::CapabilityDistributionReceiptRef,
    ) -> p::Result<Option<p::CapabilityPackageDistributionReceipt>>;
}

/// Content-addressed authority-local package bodies used to rebuild the
/// declarative registry after restart. Presence in this archive is never an
/// admission, lifecycle, or permission fact.
pub trait EcosystemPackageArchive {
    fn archive_package(&self, package: &p::SignedCapabilityPackage) -> p::Result<bool>;
    fn archived_package(
        &self,
        release: &p::CapabilityReleaseRef,
    ) -> p::Result<Option<p::SignedCapabilityPackage>>;
    fn enabled_package_states(&self) -> p::Result<Vec<p::CapabilityPackageState>>;
}

/// A replica is deliberately not an EventStore.  Its API cannot append an
/// authority event or apply the legacy M2 authoritative sync batch.
pub trait ReplicaProjectionStore {
    fn cursor(
        &self,
        peer: &p::FederatedPeerRef,
        aggregate: &p::RunId,
    ) -> p::Result<p::ReplicationCursor>;
    fn apply(
        &self,
        batch: p::ReplicationBatch,
        expected: p::ReplicationCursor,
    ) -> p::Result<p::ReplicaApplyReport>;
    fn rebuild(&self, scope: p::ReplicaScope) -> p::Result<p::ReplicaProjectionDigestRef>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableStrategyRecord {
    pub candidate: p::StrategyCandidate,
    pub created_event: p::EventId,
    pub promotion_event: p::EventId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvolutionHistoryEntry {
    pub aggregate: p::EvolutionAggregateRef,
    pub committed_version: u64,
    pub event_id: p::EventId,
    pub kind: p::EventKind,
    pub active: p::ActiveStrategyRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncCursorState {
    pub schema_version: p::SchemaVersion,
    pub peer: p::SyncPeerRef,
    pub aggregate: p::RunId,
    pub applied_version: u64,
    pub exported_version: u64,
}
