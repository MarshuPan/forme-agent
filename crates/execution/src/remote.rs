use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use forme_protocol as p;

use crate::{
    planner::{plan_with, validate_plan_for},
    ActionBackend, ActionResult, ActionStatus, BackendKind, CancelToken, EventSink, ExecutionPlan,
    ExecutorLedgerDecision, ExecutorReplayLedger, InMemoryExecutorLedger, OutputBudget,
    ResolvedSecret, SecretResolver,
};

pub trait RemoteTransport: Send + Sync {
    fn dispatch(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
    ) -> p::Result<p::RemoteDispatchAcceptance>;
    fn probe(&self, request: p::RemoteProbeRequest) -> p::Result<p::RemoteProbeResult>;
    fn cancel(&self, request: p::RemoteCancelRequest) -> p::Result<p::RemoteCancelResult>;
}

pub trait RemoteReceiptSource: Send + Sync {
    fn receipt(&self, request: p::RemoteReceiptRequest) -> p::Result<p::RemoteDriverReceipt>;
}

pub trait RemoteLeaseCoordinator {
    fn acquire(&self, request: p::RemoteLeaseRequest) -> p::Result<p::RemoteExecutionLease>;
    fn transition(&self, request: p::RemoteLeaseTransition) -> p::Result<p::RemoteExecutionLease>;
}

pub trait RemoteExecutorDriver {
    fn admit(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
    ) -> p::Result<p::RemoteAdmission>;
    fn execute(&self, admission: p::RemoteAdmission) -> p::Result<p::RemoteDriverReceipt>;
    fn probe(&self, lease: &p::RemoteExecutionLeaseRef) -> p::Result<p::RemoteProbeResult>;
    fn cancel(&self, lease: &p::RemoteExecutionLeaseRef) -> p::Result<p::RemoteCancelResult>;
}

pub trait RemoteReceiptRepository: Send + Sync {
    fn fetch_receipt(
        &self,
        receipt: &p::RemoteDriverReceiptRef,
    ) -> p::Result<p::RemoteDriverReceipt>;
}

pub trait RemoteExecutorService:
    RemoteExecutorDriver + RemoteReceiptRepository + Send + Sync
{
}

impl<T> RemoteExecutorService for T where
    T: RemoteExecutorDriver + RemoteReceiptRepository + Send + Sync
{
}

pub trait RemoteClock: Send + Sync {
    fn now_ms(&self) -> p::Timestamp;
}

#[derive(Debug, Default)]
pub struct SystemRemoteClock;

impl RemoteClock for SystemRemoteClock {
    fn now_ms(&self) -> p::Timestamp {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_millis()).ok())
            .unwrap_or(i64::MAX)
    }
}

pub trait RemoteLeaseBinding: Send + Sync {
    fn take(
        &self,
        intent: &p::ActionId,
        plan: &p::PlanDigest,
    ) -> p::Result<p::RemoteExecutionLease>;
    fn active(&self, intent: &p::ActionId) -> p::Result<Option<p::RemoteExecutionLease>>;
}

/// Harness-populated one-shot lease bindings.  A backend cannot dispatch by
/// constructing or caching its own lease.
#[derive(Default)]
pub struct InMemoryRemoteLeaseBindings {
    pending: Mutex<BTreeMap<(p::ActionId, p::PlanDigest), p::RemoteExecutionLease>>,
    active: Mutex<BTreeMap<p::ActionId, p::RemoteExecutionLease>>,
}

impl InMemoryRemoteLeaseBindings {
    pub fn bind(&self, lease: p::RemoteExecutionLease) -> p::Result<()> {
        lease.validate()?;
        if lease.state != p::RemoteLeaseState::Acquired {
            return Err(p::Error(
                "only an acquired lease can bind a remote plan".into(),
            ));
        }
        let key = (lease.intent.clone(), lease.plan_digest.clone());
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| p::Error("remote lease bindings are unavailable".into()))?;
        if pending.insert(key, lease).is_some() {
            return Err(p::Error("remote lease binding already exists".into()));
        }
        Ok(())
    }
}

impl RemoteLeaseBinding for InMemoryRemoteLeaseBindings {
    fn take(
        &self,
        intent: &p::ActionId,
        plan: &p::PlanDigest,
    ) -> p::Result<p::RemoteExecutionLease> {
        let lease = self
            .pending
            .lock()
            .map_err(|_| p::Error("remote lease bindings are unavailable".into()))?
            .remove(&(intent.clone(), plan.clone()))
            .ok_or_else(|| p::Error("remote execution has no matching one-shot lease".into()))?;
        self.active
            .lock()
            .map_err(|_| p::Error("active remote lease bindings are unavailable".into()))?
            .insert(intent.clone(), lease.clone());
        Ok(lease)
    }

