//! M4 single-authority federation orchestration.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use forme_approval::{
    ApprovalAuthorization, ApprovalBroker, ApprovalGrant, ApprovalTicket, GrantScope,
    InMemoryApprovalBroker,
};
use forme_cognition::{CompetenceGate, CompetenceInputs, InterventionLevel};
use forme_coordination::{AuthorizedFederatedPlacementSelector, FederatedPlacementRequest};
use forme_eval::{RemoteAuthorityVerifier, RemoteGroundTruth};
use forme_execution::{
    ActionBackend, ActionStatus, CancelToken, EventSink, InMemoryRemoteLeaseBindings, OutputBudget,
    RemoteExecutorBackend, RemoteReceiptSource, RemoteTransport, SystemRemoteClock,
};
use forme_policy::{DefaultPolicyEngine, EnvelopeDecision, PolicyEngine};
use forme_protocol as p;
use forme_store::{
    EventStore, EvolutionProjection, FederationEventStore, FederationProjection,
    FederationRuntimeLedger, RemoteDispatchLedger, SqliteEventStore,
};

use crate::{approval_request, now_ms, protocol_intervention, GovernanceConfig, HarnessConfig};

const FEDERATION_AGGREGATE: &str = "federation";
const MAX_PEER_GRANT_TTL_MS: i64 = 90 * 24 * 60 * 60 * 1_000;
const MAX_EXECUTOR_HEALTH_AGE_MS: u64 = 60_000;

pub trait RemoteGroundTruthSource: Send + Sync {
    fn observe(
        &self,
        plan: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
        receipt: &p::RemoteDriverReceipt,
    ) -> p::Result<Option<RemoteGroundTruth>>;
}

/// Additive Harness-internal guard for domain state that must be rechecked at
/// the existing M4 dispatch choke point. It cannot authorize or execute an
/// action; any error stops before lease acquisition and network I/O.
pub trait RemoteDispatchGuard: Send + Sync {
    fn recheck(
        &self,
        run: &p::RunId,
        placement: &p::RemotePlacementPlan,
        now: p::Timestamp,
    ) -> p::Result<()>;
}

