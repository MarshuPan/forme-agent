//! M4 federated-runtime protocol contracts.
//!
//! The types in this module are intentionally closed and boring.  They carry
//! the authority's decisions across process boundaries; they do not contain a
//! second policy engine or a second event writer.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::*;

pub const M4_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

fn required(value: &str, name: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error(format!("{name} is incomplete")))
    } else {
        Ok(())
    }
}

fn required_digest(value: &SchemaDigest, name: &str) -> Result<()> {
    required(&value.0, name)
}

/// A deterministic digest for a versioned wire object.  Callers must clear a
/// self-referential `digest` field before passing the object to this helper.
pub fn canonical_digest<T: Serialize>(value: &T) -> Result<SchemaDigest> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| Error(format!("failed to encode canonical digest input: {error}")))?;
    let digest = Sha256::digest(bytes);
    let mut encoded = String::from("sha256:");
    for byte in digest {
        use core::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}")
            .map_err(|_| Error("failed to encode canonical digest".into()))?;
    }
    Ok(SchemaDigest(encoded))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FederatedPeerRole {
    OwnerClient,
    Executor,
    Replica,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederationAggregateVersion {
    pub schema_version: SchemaVersion,
    pub aggregate: FederationAggregateRef,
    pub version: u64,
}

impl FederationAggregateVersion {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error(
                "federation aggregate version schema is invalid".into(),
            ));
        }
        required(&self.aggregate.0, "federation aggregate")
    }

    pub fn next(&self) -> Result<Self> {
        self.validate()?;
        Ok(Self {
            schema_version: self.schema_version,
            aggregate: self.aggregate.clone(),
            version: self
                .version
                .checked_add(1)
                .ok_or_else(|| Error("federation aggregate version is exhausted".into()))?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedPeerGrant {
    pub schema_version: SchemaVersion,
    pub peer: FederatedPeerRef,
    pub owner: VerifiedPrincipal,
    pub roles: Vec<FederatedPeerRole>,
    pub scopes: Vec<Scope>,
    pub capabilities: Vec<CapabilityRef>,
    pub transport_identity: TransportIdentityDigest,
    pub authority_epoch: AuthorityEpoch,
    pub grant_version: PeerGrantVersion,
    pub expires_at: Timestamp,
    pub created_by: OwnerControlRef,
}

impl FederatedPeerGrant {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.roles.is_empty()
            || self.scopes.is_empty()
            || self.authority_epoch.0 == 0
            || self.expires_at <= 0
        {
            return Err(Error("federated peer grant is incomplete".into()));
        }
        required(&self.peer.0, "federated peer")?;
        required(&self.owner.0, "grant owner")?;
        required(&self.transport_identity.0, "transport identity digest")?;
        required(&self.created_by.0, "owner control reference")?;
        self.grant_version.validate()?;
        if self.roles.windows(2).any(|pair| pair[0] >= pair[1])
            || self.scopes.windows(2).any(|pair| pair[0].0 >= pair[1].0)
            || self
                .capabilities
                .windows(2)
                .any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err(Error(
                "federated peer grant lists must be sorted and unique".into(),
            ));
        }
        Ok(())
    }

    pub fn reference(&self) -> Result<FederatedPeerGrantRef> {
        self.validate()?;
        Ok(FederatedPeerGrantRef(canonical_digest(self)?.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederationSnapshot {
    pub schema_version: SchemaVersion,
    pub authority: AuthorityRef,
    pub authority_epoch: AuthorityEpoch,
    pub registry_version: FederationAggregateVersion,
    pub grants: Vec<FederatedPeerGrantRef>,
    pub digest: SchemaDigest,
}

impl FederationSnapshot {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("federation snapshot schema is invalid".into()));
        }
        required(&self.authority.0, "authority")?;
        self.registry_version.validate()?;
        if self.registry_version.aggregate.0 != "federation" {
            return Err(Error("federation snapshot has the wrong aggregate".into()));
        }
        required_digest(&self.digest, "federation snapshot digest")?;
        if self.grants.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(Error("federation snapshot grants are not sorted".into()));
        }
        let expected = canonical_digest(&(
            &self.authority,
            self.authority_epoch,
            &self.registry_version,
            &self.grants,
        ))?;
        if self.digest != expected {
            return Err(Error(
                "federation snapshot digest does not match its contents".into(),
            ));
        }
        Ok(())
    }

    pub fn empty(authority: AuthorityRef) -> Self {
        let mut snapshot = Self {
            schema_version: M4_SCHEMA_VERSION,
            authority,
            authority_epoch: AuthorityEpoch::initial(),
            registry_version: FederationAggregateVersion {
                schema_version: M4_SCHEMA_VERSION,
                aggregate: FederationAggregateRef("federation".into()),
                version: 0,
            },
            grants: Vec::new(),
            digest: SchemaDigest(String::new()),
        };
        snapshot.digest = canonical_digest(&(
            &snapshot.authority,
            snapshot.authority_epoch,
            &snapshot.registry_version,
            &snapshot.grants,
        ))
        .unwrap_or_else(|_| SchemaDigest("sha256:invalid".into()));
        snapshot
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteOperation {
    pub schema_version: SchemaVersion,
    pub backend: BackendKind,
    pub parameters: ActionParameters,
    pub capability: CapabilityRef,
    pub scope: Scope,
    pub action_type: ActionType,
    pub expected_effect: ExpectedEffect,
    pub rollback_boundary: RollbackBoundary,
    pub credential_slot: Option<ExecutorCredentialSlotRef>,
    pub digest: SchemaDigest,
}

impl RemoteOperation {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.backend == BackendKind::Remote {
            return Err(Error("remote operation backend is invalid".into()));
        }
        required(&self.capability.0, "remote operation capability")?;
        required(&self.scope.0, "remote operation scope")?;
        required(&self.rollback_boundary.0, "remote rollback boundary")?;
        required_digest(&self.digest, "remote operation digest")?;
        if !parameters_match_backend(self.backend, &self.parameters) {
            return Err(Error(
                "remote operation backend and parameters disagree".into(),
            ));
        }
        if contains_secret(&self.parameters) {
            return Err(Error(
                "remote operation contains a central secret reference".into(),
            ));
        }
        if contains_host_path(&self.parameters) {
            return Err(Error("remote operation contains a host path".into()));
        }
        if self.expected_effect == ExpectedEffect::Outward
            && matches!(self.action_type, ActionType::Observe | ActionType::Analyze)
        {
            return Err(Error(
                "remote observation cannot declare an outward effect".into(),
            ));
        }
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error(
                "remote operation digest does not match its contents".into(),
            ));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotePlacementPlan {
    pub schema_version: SchemaVersion,
    pub executor: FederatedPeerRef,
    pub peer_grant: FederatedPeerGrantRef,
    pub grant_version: PeerGrantVersion,
    pub authority_epoch: AuthorityEpoch,
    pub executor_profile: ExecutorProfileRef,
    pub operation: RemoteOperation,
    pub digest: SchemaDigest,
}

impl RemotePlacementPlan {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.authority_epoch.0 == 0 {
            return Err(Error("remote placement plan is incomplete".into()));
        }
        required(&self.executor.0, "remote executor")?;
        required(&self.peer_grant.0, "remote peer grant")?;
        required(&self.executor_profile.0, "executor profile")?;
        self.grant_version.validate()?;
        self.operation.validate()?;
        required_digest(&self.digest, "remote placement digest")?;
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error(
                "remote placement digest does not match its contents".into(),
            ));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }

    pub fn reference(&self) -> Result<RemotePlacementPlanRef> {
        self.validate()?;
        Ok(RemotePlacementPlanRef(self.digest.0.clone()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteActionSpec {
    pub schema_version: SchemaVersion,
    pub placement: RemotePlacementPlan,
}

impl RemoteActionSpec {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("remote action schema is invalid".into()));
        }
        self.placement.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RemoteLeaseState {
    Reserved,
    Acquired,
    Released,
    Expired,
    Fenced,
}

impl RemoteLeaseState {
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Released | Self::Expired | Self::Fenced)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteExecutionLease {
    pub schema_version: SchemaVersion,
    pub lease: RemoteExecutionLeaseRef,
    pub dispatch: RemoteDispatchId,
    pub intent: ActionId,
    pub plan_digest: PlanDigest,
    pub placement: RemotePlacementPlanRef,
    pub executor: FederatedPeerRef,
    pub peer_grant: FederatedPeerGrantRef,
    pub grant_version: PeerGrantVersion,
    pub authority_epoch: AuthorityEpoch,
    pub fence: FenceToken,
    pub expires_at: Timestamp,
    pub state: RemoteLeaseState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RemoteDispatchClaimStatus {
    Reserved,
    Claimed,
    AlreadyAttempted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteDispatchClaim {
    pub schema_version: SchemaVersion,
    pub dispatch: RemoteDispatchId,
    pub lease: RemoteExecutionLeaseRef,
    pub plan_digest: PlanDigest,
    pub authority_epoch: AuthorityEpoch,
    pub status: RemoteDispatchClaimStatus,
}

impl RemoteDispatchClaim {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.authority_epoch.0 == 0 {
            return Err(Error("remote dispatch claim is incomplete".into()));
        }
        required(&self.dispatch.0, "dispatch claim")?;
        required(&self.lease.0, "dispatch claim lease")?;
        required(&self.plan_digest.0, "dispatch claim plan")
    }
}

impl RemoteExecutionLease {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.expires_at <= 0 {
            return Err(Error("remote execution lease is incomplete".into()));
        }
        for (value, name) in [
            (&self.lease.0, "lease"),
            (&self.dispatch.0, "dispatch"),
            (&self.intent.0, "intent"),
            (&self.plan_digest.0, "plan digest"),
            (&self.placement.0, "placement"),
            (&self.executor.0, "executor"),
            (&self.peer_grant.0, "peer grant"),
        ] {
            required(value, name)?;
        }
        self.grant_version.validate()?;
        if self.authority_epoch.0 == 0 {
            return Err(Error("lease authority epoch is invalid".into()));
        }
        self.fence.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteActionRecoveryRecord {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub source: Source,
    pub intent: ActionIntent,
    pub placement: RemotePlacementPlan,
    pub lease: RemoteExecutionLease,
    pub driver_receipt: Option<RemoteDriverReceiptRef>,
    pub digest: SchemaDigest,
}

impl RemoteActionRecoveryRecord {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.source != self.intent.source
            || self.intent.backend_hint != BackendKind::Remote
            || self.lease.state != RemoteLeaseState::Acquired
        {
            return Err(Error("remote recovery record is incomplete".into()));
        }
        required(&self.run.0, "remote recovery run")?;
        required(&self.intent.intent_id.0, "remote recovery intent")?;
        self.placement.validate()?;
        self.lease.validate()?;
        let ActionParameters::Remote(spec) = &self.intent.parameters else {
            return Err(Error("remote recovery intent has no placement".into()));
        };
        if spec.placement != self.placement
            || self.lease.intent != self.intent.intent_id
            || self.lease.placement != self.placement.reference()?
            || self.lease.executor != self.placement.executor
            || self.lease.peer_grant != self.placement.peer_grant
            || self.lease.grant_version != self.placement.grant_version
            || self.lease.authority_epoch != self.placement.authority_epoch
        {
            return Err(Error("remote recovery binding is invalid".into()));
        }
        if self
            .driver_receipt
            .as_ref()
            .is_some_and(|receipt| receipt.0.trim().is_empty())
        {
            return Err(Error("remote recovery receipt is empty".into()));
        }
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("remote recovery digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RemoteReceiptOutcome {
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteDriverReceipt {
    pub schema_version: SchemaVersion,
    pub receipt: RemoteDriverReceiptRef,
    pub lease: RemoteExecutionLeaseRef,
    pub dispatch: RemoteDispatchId,
    pub intent: ActionId,
    pub plan_digest: PlanDigest,
    pub operation_digest: SchemaDigest,
    pub executor: FederatedPeerRef,
    pub authority_epoch: AuthorityEpoch,
    pub fence: FenceToken,
    pub outcome: RemoteReceiptOutcome,
    pub result_digest: Option<SchemaDigest>,
    pub observations: Vec<EvidenceRef>,
    pub observed_at: Timestamp,
}

impl RemoteDriverReceipt {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.observed_at <= 0 || self.observations.is_empty() {
            return Err(Error("remote driver receipt is incomplete".into()));
        }
        for (value, name) in [
            (&self.receipt.0, "driver receipt"),
            (&self.lease.0, "lease"),
            (&self.dispatch.0, "dispatch"),
            (&self.intent.0, "intent"),
            (&self.plan_digest.0, "plan digest"),
            (&self.executor.0, "executor"),
        ] {
            required(value, name)?;
        }
        required_digest(&self.operation_digest, "operation digest")?;
        if self.authority_epoch.0 == 0 {
            return Err(Error("receipt authority epoch is invalid".into()));
        }
        self.fence.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteExecutionReceipt {
    pub schema_version: SchemaVersion,
    pub receipt: RemoteExecutionReceiptRef,
    pub driver_receipt: RemoteDriverReceiptRef,
    pub driver_receipt_digest: SchemaDigest,
    pub lease: RemoteExecutionLeaseRef,
    pub intent: ActionId,
    pub plan_digest: PlanDigest,
    pub operation_digest: SchemaDigest,
    pub executor: FederatedPeerRef,
    pub rollback_boundary: RollbackBoundary,
    pub outcome: RemoteReceiptOutcome,
    pub verification: Vec<EvidenceRef>,
    pub ground_truth: Vec<EvidenceRef>,
    pub authority_verified: RequiredTrue,
}

impl RemoteExecutionReceipt {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.verification.is_empty()
            || self.ground_truth.is_empty()
        {
            return Err(Error("remote execution receipt is incomplete".into()));
        }
        for (value, name) in [
            (&self.receipt.0, "execution receipt"),
            (&self.driver_receipt.0, "driver receipt"),
            (&self.lease.0, "lease"),
            (&self.intent.0, "intent"),
            (&self.plan_digest.0, "plan digest"),
            (&self.executor.0, "executor"),
            (&self.rollback_boundary.0, "rollback boundary"),
        ] {
            required(value, name)?;
        }
        required_digest(&self.driver_receipt_digest, "driver receipt digest")?;
        required_digest(&self.operation_digest, "operation digest")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLeaseRequest {
    pub schema_version: SchemaVersion,
    pub lease: RemoteExecutionLeaseRef,
    pub dispatch: RemoteDispatchId,
    pub intent: ActionId,
    pub plan_digest: PlanDigest,
    pub placement: RemotePlacementPlan,
    pub expires_at: Timestamp,
}

impl RemoteLeaseRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.expires_at <= 0 {
            return Err(Error("remote lease request is incomplete".into()));
        }
        self.placement.validate()?;
        required(&self.lease.0, "lease")?;
        required(&self.dispatch.0, "dispatch")?;
        required(&self.intent.0, "intent")?;
        required(&self.plan_digest.0, "plan digest")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLeaseTransition {
    pub schema_version: SchemaVersion,
    pub lease: RemoteExecutionLeaseRef,
    pub expected: RemoteLeaseState,
    pub next: RemoteLeaseState,
    pub reason: ReasonRef,
    pub authority_epoch: AuthorityEpoch,
}

impl RemoteLeaseTransition {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.authority_epoch.0 == 0 {
            return Err(Error("remote lease transition is incomplete".into()));
        }
        required(&self.lease.0, "lease")?;
        required(&self.reason.0, "lease transition reason")?;
        if self.expected.terminal() || !self.next.terminal() {
            return Err(Error("remote lease transition is not terminal".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteDispatchAcceptance {
    pub schema_version: SchemaVersion,
    pub dispatch: RemoteDispatchId,
    pub lease: RemoteExecutionLeaseRef,
    pub accepted: RequiredTrue,
    pub receipt: RemoteDriverReceiptRef,
    pub authority_epoch: AuthorityEpoch,
    pub fence: FenceToken,
}

impl RemoteDispatchAcceptance {
    pub fn validate_for(&self, lease: &RemoteExecutionLease) -> Result<()> {
        if self.schema_version.0 == 0
            || self.dispatch != lease.dispatch
            || self.lease != lease.lease
            || self.authority_epoch != lease.authority_epoch
            || self.fence != lease.fence
        {
            return Err(Error(
                "remote dispatch acceptance binding is invalid".into(),
            ));
        }
        required(&self.receipt.0, "remote driver receipt")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteProbeRequest {
    pub schema_version: SchemaVersion,
    pub lease: RemoteExecutionLeaseRef,
    pub dispatch: RemoteDispatchId,
    pub authority_epoch: AuthorityEpoch,
    pub fence: FenceToken,
    pub nonce: Nonce,
}

impl RemoteProbeRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.authority_epoch.0 == 0 {
            return Err(Error("remote probe request is incomplete".into()));
        }
        required(&self.lease.0, "probe lease")?;
        required(&self.dispatch.0, "probe dispatch")?;
        required(&self.nonce.0, "probe nonce")?;
        self.fence.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteCancelRequest {
    pub schema_version: SchemaVersion,
    pub lease: RemoteExecutionLeaseRef,
    pub dispatch: RemoteDispatchId,
    pub authority_epoch: AuthorityEpoch,
    pub fence: FenceToken,
    pub nonce: Nonce,
    pub reason: ReasonRef,
}

impl RemoteCancelRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.authority_epoch.0 == 0 {
            return Err(Error("remote cancel request is incomplete".into()));
        }
        required(&self.lease.0, "cancel lease")?;
        required(&self.dispatch.0, "cancel dispatch")?;
        required(&self.nonce.0, "cancel nonce")?;
        required(&self.reason.0, "cancel reason")?;
        self.fence.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RemoteProbeOutcome {
    NotDispatched,
    Completed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteProbeResult {
    pub schema_version: SchemaVersion,
    pub lease: RemoteExecutionLeaseRef,
    pub outcome: RemoteProbeOutcome,
    pub receipt: Option<RemoteDriverReceiptRef>,
    pub evidence: Vec<EvidenceRef>,
}

impl RemoteProbeResult {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("remote probe result schema is invalid".into()));
        }
        required(&self.lease.0, "probe result lease")?;
        match self.outcome {
            RemoteProbeOutcome::Completed | RemoteProbeOutcome::Failed
                if self.receipt.is_none() =>
            {
                Err(Error("terminal remote probe has no receipt".into()))
            }
            RemoteProbeOutcome::NotDispatched if self.receipt.is_some() => {
                Err(Error("not-dispatched probe cannot carry a receipt".into()))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RemoteCancelOutcome {
    NotDispatched,
    Cancelled,
    AlreadyCompleted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteCancelResult {
    pub schema_version: SchemaVersion,
    pub lease: RemoteExecutionLeaseRef,
    pub outcome: RemoteCancelOutcome,
    pub evidence: Vec<EvidenceRef>,
}

impl RemoteCancelResult {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("remote cancel result schema is invalid".into()));
        }
        required(&self.lease.0, "cancel result lease")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteReceiptRequest {
    pub schema_version: SchemaVersion,
    pub receipt: RemoteDriverReceiptRef,
    pub lease: RemoteExecutionLeaseRef,
    pub dispatch: RemoteDispatchId,
    pub authority_epoch: AuthorityEpoch,
    pub fence: FenceToken,
    pub nonce: Nonce,
}

impl RemoteReceiptRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.authority_epoch.0 == 0 {
            return Err(Error("remote receipt request is incomplete".into()));
        }
        required(&self.receipt.0, "requested receipt")?;
        required(&self.lease.0, "receipt lease")?;
        required(&self.dispatch.0, "receipt dispatch")?;
        required(&self.nonce.0, "receipt nonce")?;
        self.fence.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body", rename_all = "snake_case")]
pub enum RemoteWireCommand {
    Dispatch {
        plan: Box<RemotePlacementPlan>,
        lease: RemoteExecutionLease,
    },
    Probe(RemoteProbeRequest),
    Cancel(RemoteCancelRequest),
    Receipt(RemoteReceiptRequest),
}

impl RemoteWireCommand {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Dispatch { plan, lease } => {
                plan.validate()?;
                lease.validate()?;
                if lease.placement != plan.reference()?
                    || lease.executor != plan.executor
                    || lease.peer_grant != plan.peer_grant
                    || lease.grant_version != plan.grant_version
                    || lease.authority_epoch != plan.authority_epoch
                    || lease.state != RemoteLeaseState::Acquired
                {
                    return Err(Error("remote dispatch wire binding is invalid".into()));
                }
                Ok(())
            }
            Self::Probe(request) => request.validate(),
            Self::Cancel(request) => request.validate(),
            Self::Receipt(request) => request.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteWireEnvelope {
    pub schema_version: SchemaVersion,
    pub request: RemoteWireRequestRef,
    pub authority: AuthorityRef,
    pub peer: FederatedPeerRef,
    pub nonce: Nonce,
    pub expires_at: Timestamp,
    pub command: RemoteWireCommand,
    pub digest: SchemaDigest,
}

impl RemoteWireEnvelope {
    pub fn validate(&self, now: Timestamp) -> Result<()> {
        if self.schema_version.0 == 0 || self.expires_at <= now {
            return Err(Error("remote wire envelope is expired or invalid".into()));
        }
        required(&self.request.0, "remote wire request")?;
        required(&self.authority.0, "remote wire authority")?;
        required(&self.peer.0, "remote wire peer")?;
        required(&self.nonce.0, "remote wire nonce")?;
        self.command.validate()?;
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("remote wire envelope digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body", rename_all = "snake_case")]
pub enum RemoteWireResponse {
    Dispatch(RemoteDispatchAcceptance),
    Probe(RemoteProbeResult),
    Cancel(RemoteCancelResult),
    Receipt(RemoteDriverReceipt),
}

impl RemoteWireResponse {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Dispatch(response) => {
                if response.schema_version.0 == 0 || response.authority_epoch.0 == 0 {
                    return Err(Error("remote dispatch response is incomplete".into()));
                }
                required(&response.dispatch.0, "dispatch response id")?;
                required(&response.lease.0, "dispatch response lease")?;
                required(&response.receipt.0, "dispatch response receipt")?;
                response.fence.validate()
            }
            Self::Probe(response) => response.validate(),
            Self::Cancel(response) => response.validate(),
            Self::Receipt(response) => response.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteWireReply {
    pub schema_version: SchemaVersion,
    pub request: RemoteWireRequestRef,
    pub response: RemoteWireResponse,
    pub digest: SchemaDigest,
}

impl RemoteWireReply {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("remote wire reply schema is invalid".into()));
        }
        required(&self.request.0, "remote reply request")?;
        self.response.validate()?;
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("remote wire reply digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteAdmission {
    pub schema_version: SchemaVersion,
    pub admission: RemoteDispatchId,
    pub plan: RemotePlacementPlan,
    pub lease: RemoteExecutionLease,
}

impl RemoteAdmission {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("remote admission schema is invalid".into()));
        }
        self.plan.validate()?;
        self.lease.validate()?;
        if self.lease.state != RemoteLeaseState::Acquired
            || self.lease.placement != self.plan.reference()?
        {
            return Err(Error("remote admission lease binding is invalid".into()));
        }
        required(&self.admission.0, "admission")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationCursor {
    pub schema_version: SchemaVersion,
    pub peer: FederatedPeerRef,
    pub aggregate: RunId,
    pub stream_seq: u64,
    pub authority_epoch: AuthorityEpoch,
}

impl ReplicationCursor {
    pub fn zero(peer: FederatedPeerRef, aggregate: RunId, epoch: AuthorityEpoch) -> Self {
        Self {
            schema_version: M4_SCHEMA_VERSION,
            peer,
            aggregate,
            stream_seq: 0,
            authority_epoch: epoch,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.authority_epoch.0 == 0 {
            return Err(Error("replication cursor is incomplete".into()));
        }
        required(&self.peer.0, "replication peer")?;
        required(&self.aggregate.0, "replication aggregate")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplicationBatch {
    pub schema_version: SchemaVersion,
    pub batch: ReplicationBatchRef,
    pub peer: FederatedPeerRef,
    pub peer_grant: FederatedPeerGrantRef,
    pub aggregate: RunId,
    pub from: ReplicationCursor,
    pub to: ReplicationCursor,
    pub redaction: RedactionPolicyRef,
    pub events: Vec<SyncTransferEvent>,
    pub content_digest: SchemaDigest,
}

impl ReplicationBatch {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.events.is_empty() {
            return Err(Error("replication batch is incomplete".into()));
        }
        required(&self.batch.0, "replication batch")?;
        required(&self.peer.0, "replication peer")?;
        required(&self.peer_grant.0, "replication grant")?;
        required(&self.aggregate.0, "replication aggregate")?;
        required(&self.redaction.0, "redaction profile")?;
        self.from.validate()?;
        self.to.validate()?;
        if self.from.peer != self.peer
            || self.to.peer != self.peer
            || self.from.aggregate != self.aggregate
            || self.to.aggregate != self.aggregate
            || self.from.authority_epoch != self.to.authority_epoch
            || self.from.stream_seq >= self.to.stream_seq
        {
            return Err(Error("replication cursor binding is invalid".into()));
        }
        for (index, event) in self.events.iter().enumerate() {
            let expected = self
                .from
                .stream_seq
                .checked_add(index as u64 + 1)
                .ok_or_else(|| Error("replication cursor overflow".into()))?;
            if event.aggregate != self.aggregate || event.source_stream_seq != expected {
                return Err(Error("replication events are not contiguous".into()));
            }
        }
        if self.events.last().map(|event| event.source_stream_seq) != Some(self.to.stream_seq) {
            return Err(Error(
                "replication batch endpoint does not match events".into(),
            ));
        }
        required_digest(&self.content_digest, "replication content digest")?;
        let mut material = self.clone();
        material.content_digest = SchemaDigest(String::new());
        if self.content_digest != canonical_digest(&material)? {
            return Err(Error(
                "replication content digest does not match its contents".into(),
            ));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.content_digest = SchemaDigest(String::new());
        self.content_digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationAck {
    pub schema_version: SchemaVersion,
    pub batch: ReplicationBatchRef,
    pub peer: FederatedPeerRef,
    pub aggregate: RunId,
    pub applied: ReplicationCursor,
    pub projection_digest: SchemaDigest,
}

impl ReplicationAck {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 {
            return Err(Error("replication ack schema is invalid".into()));
        }
        required(&self.batch.0, "replication batch")?;
        required(&self.peer.0, "replication peer")?;
        required(&self.aggregate.0, "replication aggregate")?;
        self.applied.validate()?;
        required_digest(&self.projection_digest, "replica projection digest")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationExportRequest {
    pub schema_version: SchemaVersion,
    pub peer: FederatedPeerRef,
    pub aggregate: RunId,
    pub after: ReplicationCursor,
    pub limit: u32,
    pub redaction: RedactionPolicyRef,
    pub grant: FederatedPeerGrantRef,
}

impl ReplicationExportRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.limit == 0 {
            return Err(Error("replication export request is incomplete".into()));
        }
        required(&self.peer.0, "replication peer")?;
        required(&self.aggregate.0, "replication aggregate")?;
        required(&self.grant.0, "replication grant")?;
        self.after.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaScope {
    pub schema_version: SchemaVersion,
    pub scopes: Vec<Scope>,
    pub allow_owner_view: bool,
}

impl ReplicaScope {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.scopes.is_empty() {
            return Err(Error("replica scope is incomplete".into()));
        }
        if self.scopes.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(Error("replica scopes are not sorted".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaApplyReport {
    pub schema_version: SchemaVersion,
    pub batch: ReplicationBatchRef,
    pub status: ReplicaApplyStatus,
    pub cursor: ReplicationCursor,
    pub projection_digest: SchemaDigest,
    pub applied_events: Vec<EventId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReplicaApplyStatus {
    Applied,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedPeerState {
    pub schema_version: SchemaVersion,
    pub grant: FederatedPeerGrant,
    pub revoked: bool,
    pub last_seen_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedControlEnvelope {
    pub schema_version: SchemaVersion,
    pub peer: FederatedPeerRef,
    pub session: FederatedSessionRef,
    pub owner: VerifiedPrincipal,
    pub nonce: Nonce,
    pub expires_at: Timestamp,
    pub command_digest: SchemaDigest,
}

impl FederatedControlEnvelope {
    pub fn validate(&self, now: Timestamp) -> Result<()> {
        if self.schema_version.0 == 0 || self.expires_at <= now {
            return Err(Error(
                "federated control envelope is expired or invalid".into(),
            ));
        }
        required(&self.peer.0, "control peer")?;
        required(&self.session.0, "control session")?;
        required(&self.owner.0, "control owner")?;
        required(&self.nonce.0, "control nonce")?;
        required_digest(&self.command_digest, "control command digest")
    }

    pub fn validate_command<T: Serialize>(&self, command: &T, now: Timestamp) -> Result<()> {
        self.validate(now)?;
        if self.command_digest != canonical_digest(command)? {
            return Err(Error(
                "federated control command digest does not match its envelope".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RetentionStatus {
    NotRequested,
    Requested,
    Verified,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedRetentionRequest {
    pub schema_version: SchemaVersion,
    pub request: RetentionRequestRef,
    pub peer: FederatedPeerRef,
    pub scope: Scope,
    pub authority_epoch: AuthorityEpoch,
    pub requested_by: OwnerControlRef,
    pub expires_at: Timestamp,
    pub digest: SchemaDigest,
}

impl FederatedRetentionRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.authority_epoch.0 == 0 || self.expires_at <= 0 {
            return Err(Error("federated retention request is incomplete".into()));
        }
        required(&self.request.0, "retention request")?;
        required(&self.peer.0, "retention peer")?;
        required(&self.scope.0, "retention scope")?;
        required(&self.requested_by.0, "retention owner control")?;
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("retention request digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedRetentionReceipt {
    pub schema_version: SchemaVersion,
    pub receipt: RetentionReceiptRef,
    pub request: RetentionRequestRef,
    pub peer: FederatedPeerRef,
    pub authority_epoch: AuthorityEpoch,
    pub deleted_projection: ReplicaProjectionDigestRef,
    pub evidence: Vec<EvidenceRef>,
    pub observed_at: Timestamp,
    pub digest: SchemaDigest,
}

impl FederatedRetentionReceipt {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.authority_epoch.0 == 0
            || self.observed_at <= 0
            || self.evidence.is_empty()
        {
            return Err(Error("federated retention receipt is incomplete".into()));
        }
        required(&self.receipt.0, "retention receipt")?;
        required(&self.request.0, "retention receipt request")?;
        required(&self.peer.0, "retention receipt peer")?;
        required(&self.deleted_projection.0, "deleted replica projection")?;
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("retention receipt digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedRetentionState {
    pub schema_version: SchemaVersion,
    pub request: FederatedRetentionRequest,
    pub status: RetentionStatus,
    pub receipt: Option<FederatedRetentionReceipt>,
    pub digest: SchemaDigest,
}

impl FederatedRetentionState {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.status == RetentionStatus::NotRequested {
            return Err(Error("federated retention state is incomplete".into()));
        }
        self.request.validate()?;
        match (&self.status, &self.receipt) {
            (RetentionStatus::Verified, Some(receipt)) => {
                receipt.validate()?;
                if receipt.request != self.request.request
                    || receipt.peer != self.request.peer
                    || receipt.authority_epoch != self.request.authority_epoch
                {
                    return Err(Error("retention state lineage is invalid".into()));
                }
            }
            (RetentionStatus::Requested | RetentionStatus::Unknown, None) => {}
            _ => return Err(Error("retention state status and receipt disagree".into())),
        }
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("retention state digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FederatedOwnerCommand {
    Register(FederatedPeerGrant),
    Revoke {
        peer: FederatedPeerRef,
        grant: FederatedPeerGrantRef,
        in_flight: InFlightDisposition,
    },
    ResolveApproval {
        approval: ApprovalId,
        plan_digest: PlanDigest,
        outcome: ApprovalOutcome,
    },
    Cancel {
        lease: RemoteExecutionLeaseRef,
        reason: ReasonRef,
    },
    RequestRetention(FederatedRetentionRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutorHealthState {
    Healthy,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedExecutorCandidate {
    pub schema_version: SchemaVersion,
    pub peer: FederatedPeerRef,
    pub grant: FederatedPeerGrantRef,
    pub profile: ExecutorProfileRef,
    pub scope: Scope,
    pub capability: CapabilityRef,
    pub expires_at: Timestamp,
    pub health: ExecutorHealthState,
    pub health_observed_at: Timestamp,
    pub capability_evidence: Vec<CapabilityEvidenceRef>,
    pub failure_evidence: Vec<FailureEvidenceRef>,
    pub managed_policy_allowed: bool,
    pub score_basis_points: u16,
}

impl FederatedExecutorCandidate {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.expires_at <= 0
            || self.health_observed_at <= 0
            || self.score_basis_points > 10_000
        {
            return Err(Error("federated executor candidate is incomplete".into()));
        }
        required(&self.peer.0, "candidate peer")?;
        required(&self.grant.0, "candidate grant")?;
        required(&self.profile.0, "candidate profile")?;
        required(&self.scope.0, "candidate scope")?;
        required(&self.capability.0, "candidate capability")?;
        if self
            .capability_evidence
            .windows(2)
            .any(|pair| pair[0].0 >= pair[1].0)
            || self
                .failure_evidence
                .windows(2)
                .any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err(Error("candidate evidence must be sorted and unique".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlacementFilterReason {
    GrantInactive,
    RoleDenied,
    ScopeDenied,
    CapabilityDenied,
    ManagedPolicyDenied,
    SchemaIncompatible,
    HealthStale,
    EvidenceInsufficient,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementCandidateTrace {
    pub schema_version: SchemaVersion,
    pub candidate: FederatedExecutorCandidate,
    pub eligible: bool,
    pub reasons: Vec<PlacementFilterReason>,
}

impl PlacementCandidateTrace {
    pub fn validate(&self) -> Result<()> {
        self.candidate.validate()?;
        if self.schema_version.0 == 0
            || (self.eligible && !self.reasons.is_empty())
            || (!self.eligible && self.reasons.is_empty())
        {
            return Err(Error("placement candidate trace is inconsistent".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedPlacementDecision {
    pub schema_version: SchemaVersion,
    pub decision: PlacementDecisionRef,
    pub federation_snapshot: FederationSnapshotRef,
    pub scope: Scope,
    pub capability: CapabilityRef,
    pub evaluated_at: Timestamp,
    pub candidates: Vec<PlacementCandidateTrace>,
    pub chosen: Option<FederatedPeerRef>,
    pub placement: Option<RemotePlacementPlanRef>,
    pub digest: SchemaDigest,
}

impl FederatedPlacementDecision {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.evaluated_at <= 0 || self.candidates.is_empty() {
            return Err(Error("federated placement decision is incomplete".into()));
        }
        required(&self.decision.0, "placement decision")?;
        required(&self.federation_snapshot.0, "placement federation snapshot")?;
        required(&self.scope.0, "placement scope")?;
        required(&self.capability.0, "placement capability")?;
        for candidate in &self.candidates {
            candidate.validate()?;
        }
        if self
            .candidates
            .windows(2)
            .any(|pair| pair[0].candidate.peer.0 >= pair[1].candidate.peer.0)
        {
            return Err(Error(
                "placement candidates must be sorted and unique".into(),
            ));
        }
        match (&self.chosen, &self.placement) {
            (Some(chosen), Some(_))
                if self
                    .candidates
                    .iter()
                    .any(|candidate| candidate.eligible && &candidate.candidate.peer == chosen) => {
            }
            (None, None) => {}
            _ => return Err(Error("placement chose an unauthorized executor".into())),
        }
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("placement decision digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedCheckpointArtifact {
    pub schema_version: SchemaVersion,
    pub reference: FederatedCheckpointArtifactRef,
    pub checkpoint: GoalCheckpoint,
    pub done_contract: DoneContractRef,
    pub verification_outcome: VerificationOutcome,
    pub verification_events: Vec<EventId>,
    pub artifacts: Vec<ContentRef>,
    pub spent_budget: Budget,
    pub remaining_budget: Budget,
    pub external_effects: Vec<EvidenceRef>,
    pub scope: Scope,
    pub policy: PolicyProfileRef,
    pub toolset: ToolsetRef,
    pub model: ModelProfileRef,
    pub evolution_snapshot: EvolutionSnapshotRef,
    pub federation_snapshot: FederationSnapshotRef,
    pub digest: SchemaDigest,
}

impl FederatedCheckpointArtifact {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.verification_outcome != VerificationOutcome::Pass
            || self.verification_events.is_empty()
            || self.artifacts.is_empty()
        {
            return Err(Error("federated checkpoint is not durably verified".into()));
        }
        self.checkpoint.validate()?;
        for (value, name) in [
            (&self.reference.0, "federated checkpoint"),
            (&self.done_contract.0, "checkpoint done contract"),
            (&self.spent_budget.0, "spent budget"),
            (&self.remaining_budget.0, "remaining budget"),
            (&self.scope.0, "checkpoint scope"),
            (&self.policy.0, "checkpoint policy"),
            (&self.toolset.0, "checkpoint toolset"),
            (&self.model.0, "checkpoint model"),
            (&self.evolution_snapshot.0, "checkpoint evolution snapshot"),
            (
                &self.federation_snapshot.0,
                "checkpoint federation snapshot",
            ),
        ] {
            required(value, name)?;
        }
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("federated checkpoint digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedHandoffPlan {
    pub schema_version: SchemaVersion,
    pub checkpoint: FederatedCheckpointArtifactRef,
    pub from_run: RunId,
    pub next_run: RunId,
    pub target: FederatedPeerRef,
    pub placement: RemotePlacementPlanRef,
    pub evolution_snapshot: EvolutionSnapshotRef,
    pub federation_snapshot: FederationSnapshotRef,
    pub budget: Budget,
    pub digest: SchemaDigest,
}

impl FederatedHandoffPlan {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.from_run == self.next_run {
            return Err(Error("federated handoff plan is incomplete".into()));
        }
        for (value, name) in [
            (&self.checkpoint.0, "handoff checkpoint"),
            (&self.from_run.0, "handoff source run"),
            (&self.next_run.0, "handoff next run"),
            (&self.target.0, "handoff target"),
            (&self.placement.0, "handoff placement"),
            (&self.evolution_snapshot.0, "handoff evolution snapshot"),
            (&self.federation_snapshot.0, "handoff federation snapshot"),
            (&self.budget.0, "handoff budget"),
        ] {
            required(value, name)?;
        }
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("federated handoff digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FederatedSignalKind {
    Tick,
    Reconnect,
    Foreground,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedDeviceSignal {
    pub schema_version: SchemaVersion,
    pub signal: FederatedDeviceSignalRef,
    pub peer: FederatedPeerRef,
    pub session: FederatedSessionRef,
    pub kind: FederatedSignalKind,
    pub nonce: Nonce,
    pub observed_at: Timestamp,
    pub expires_at: Timestamp,
    pub digest: SchemaDigest,
}

impl FederatedDeviceSignal {
    pub fn validate(&self, now: Timestamp) -> Result<()> {
        if self.schema_version.0 == 0
            || self.observed_at <= 0
            || self.expires_at <= now
            || self.observed_at >= self.expires_at
        {
            return Err(Error(
                "federated device signal is expired or invalid".into(),
            ));
        }
        required(&self.signal.0, "device signal")?;
        required(&self.peer.0, "device signal peer")?;
        required(&self.session.0, "device signal session")?;
        required(&self.nonce.0, "device signal nonce")?;
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        if self.digest != canonical_digest(&material)? {
            return Err(Error("federated device signal digest mismatch".into()));
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) -> Result<()> {
        let mut material = self.clone();
        material.digest = SchemaDigest(String::new());
        self.digest = canonical_digest(&material)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedPeerManifest {
    pub schema_version: SchemaVersion,
    pub peer: FederatedPeerRef,
    pub roles: Vec<FederatedPeerRole>,
    pub scopes: Vec<Scope>,
    pub transport_identity: TransportIdentityDigest,
    pub authority_epoch: AuthorityEpoch,
    pub grant_version: PeerGrantVersion,
    pub expires_at: Timestamp,
    pub grant_ref: FederatedPeerGrantRef,
}

impl FederatedPeerManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.roles.is_empty()
            || self.scopes.is_empty()
            || self.authority_epoch.0 == 0
            || self.expires_at <= 0
        {
            return Err(Error("federated peer manifest is incomplete".into()));
        }
        required(&self.peer.0, "manifest peer")?;
        required(&self.transport_identity.0, "manifest transport identity")?;
        required(&self.grant_ref.0, "manifest grant")?;
        self.grant_version.validate()?;
        if self.roles.windows(2).any(|pair| pair[0] >= pair[1])
            || self.scopes.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err(Error(
                "peer manifest lists must be sorted and unique".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicationManifest {
    pub schema_version: SchemaVersion,
    pub peer: FederatedPeerRef,
    pub aggregate: RunId,
    pub from: ReplicationCursor,
    pub to: ReplicationCursor,
    pub batch: ReplicationBatchRef,
    pub batch_digest: SchemaDigest,
    pub event_digests: Vec<SchemaDigest>,
    pub redaction: RedactionPolicyRef,
    pub authority_epoch: AuthorityEpoch,
}

impl ReplicationManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.authority_epoch.0 == 0
            || self.event_digests.is_empty()
        {
            return Err(Error("replication manifest is incomplete".into()));
        }
        self.from.validate()?;
        self.to.validate()?;
        required(&self.peer.0, "replication manifest peer")?;
        required(&self.aggregate.0, "replication manifest aggregate")?;
        required(&self.batch.0, "replication manifest batch")?;
        required_digest(&self.batch_digest, "replication manifest batch digest")?;
        required(&self.redaction.0, "replication manifest redaction")?;
        if self.peer != self.from.peer
            || self.peer != self.to.peer
            || self.aggregate != self.from.aggregate
            || self.aggregate != self.to.aggregate
            || self.authority_epoch != self.from.authority_epoch
            || self.authority_epoch != self.to.authority_epoch
            || self.to.stream_seq <= self.from.stream_seq
            || self.event_digests.len()
                != usize::try_from(self.to.stream_seq - self.from.stream_seq)
                    .map_err(|_| Error("replication manifest range is too large".into()))?
        {
            return Err(Error(
                "replication manifest cursor lineage is invalid".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedTraceManifest {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub owner_commands: Vec<EventId>,
    pub approvals: Vec<EventId>,
    pub leases: Vec<EventId>,
    pub actions: Vec<EventId>,
    pub verifications: Vec<EventId>,
    pub replication: Vec<EventId>,
    pub recovery: Vec<EventId>,
    pub revocations: Vec<EventId>,
}

impl FederatedTraceManifest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0 || self.run.0.trim().is_empty() {
            return Err(Error("federated trace manifest is incomplete".into()));
        }
        let all = self
            .owner_commands
            .iter()
            .chain(&self.approvals)
            .chain(&self.leases)
            .chain(&self.actions)
            .chain(&self.verifications)
            .chain(&self.replication)
            .chain(&self.recovery)
            .chain(&self.revocations)
            .collect::<Vec<_>>();
        if all.is_empty()
            || all.iter().any(|event| event.0.trim().is_empty())
            || all.iter().collect::<std::collections::BTreeSet<_>>().len() != all.len()
        {
            return Err(Error(
                "federated trace references must be non-empty and unique".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederationGoldenReport {
    pub schema_version: SchemaVersion,
    pub scenario: String,
    pub event_kinds: Vec<EventKind>,
    pub authority_driver_calls: u32,
    pub mutation_ordinal: u64,
    pub replica_cursor: Option<ReplicationCursor>,
    pub secret_scan_matches: u32,
    pub negative_assertions: Vec<String>,
    pub trace_digest: SchemaDigest,
}

impl FederationGoldenReport {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version.0 == 0
            || self.scenario.trim().is_empty()
            || self.event_kinds.is_empty()
            || self.authority_driver_calls != 1
            || self.mutation_ordinal != 1
            || self.secret_scan_matches != 0
            || self.negative_assertions.is_empty()
            || self
                .negative_assertions
                .iter()
                .any(|assertion| assertion.trim().is_empty())
        {
            return Err(Error(
                "federation golden report is not complete evidence".into(),
            ));
        }
        required_digest(&self.trace_digest, "federation golden trace digest")?;
        if let Some(cursor) = &self.replica_cursor {
            cursor.validate()?;
        }
        Ok(())
    }
}

fn parameters_match_backend(backend: BackendKind, parameters: &ActionParameters) -> bool {
    matches!(
        (backend, parameters),
        (BackendKind::Shell, ActionParameters::Shell { .. })
            | (BackendKind::File, ActionParameters::File { .. })
            | (BackendKind::Mcp, ActionParameters::Mcp { .. })
            | (
                BackendKind::Notification,
                ActionParameters::Notification { .. }
            )
            | (BackendKind::Browser, ActionParameters::Browser(_))
            | (BackendKind::Computer, ActionParameters::Computer(_))
            | (BackendKind::Pty, ActionParameters::Pty(_))
            | (BackendKind::AppApi, ActionParameters::AppApi(_))
    )
}

fn contains_secret(parameters: &ActionParameters) -> bool {
    fn input_has_secret(input: &ExternalInput) -> bool {
        matches!(input, ExternalInput::Secret(_))
    }
    match parameters {
        ActionParameters::Shell { .. }
        | ActionParameters::File { .. }
        | ActionParameters::Notification { .. }
        | ActionParameters::Browser(BrowserActionSpec {
            operation:
                BrowserOperation::Navigate
                | BrowserOperation::ReadText { .. }
                | BrowserOperation::Click { .. }
                | BrowserOperation::Screenshot { .. },
            ..
        })
        | ActionParameters::Computer(ComputerActionSpec {
            operation:
                ComputerOperation::Move { .. }
                | ComputerOperation::Click { .. }
                | ComputerOperation::Key { .. }
                | ComputerOperation::Scroll { .. }
                | ComputerOperation::Screenshot,
            ..
        }) => false,
        ActionParameters::Browser(BrowserActionSpec {
            operation: BrowserOperation::Type { input, .. },
            ..
        })
        | ActionParameters::Computer(ComputerActionSpec {
            operation: ComputerOperation::Type { input },
            ..
        }) => input_has_secret(input),
        ActionParameters::Pty(spec) => {
            spec.input.as_ref().is_some_and(input_has_secret) || !spec.environment.is_empty()
        }
        ActionParameters::AppApi(spec) => {
            spec.credential.is_some()
                || match &spec.operation {
                    AppApiOperation::Read => false,
                    AppApiOperation::Mutation { body, .. } => {
                        body.as_ref().is_some_and(input_has_secret)
                    }
                }
        }
        ActionParameters::Mcp { .. } => false,
        ActionParameters::Remote(spec) => contains_secret(&spec.placement.operation.parameters),
    }
}

fn contains_host_path(parameters: &ActionParameters) -> bool {
    fn unsafe_path(value: &str) -> bool {
        let path = std::path::Path::new(value);
        path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
    }
    match parameters {
        ActionParameters::Shell { cwd, .. } => cwd.as_deref().is_some_and(unsafe_path),
        ActionParameters::File { path, .. } => unsafe_path(path),
        ActionParameters::Pty(spec) => unsafe_path(&spec.cwd),
        ActionParameters::Remote(spec) => contains_host_path(&spec.placement.operation.parameters),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_snapshot_is_valid_and_deterministic() {
        let snapshot = FederationSnapshot::empty(AuthorityRef("authority:test".into()));
        snapshot
            .validate()
            .expect("empty local federation snapshot");
        assert!(snapshot.digest.0.starts_with("sha256:"));
    }

    #[test]
    fn remote_parameters_cannot_recurse_or_carry_secret() {
        let operation = RemoteOperation {
            schema_version: M4_SCHEMA_VERSION,
            backend: BackendKind::AppApi,
            parameters: ActionParameters::AppApi(AppApiActionSpec {
                schema_version: M4_SCHEMA_VERSION,
                connector: ProviderId("connector".into()),
                endpoint: "https://example.invalid".into(),
                schema_digest: SchemaDigest("sha256:schema".into()),
                credential: Some(SecretRef("central-secret".into())),
                operation: AppApiOperation::Read,
                timeout: DurationMs(1000),
                participant: None,
                representation: None,
                disclosure_request: None,
            }),
            capability: CapabilityRef("api.read".into()),
            scope: Scope("workspace:test".into()),
            action_type: ActionType::Observe,
            expected_effect: ExpectedEffect::Internal,
            rollback_boundary: RollbackBoundary("none".into()),
            credential_slot: Some(ExecutorCredentialSlotRef("slot:api".into())),
            digest: SchemaDigest("sha256:operation".into()),
        };
        assert!(operation.validate().is_err());
    }

    #[test]
    fn replication_batch_rejects_gap() {
        let peer = FederatedPeerRef("peer:1".into());
        let aggregate = RunId("run:1".into());
        let epoch = AuthorityEpoch(1);
        let mut batch = ReplicationBatch {
            schema_version: M4_SCHEMA_VERSION,
            batch: ReplicationBatchRef("batch:1".into()),
            peer: peer.clone(),
            peer_grant: FederatedPeerGrantRef("grant:1".into()),
            aggregate: aggregate.clone(),
            from: ReplicationCursor::zero(peer.clone(), aggregate.clone(), epoch),
            to: ReplicationCursor {
                schema_version: M4_SCHEMA_VERSION,
                peer: peer.clone(),
                aggregate: aggregate.clone(),
                stream_seq: 2,
                authority_epoch: epoch,
            },
            redaction: RedactionPolicyRef("m4-safe".into()),
            events: Vec::new(),
            content_digest: SchemaDigest("sha256:batch".into()),
        };
        assert!(batch.validate().is_err());
        batch.to.stream_seq = 1;
        assert!(batch.validate().is_err());
    }
}