    fn active(&self, intent: &p::ActionId) -> p::Result<Option<p::RemoteExecutionLease>> {
        Ok(self
            .active
            .lock()
            .map_err(|_| p::Error("active remote lease bindings are unavailable".into()))?
            .get(intent)
            .cloned())
    }
}

pub struct RemoteExecutorBackend {
    transport: Arc<dyn RemoteTransport>,
    leases: Arc<dyn RemoteLeaseBinding>,
    clock: Arc<dyn RemoteClock>,
    budget: OutputBudget,
    timeout: p::DurationMs,
}

impl RemoteExecutorBackend {
    pub fn new(
        transport: Arc<dyn RemoteTransport>,
        leases: Arc<dyn RemoteLeaseBinding>,
        clock: Arc<dyn RemoteClock>,
        budget: OutputBudget,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        if budget.max_bytes == 0 || timeout.0 == 0 {
            return Err(p::Error("remote execution limits must be non-zero".into()));
        }
        Ok(Self {
            transport,
            leases,
            clock,
            budget,
            timeout,
        })
    }

    fn remote_spec<'a>(&self, intent: &'a p::ActionIntent) -> p::Result<&'a p::RemoteActionSpec> {
        let p::ActionParameters::Remote(spec) = &intent.parameters else {
            return Err(p::Error("remote backend requires remote parameters".into()));
        };
        spec.validate()?;
        let operation = &spec.placement.operation;
        if operation.capability != intent.capability_ref
            || operation.scope != intent.scope
            || operation.action_type != intent.action_type
            || operation.expected_effect != intent.expected_effect
            || operation.rollback_boundary != intent.rollback_expectation
        {
            return Err(p::Error(
                "remote operation does not match the authority action intent".into(),
            ));
        }
        Ok(spec)
    }
}

impl ActionBackend for RemoteExecutorBackend {
    fn kind(&self) -> BackendKind {
        p::BackendKind::Remote
    }

    fn plan(&self, intent: &p::ActionIntent) -> p::Result<ExecutionPlan> {
        self.remote_spec(intent)?;
        plan_with(intent, self.budget.clone(), self.timeout)
    }

    fn execute(
        &self,
        plan: ExecutionPlan,
        sink: &EventSink,
        cancel: CancelToken,
    ) -> p::Result<ActionResult> {
        validate_plan_for(&plan, p::BackendKind::Remote)?;
        let spec = self.remote_spec(&plan.intent)?;
        if plan.approval_ref.is_none() {
            return Err(p::Error(
                "remote execution requires a plan-bound approval".into(),
            ));
        }
        if cancel.is_cancelled() {
            return Err(p::Error(
                "remote execution was cancelled before dispatch".into(),
            ));
        }
        let lease = self.leases.take(&plan.intent.intent_id, &plan.digest)?;
        let placement_ref = spec.placement.reference()?;
        if lease.state != p::RemoteLeaseState::Acquired
            || lease.intent != plan.intent.intent_id
            || lease.plan_digest != plan.digest
            || lease.placement != placement_ref
            || lease.executor != spec.placement.executor
            || lease.peer_grant != spec.placement.peer_grant
            || lease.grant_version != spec.placement.grant_version
            || lease.authority_epoch != spec.placement.authority_epoch
            || lease.expires_at <= self.clock.now_ms()
        {
            return Err(p::Error(
                "remote lease no longer matches the approved plan".into(),
            ));
        }
        let acceptance = self.transport.dispatch(&spec.placement, &lease)?;
        acceptance.validate_for(&lease)?;
        sink.emit(p::EventPayload::ActionStarted(p::ActionStartedPayload {
            intent_id: plan.intent.intent_id.clone(),
            backend: p::BackendKind::Remote,
            scope: plan.scope.clone(),
            remote_lease: Some(lease.lease.clone()),
        }))?;
        Ok(ActionResult {
            schema_version: p::M4_SCHEMA_VERSION,
            result_ref: p::ActionResultRef(format!("remote-pending:{}", plan.intent.intent_id.0)),
            status: ActionStatus::Unknown,
            output_ref: p::OutputRef(format!("remote-receipt:{}", acceptance.receipt.0)),
            output: "remote outcome awaits authority verification".into(),
            truncated: false,
            evidence: p::CapabilityEvidence {
                schema_version: p::M4_SCHEMA_VERSION,
                capability: plan.intent.capability_ref,
                outcome: p::CapabilityOutcome("unverified".into()),
                reliability: p::Reliability("remote-observation".into()),
            },
            diff: None,
            rollback: None,
            external_receipt: Some(p::ExternalActionReceipt {
                schema_version: p::M4_SCHEMA_VERSION,
                action: plan.intent.intent_id,
                content_ref: Some(p::ContentRef(acceptance.receipt.0)),
                content_digest: None,
                trust: p::TrustTier::Untrusted,
                effect: p::EffectStatus::Unknown,
                probe_hint: Some(p::ProbeHintRef(format!("probe:{}", lease.lease.0))),
            }),
        })
    }