/// Additive observer invoked only after the M4 authority verifier has matched
/// the remote receipt to independent ground truth, while the one-shot lease is
/// still acquired. Domain facts appended here remain authority-owned.
pub trait RemoteCompletionObserver: Send + Sync {
    fn record_verified(
        &self,
        run: &p::RunId,
        placement: &p::RemotePlacementPlan,
        lease: &p::RemoteExecutionLease,
        driver: &p::RemoteDriverReceipt,
        receipt: &p::RemoteExecutionReceipt,
    ) -> p::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FederationControlResult {
    PeerMutation(p::ExpectedAppend),
    ApprovalResolved {
        run: p::RunId,
        terminal: bool,
    },
    Cancelled {
        lease: p::RemoteExecutionLeaseRef,
        terminal: bool,
    },
    RetentionRequested(p::RetentionRequestRef),
}

pub trait FederationGatewayControl: Send + Sync {
    fn federation_snapshot(&self, scope: p::Scope) -> p::Result<p::FederationSnapshot>;
    fn federated_peer(
        &self,
        peer: &p::FederatedPeerRef,
    ) -> p::Result<Option<p::FederatedPeerState>>;
    fn register_federated_peer(
        &self,
        run: p::RunId,
        grant: p::FederatedPeerGrant,
        previous: Option<p::FederatedPeerGrantRef>,
        expected: p::FederationAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend>;
    fn revoke_federated_peer(
        &self,
        run: p::RunId,
        peer: p::FederatedPeerRef,
        grant: p::FederatedPeerGrantRef,
        in_flight: p::InFlightDisposition,
        expected: p::FederationAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend>;
    fn apply_federated_owner_command(
        &self,
        envelope: p::FederatedControlEnvelope,
        command: p::FederatedOwnerCommand,
        now: p::Timestamp,
    ) -> p::Result<FederationControlResult>;
    fn federated_lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteExecutionLease>>;
    fn federated_retention_state(
        &self,
        request: &p::RetentionRequestRef,
    ) -> p::Result<Option<RetentionState>>;
    fn recover_federated_remote_action(&self, run: &p::RunId) -> p::Result<RemoteRecoveryResult>;
    fn acknowledge_federated_replication(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        batch: &p::ReplicationBatch,
        ack: p::ReplicationAck,
        expected: p::FederationAggregateVersion,
        now: p::Timestamp,
    ) -> p::Result<p::ExpectedAppend>;
    fn accept_federated_retention_receipt(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        receipt: p::FederatedRetentionReceipt,
        now: p::Timestamp,
    ) -> p::Result<()>;
    fn accept_federated_device_signal(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        signal: p::FederatedDeviceSignal,
        now: p::Timestamp,
    ) -> p::Result<bool>;
}

pub trait FederationActionGateway: Send + Sync {
    fn submit_federated_remote_action(
        &self,
        run: p::RunId,
        request: p::RunRequest,
        intent: p::ActionIntent,
    ) -> p::Result<RemoteActionSubmission>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteActionSubmission {
    pub run: p::RunId,
    pub approval: p::ApprovalId,
    pub plan_digest: p::PlanDigest,
    pub federation_snapshot: p::FederationSnapshotRef,
    pub placement_decision: p::FederatedPlacementDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRecoveryResult {
    pub run: p::RunId,
    pub terminal: bool,
    pub outcome: p::RemoteReceiptOutcome,
    pub receipt: Option<p::RemoteExecutionReceipt>,
}

pub type RetentionState = p::FederatedRetentionState;

struct PendingRemoteAction {
    request: p::RunRequest,
    intent: p::ActionIntent,
    governance: GovernanceConfig,
    toolset_ref: p::ToolsetRef,
    policy_version: p::Version,
    tool_schema_version: p::Version,
    plan: forme_execution::ExecutionPlan,
    placement: p::RemotePlacementPlan,
    snapshot: p::FederationSnapshot,
    approval: InMemoryApprovalBroker,
    ticket: ApprovalTicket,
    lease: Option<p::RemoteExecutionLease>,
    driver_receipt: Option<p::RemoteDriverReceiptRef>,
    terminal_receipt: Option<p::RemoteExecutionReceipt>,
    terminal: bool,
}

#[derive(Default)]
struct RemoteRuntimeBindings {
    backend: Option<Arc<dyn ActionBackend + Send + Sync>>,
    transport: Option<Arc<dyn RemoteTransport>>,
    receipts: Option<Arc<dyn RemoteReceiptSource>>,
    ground_truth: Option<Arc<dyn RemoteGroundTruthSource>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlacementCatalogEntry {
    candidate: p::FederatedExecutorCandidate,
    placement: p::RemotePlacementPlan,
}

pub struct FederatedHarnessRuntime {
    store: SqliteEventStore,
    leases: Arc<InMemoryRemoteLeaseBindings>,
    remote: Mutex<RemoteRuntimeBindings>,
    pending: Mutex<BTreeMap<p::RunId, PendingRemoteAction>>,
    placement_catalog: Mutex<BTreeMap<p::FederatedPeerRef, PlacementCatalogEntry>>,
    dispatch_guards: Mutex<BTreeMap<p::RunId, Arc<dyn RemoteDispatchGuard>>>,
    completion_observers: Mutex<BTreeMap<p::RunId, Arc<dyn RemoteCompletionObserver>>>,
    control_lock: Mutex<()>,
    event_sequence: std::sync::atomic::AtomicU64,
}

impl FederatedHarnessRuntime {
    pub fn new(store: SqliteEventStore) -> Self {
        Self {
            store,
            leases: Arc::new(InMemoryRemoteLeaseBindings::default()),
            remote: Mutex::new(RemoteRuntimeBindings::default()),
            pending: Mutex::new(BTreeMap::new()),
            placement_catalog: Mutex::new(BTreeMap::new()),
            dispatch_guards: Mutex::new(BTreeMap::new()),
            completion_observers: Mutex::new(BTreeMap::new()),
            control_lock: Mutex::new(()),
            event_sequence: std::sync::atomic::AtomicU64::new(1),
        }
    }

    pub fn configure_remote<T>(&self, transport: Arc<T>) -> p::Result<()>
    where
        T: RemoteTransport + RemoteReceiptSource + 'static,
    {
        let backend = Arc::new(RemoteExecutorBackend::new(
            transport.clone(),
            self.leases.clone(),
            Arc::new(SystemRemoteClock),
            OutputBudget {
                schema_version: p::M4_SCHEMA_VERSION,
                max_bytes: 65_536,
                truncate: true,
            },
            p::DurationMs(30_000),
        )?);
        let mut remote = self
            .remote
            .lock()
            .map_err(|_| p::Error("federation remote runtime is unavailable".into()))?;
        remote.backend = Some(backend);
        remote.transport = Some(transport.clone());
        remote.receipts = Some(transport);
        Ok(())
    }

    pub fn configure_ground_truth(
        &self,
        source: Arc<dyn RemoteGroundTruthSource>,
    ) -> p::Result<()> {
        self.remote
            .lock()
            .map_err(|_| p::Error("federation remote runtime is unavailable".into()))?
            .ground_truth = Some(source);
        Ok(())
    }

    pub fn configure_remote_governance(
        &self,
        run: p::RunId,
        guard: Arc<dyn RemoteDispatchGuard>,
        observer: Arc<dyn RemoteCompletionObserver>,
    ) -> p::Result<()> {
        if run.0.trim().is_empty() {
            return Err(p::Error("remote governance run is empty".into()));
        }
        let mut guards = self
            .dispatch_guards
            .lock()
            .map_err(|_| p::Error("remote dispatch guards are unavailable".into()))?;
        let mut observers = self
            .completion_observers
            .lock()
            .map_err(|_| p::Error("remote completion observers are unavailable".into()))?;
        if guards.contains_key(&run) || observers.contains_key(&run) {
            return Err(p::Error(
                "remote governance is already configured for this run".into(),
            ));
        }
        guards.insert(run.clone(), guard);
        observers.insert(run, observer);
        Ok(())
    }

    pub fn snapshot(&self, scope: p::Scope) -> p::Result<p::FederationSnapshot> {
        FederationProjection::snapshot(&self.store, scope)
    }

    pub fn peer(&self, peer: &p::FederatedPeerRef) -> p::Result<Option<p::FederatedPeerState>> {
        FederationProjection::peer(&self.store, peer)
    }

    pub fn lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
    ) -> p::Result<Option<p::RemoteExecutionLease>> {
        FederationProjection::lease(&self.store, lease)
    }

    pub fn configure_executor_candidate(
        &self,
        candidate: p::FederatedExecutorCandidate,
        placement: p::RemotePlacementPlan,
    ) -> p::Result<()> {
        candidate.validate()?;
        placement.validate()?;
        let state = self
            .peer(&candidate.peer)?
            .ok_or_else(|| p::Error("placement candidate peer is not registered".into()))?;
        if state.revoked
            || candidate.peer != placement.executor
            || candidate.grant != placement.peer_grant
            || candidate.profile != placement.executor_profile
            || candidate.scope != placement.operation.scope
            || candidate.capability != placement.operation.capability
            || candidate.expires_at > state.grant.expires_at
            || candidate.grant != state.grant.reference()?
        {
            return Err(p::Error(
                "placement candidate is not bound to its authority grant and plan".into(),
            ));
        }
        let mut catalog = self
            .placement_catalog
            .lock()
            .map_err(|_| p::Error("federated placement catalog is unavailable".into()))?;
        let entry = PlacementCatalogEntry {
            candidate: candidate.clone(),
            placement,
        };
        match catalog.get(&candidate.peer) {
            Some(existing) if existing == &entry => Ok(()),
            Some(_) => Err(p::Error(
                "placement candidate changed without an authority refresh".into(),
            )),
            None => {
                catalog.insert(candidate.peer, entry);
                Ok(())
            }
        }
    }

    fn select_remote_placement(
        &self,
        run: &p::RunId,
        snapshot: &p::FederationSnapshot,
        requested: &p::RemotePlacementPlan,
        now: p::Timestamp,
    ) -> p::Result<p::FederatedPlacementDecision> {
        let catalog = self
            .placement_catalog
            .lock()
            .map_err(|_| p::Error("federated placement catalog is unavailable".into()))?;
        let entries = catalog.values().cloned().collect::<Vec<_>>();
        drop(catalog);
        if entries.is_empty() {
            return Err(p::Error(
                "remote action has no authority placement candidates".into(),
            ));
        }
        let mut grants = BTreeMap::new();
        let mut candidates = Vec::new();
        let mut placements = BTreeMap::new();
        for entry in entries {
            if let Some(state) = self.peer(&entry.candidate.peer)? {
                grants.insert(state.grant.reference()?, state.grant);
            }
            placements.insert(entry.candidate.peer.clone(), entry.placement.reference()?);
            candidates.push(entry.candidate);
        }
        let decision = AuthorizedFederatedPlacementSelector.select(FederatedPlacementRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            decision: p::PlacementDecisionRef(format!("placement-decision:{}", run.0)),
            snapshot: snapshot.clone(),
            scope: requested.operation.scope.clone(),
            capability: requested.operation.capability.clone(),
            evaluated_at: now,
            maximum_health_age: p::DurationMs(MAX_EXECUTOR_HEALTH_AGE_MS),
            compatible_profiles: BTreeSet::from([requested.executor_profile.clone()]),
            grants,
            candidates,
            placements,
        })?;
        if decision.chosen.as_ref() != Some(&requested.executor)
            || decision.placement.as_ref() != Some(&requested.reference()?)
        {
            return Err(p::Error(
                "requested remote placement was not selected by authority governance".into(),
            ));
        }
        Ok(decision)
    }

    fn current_evolution_snapshot(
        &self,
        scope: p::Scope,
    ) -> p::Result<Option<p::EvolutionSnapshotRef>> {
        match EvolutionProjection::snapshot(&self.store, scope) {
            Ok(snapshot) => Ok(Some(snapshot.snapshot)),
            Err(error)
                if error.0 == "no active strategy projection exists for the requested scope" =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub fn register_peer(
        &self,
        run: p::RunId,
        grant: p::FederatedPeerGrant,
        previous: Option<p::FederatedPeerGrantRef>,
        expected: p::FederationAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        let _control = self
            .control_lock
            .lock()
            .map_err(|_| p::Error("federation owner control is unavailable".into()))?;
        grant.validate()?;
        expected.validate()?;
        let now = now_ms();
        if grant.owner != owner
            || grant.created_by.0.trim().is_empty()
            || grant.expires_at <= now
            || grant.expires_at.saturating_sub(now) > MAX_PEER_GRANT_TTL_MS
            || grant.authority_epoch.0 == 0
        {
            return Err(p::Error(
                "peer registration is not a bounded owner-provisioned grant".into(),
            ));
        }
        self.require_expected(&expected)?;
        let committed = expected.next()?;
        self.append_owner_run_start(&run, &owner, Some(&grant))?;
        let event = self.event(
            run.clone(),
            p::EventPayload::FederatedPeerRegistered(p::FederatedPeerRegisteredPayload {
                grant,
                previous,
                committed_version: committed,
            }),
            owner_provenance(),
        );
        let outcome =
            self.store
                .append_federation_expected(event, &federation_aggregate(), expected)?;
        if outcome.status != p::ExpectedAppendStatus::Applied
            && outcome.status != p::ExpectedAppendStatus::Duplicate
        {
            return Err(p::Error("peer registration lost its authority CAS".into()));
        }
        self.append_run_complete(&run, p::Source::OwnerControl)?;
        Ok(outcome)
    }

    pub fn revoke_peer(
        &self,
        run: p::RunId,
        peer: p::FederatedPeerRef,
        grant: p::FederatedPeerGrantRef,
        in_flight: p::InFlightDisposition,
        expected: p::FederationAggregateVersion,
        owner: p::VerifiedPrincipal,
    ) -> p::Result<p::ExpectedAppend> {
        let _control = self
            .control_lock
            .lock()
            .map_err(|_| p::Error("federation owner control is unavailable".into()))?;
        expected.validate()?;
        self.require_expected(&expected)?;
        let state = self
            .peer(&peer)?
            .ok_or_else(|| p::Error("cannot revoke an unknown federated peer".into()))?;
        if state.revoked || state.grant.owner != owner || state.grant.reference()? != grant {
            return Err(p::Error(
                "peer revocation lineage is not owner-authorized".into(),
            ));
        }
        self.append_owner_run_start(&run, &owner, Some(&state.grant))?;
        let committed = expected.next()?;
        let current_epoch = self
            .snapshot(state.grant.scopes[0].clone())?
            .authority_epoch;
        let event = self.event(
            run.clone(),
            p::EventPayload::FederatedPeerRevoked(p::FederatedPeerRevokedPayload {
                peer,
                revoked_grant: grant,
                new_authority_epoch: current_epoch.next()?,
                in_flight,
                committed_version: committed,
            }),
            owner_provenance(),
        );
        let outcome =
            self.store
                .append_federation_expected(event, &federation_aggregate(), expected)?;
        if outcome.status != p::ExpectedAppendStatus::Applied
            && outcome.status != p::ExpectedAppendStatus::Duplicate
        {
            return Err(p::Error("peer revocation lost its authority CAS".into()));
        }
        self.append_run_complete(&run, p::Source::OwnerControl)?;
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_remote_action(
        &self,
        run: p::RunId,
        request: p::RunRequest,
        intent: p::ActionIntent,
        governance: &GovernanceConfig,
        config: &HarnessConfig,
        competence: &dyn CompetenceGate,
        competence_inputs: &CompetenceInputs,
    ) -> p::Result<RemoteActionSubmission> {
        if request.source == p::Source::OwnerControl
            || request.source == p::Source::Replay
            || request.source == p::Source::Simulation
            || intent.backend_hint != p::BackendKind::Remote
            || intent.source != request.source
            || run.0.trim().is_empty()
        {
            return Err(p::Error(
                "remote action submission has an invalid authority source".into(),
            ));
        }
        let p::ActionParameters::Remote(spec) = &intent.parameters else {
            return Err(p::Error("remote action submission has no placement".into()));
        };
        spec.validate()?;
        let placement = spec.placement.clone();
        if placement.operation.capability != intent.capability_ref
            || placement.operation.scope != intent.scope
            || placement.operation.action_type != intent.action_type
            || placement.operation.expected_effect != intent.expected_effect
            || placement.operation.rollback_boundary != intent.rollback_expectation
        {
            return Err(p::Error(
                "remote placement does not match its action intent".into(),
            ));
        }
        let snapshot = self.snapshot(intent.scope.clone())?;
        let peer = self
            .peer(&placement.executor)?
            .ok_or_else(|| p::Error("remote executor is not registered".into()))?;
        validate_remote_peer(&peer, &snapshot, &placement, &intent, now_ms())?;
        let backend = self.remote_backend()?;
        let plan = backend.plan(&intent)?;
        plan.validate_digest()?;
        let decision = self.select_remote_placement(&run, &snapshot, &placement, now_ms())?;
        let current_evolution = self.current_evolution_snapshot(intent.scope.clone())?;
        let snapshot_ref = p::FederationSnapshotRef(snapshot.digest.0.clone());
        let handoff = self.store.federated_handoff_for_run(&run)?;
        let evolution_snapshot = match handoff {
            Some(ref handoff)
                if handoff.target == placement.executor
                    && handoff.placement == placement.reference()?
                    && handoff.federation_snapshot == snapshot_ref
                    && current_evolution
                        .as_ref()
                        .is_none_or(|snapshot| snapshot == &handoff.evolution_snapshot)
                    && request.budget.as_ref() == Some(&handoff.budget) =>
            {
                Some(handoff.evolution_snapshot.clone())
            }
            Some(_) => {
                return Err(p::Error(
                    "handoff segment no longer matches its pinned snapshots and budget".into(),
                ))
            }
            None => current_evolution,
        };
        let engine = DefaultPolicyEngine;
        let context = governance.policy_context(
            p::SessionId(request.session.0.clone()),
            config.toolset_ref.clone(),
            &engine,
            governance.envelope.as_ref(),
        );
        let evaluation = engine.evaluate_detailed(&context, &intent);
        self.append(
            run.clone(),
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: request.source,
                session_ref: p::SessionId(request.session.0.clone()),
                input_ref: p::InputRef(format!("federated-input:{}", run.0)),
                idempotency_key: request.idempotency_key.clone(),
            }),
            internal_provenance(request.source),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::SessionBound(p::SessionBoundPayload {
                policy_profile: config.policy_profile.clone(),
                model_profile: config.model_profile.clone(),
                toolset_ref: config.toolset_ref.clone(),
                workspace: config.workspace.clone(),
                effect_mode: None,
                evolution_snapshot: evolution_snapshot.clone(),
                federation_snapshot: Some(snapshot_ref.clone()),
            }),
            internal_provenance(request.source),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::ResourcePlanned(p::ResourcePlannedPayload {
                plan: p::ResourcePlanRef(format!("federated-placement:{}", decision.digest.0)),
            }),
            internal_provenance(p::Source::Internal),
        )?;
        let mut failures = decision
            .candidates
            .iter()
            .flat_map(|candidate| candidate.candidate.failure_evidence.clone())
            .collect::<Vec<_>>();
        failures.sort();
        failures.dedup();
        let workspace_snapshot = p::AgentWorkspaceSnapshotRef(
            p::canonical_digest(&(
                run.clone(),
                config.workspace.clone(),
                decision.digest.clone(),
            ))?
            .0,
        );
        self.append(
            run.clone(),
            p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                trace_ref: p::DecisionTraceRef(format!(
                    "federated-placement:{}",
                    decision.digest.0
                )),
                refs: p::DecisionRefs {
                    map: None,
                    user: None,
                    agent_self: None,
                    trust: None,
                    failure: failures,
                },
                rationale: p::Rationale(format!(
                    "authority-filtered placement {} selected {}",
                    decision.decision.0, placement.executor.0
                )),
                workspace_snapshot,
                resource_graph_snapshot: None,
                evolution_snapshot,
                federation_snapshot: Some(snapshot_ref.clone()),
            }),
            internal_provenance(p::Source::Internal),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::ToolCallProposed(p::ToolCallProposedPayload {
                call_id: p::ToolCallId(format!("remote-call:{}", intent.intent_id.0)),
                tool: p::ToolRef("remote-executor".into()),
                args: serde_json::to_value(&intent.parameters)
                    .map_err(|_| p::Error("remote action parameters cannot be audited".into()))?,
            }),
            internal_provenance(request.source),
        )?;
        if evaluation.decision == p::PolicyDecision::Deny {
            self.append(
                run,
                evaluation.event_payload(),
                internal_provenance(request.source),
            )?;
            return Err(p::Error("remote action is denied by policy".into()));
        }
        match (&governance.delegation, governance.envelope.as_ref()) {
            (Some(grant), Some(envelope)) => {
                match engine.enforce_envelope(grant, envelope, &intent) {
                    EnvelopeDecision::Within | EnvelopeDecision::NeedsApproval => {}
                    EnvelopeDecision::OutOfScope => {
                        return Err(p::Error(
                            "remote action is outside its autonomy envelope".into(),
                        ))
                    }
                }
            }
            _ => {
                return Err(p::Error(
                    "remote action has no delegation and autonomy envelope".into(),
                ))
            }
        }
        self.append(
            run.clone(),
            p::EventPayload::ToolPolicyEvaluated(p::ToolPolicyEvaluatedPayload {
                decision: p::PolicyDecision::Ask,
                rule_source: p::RuleSourceRef("federation-remote-action-floor".into()),
                reason: p::ReasonRef(
                    "remote real-world action requires plan-bound owner approval".into(),
                ),
            }),
            internal_provenance(request.source),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::ActionPlanned(p::ActionPlannedPayload {
                intent_id: intent.intent_id.clone(),
                plan_digest: plan.digest.clone(),
                backend: p::BackendKind::Remote,
                expected_effect: intent.expected_effect,
                source: intent.source,
                scope: intent.scope.clone(),
                approval_ref: None,
                remote_placement: Some(placement.reference()?),
            }),
            internal_provenance(request.source),
        )?;
        let ceiling = competence.ceiling(intent.scope.clone(), intent.risk_hint, competence_inputs);
        if ceiling < InterventionLevel::L3ActWithApproval
            || (intent.risk_hint == p::Risk::High && ceiling < InterventionLevel::L5HighImpact)
        {
            return Err(p::Error(
                "competence evidence does not permit remote execution".into(),
            ));
        }
        let approval_id = p::ApprovalId(format!("approval:remote:{}", run.0));
        let approval = InMemoryApprovalBroker::default();
        let approval_request = approval_request(
            approval_id.clone(),
            &p::SessionId(request.session.0.clone()),
            &intent,
            &plan,
            config,
        )?;
        let ticket = approval.request(approval_request)?;
        for payload in approval.take_events() {
            self.append(run.clone(), payload, internal_provenance(request.source))?;
        }
        self.append(
            run.clone(),
            p::EventPayload::RunWaiting(p::RunWaitingPayload {
                wait_reason: p::WaitReason("remote_approval".into()),
                resume_ref: p::ResumeRef(format!("approval:{}", approval_id.0)),
            }),
            internal_provenance(request.source),
        )?;
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| p::Error("remote action state is unavailable".into()))?;
        if pending.contains_key(&run) {
            return Err(p::Error("remote run already exists".into()));
        }
        pending.insert(
            run.clone(),
            PendingRemoteAction {
                request,
                intent,
                governance: governance.clone(),
                toolset_ref: config.toolset_ref.clone(),
                policy_version: config.policy_version,
                tool_schema_version: config.tool_schema_version,
                plan: plan.clone(),
                placement,
                snapshot,
                approval,
                ticket,
                lease: None,
                driver_receipt: None,
                terminal_receipt: None,
                terminal: false,
            },
        );
        Ok(RemoteActionSubmission {
            run,
            approval: approval_id,
            plan_digest: plan.digest,
            federation_snapshot: snapshot_ref,
            placement_decision: decision,
        })
    }

    pub fn apply_owner_command(
        &self,
        envelope: p::FederatedControlEnvelope,
        command: p::FederatedOwnerCommand,
        now: p::Timestamp,
    ) -> p::Result<FederationControlResult> {
        envelope.validate_command(&command, now)?;
        let peer = self
            .peer(&envelope.peer)?
            .ok_or_else(|| p::Error("owner control peer is not registered".into()))?;
        if peer.revoked
            || peer.grant.owner != envelope.owner
            || !peer
                .grant
                .roles
                .contains(&p::FederatedPeerRole::OwnerClient)
            || peer.grant.expires_at <= now
            || !self
                .snapshot(peer.grant.scopes[0].clone())?
                .grants
                .contains(&peer.grant.reference()?)
        {
            return Err(p::Error(
                "peer identity does not authorize an owner control channel".into(),
            ));
        }
        if !self.store.claim_control_nonce(
            &envelope.peer,
            &envelope.nonce,
            &envelope.command_digest,
        )? {
            return Err(p::Error("owner control nonce was already consumed".into()));
        }
        match command {
            p::FederatedOwnerCommand::Register(_) => Err(p::Error(
                "peer registration requires the local owner enrollment surface".into(),
            )),
            p::FederatedOwnerCommand::Revoke {
                peer,
                grant,
                in_flight,
            } => {
                let expected = self.store.federation_version(&federation_aggregate())?;
                let run = p::RunId(format!("owner-control:revoke:{}", envelope.nonce.0));
                self.revoke_peer(run, peer, grant, in_flight, expected, envelope.owner)
                    .map(FederationControlResult::PeerMutation)
            }
            p::FederatedOwnerCommand::ResolveApproval {
                approval,
                plan_digest,
                outcome,
            } => {
                let (run, terminal) = self.resolve_remote_approval(
                    approval,
                    plan_digest,
                    outcome,
                    envelope.owner,
                    envelope.nonce,
                    envelope.expires_at,
                    now,
                )?;
                Ok(FederationControlResult::ApprovalResolved { run, terminal })
            }
            p::FederatedOwnerCommand::Cancel { lease, reason } => {
                let terminal = self.cancel_remote(&lease, reason, now)?;
                Ok(FederationControlResult::Cancelled { lease, terminal })
            }
            p::FederatedOwnerCommand::RequestRetention(request) => {
                request.validate()?;
                let current_epoch = self.snapshot(peer.grant.scopes[0].clone())?.authority_epoch;
                let target = self
                    .peer(&request.peer)?
                    .ok_or_else(|| p::Error("retention target peer is not registered".into()))?;
                if target.grant.owner != envelope.owner
                    || !target.grant.scopes.contains(&request.scope)
                    || request.authority_epoch != current_epoch
                    || request.expires_at <= now
                {
                    return Err(p::Error(
                        "retention request is outside the owner control session".into(),
                    ));
                }
                let reference = request.request.clone();
                if !self.store.record_retention_request(&request)? {
                    return Err(p::Error("retention request was already recorded".into()));
                }
                let run = p::RunId(format!("owner-control:retention:{}", envelope.nonce.0));
                self.append_owner_run_start(&run, &envelope.owner, Some(&peer.grant))?;
                self.append(
                    run.clone(),
                    p::EventPayload::ResourcePlanned(p::ResourcePlannedPayload {
                        plan: p::ResourcePlanRef(format!(
                            "federated-retention:{}",
                            request.digest.0
                        )),
                    }),
                    owner_provenance(),
                )?;
                self.append(
                    run.clone(),
                    p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                        trace_ref: p::DecisionTraceRef(format!("retention-trace:{}", reference.0)),
                        refs: p::DecisionRefs {
                            map: None,
                            user: None,
                            agent_self: None,
                            trust: None,
                            failure: Vec::new(),
                        },
                        rationale: p::Rationale(
                            "owner requested bounded retention verification".into(),
                        ),
                        workspace_snapshot: p::AgentWorkspaceSnapshotRef(format!(
                            "retention-workspace:{}",
                            request.scope.0
                        )),
                        resource_graph_snapshot: None,
                        evolution_snapshot: None,
                        federation_snapshot: Some(p::FederationSnapshotRef(
                            self.snapshot(request.scope.clone())?.digest.0,
                        )),
                    }),
                    owner_provenance(),
                )?;
                self.append_run_complete(&run, p::Source::OwnerControl)?;
                Ok(FederationControlResult::RetentionRequested(reference))
            }
        }
    }