    fn cancel(&self, action: p::ActionId) -> p::Result<()> {
        let lease = self
            .leases
            .active(&action)?
            .ok_or_else(|| p::Error("remote action has no active lease".into()))?;
        let result = self.transport.cancel(p::RemoteCancelRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            lease: lease.lease,
            dispatch: lease.dispatch,
            authority_epoch: lease.authority_epoch,
            fence: lease.fence,
            nonce: p::Nonce(format!("cancel:{}", action.0)),
            reason: p::ReasonRef("authority cancellation".into()),
        })?;
        if result.outcome == p::RemoteCancelOutcome::Unknown {
            return Err(p::Error("remote cancellation outcome is unknown".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInnerOutcome {
    pub outcome: p::RemoteReceiptOutcome,
    pub result_digest: Option<p::SchemaDigest>,
    pub observations: Vec<p::EvidenceRef>,
}

pub trait RemoteInnerDriver: Send + Sync {
    fn execute(
        &self,
        operation: &p::RemoteOperation,
        credential: Option<&ResolvedSecret>,
    ) -> p::Result<RemoteInnerOutcome>;
    fn cancel(&self, _operation: &p::RemoteOperation) -> p::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorCredentialBinding {
    pub peer: p::FederatedPeerRef,
    pub scope: p::Scope,
    pub slot: p::ExecutorCredentialSlotRef,
    pub secret: p::SecretRef,
}

pub trait ExecutorCredentialResolver: Send + Sync {
    fn resolve(
        &self,
        peer: &p::FederatedPeerRef,
        scope: &p::Scope,
        slot: &p::ExecutorCredentialSlotRef,
    ) -> p::Result<ResolvedSecret>;
}

#[derive(Default)]
pub struct RejectingExecutorCredentialResolver;

impl ExecutorCredentialResolver for RejectingExecutorCredentialResolver {
    fn resolve(
        &self,
        _peer: &p::FederatedPeerRef,
        _scope: &p::Scope,
        _slot: &p::ExecutorCredentialSlotRef,
    ) -> p::Result<ResolvedSecret> {
        Err(p::Error(
            "executor credential slot is not provisioned".into(),
        ))
    }
}

pub struct ScopedExecutorCredentialResolver {
    bindings: BTreeMap<(p::FederatedPeerRef, p::Scope, p::ExecutorCredentialSlotRef), p::SecretRef>,
    secrets: Arc<dyn SecretResolver>,
}

impl ScopedExecutorCredentialResolver {
    pub fn new(
        bindings: Vec<ExecutorCredentialBinding>,
        secrets: Arc<dyn SecretResolver>,
    ) -> p::Result<Self> {
        if bindings.is_empty() {
            return Err(p::Error("executor credential bindings are empty".into()));
        }
        let mut indexed = BTreeMap::new();
        for binding in bindings {
            if binding.peer.0.trim().is_empty()
                || binding.scope.0.trim().is_empty()
                || binding.slot.0.trim().is_empty()
                || binding.secret.0.trim().is_empty()
                || indexed
                    .insert((binding.peer, binding.scope, binding.slot), binding.secret)
                    .is_some()
            {
                return Err(p::Error(
                    "executor credential binding is invalid or duplicated".into(),
                ));
            }
        }
        Ok(Self {
            bindings: indexed,
            secrets,
        })
    }
}

impl ExecutorCredentialResolver for ScopedExecutorCredentialResolver {
    fn resolve(
        &self,
        peer: &p::FederatedPeerRef,
        scope: &p::Scope,
        slot: &p::ExecutorCredentialSlotRef,
    ) -> p::Result<ResolvedSecret> {
        let secret = self
            .bindings
            .get(&(peer.clone(), scope.clone(), slot.clone()))
            .ok_or_else(|| p::Error("executor credential slot is outside the peer scope".into()))?;
        self.secrets.resolve(secret)
    }
}

#[derive(Debug, Clone)]
pub struct ExecutorAdmissionProfile {
    pub schema_version: p::SchemaVersion,
    pub peer: p::FederatedPeerRef,
    pub grant: p::FederatedPeerGrant,
    pub profile: p::ExecutorProfileRef,
    pub authority_epoch: p::AuthorityEpoch,
    pub minimum_fence: p::FenceToken,
    pub enabled_backends: BTreeSet<p::BackendKind>,
}

impl ExecutorAdmissionProfile {
    pub fn validate(&self) -> p::Result<()> {
        self.grant.validate()?;
        if self.schema_version.0 == 0
            || self.peer != self.grant.peer
            || self.profile.0.trim().is_empty()
            || self.authority_epoch.0 == 0
            || self.minimum_fence.0 == 0
            || !self.grant.roles.contains(&p::FederatedPeerRole::Executor)
            || self.enabled_backends.is_empty()
            || self.enabled_backends.contains(&p::BackendKind::Remote)
        {
            return Err(p::Error("executor admission profile is incomplete".into()));
        }
        Ok(())
    }
}

pub struct GuardedRemoteExecutor {
    profile: ExecutorAdmissionProfile,
    driver: Arc<dyn RemoteInnerDriver>,
    clock: Arc<dyn RemoteClock>,
    ledger: Arc<dyn ExecutorReplayLedger>,
    credentials: Arc<dyn ExecutorCredentialResolver>,
}

impl GuardedRemoteExecutor {
    pub fn new(
        profile: ExecutorAdmissionProfile,
        driver: Arc<dyn RemoteInnerDriver>,
        clock: Arc<dyn RemoteClock>,
    ) -> p::Result<Self> {
        profile.validate()?;
        Ok(Self {
            profile,
            driver,
            clock,
            ledger: Arc::new(InMemoryExecutorLedger::default()),
            credentials: Arc::new(RejectingExecutorCredentialResolver),
        })
    }

    pub fn with_ledger(mut self, ledger: Arc<dyn ExecutorReplayLedger>) -> Self {
        self.ledger = ledger;
        self
    }

    pub fn with_credentials(mut self, credentials: Arc<dyn ExecutorCredentialResolver>) -> Self {
        self.credentials = credentials;
        self
    }

    pub fn receipt(
        &self,
        receipt: &p::RemoteDriverReceiptRef,
    ) -> p::Result<p::RemoteDriverReceipt> {
        self.ledger
            .receipt(receipt)?
            .ok_or_else(|| p::Error("remote driver receipt is not available".into()))
    }
}

impl RemoteExecutorDriver for GuardedRemoteExecutor {
    fn admit(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
    ) -> p::Result<p::RemoteAdmission> {
        plan.validate()?;
        lease.validate()?;
        let grant_ref = self.profile.grant.reference()?;
        if plan.executor != self.profile.peer
            || plan.peer_grant != grant_ref
            || plan.grant_version != self.profile.grant.grant_version
            || plan.authority_epoch != self.profile.authority_epoch
            || plan.executor_profile != self.profile.profile
            || lease.executor != plan.executor
            || lease.peer_grant != plan.peer_grant
            || lease.grant_version != plan.grant_version
            || lease.authority_epoch != plan.authority_epoch
            || lease.fence.0 < self.profile.minimum_fence.0
            || lease.placement != plan.reference()?
            || lease.state != p::RemoteLeaseState::Acquired
            || lease.expires_at <= self.clock.now_ms()
            || !self
                .profile
                .enabled_backends
                .contains(&plan.operation.backend)
            || !self.profile.grant.scopes.contains(&plan.operation.scope)
            || !self
                .profile
                .grant
                .capabilities
                .contains(&plan.operation.capability)
        {
            return Err(p::Error(
                "executor admission rejected the remote plan".into(),
            ));
        }
        Ok(p::RemoteAdmission {
            schema_version: p::M4_SCHEMA_VERSION,
            admission: lease.dispatch.clone(),
            plan: plan.clone(),
            lease: lease.clone(),
        })
    }

    fn execute(&self, admission: p::RemoteAdmission) -> p::Result<p::RemoteDriverReceipt> {
        admission.validate()?;
        let credential = admission
            .plan
            .operation
            .credential_slot
            .as_ref()
            .map(|slot| {
                self.credentials.resolve(
                    &admission.plan.executor,
                    &admission.plan.operation.scope,
                    slot,
                )
            })
            .transpose()?;
        let semantics = p::canonical_digest(&(&admission.plan, &admission.lease))?;
        match self.ledger.begin(
            &admission.lease.dispatch,
            &admission.lease.lease,
            &semantics,
        )? {
            ExecutorLedgerDecision::Completed(receipt) => return Ok(*receipt),
            ExecutorLedgerDecision::Pending => {
                return Err(p::Error(
                    "accepted dispatch has no terminal receipt; probe is required".into(),
                ));
            }
            ExecutorLedgerDecision::New => {}
        }
        let outcome = self
            .driver
            .execute(&admission.plan.operation, credential.as_ref())?;
        if outcome.observations.is_empty() {
            return Err(p::Error(
                "remote driver returned no bounded observation".into(),
            ));
        }
        let receipt = p::RemoteDriverReceipt {
            schema_version: p::M4_SCHEMA_VERSION,
            receipt: p::RemoteDriverReceiptRef(format!(
                "driver-receipt:{}",
                admission.lease.dispatch.0
            )),
            lease: admission.lease.lease.clone(),
            dispatch: admission.lease.dispatch.clone(),
            intent: admission.lease.intent.clone(),
            plan_digest: admission.lease.plan_digest.clone(),
            operation_digest: admission.plan.operation.digest.clone(),
            executor: admission.plan.executor,
            authority_epoch: admission.lease.authority_epoch,
            fence: admission.lease.fence,
            outcome: outcome.outcome,
            result_digest: outcome.result_digest,
            observations: outcome.observations,
            observed_at: self.clock.now_ms(),
        };
        receipt.validate()?;
        self.ledger
            .complete(&admission.lease.dispatch, &semantics, receipt.clone())?;
        Ok(receipt)
    }

    fn probe(&self, lease: &p::RemoteExecutionLeaseRef) -> p::Result<p::RemoteProbeResult> {
        let receipt = self.ledger.receipt_for_lease(lease)?;
        let pending = self.ledger.pending(lease)?;
        Ok(p::RemoteProbeResult {
            schema_version: p::M4_SCHEMA_VERSION,
            lease: lease.clone(),
            outcome: match receipt.as_ref().map(|receipt| receipt.outcome) {
                Some(p::RemoteReceiptOutcome::Completed) => p::RemoteProbeOutcome::Completed,
                Some(p::RemoteReceiptOutcome::Failed | p::RemoteReceiptOutcome::Cancelled) => {
                    p::RemoteProbeOutcome::Failed
                }
                Some(p::RemoteReceiptOutcome::Unknown) => p::RemoteProbeOutcome::Unknown,
                None if pending => p::RemoteProbeOutcome::Unknown,
                None => p::RemoteProbeOutcome::NotDispatched,
            },
            receipt: receipt.map(|receipt| receipt.receipt),
            evidence: Vec::new(),
        })
    }

    fn cancel(&self, lease: &p::RemoteExecutionLeaseRef) -> p::Result<p::RemoteCancelResult> {
        let receipt = self.ledger.receipt_for_lease(lease)?;
        let pending = self.ledger.pending(lease)?;
        Ok(p::RemoteCancelResult {
            schema_version: p::M4_SCHEMA_VERSION,
            lease: lease.clone(),
            outcome: if receipt.is_some() {
                p::RemoteCancelOutcome::AlreadyCompleted
            } else if pending {
                p::RemoteCancelOutcome::Unknown
            } else {
                p::RemoteCancelOutcome::NotDispatched
            },
            evidence: receipt
                .map(|receipt| receipt.observations)
                .unwrap_or_default(),
        })
    }
}

impl RemoteReceiptRepository for GuardedRemoteExecutor {
    fn fetch_receipt(
        &self,
        receipt: &p::RemoteDriverReceiptRef,
    ) -> p::Result<p::RemoteDriverReceipt> {
        self.receipt(receipt)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct FixedClock(i64);

    impl RemoteClock for FixedClock {
        fn now_ms(&self) -> p::Timestamp {
            self.0
        }
    }

    #[derive(Default)]
    struct CountingTransport(AtomicUsize);

    impl RemoteTransport for CountingTransport {
        fn dispatch(
            &self,
            _plan: &p::RemotePlacementPlan,
            lease: &p::RemoteExecutionLease,
        ) -> p::Result<p::RemoteDispatchAcceptance> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(p::RemoteDispatchAcceptance {
                schema_version: p::M4_SCHEMA_VERSION,
                dispatch: lease.dispatch.clone(),
                lease: lease.lease.clone(),
                accepted: p::RequiredTrue,
                receipt: p::RemoteDriverReceiptRef("receipt:1".into()),
                authority_epoch: lease.authority_epoch,
                fence: lease.fence,
            })
        }

        fn probe(&self, request: p::RemoteProbeRequest) -> p::Result<p::RemoteProbeResult> {
            Ok(p::RemoteProbeResult {
                schema_version: p::M4_SCHEMA_VERSION,
                lease: request.lease,
                outcome: p::RemoteProbeOutcome::Unknown,
                receipt: None,
                evidence: Vec::new(),
            })
        }

        fn cancel(&self, request: p::RemoteCancelRequest) -> p::Result<p::RemoteCancelResult> {
            Ok(p::RemoteCancelResult {
                schema_version: p::M4_SCHEMA_VERSION,
                lease: request.lease,
                outcome: p::RemoteCancelOutcome::Unknown,
                evidence: Vec::new(),
            })
        }
    }

    #[test]
    fn missing_lease_stops_before_transport() {
        let transport = Arc::new(CountingTransport::default());
        let backend = RemoteExecutorBackend::new(
            transport.clone(),
            Arc::new(InMemoryRemoteLeaseBindings::default()),
            Arc::new(FixedClock(10)),
            OutputBudget::strict(1024),
            p::DurationMs(1000),
        )
        .expect("remote backend");
        let mut operation = p::RemoteOperation {
            schema_version: p::M4_SCHEMA_VERSION,
            backend: p::BackendKind::File,
            parameters: p::ActionParameters::File {
                operation: p::FileOperation::Write,
                path: "fixture-state.txt".into(),
                content: Some(b"once".to_vec()),
            },
            capability: p::CapabilityRef("file.write".into()),
            scope: p::Scope("workspace".into()),
            action_type: p::ActionType::Write,
            expected_effect: p::ExpectedEffect::Outward,
            rollback_boundary: p::RollbackBoundary("fixture-state-version".into()),
            credential_slot: None,
            digest: p::SchemaDigest(String::new()),
        };
        operation.refresh_digest().expect("operation digest");
        let mut placement = p::RemotePlacementPlan {
            schema_version: p::M4_SCHEMA_VERSION,
            executor: p::FederatedPeerRef("peer:executor".into()),
            peer_grant: p::FederatedPeerGrantRef("grant:executor".into()),
            grant_version: p::PeerGrantVersion(1),
            authority_epoch: p::AuthorityEpoch(1),
            executor_profile: p::ExecutorProfileRef("profile:narrow".into()),
            operation: operation.clone(),
            digest: p::SchemaDigest(String::new()),
        };
        placement.refresh_digest().expect("placement digest");
        let intent = p::ActionIntent {
            schema_version: p::M4_SCHEMA_VERSION,
            intent_id: p::ActionId("action:1".into()),
            source: p::Source::UserTurn,
            goal: p::GoalRef("goal:1".into()),
            backend_hint: p::BackendKind::Remote,
            capability_ref: operation.capability.clone(),
            action_type: operation.action_type,
            scope: operation.scope.clone(),
            risk_hint: p::Risk::High,
            expected_effect: operation.expected_effect,
            rollback_expectation: operation.rollback_boundary.clone(),
            parameters: p::ActionParameters::Remote(Box::new(p::RemoteActionSpec {
                schema_version: p::M4_SCHEMA_VERSION,
                placement,
            })),
            requested_permissions: vec![p::PermissionRef("remote.execute".into())],
            requested_at: 1,
            estimated_output_bytes: 100,
            estimated_duration: p::DurationMs(100),
        };
        let plan = backend
            .plan(&intent)
            .expect("plan")
            .with_approval(p::ApprovalId("approval:1".into()));
        let error = backend
            .execute(plan, &EventSink::default(), CancelToken::default())
            .expect_err("missing lease must fail");
        assert!(error.0.contains("one-shot lease"));
        assert_eq!(transport.0.load(Ordering::SeqCst), 0);
    }
}