    pub fn recover_remote_action(
        &self,
        run: &p::RunId,
        ground_truth: Option<RemoteGroundTruth>,
    ) -> p::Result<RemoteRecoveryResult> {
        let pending_state = {
            let pending = self
                .pending
                .lock()
                .map_err(|_| p::Error("remote action state is unavailable".into()))?;
            pending.get(run).map(|pending| {
                (
                    pending.terminal,
                    pending.terminal_receipt.clone(),
                    pending.placement.clone(),
                    pending.lease.clone(),
                    pending.driver_receipt.clone(),
                    pending.request.source,
                    pending.intent.clone(),
                )
            })
        };
        let (plan, lease, driver_receipt, source, intent) = if let Some((
            terminal,
            terminal_receipt,
            plan,
            lease,
            driver_receipt,
            source,
            intent,
        )) = pending_state
        {
            if terminal {
                return Ok(RemoteRecoveryResult {
                    run: run.clone(),
                    terminal: true,
                    outcome: terminal_receipt
                        .as_ref()
                        .map(|receipt| receipt.outcome)
                        .unwrap_or(p::RemoteReceiptOutcome::Unknown),
                    receipt: terminal_receipt,
                });
            }
            (
                plan,
                lease.ok_or_else(|| p::Error("remote run has no acquired lease".into()))?,
                driver_receipt,
                source,
                intent,
            )
        } else {
            let recovery = self
                .store
                .remote_recovery(run)?
                .ok_or_else(|| p::Error("remote run has no durable recovery record".into()))?;
            let live = self
                .store
                .lease(&recovery.lease.lease)?
                .ok_or_else(|| p::Error("remote recovery lease is unavailable".into()))?;
            if live.state.terminal() {
                let events = self
                    .store
                    .read_run(run.clone())
                    .collect::<p::Result<Vec<_>>>()?;
                let outcome = events
                    .iter()
                    .rev()
                    .find_map(|event| match event.kind {
                        p::EventKind::ActionCompleted => Some(p::RemoteReceiptOutcome::Completed),
                        p::EventKind::ActionFailed => Some(p::RemoteReceiptOutcome::Failed),
                        p::EventKind::ActionCancelled => Some(p::RemoteReceiptOutcome::Cancelled),
                        _ => None,
                    })
                    .unwrap_or(p::RemoteReceiptOutcome::Unknown);
                return Ok(RemoteRecoveryResult {
                    run: run.clone(),
                    terminal: true,
                    outcome,
                    receipt: None,
                });
            }
            (
                recovery.placement,
                live,
                recovery.driver_receipt,
                recovery.source,
                recovery.intent,
            )
        };
        let claim = self
            .store
            .remote_dispatch_claim(&lease.dispatch)?
            .ok_or_else(|| p::Error("remote recovery has no dispatch claim".into()))?;
        if claim.status == p::RemoteDispatchClaimStatus::Reserved {
            let _control = self
                .control_lock
                .lock()
                .map_err(|_| p::Error("federation terminal control is unavailable".into()))?;
            let current = self
                .store
                .remote_dispatch_claim(&lease.dispatch)?
                .ok_or_else(|| p::Error("remote recovery dispatch claim disappeared".into()))?;
            if current.status == p::RemoteDispatchClaimStatus::Reserved {
                return self.finish_remote_not_dispatched(run, source, &intent, &lease);
            }
        }
        let remote = self.remote_bindings()?;
        let driver = if let Some(receipt) = driver_receipt {
            remote.receipts.receipt(p::RemoteReceiptRequest {
                schema_version: p::M4_SCHEMA_VERSION,
                receipt,
                lease: lease.lease.clone(),
                dispatch: lease.dispatch.clone(),
                authority_epoch: lease.authority_epoch,
                fence: lease.fence,
                nonce: p::Nonce(format!("receipt:{}:{}", run.0, now_ms())),
            })?
        } else {
            let probe = remote.transport.probe(p::RemoteProbeRequest {
                schema_version: p::M4_SCHEMA_VERSION,
                lease: lease.lease.clone(),
                dispatch: lease.dispatch.clone(),
                authority_epoch: lease.authority_epoch,
                fence: lease.fence,
                nonce: p::Nonce(format!("probe:{}:{}", run.0, now_ms())),
            })?;
            let receipt = probe.receipt.ok_or_else(|| {
                p::Error("remote probe has no receipt; outcome remains unknown".into())
            })?;
            remote.receipts.receipt(p::RemoteReceiptRequest {
                schema_version: p::M4_SCHEMA_VERSION,
                receipt,
                lease: lease.lease.clone(),
                dispatch: lease.dispatch.clone(),
                authority_epoch: lease.authority_epoch,
                fence: lease.fence,
                nonce: p::Nonce(format!("receipt:{}:{}", run.0, now_ms())),
            })?
        };
        let truth = match ground_truth {
            Some(truth) => truth,
            None => remote
                .ground_truth
                .as_ref()
                .ok_or_else(|| p::Error("remote outcome has no independent ground truth".into()))?
                .observe(&plan, &lease, &driver)?
                .ok_or_else(|| {
                    p::Error("remote outcome remains independently unverifiable".into())
                })?,
        };
        self.finish_remote(run, source, plan, lease, driver, truth)
    }

    pub fn retention_state(
        &self,
        request: &p::RetentionRequestRef,
    ) -> p::Result<Option<RetentionState>> {
        self.store.federated_retention_state(request)
    }

    pub fn acknowledge_replication(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        batch: &p::ReplicationBatch,
        ack: p::ReplicationAck,
        expected: p::FederationAggregateVersion,
        now: p::Timestamp,
    ) -> p::Result<p::ExpectedAppend> {
        batch.validate()?;
        ack.validate()?;
        let peer = self
            .peer(authenticated_peer)?
            .ok_or_else(|| p::Error("replication acknowledgement peer is not registered".into()))?;
        let grant = peer.grant.reference()?;
        let snapshot = self.snapshot(
            peer.grant
                .scopes
                .first()
                .cloned()
                .ok_or_else(|| p::Error("replication peer has no scope".into()))?,
        )?;
        if peer.revoked
            || peer.grant.expires_at <= now
            || !peer.grant.roles.contains(&p::FederatedPeerRole::Replica)
            || !snapshot.grants.contains(&grant)
            || snapshot.authority_epoch != batch.to.authority_epoch
            || *authenticated_peer != batch.peer
            || batch.peer != ack.peer
            || batch.batch != ack.batch
            || batch.aggregate != ack.aggregate
            || batch.to != ack.applied
            || batch.peer_grant != grant
        {
            return Err(p::Error(
                "authenticated replica acknowledgement is outside its active grant".into(),
            ));
        }
        let committed = expected.next()?;
        let event = self.event(
            ack.aggregate.clone(),
            p::EventPayload::ReplicationCheckpointAdvanced(
                p::ReplicationCheckpointAdvancedPayload {
                    peer: ack.peer.clone(),
                    aggregate: ack.aggregate.clone(),
                    from_stream_seq: batch.from.stream_seq,
                    to_stream_seq: batch.to.stream_seq,
                    batch_digest: batch.content_digest.clone(),
                    redaction: batch.redaction.clone(),
                    authority_epoch: batch.to.authority_epoch,
                    committed_version: committed,
                },
            ),
            internal_provenance(p::Source::Internal),
        );
        self.store.acknowledge_replication(event, ack, expected)
    }

    pub fn accept_retention_receipt(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        receipt: p::FederatedRetentionReceipt,
        now: p::Timestamp,
    ) -> p::Result<()> {
        receipt.validate()?;
        let peer = self
            .peer(authenticated_peer)?
            .ok_or_else(|| p::Error("retention receipt peer is not provisioned".into()))?;
        let state = self
            .store
            .federated_retention_state(&receipt.request)?
            .ok_or_else(|| p::Error("retention receipt has no authority request".into()))?;
        if *authenticated_peer != receipt.peer
            || !peer.grant.roles.contains(&p::FederatedPeerRole::Replica)
            || state.request.peer != receipt.peer
            || state.request.authority_epoch != receipt.authority_epoch
            || receipt.observed_at > state.request.expires_at
            || now > state.request.expires_at
        {
            return Err(p::Error(
                "authenticated retention receipt is outside its request lineage".into(),
            ));
        }
        self.store.accept_retention_receipt(&receipt)?;
        Ok(())
    }

    pub fn record_checkpoint(
        &self,
        source_run: &p::RunId,
        checkpoint: p::FederatedCheckpointArtifact,
    ) -> p::Result<()> {
        checkpoint.validate()?;
        self.validate_checkpoint_source(source_run, &checkpoint)?;
        self.store
            .record_federated_checkpoint(source_run, &checkpoint)?;
        Ok(())
    }

    fn validate_checkpoint_source(
        &self,
        source_run: &p::RunId,
        checkpoint: &p::FederatedCheckpointArtifact,
    ) -> p::Result<()> {
        let events = self
            .store
            .read_run(source_run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let bound = events.iter().find_map(|event| match &event.payload {
            p::EventPayload::SessionBound(payload) => Some(payload),
            _ => None,
        });
        if bound.is_none_or(|bound| {
            bound.evolution_snapshot.as_ref() != Some(&checkpoint.evolution_snapshot)
                || bound.federation_snapshot.as_ref() != Some(&checkpoint.federation_snapshot)
        }) {
            return Err(p::Error(
                "checkpoint source run is not bound to its recorded snapshots".into(),
            ));
        }
        let mut verification_sequence = 0_u64;
        for reference in &checkpoint.verification_events {
            let event = events
                .iter()
                .find(|event| &event.event_id == reference)
                .ok_or_else(|| {
                    p::Error("checkpoint verification event is absent from its source run".into())
                })?;
            match &event.payload {
                p::EventPayload::VerificationFinished(payload)
                    if payload.outcome == p::VerificationOutcome::Pass
                        && payload.against == checkpoint.done_contract =>
                {
                    verification_sequence = verification_sequence.max(event.stream_seq);
                }
                _ => {
                    return Err(p::Error(
                        "checkpoint verification is not a passing DoneContract result".into(),
                    ))
                }
            }
        }
        let durable_sequence = events
            .iter()
            .filter_map(|event| match &event.payload {
                p::EventPayload::MemoryNodeAppended(payload)
                    if payload.kind.0 == "checkpoint"
                        && payload.content_ref == checkpoint.checkpoint.artifact
                        && payload.scope == checkpoint.scope =>
                {
                    Some(event.stream_seq)
                }
                _ => None,
            })
            .max()
            .ok_or_else(|| {
                p::Error("checkpoint source run has no durable checkpoint memory event".into())
            })?;
        if durable_sequence <= verification_sequence
            || events.iter().any(|event| {
                event.stream_seq > durable_sequence && event.kind == p::EventKind::ActionStarted
            })
        {
            return Err(p::Error(
                "checkpoint was recorded before verification or during an active action".into(),
            ));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn plan_handoff(
        &self,
        checkpoint: &p::FederatedCheckpointArtifactRef,
        from_run: p::RunId,
        next_run: p::RunId,
        target: p::FederatedPeerRef,
        placement: p::RemotePlacementPlanRef,
        evolution_snapshot: p::EvolutionSnapshotRef,
        federation_snapshot: p::FederationSnapshotRef,
        budget: p::Budget,
    ) -> p::Result<p::FederatedHandoffPlan> {
        let (checkpoint_source, artifact) =
            self.store
                .federated_checkpoint_artifact(checkpoint)?
                .ok_or_else(|| p::Error("handoff checkpoint is not durable".into()))?;
        if checkpoint_source != from_run {
            return Err(p::Error(
                "handoff checkpoint does not belong to its source run".into(),
            ));
        }
        artifact.validate()?;
        self.validate_checkpoint_source(&from_run, &artifact)?;
        let current = self.snapshot(artifact.scope.clone())?;
        let current_evolution = self.current_evolution_snapshot(artifact.scope.clone())?;
        let target_state = self
            .peer(&target)?
            .ok_or_else(|| p::Error("handoff target is not registered".into()))?;
        if artifact.federation_snapshot == federation_snapshot
            || federation_snapshot.0 != current.digest.0
            || current_evolution
                .as_ref()
                .is_some_and(|snapshot| snapshot != &evolution_snapshot)
            || target_state.revoked
            || !target_state
                .grant
                .roles
                .contains(&p::FederatedPeerRole::Executor)
            || !target_state.grant.scopes.contains(&artifact.scope)
            || !current.grants.contains(&target_state.grant.reference()?)
        {
            return Err(p::Error(
                "handoff does not bind a new authorized segment snapshot".into(),
            ));
        }
        let mut handoff = p::FederatedHandoffPlan {
            schema_version: p::M4_SCHEMA_VERSION,
            checkpoint: checkpoint.clone(),
            from_run,
            next_run,
            target,
            placement,
            evolution_snapshot,
            federation_snapshot,
            budget,
            digest: p::SchemaDigest(String::new()),
        };
        handoff.refresh_digest()?;
        handoff.validate()?;
        if self.store.record_federated_handoff(&handoff)? {
            let target_ref = p::HandoffTargetRef(handoff.target.0.clone());
            self.append(
                handoff.from_run.clone(),
                p::EventPayload::HandoffRequested(p::HandoffRequestedPayload {
                    target: target_ref.clone(),
                    reason: p::ReasonRef(format!("verified-checkpoint:{}", handoff.checkpoint.0)),
                }),
                internal_provenance(p::Source::Internal),
            )?;
            self.append(
                handoff.from_run.clone(),
                p::EventPayload::HandoffResolved(p::HandoffResolvedPayload {
                    target: target_ref,
                    reason: p::ReasonRef(format!("new-segment:{}", handoff.next_run.0)),
                }),
                internal_provenance(p::Source::Internal),
            )?;
        }
        Ok(handoff)
    }

    pub fn accept_device_signal(
        &self,
        authenticated_peer: &p::FederatedPeerRef,
        signal: p::FederatedDeviceSignal,
        now: p::Timestamp,
    ) -> p::Result<bool> {
        signal.validate(now)?;
        if *authenticated_peer != signal.peer {
            return Err(p::Error(
                "device signal does not match its authenticated peer channel".into(),
            ));
        }
        let peer = self
            .peer(&signal.peer)?
            .ok_or_else(|| p::Error("device signal peer is not registered".into()))?;
        if peer.revoked
            || !peer
                .grant
                .roles
                .contains(&p::FederatedPeerRole::OwnerClient)
            || peer.grant.expires_at <= now
            || !self
                .snapshot(peer.grant.scopes[0].clone())?
                .grants
                .contains(&peer.grant.reference()?)
        {
            return Err(p::Error("device signal peer is not active".into()));
        }
        self.store.claim_device_signal(&signal)
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_remote_approval(
        &self,
        approval_id: p::ApprovalId,
        plan_digest: p::PlanDigest,
        outcome: p::ApprovalOutcome,
        owner: p::VerifiedPrincipal,
        nonce: p::Nonce,
        use_by: p::Timestamp,
        now: p::Timestamp,
    ) -> p::Result<(p::RunId, bool)> {
        let _control = self
            .control_lock
            .lock()
            .map_err(|_| p::Error("federation owner control is unavailable".into()))?;
        let (preflight_intent, preflight_placement, preflight_snapshot) = {
            let pending = self
                .pending
                .lock()
                .map_err(|_| p::Error("remote action state is unavailable".into()))?;
            let action = pending
                .values()
                .find(|action| action.ticket.0 == approval_id)
                .ok_or_else(|| p::Error("remote approval is not pending".into()))?;
            if action.terminal || action.plan.digest != plan_digest || action.lease.is_some() {
                return Err(p::Error(
                    "remote approval no longer matches its plan".into(),
                ));
            }
            (
                action.intent.clone(),
                action.placement.clone(),
                action.snapshot.clone(),
            )
        };
        let preflight_peer = self
            .peer(&preflight_placement.executor)?
            .ok_or_else(|| p::Error("remote executor disappeared before approval".into()))?;
        validate_remote_peer(
            &preflight_peer,
            &preflight_snapshot,
            &preflight_placement,
            &preflight_intent,
            now,
        )?;
        let live_snapshot = self.snapshot(preflight_intent.scope.clone())?;
        if live_snapshot.digest != preflight_snapshot.digest
            || live_snapshot.authority_epoch != preflight_placement.authority_epoch
        {
            return Err(p::Error(
                "federation snapshot changed before approval consumption".into(),
            ));
        }
        let (run, request_source, intent, plan, placement, snapshot) = {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| p::Error("remote action state is unavailable".into()))?;
            let (run, action) = pending
                .iter_mut()
                .find(|(_, action)| action.ticket.0 == approval_id)
                .ok_or_else(|| p::Error("remote approval is not pending".into()))?;
            if action.terminal || action.plan.digest != plan_digest || action.lease.is_some() {
                return Err(p::Error(
                    "remote approval no longer matches its plan".into(),
                ));
            }
            let grant = ApprovalGrant {
                schema_version: p::M4_SCHEMA_VERSION,
                approval_id: approval_id.clone(),
                outcome,
                granted_scope: GrantScope::OneShot,
                approver: owner,
                bound_plan_digest: plan_digest.clone(),
                policy_version: action.policy_version,
                tool_schema_version: action.tool_schema_version,
                nonce,
                use_by,
            };
            action
                .approval
                .resolve(action.ticket.clone(), grant.clone())?;
            let source = action.request.source;
            for payload in action.approval.take_events() {
                self.append(run.clone(), payload, internal_provenance(source))?;
            }
            self.append(
                run.clone(),
                p::EventPayload::RunResumed(p::RunResumedPayload {
                    wait_reason: p::WaitReason("remote_approval".into()),
                    resume_ref: p::ResumeRef(format!("approval:{}", approval_id.0)),
                }),
                internal_provenance(source),
            )?;
            if outcome != p::ApprovalOutcome::Granted {
                self.append(
                    run.clone(),
                    p::EventPayload::ActionDenied(p::ActionDeniedPayload {
                        intent_id: action.intent.intent_id.clone(),
                        reason: p::ReasonRef("remote approval was not granted".into()),
                    }),
                    internal_provenance(source),
                )?;
                self.append_run_aborted(run, source, "remote approval denied")?;
                action.terminal = true;
                return Ok((run.clone(), true));
            }
            action.approval.authorize(&ApprovalAuthorization {
                approval_id,
                session: p::SessionId(action.request.session.0.clone()),
                scope: action.intent.scope.clone(),
                plan_digest: plan_digest.clone(),
                policy_version: action.policy_version,
                tool_schema_version: action.tool_schema_version,
                now,
                intent: action.intent.clone(),
            })?;
            let engine = DefaultPolicyEngine;
            let context = action.governance.policy_context(
                p::SessionId(action.request.session.0.clone()),
                action.toolset_ref.clone(),
                &engine,
                action.governance.envelope.as_ref(),
            );
            let evaluation = engine.evaluate_detailed(&context, &action.intent);
            if evaluation.decision == p::PolicyDecision::Deny {
                return Err(p::Error(
                    "remote action was denied by its final policy recheck".into(),
                ));
            }
            let (Some(delegation), Some(envelope)) = (
                action.governance.delegation.as_ref(),
                action.governance.envelope.as_ref(),
            ) else {
                return Err(p::Error(
                    "remote action lost its delegation before dispatch".into(),
                ));
            };
            if engine.enforce_envelope(delegation, envelope, &action.intent)
                == EnvelopeDecision::OutOfScope
            {
                return Err(p::Error(
                    "remote action left its autonomy envelope before dispatch".into(),
                ));
            }
            (
                run.clone(),
                source,
                action.intent.clone(),
                action.plan.clone().with_approval(action.ticket.0.clone()),
                action.placement.clone(),
                action.snapshot.clone(),
            )
        };

        let peer = self
            .peer(&placement.executor)?
            .ok_or_else(|| p::Error("remote executor disappeared before dispatch".into()))?;
        validate_remote_peer(&peer, &snapshot, &placement, &intent, now)?;
        let current_snapshot = self.snapshot(intent.scope.clone())?;
        if current_snapshot.digest != snapshot.digest
            || current_snapshot.authority_epoch != placement.authority_epoch
        {
            return Err(p::Error(
                "federation snapshot changed after remote approval".into(),
            ));
        }
        self.append(
            run.clone(),
            p::EventPayload::ToolPolicyEvaluated(p::ToolPolicyEvaluatedPayload {
                decision: p::PolicyDecision::Ask,
                rule_source: p::RuleSourceRef("federation-final-recheck".into()),
                reason: p::ReasonRef("remote plan remains in its approved scope".into()),
            }),
            internal_provenance(request_source),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::CompetenceGateEvaluated(p::CompetenceGateEvaluatedPayload {
                scope: intent.scope.clone(),
                risk: intent.risk_hint,
                max_level: protocol_intervention(if intent.risk_hint == p::Risk::High {
                    InterventionLevel::L5HighImpact
                } else {
                    InterventionLevel::L3ActWithApproval
                }),
                reads: CompetenceInputs::default().protocol_reads(),
            }),
            internal_provenance(request_source),
        )?;
        if let Some(guard) = self
            .dispatch_guards
            .lock()
            .map_err(|_| p::Error("remote dispatch guards are unavailable".into()))?
            .get(&run)
            .cloned()
        {
            guard.recheck(&run, &placement, now)?;
        }
        let lease = self.acquire_for_run(
            &run,
            p::RemoteLeaseRequest {
                schema_version: p::M4_SCHEMA_VERSION,
                lease: p::RemoteExecutionLeaseRef(format!("lease:{}", intent.intent_id.0)),
                dispatch: p::RemoteDispatchId(format!("dispatch:{}", intent.intent_id.0)),
                intent: intent.intent_id.clone(),
                plan_digest: plan.digest.clone(),
                placement: placement.clone(),
                expires_at: now.saturating_add(30_000),
            },
        )?;
        let mut recovery = p::RemoteActionRecoveryRecord {
            schema_version: p::M4_SCHEMA_VERSION,
            run: run.clone(),
            source: request_source,
            intent: intent.clone(),
            placement: placement.clone(),
            lease: lease.clone(),
            driver_receipt: None,
            digest: p::SchemaDigest(String::new()),
        };
        recovery.refresh_digest()?;
        self.store.save_remote_recovery(&recovery)?;
        let claim = self.store.claim_remote_dispatch(
            &lease.lease,
            &lease.plan_digest,
            lease.authority_epoch,
        )?;
        if claim.status != p::RemoteDispatchClaimStatus::Claimed {
            return Err(p::Error(
                "remote lease was already dispatched and may only be probed".into(),
            ));
        }
        self.leases.bind(lease.clone())?;
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| p::Error("remote action state is unavailable".into()))?;
            pending
                .get_mut(&run)
                .ok_or_else(|| p::Error("remote run disappeared before dispatch".into()))?
                .lease = Some(lease.clone());
        }
        let backend = self.remote_backend()?;
        let store = self.store.clone();
        let sink_run = run.clone();
        let sink_sequence = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let sink = EventSink::with_observer(move |payload| {
            let sequence = sink_sequence.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            store
                .append(p::Event::new(
                    p::EventId(format!("m4-sink-event:{}:{sequence}", sink_run.0)),
                    sink_run.clone(),
                    None,
                    payload.clone(),
                    p::M4_SCHEMA_VERSION,
                    now_ms(),
                    internal_provenance(request_source),
                ))
                .map(|_| ())
        });
        let execution = backend.execute(plan, &sink, CancelToken::default());
        let result = match execution {
            Ok(result) => result,
            Err(_) => {
                self.mark_remote_unknown(
                    &run,
                    &intent,
                    &lease,
                    request_source,
                    "remote dispatch did not return a verifiable terminal state",
                )?;
                return Ok((run, false));
            }
        };
        if result.status != ActionStatus::Unknown {
            return Err(p::Error(
                "remote backend returned authority completion without verification".into(),
            ));
        }
        let driver_receipt = result
            .external_receipt
            .as_ref()
            .and_then(|receipt| receipt.content_ref.as_ref())
            .map(|reference| p::RemoteDriverReceiptRef(reference.0.clone()))
            .ok_or_else(|| {
                p::Error("remote dispatch acceptance has no receipt reference".into())
            })?;
        recovery.driver_receipt = Some(driver_receipt.clone());
        recovery.refresh_digest()?;
        self.store.save_remote_recovery(&recovery)?;
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| p::Error("remote action state is unavailable".into()))?;
            pending
                .get_mut(&run)
                .ok_or_else(|| p::Error("remote run disappeared after dispatch".into()))?
                .driver_receipt = Some(driver_receipt);
        }
        // Terminal verification owns the same control lock and must recheck the
        // authority lease after dispatch. Release the approval phase before
        // entering that independently fenced terminal phase.
        drop(_control);
        match self.try_complete_with_configured_truth(&run) {
            Ok(Some(_)) => Ok((run, true)),
            Ok(None) => {
                self.mark_remote_unknown(
                    &run,
                    &intent,
                    &lease,
                    request_source,
                    "remote result awaits independent authority verification",
                )?;
                Ok((run, false))
            }
            Err(_) => {
                self.append_remote_verification_failure(
                    &run,
                    &intent.scope,
                    &lease,
                    request_source,
                )?;
                self.mark_remote_unknown(
                    &run,
                    &intent,
                    &lease,
                    request_source,
                    "remote result failed authority verification and requires review",
                )?;
                Ok((run, false))
            }
        }
    }

    fn try_complete_with_configured_truth(
        &self,
        run: &p::RunId,
    ) -> p::Result<Option<RemoteRecoveryResult>> {
        let (plan, lease, receipt_ref, source) = {
            let pending = self
                .pending
                .lock()
                .map_err(|_| p::Error("remote action state is unavailable".into()))?;
            let pending = pending
                .get(run)
                .ok_or_else(|| p::Error("remote run is not pending".into()))?;
            (
                pending.placement.clone(),
                pending
                    .lease
                    .clone()
                    .ok_or_else(|| p::Error("remote run has no acquired lease".into()))?,
                pending
                    .driver_receipt
                    .clone()
                    .ok_or_else(|| p::Error("remote run has no driver receipt".into()))?,
                pending.request.source,
            )
        };
        let remote = self.remote_bindings()?;
        let Some(ground_truth) = remote.ground_truth else {
            return Ok(None);
        };
        let driver = remote.receipts.receipt(p::RemoteReceiptRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            receipt: receipt_ref,
            lease: lease.lease.clone(),
            dispatch: lease.dispatch.clone(),
            authority_epoch: lease.authority_epoch,
            fence: lease.fence,
            nonce: p::Nonce(format!("receipt:{}:{}", run.0, now_ms())),
        })?;
        let Some(truth) = ground_truth.observe(&plan, &lease, &driver)? else {
            return Ok(None);
        };
        self.finish_remote(run, source, plan, lease, driver, truth)
            .map(Some)
    }

    fn finish_remote(
        &self,
        run: &p::RunId,
        source: p::Source,
        plan: p::RemotePlacementPlan,
        lease: p::RemoteExecutionLease,
        driver: p::RemoteDriverReceipt,
        ground_truth: RemoteGroundTruth,
    ) -> p::Result<RemoteRecoveryResult> {
        let _control = self
            .control_lock
            .lock()
            .map_err(|_| p::Error("federation terminal control is unavailable".into()))?;
        let current_lease = self
            .store
            .lease(&lease.lease)?
            .ok_or_else(|| p::Error("remote completion has no authority lease".into()))?;
        if current_lease != lease || current_lease.state != p::RemoteLeaseState::Acquired {
            return Err(p::Error(
                "remote completion was fenced before authority verification".into(),
            ));
        }
        let receipt = RemoteAuthorityVerifier.verify(&plan, &lease, &driver, ground_truth)?;
        if receipt.outcome != p::RemoteReceiptOutcome::Completed {
            return Err(p::Error(
                "remote outcome is not a verified completion".into(),
            ));
        }
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        if events.iter().any(|event| {
            matches!(
                &event.payload,
                p::EventPayload::ActionOutcomeUnknown(payload)
                    if payload.intent_id == lease.intent
            )
        }) {
            self.append(
                run.clone(),
                p::EventPayload::RunResumed(p::RunResumedPayload {
                    wait_reason: p::WaitReason("remote_outcome_verification".into()),
                    resume_ref: p::ResumeRef(format!("remote-receipt:{}", receipt.receipt.0)),
                }),
                internal_provenance(source),
            )?;
        }
        self.append(
            run.clone(),
            p::EventPayload::ActionCompleted(p::ActionCompletedPayload {
                intent_id: lease.intent.clone(),
                result_ref: p::ActionResultRef(format!("remote-result:{}", receipt.receipt.0)),
                receipt: None,
                remote_receipt: Some(receipt.receipt.clone()),
            }),
            internal_provenance(source),
        )?;
        if let Some(observer) = self
            .completion_observers
            .lock()
            .map_err(|_| p::Error("remote completion observers are unavailable".into()))?
            .get(run)
            .cloned()
        {
            observer.record_verified(run, &plan, &lease, &driver, &receipt)?;
        }
        self.transition_for_run(
            run,
            p::RemoteLeaseTransition {
                schema_version: p::M4_SCHEMA_VERSION,
                lease: lease.lease.clone(),
                expected: p::RemoteLeaseState::Acquired,
                next: p::RemoteLeaseState::Released,
                reason: p::ReasonRef("authority verified the remote ground truth".into()),
                authority_epoch: lease.authority_epoch,
            },
        )?;
        let done = p::DoneContractRef(format!("done-contract:{}", run.0));
        let verifier = p::VerifierKind("remote-authority-ground-truth".into());
        self.append(
            run.clone(),
            p::EventPayload::VerificationStarted(p::VerificationStartedPayload {
                verifier_kind: verifier.clone(),
                against: done.clone(),
            }),
            internal_provenance(source),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                verifier_kind: verifier,
                outcome: p::VerificationOutcome::Pass,
                against: done,
            }),
            internal_provenance(source),
        )?;
        self.append_run_complete(run, source)?;
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| p::Error("remote action state is unavailable".into()))?;
        if let Some(action) = pending.get_mut(run) {
            action.terminal = true;
            action.terminal_receipt = Some(receipt.clone());
        }
        Ok(RemoteRecoveryResult {
            run: run.clone(),
            terminal: true,
            outcome: receipt.outcome,
            receipt: Some(receipt),
        })
    }

    fn finish_remote_not_dispatched(
        &self,
        run: &p::RunId,
        source: p::Source,
        intent: &p::ActionIntent,
        lease: &p::RemoteExecutionLease,
    ) -> p::Result<RemoteRecoveryResult> {
        let failure = p::FailureEvidenceRef(format!("failure:remote-not-dispatched:{}", run.0));
        self.append(
            run.clone(),
            p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
                failure_ref: failure.clone(),
                class: p::FailureClass::ExecutionFailure,
                impact: p::Impact::Medium,
                scope: intent.scope.clone(),
                related_refs: vec![p::EvidenceRef(lease.dispatch.0.clone())],
                suggested_fix: Some(p::SuggestedFixRef(
                    "submit a new plan and approval after authority recovery".into(),
                )),
            }),
            internal_provenance(source),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::ActionFailed(p::ActionFailedPayload {
                intent_id: intent.intent_id.clone(),
                failure_ref: failure,
                remote_lease: Some(lease.lease.clone()),
            }),
            internal_provenance(source),
        )?;
        self.transition_for_run(
            run,
            p::RemoteLeaseTransition {
                schema_version: p::M4_SCHEMA_VERSION,
                lease: lease.lease.clone(),
                expected: p::RemoteLeaseState::Acquired,
                next: p::RemoteLeaseState::Released,
                reason: p::ReasonRef(
                    "authority restart proved no remote dispatch was claimed".into(),
                ),
                authority_epoch: lease.authority_epoch,
            },
        )?;
        let contract = p::DoneContractRef(format!("done-contract:{}", run.0));
        let verifier = p::VerifierKind("remote-authority-recovery".into());
        self.append(
            run.clone(),
            p::EventPayload::VerificationStarted(p::VerificationStartedPayload {
                verifier_kind: verifier.clone(),
                against: contract.clone(),
            }),
            internal_provenance(source),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                verifier_kind: verifier,
                outcome: p::VerificationOutcome::Fail,
                against: contract,
            }),
            internal_provenance(source),
        )?;
        self.append(
            run.clone(),
            p::EventPayload::RunFailed(p::RunFailedPayload {
                stop_reason: p::StopReason(
                    "remote dispatch was not attempted before restart".into(),
                ),
                result_ref: None,
            }),
            internal_provenance(source),
        )?;
        if let Some(action) = self
            .pending
            .lock()
            .map_err(|_| p::Error("remote action state is unavailable".into()))?
            .get_mut(run)
        {
            action.terminal = true;
        }
        Ok(RemoteRecoveryResult {
            run: run.clone(),
            terminal: true,
            outcome: p::RemoteReceiptOutcome::Failed,
            receipt: None,
        })
    }

    fn mark_remote_unknown(
        &self,
        run: &p::RunId,
        intent: &p::ActionIntent,
        lease: &p::RemoteExecutionLease,
        source: p::Source,
        reason: &str,
    ) -> p::Result<()> {
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        if !events.iter().any(|event| {
            matches!(
                &event.payload,
                p::EventPayload::ActionOutcomeUnknown(payload)
                    if payload.intent_id == intent.intent_id
            )
        }) {
            self.append(
                run.clone(),
                p::EventPayload::ActionOutcomeUnknown(p::ActionOutcomeUnknownPayload {
                    intent_id: intent.intent_id.clone(),
                    probe_hint: p::ProbeHintRef(reason.to_owned()),
                    remote_lease: Some(lease.lease.clone()),
                }),
                internal_provenance(source),
            )?;
            self.append(
                run.clone(),
                p::EventPayload::RunWaiting(p::RunWaitingPayload {
                    wait_reason: p::WaitReason("remote_outcome_unknown".into()),
                    resume_ref: p::ResumeRef(format!("probe:{}", lease.lease.0)),
                }),
                internal_provenance(source),
            )?;
        }
        Ok(())
    }

    fn append_remote_verification_failure(
        &self,
        run: &p::RunId,
        scope: &p::Scope,
        lease: &p::RemoteExecutionLease,
        source: p::Source,
    ) -> p::Result<()> {
        let sequence = self
            .event_sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.append(
            run.clone(),
            p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
                failure_ref: p::FailureEvidenceRef(format!(
                    "failure:remote-verification:{}:{sequence}",
                    run.0
                )),
                class: p::FailureClass::VerificationFailure,
                impact: p::Impact::High,
                scope: scope.clone(),
                related_refs: vec![p::EvidenceRef(lease.lease.0.clone())],
                suggested_fix: Some(p::SuggestedFixRef(
                    "inspect the original receipt and independent ground truth".into(),
                )),
            }),
            internal_provenance(source),
        )?;
        Ok(())
    }

    fn acquire_for_run(
        &self,
        run: &p::RunId,
        request: p::RemoteLeaseRequest,
    ) -> p::Result<p::RemoteExecutionLease> {
        request.validate()?;
        let snapshot = self.snapshot(request.placement.operation.scope.clone())?;
        let peer = self
            .peer(&request.placement.executor)?
            .ok_or_else(|| p::Error("remote lease executor is not registered".into()))?;
        let intent = p::ActionIntent {
            schema_version: p::M4_SCHEMA_VERSION,
            intent_id: request.intent.clone(),
            source: p::Source::Internal,
            goal: p::GoalRef(format!("lease:{}", request.lease.0)),
            backend_hint: p::BackendKind::Remote,
            capability_ref: request.placement.operation.capability.clone(),
            action_type: request.placement.operation.action_type,
            scope: request.placement.operation.scope.clone(),
            risk_hint: p::Risk::High,
            expected_effect: request.placement.operation.expected_effect,
            rollback_expectation: request.placement.operation.rollback_boundary.clone(),
            parameters: p::ActionParameters::Remote(Box::new(p::RemoteActionSpec {
                schema_version: p::M4_SCHEMA_VERSION,
                placement: request.placement.clone(),
            })),
            requested_permissions: Vec::new(),
            requested_at: now_ms(),
            estimated_output_bytes: 1,
            estimated_duration: p::DurationMs(1),
        };
        validate_remote_peer(&peer, &snapshot, &request.placement, &intent, now_ms())?;
        if request.expires_at <= now_ms() || request.expires_at > now_ms().saturating_add(60_000) {
            return Err(p::Error(
                "remote lease expiry is outside the bounded window".into(),
            ));
        }
        let expected = self.store.federation_version(&federation_aggregate())?;
        let lease = p::RemoteExecutionLease {
            schema_version: p::M4_SCHEMA_VERSION,
            lease: request.lease,
            dispatch: request.dispatch,
            intent: request.intent,
            plan_digest: request.plan_digest,
            placement: request.placement.reference()?,
            executor: request.placement.executor,
            peer_grant: request.placement.peer_grant,
            grant_version: request.placement.grant_version,
            authority_epoch: request.placement.authority_epoch,
            fence: p::FenceToken(expected.version.saturating_add(1)),
            expires_at: request.expires_at,
            state: p::RemoteLeaseState::Acquired,
        };
        lease.validate()?;
        let event = self.event(
            run.clone(),
            p::EventPayload::RemoteExecutionLeaseChanged(p::RemoteExecutionLeaseChangedPayload {
                lease: lease.clone(),
                reason: p::ReasonRef("approved plan passed final authority recheck".into()),
                committed_version: expected.next()?,
            }),
            internal_provenance(p::Source::Internal),
        );
        let outcome =
            self.store
                .append_federation_expected(event, &federation_aggregate(), expected)?;
        if outcome.status != p::ExpectedAppendStatus::Applied {
            return Err(p::Error(
                "remote lease acquisition lost its authority CAS".into(),
            ));
        }
        Ok(lease)
    }

    fn transition_for_run(
        &self,
        run: &p::RunId,
        transition: p::RemoteLeaseTransition,
    ) -> p::Result<p::RemoteExecutionLease> {
        transition.validate()?;
        let mut lease = self
            .store
            .lease(&transition.lease)?
            .ok_or_else(|| p::Error("remote lease transition has no authority lease".into()))?;
        if lease.state != transition.expected || lease.authority_epoch != transition.authority_epoch
        {
            return Err(p::Error(
                "remote lease transition lost its state or epoch".into(),
            ));
        }
        lease.state = transition.next;
        let expected = self.store.federation_version(&federation_aggregate())?;
        let event = self.event(
            run.clone(),
            p::EventPayload::RemoteExecutionLeaseChanged(p::RemoteExecutionLeaseChangedPayload {
                lease: lease.clone(),
                reason: transition.reason,
                committed_version: expected.next()?,
            }),
            internal_provenance(p::Source::Internal),
        );
        let outcome =
            self.store
                .append_federation_expected(event, &federation_aggregate(), expected)?;
        if outcome.status != p::ExpectedAppendStatus::Applied {
            return Err(p::Error(
                "remote lease transition lost its authority CAS".into(),
            ));
        }
        Ok(lease)
    }

    fn cancel_remote(
        &self,
        lease_ref: &p::RemoteExecutionLeaseRef,
        reason: p::ReasonRef,
        now: p::Timestamp,
    ) -> p::Result<bool> {
        let _control = self
            .control_lock
            .lock()
            .map_err(|_| p::Error("federation terminal control is unavailable".into()))?;
        if reason.0.trim().is_empty() {
            return Err(p::Error("remote cancellation reason is empty".into()));
        }
        let lease = self
            .store
            .lease(lease_ref)?
            .ok_or_else(|| p::Error("remote cancellation has no authority lease".into()))?;
        if lease.state.terminal() {
            return Ok(true);
        }
        let pending_context = self
            .pending
            .lock()
            .map_err(|_| p::Error("remote action state is unavailable".into()))?
            .iter()
            .find(|(_, action)| {
                action
                    .lease
                    .as_ref()
                    .is_some_and(|value| value.lease == *lease_ref)
            })
            .map(|(run, action)| (run.clone(), action.intent.clone()));
        let (run, intent) = match pending_context {
            Some(context) => context,
            None => {
                let recovery = self
                    .store
                    .remote_recovery_for_lease(lease_ref)?
                    .ok_or_else(|| p::Error("remote cancellation has no pending run".into()))?;
                (recovery.run, recovery.intent)
            }
        };
        let fenced = self.transition_for_run(
            &run,
            p::RemoteLeaseTransition {
                schema_version: p::M4_SCHEMA_VERSION,
                lease: lease_ref.clone(),
                expected: lease.state,
                next: p::RemoteLeaseState::Fenced,
                reason: reason.clone(),
                authority_epoch: lease.authority_epoch,
            },
        )?;
        let remote = self.remote_bindings()?;
        let cancelled = remote.transport.cancel(p::RemoteCancelRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            lease: lease_ref.clone(),
            dispatch: fenced.dispatch.clone(),
            authority_epoch: fenced.authority_epoch,
            fence: fenced.fence,
            nonce: p::Nonce(format!("cancel:{}:{now}", lease_ref.0)),
            reason: reason.clone(),
        });
        match cancelled {
            Ok(result) if result.outcome == p::RemoteCancelOutcome::Cancelled => {
                self.append(
                    run.clone(),
                    p::EventPayload::ActionCancelled(p::ActionCancelledPayload {
                        intent_id: fenced.intent,
                        reason,
                    }),
                    internal_provenance(p::Source::OwnerControl),
                )?;
                Ok(true)
            }
            _ => {
                self.mark_remote_unknown(
                    &run,
                    &intent,
                    &fenced,
                    p::Source::OwnerControl,
                    "remote cancellation could not prove the side effect stopped",
                )?;
                Ok(false)
            }
        }
    }

    fn remote_backend(&self) -> p::Result<Arc<dyn ActionBackend + Send + Sync>> {
        self.remote
            .lock()
            .map_err(|_| p::Error("federation remote runtime is unavailable".into()))?
            .backend
            .clone()
            .ok_or_else(|| p::Error("remote executor backend is not configured".into()))
    }

    fn remote_bindings(&self) -> p::Result<RemoteBindingsSnapshot> {
        let remote = self
            .remote
            .lock()
            .map_err(|_| p::Error("federation remote runtime is unavailable".into()))?;
        Ok(RemoteBindingsSnapshot {
            transport: remote
                .transport
                .clone()
                .ok_or_else(|| p::Error("remote transport is not configured".into()))?,
            receipts: remote
                .receipts
                .clone()
                .ok_or_else(|| p::Error("remote receipt source is not configured".into()))?,
            ground_truth: remote.ground_truth.clone(),
        })
    }

    fn require_expected(&self, expected: &p::FederationAggregateVersion) -> p::Result<()> {
        let actual = self.store.federation_version(&federation_aggregate())?;
        if &actual != expected {
            return Err(p::Error(
                "federation owner control has a stale version".into(),
            ));
        }
        Ok(())
    }

    fn append_owner_run_start(
        &self,
        run: &p::RunId,
        owner: &p::VerifiedPrincipal,
        grant: Option<&p::FederatedPeerGrant>,
    ) -> p::Result<()> {
        self.append(
            run.clone(),
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::OwnerControl,
                session_ref: p::SessionId(format!("federation-owner:{}", owner.0)),
                input_ref: p::InputRef(format!("owner-control:{}", run.0)),
                idempotency_key: Some(p::IdempotencyKey(format!("owner-control:{}", run.0))),
            }),
            owner_provenance(),
        )?;
        let scope = grant
            .and_then(|grant| grant.scopes.first())
            .cloned()
            .unwrap_or_else(|| p::Scope("workspace:default".into()));
        let snapshot = self.snapshot(scope)?;
        self.append(
            run.clone(),
            p::EventPayload::SessionBound(p::SessionBoundPayload {
                policy_profile: p::PolicyProfileRef("policy:owner-control".into()),
                model_profile: p::ModelProfileRef("model:none".into()),
                toolset_ref: p::ToolsetRef("toolset:owner-control".into()),
                workspace: p::WorkspaceRef("workspace:default".into()),
                effect_mode: None,
                evolution_snapshot: None,
                federation_snapshot: Some(p::FederationSnapshotRef(snapshot.digest.0)),
            }),
            owner_provenance(),
        )?;
        Ok(())
    }

    fn append_run_complete(&self, run: &p::RunId, source: p::Source) -> p::Result<()> {
        self.append(
            run.clone(),
            p::EventPayload::RunComplete(p::RunCompletePayload {
                stop_reason: p::StopReason("federation_control_complete".into()),
                result_ref: None,
            }),
            internal_provenance(source),
        )?;
        Ok(())
    }

    fn append_run_aborted(&self, run: &p::RunId, source: p::Source, reason: &str) -> p::Result<()> {
        self.append(
            run.clone(),
            p::EventPayload::RunAborted(p::RunAbortedPayload {
                stop_reason: p::StopReason(reason.into()),
                result_ref: None,
            }),
            internal_provenance(source),
        )?;
        Ok(())
    }

    fn append(
        &self,
        run: p::RunId,
        payload: p::EventPayload,
        provenance: p::Provenance,
    ) -> p::Result<p::EventId> {
        self.store.append(self.event(run, payload, provenance))
    }

    fn event(
        &self,
        run: p::RunId,
        payload: p::EventPayload,
        provenance: p::Provenance,
    ) -> p::Event {
        let sequence = self
            .event_sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let timestamp = now_ms();
        let identity = p::canonical_digest(&(&run, &payload, &provenance, timestamp, sequence))
            .unwrap_or_else(|_| p::SchemaDigest(format!("fallback:{timestamp}:{sequence}")));
        p::Event::new(
            p::EventId(format!("m4-event:{}", identity.0)),
            run,
            None,
            payload,
            p::M4_SCHEMA_VERSION,
            timestamp,
            provenance,
        )
    }
}

struct RemoteBindingsSnapshot {
    transport: Arc<dyn RemoteTransport>,
    receipts: Arc<dyn RemoteReceiptSource>,
    ground_truth: Option<Arc<dyn RemoteGroundTruthSource>>,
}

fn validate_remote_peer(
    peer: &p::FederatedPeerState,
    snapshot: &p::FederationSnapshot,
    placement: &p::RemotePlacementPlan,
    intent: &p::ActionIntent,
    now: p::Timestamp,
) -> p::Result<()> {
    let grant = &peer.grant;
    if peer.revoked
        || grant.expires_at <= now
        || !grant.roles.contains(&p::FederatedPeerRole::Executor)
        || grant.reference()? != placement.peer_grant
        || grant.grant_version != placement.grant_version
        || snapshot.authority_epoch != placement.authority_epoch
        || !snapshot.grants.contains(&placement.peer_grant)
        || !grant.scopes.contains(&intent.scope)
        || !grant.capabilities.contains(&intent.capability_ref)
    {
        return Err(p::Error(
            "remote executor grant, epoch, scope, or capability is inactive".into(),
        ));
    }
    Ok(())
}

fn federation_aggregate() -> p::FederationAggregateRef {
    p::FederationAggregateRef(FEDERATION_AGGREGATE.into())
}

fn owner_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::OwnerControl,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_runtime_has_an_empty_authority_snapshot() {
        let store = SqliteEventStore::open_in_memory(forme_store::StoreOptions::default()).unwrap();
        let runtime = FederatedHarnessRuntime::new(store);
        let snapshot = runtime.snapshot(p::Scope("workspace:test".into())).unwrap();
        assert!(snapshot.grants.is_empty());
        assert_eq!(snapshot.registry_version.version, 0);
    }
}
