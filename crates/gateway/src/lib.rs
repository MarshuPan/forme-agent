//! Trusted local gateway: authentication, provenance stamping, and harness routing (prd/14).
#![forbid(unsafe_code)]

mod server;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use forme_communication::{
    AuthContext, AuthMethod, AuthPrincipal, AuthorizedDeviceObservation, ChannelAdapter,
    CommunicationEvent, CommunicationFabric, DisclosureOutcome, DisclosureRequest,
    ExternalCommunicationGrant, InMemoryCommunicationFabric, LocalDeviceChannelAdapter,
    LoopbackChannelAdapter, OutboundMessage, ParticipantProfile, RawInput, SessionId,
};
use forme_harness::{
    ApprovalGrant, ApprovalTicket, EcosystemGatewayControl as HarnessEcosystemGatewayControl,
    EvolutionGatewayControl as HarnessEvolutionGatewayControl,
    FederationActionGateway as HarnessFederationActionGateway, FederationControlResult,
    FederationGatewayControl as HarnessFederationGatewayControl,
    GatewayControl as HarnessGatewayControl, HarnessActionIngress, HarnessIngress,
    IngressAuthority, ManualEvaluator as HarnessManualEvaluator, ReactiveHarness,
    SchedulerGatewayControl as HarnessSchedulerGatewayControl,
    SchedulerService as HarnessSchedulerService,
};
pub use forme_harness::{EventStream, EvolutionActivationResult, EvolutionEvaluationResult};
use forme_protocol as p;

pub use server::{serve_environment, GatewayServerConfig};

pub trait Gateway {
    fn submit(&self, event: CommunicationEvent) -> p::Result<p::RunId>;
    fn stream_events(&self, run: p::RunId) -> EventStream;
    fn approve(&self, ticket: ApprovalTicket, grant: ApprovalGrant) -> p::Result<()>;
}

pub trait RunGateway: Send + Sync {
    fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId>;
    fn stream_run(&self, run: p::RunId) -> EventStream;
    fn wait_run(&self, run: p::RunId) -> p::Result<p::RunResult>;
    fn answer(&self, run: p::RunId) -> p::Result<Option<String>>;
}

pub fn environment_gateway() -> p::Result<Box<dyn RunGateway>> {
    let harness = Arc::new(ReactiveHarness::from_environment_local()?);
    let owner = p::VerifiedPrincipal(
        std::env::var("FORME_OWNER_ID").unwrap_or_else(|_| "local-owner".into()),
    );
    let gateway = LocalGateway::new(
        harness,
        owner,
        p::AgentProfileRef("agent:forme-local".into()),
    )?;
    Ok(Box::new(gateway))
}

pub struct LocalGateway<H: HarnessIngress + Send + Sync> {
    harness: Arc<H>,
    ingress_authority: IngressAuthority,
    owner: p::VerifiedPrincipal,
    agent_profile: p::AgentProfileRef,
    sequence: AtomicU64,
}

pub struct OwnerClientGateway<H: HarnessFederationGatewayControl + Send + Sync> {
    harness: Arc<H>,
    owner: p::VerifiedPrincipal,
    sequence: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FederatedChannelPurpose {
    OwnerControl,
    ReplicaAck,
    RetentionReceipt,
    DeviceSignal,
}

/// Opaque evidence that a transport adapter matched the presented peer
/// certificate to an owner-provisioned grant. Payloads cannot construct this
/// binding and owner authentication remains a separate proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedFederatedChannel {
    schema_version: p::SchemaVersion,
    peer: p::FederatedPeerRef,
    session: p::FederatedSessionRef,
    transport_identity: p::TransportIdentityDigest,
    grant: p::FederatedPeerGrantRef,
    purpose: FederatedChannelPurpose,
    expires_at: p::Timestamp,
}

impl AuthenticatedFederatedChannel {
    pub fn peer(&self) -> &p::FederatedPeerRef {
        &self.peer
    }

    pub fn session(&self) -> &p::FederatedSessionRef {
        &self.session
    }
}

impl<H> OwnerClientGateway<H>
where
    H: HarnessFederationGatewayControl + Send + Sync,
{
    fn validate_owner(&self, auth: AuthContext, now: p::Timestamp) -> p::Result<()> {
        auth.validate(now)?;
        if auth.method == AuthMethod::FederatedPeer {
            return Err(p::Error(
                "owner authentication must be independent from the peer channel".into(),
            ));
        }
        match auth.principal {
            AuthPrincipal::Owner(owner) if owner == self.owner => Ok(()),
            _ => Err(p::Error(
                "federation control requires the independently authenticated owner".into(),
            )),
        }
    }

    fn bind_channel(
        &self,
        peer: p::FederatedPeerRef,
        session: p::FederatedSessionRef,
        presented_identity: p::TransportIdentityDigest,
        purpose: FederatedChannelPurpose,
        expires_at: p::Timestamp,
    ) -> p::Result<AuthenticatedFederatedChannel> {
        let now = now_ms();
        if peer.0.trim().is_empty()
            || session.0.trim().is_empty()
            || presented_identity.0.trim().is_empty()
            || expires_at <= now
        {
            return Err(p::Error(
                "federated peer channel binding is incomplete or expired".into(),
            ));
        }
        let state = self
            .harness
            .federated_peer(&peer)?
            .ok_or_else(|| p::Error("federated peer channel is not provisioned".into()))?;
        let required_role = match purpose {
            FederatedChannelPurpose::OwnerControl | FederatedChannelPurpose::DeviceSignal => {
                p::FederatedPeerRole::OwnerClient
            }
            FederatedChannelPurpose::ReplicaAck | FederatedChannelPurpose::RetentionReceipt => {
                p::FederatedPeerRole::Replica
            }
        };
        let grant = state.grant.reference()?;
        if state.grant.transport_identity != presented_identity
            || !state.grant.roles.contains(&required_role)
            || state.grant.expires_at <= now
            || expires_at > state.grant.expires_at
        {
            return Err(p::Error(
                "federated peer transport identity or role is not authorized".into(),
            ));
        }
        if purpose != FederatedChannelPurpose::RetentionReceipt {
            let scope = state
                .grant
                .scopes
                .first()
                .cloned()
                .ok_or_else(|| p::Error("federated peer grant has no scope".into()))?;
            let snapshot = self.harness.federation_snapshot(scope)?;
            if state.revoked || !snapshot.grants.contains(&grant) {
                return Err(p::Error("federated peer channel is not active".into()));
            }
        }
        Ok(AuthenticatedFederatedChannel {
            schema_version: p::M4_SCHEMA_VERSION,
            peer,
            session,
            transport_identity: presented_identity,
            grant,
            purpose,
            expires_at,
        })
    }

    fn validate_channel(
        &self,
        channel: &AuthenticatedFederatedChannel,
        purpose: FederatedChannelPurpose,
        now: p::Timestamp,
    ) -> p::Result<()> {
        if channel.schema_version.0 == 0
            || channel.purpose != purpose
            || channel.expires_at <= now
            || channel.peer.0.trim().is_empty()
            || channel.session.0.trim().is_empty()
        {
            return Err(p::Error(
                "authenticated federated channel is invalid or expired".into(),
            ));
        }
        let state = self
            .harness
            .federated_peer(&channel.peer)?
            .ok_or_else(|| p::Error("authenticated federated peer disappeared".into()))?;
        if state.grant.transport_identity != channel.transport_identity
            || state.grant.reference()? != channel.grant
            || state.grant.expires_at <= now
        {
            return Err(p::Error(
                "authenticated federated channel no longer matches its grant".into(),
            ));
        }
        if purpose != FederatedChannelPurpose::RetentionReceipt {
            let scope = state
                .grant
                .scopes
                .first()
                .cloned()
                .ok_or_else(|| p::Error("authenticated peer grant has no scope".into()))?;
            if state.revoked
                || !self
                    .harness
                    .federation_snapshot(scope)?
                    .grants
                    .contains(&channel.grant)
            {
                return Err(p::Error(
                    "authenticated federated channel was revoked".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn authenticate_owner_client_channel(
        &self,
        peer: p::FederatedPeerRef,
        session: p::FederatedSessionRef,
        presented_identity: p::TransportIdentityDigest,
        expires_at: p::Timestamp,
    ) -> p::Result<AuthenticatedFederatedChannel> {
        self.bind_channel(
            peer,
            session,
            presented_identity,
            FederatedChannelPurpose::OwnerControl,
            expires_at,
        )
    }

    pub fn authenticate_replica_channel(
        &self,
        peer: p::FederatedPeerRef,
        session: p::FederatedSessionRef,
        presented_identity: p::TransportIdentityDigest,
        expires_at: p::Timestamp,
    ) -> p::Result<AuthenticatedFederatedChannel> {
        self.bind_channel(
            peer,
            session,
            presented_identity,
            FederatedChannelPurpose::ReplicaAck,
            expires_at,
        )
    }

    pub fn authenticate_retention_receipt_channel(
        &self,
        peer: p::FederatedPeerRef,
        session: p::FederatedSessionRef,
        presented_identity: p::TransportIdentityDigest,
        expires_at: p::Timestamp,
    ) -> p::Result<AuthenticatedFederatedChannel> {
        self.bind_channel(
            peer,
            session,
            presented_identity,
            FederatedChannelPurpose::RetentionReceipt,
            expires_at,
        )
    }

    pub fn authenticate_device_signal_channel(
        &self,
        peer: p::FederatedPeerRef,
        session: p::FederatedSessionRef,
        presented_identity: p::TransportIdentityDigest,
        expires_at: p::Timestamp,
    ) -> p::Result<AuthenticatedFederatedChannel> {
        self.bind_channel(
            peer,
            session,
            presented_identity,
            FederatedChannelPurpose::DeviceSignal,
            expires_at,
        )
    }

    pub fn snapshot(&self, scope: p::Scope, auth: AuthContext) -> p::Result<p::FederationSnapshot> {
        self.validate_owner(auth, now_ms())?;
        self.harness.federation_snapshot(scope)
    }

    pub fn peer(
        &self,
        peer: &p::FederatedPeerRef,
        auth: AuthContext,
    ) -> p::Result<Option<p::FederatedPeerState>> {
        self.validate_owner(auth, now_ms())?;
        self.harness.federated_peer(peer)
    }

    pub fn register(
        &self,
        grant: p::FederatedPeerGrant,
        previous: Option<p::FederatedPeerGrantRef>,
        expected: p::FederationAggregateVersion,
        auth: AuthContext,
    ) -> p::Result<p::ExpectedAppend> {
        let now = now_ms();
        self.validate_owner(auth, now)?;
        if grant.owner != self.owner {
            return Err(p::Error("peer grant belongs to another owner".into()));
        }
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        self.harness.register_federated_peer(
            p::RunId(format!("owner-control:register:{sequence}")),
            grant,
            previous,
            expected,
            self.owner.clone(),
        )
    }

    pub fn revoke(
        &self,
        peer: p::FederatedPeerRef,
        grant: p::FederatedPeerGrantRef,
        in_flight: p::InFlightDisposition,
        expected: p::FederationAggregateVersion,
        auth: AuthContext,
    ) -> p::Result<p::ExpectedAppend> {
        self.validate_owner(auth, now_ms())?;
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        self.harness.revoke_federated_peer(
            p::RunId(format!("owner-control:revoke:{sequence}")),
            peer,
            grant,
            in_flight,
            expected,
            self.owner.clone(),
        )
    }

    pub fn apply(
        &self,
        channel: &AuthenticatedFederatedChannel,
        envelope: p::FederatedControlEnvelope,
        command: p::FederatedOwnerCommand,
        owner_auth: AuthContext,
    ) -> p::Result<FederationControlResult> {
        let now = now_ms();
        self.validate_channel(channel, FederatedChannelPurpose::OwnerControl, now)?;
        self.validate_owner(owner_auth, now)?;
        if channel.peer != envelope.peer || channel.session != envelope.session {
            return Err(p::Error(
                "owner control envelope does not match its authenticated peer channel".into(),
            ));
        }
        if envelope.owner != self.owner {
            return Err(p::Error(
                "owner client envelope belongs to another principal".into(),
            ));
        }
        self.harness
            .apply_federated_owner_command(envelope, command, now)
    }

    pub fn lease(
        &self,
        lease: &p::RemoteExecutionLeaseRef,
        owner_auth: AuthContext,
    ) -> p::Result<Option<p::RemoteExecutionLease>> {
        self.validate_owner(owner_auth, now_ms())?;
        self.harness.federated_lease(lease)
    }

    pub fn retention_state(
        &self,
        request: &p::RetentionRequestRef,
        owner_auth: AuthContext,
    ) -> p::Result<Option<forme_harness::RetentionState>> {
        self.validate_owner(owner_auth, now_ms())?;
        self.harness.federated_retention_state(request)
    }

    pub fn recover_remote_action(
        &self,
        run: &p::RunId,
        owner_auth: AuthContext,
    ) -> p::Result<forme_harness::RemoteRecoveryResult> {
        self.validate_owner(owner_auth, now_ms())?;
        self.harness.recover_federated_remote_action(run)
    }

    pub fn acknowledge_replication(
        &self,
        channel: &AuthenticatedFederatedChannel,
        batch: &p::ReplicationBatch,
        ack: p::ReplicationAck,
        expected: p::FederationAggregateVersion,
    ) -> p::Result<p::ExpectedAppend> {
        let now = now_ms();
        self.validate_channel(channel, FederatedChannelPurpose::ReplicaAck, now)?;
        if channel.peer != ack.peer || channel.peer != batch.peer {
            return Err(p::Error(
                "replication acknowledgement does not match its authenticated peer channel".into(),
            ));
        }
        self.harness
            .acknowledge_federated_replication(&channel.peer, batch, ack, expected, now)
    }

    pub fn accept_retention_receipt(
        &self,
        channel: &AuthenticatedFederatedChannel,
        receipt: p::FederatedRetentionReceipt,
    ) -> p::Result<()> {
        let now = now_ms();
        self.validate_channel(channel, FederatedChannelPurpose::RetentionReceipt, now)?;
        if channel.peer != receipt.peer {
            return Err(p::Error(
                "retention receipt does not match its authenticated peer channel".into(),
            ));
        }
        self.harness
            .accept_federated_retention_receipt(&channel.peer, receipt, now)
    }

    pub fn accept_device_signal(
        &self,
        channel: &AuthenticatedFederatedChannel,
        signal: p::FederatedDeviceSignal,
    ) -> p::Result<bool> {
        let now = now_ms();
        self.validate_channel(channel, FederatedChannelPurpose::DeviceSignal, now)?;
        if channel.peer != signal.peer || channel.session != signal.session {
            return Err(p::Error(
                "device signal does not match its authenticated peer channel".into(),
            ));
        }
        self.harness
            .accept_federated_device_signal(&channel.peer, signal, now)
    }
}

impl<H> LocalGateway<H>
where
    H: HarnessIngress + Send + Sync,
{
    pub fn new(
        harness: Arc<H>,
        owner: p::VerifiedPrincipal,
        agent_profile: p::AgentProfileRef,
    ) -> p::Result<Self> {
        if owner.0.trim().is_empty() || agent_profile.0.trim().is_empty() {
            return Err(p::Error("local gateway identity is incomplete".into()));
        }
        let ingress_authority = harness.ingress_authority();
        Ok(Self {
            harness,
            ingress_authority,
            owner,
            agent_profile,
            sequence: AtomicU64::new(1),
        })
    }

    pub fn normalize_and_submit<A: ChannelAdapter>(
        &self,
        adapter: &A,
        raw: RawInput,
        auth: AuthContext,
    ) -> p::Result<p::RunId> {
        let event = adapter.normalize(raw)?;
        self.submit_event_with_auth(event, auth)
    }

    pub fn submit_event_with_auth(
        &self,
        event: CommunicationEvent,
        auth: AuthContext,
    ) -> p::Result<p::RunId> {
        event.validate()?;
        auth.validate(now_ms())?;
        validate_principal(&event.participant, &auth.principal, &self.owner)?;
        if matches!(event.participant, ParticipantProfile::ExternalAgent(_))
            && auth.method != AuthMethod::MutualChannel
        {
            return Err(p::Error(
                "external Agent input requires a mutually authenticated channel".into(),
            ));
        }
        let provenance = gateway_provenance(&event.participant, &auth.principal);
        let source = if event.participant == ParticipantProfile::Owner {
            p::Source::UserTurn
        } else {
            p::Source::Communication
        };
        let session = event
            .session_ref
            .as_ref()
            .map(|session| p::SessionRef(session.0.clone()))
            .unwrap_or_else(|| {
                p::SessionRef(format!(
                    "gateway:{}:{}",
                    event.participant.participant_id().0,
                    self.sequence.fetch_add(1, Ordering::SeqCst)
                ))
            });
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source,
            session,
            agent_profile: self.agent_profile.clone(),
            input: p::RunInput(event.content.clone()),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey(format!(
                "gateway:{}:{}",
                event.channel_adapter.0, event.message_id
            ))),
        };
        self.harness.submit_ingress(
            request,
            vec![self
                .ingress_authority
                .stamp(event.event_payload(), provenance)],
        )
    }

    pub fn submit_run(&self, request: p::RunRequest, auth: AuthContext) -> p::Result<p::RunId> {
        self.validate_run_submission(&request, auth)?;
        self.harness.submit_ingress(request, Vec::new())
    }

    fn validate_run_submission(&self, request: &p::RunRequest, auth: AuthContext) -> p::Result<()> {
        auth.validate(now_ms())?;
        match (&request.source, &auth.principal) {
            (p::Source::UserTurn, AuthPrincipal::Owner(owner)) if owner == &self.owner => {}
            (p::Source::Communication, AuthPrincipal::External(_))
            | (p::Source::Communication, AuthPrincipal::AgentRepresentingOwner(_)) => {}
            _ => {
                return Err(p::Error(
                    "authenticated principal cannot submit this run source".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn wait(&self, run: p::RunId) -> p::Result<p::RunResult> {
        self.harness.wait(run)
    }

    pub fn answer(&self, run: p::RunId) -> p::Result<Option<String>> {
        self.harness.result_text(run)
    }

    pub fn harness(&self) -> &Arc<H> {
        &self.harness
    }

    pub fn open_external_session(
        &self,
        fabric: &InMemoryCommunicationFabric,
        grant: ExternalCommunicationGrant,
        auth: AuthContext,
    ) -> p::Result<SessionId> {
        auth.validate(now_ms())?;
        if !matches!(&auth.principal, AuthPrincipal::Owner(owner) if owner == &self.owner) {
            return Err(p::Error(
                "only the verified owner can create an external communication grant".into(),
            ));
        }
        let session = fabric.open_session(grant)?;
        self.persist_fabric_events(fabric, &session)?;
        Ok(session)
    }

    pub fn apply_external_disclosure(
        &self,
        fabric: &InMemoryCommunicationFabric,
        request: DisclosureRequest,
    ) -> p::Result<DisclosureOutcome> {
        let session = request.session.clone();
        let outcome = fabric.apply_disclosure(request);
        self.persist_fabric_events(fabric, &session)?;
        Ok(outcome)
    }

    pub fn terminate_external_session(
        &self,
        fabric: &InMemoryCommunicationFabric,
        session: SessionId,
        reason: p::TerminationReason,
    ) -> p::Result<()> {
        fabric.terminate(session.clone(), reason)?;
        self.persist_fabric_events(fabric, &session)
    }

    pub fn submit_device_observation(
        &self,
        observation: AuthorizedDeviceObservation,
        auth: AuthContext,
    ) -> p::Result<p::RunId> {
        self.submit_device_observation_at(observation, auth, now_ms())
    }

    fn submit_device_observation_at(
        &self,
        observation: AuthorizedDeviceObservation,
        auth: AuthContext,
        now: p::Timestamp,
    ) -> p::Result<p::RunId> {
        auth.validate(now)?;
        if !matches!(&auth.principal, AuthPrincipal::Owner(owner) if owner == &self.owner) {
            return Err(p::Error(
                "device observation is not owner-authorized".into(),
            ));
        }
        let (event, observation) = observation.into_gateway_parts(now)?;
        event.validate()?;
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::Communication,
            session: event
                .session_ref
                .as_ref()
                .map(|session| p::SessionRef(session.0.clone()))
                .unwrap_or_else(|| {
                    p::SessionRef(format!("device-observation:{}", event.message_id))
                }),
            agent_profile: self.agent_profile.clone(),
            input: p::RunInput(event.content.clone()),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey(format!(
                "device:{}:{}",
                event.channel_adapter.0, event.message_id
            ))),
        };
        let provenance = p::Provenance {
            source: p::Source::Communication,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::Untrusted,
            caused_by: None,
        };
        self.harness.submit_ingress(
            request,
            vec![
                self.ingress_authority
                    .stamp(event.event_payload(), provenance.clone()),
                self.ingress_authority.stamp(observation, provenance),
            ],
        )
    }

    pub fn revoke_device(
        &self,
        adapter: &LocalDeviceChannelAdapter,
        lineage: p::LineageRef,
        auth: AuthContext,
    ) -> p::Result<()> {
        auth.validate(now_ms())?;
        if !matches!(&auth.principal, AuthPrincipal::Owner(owner) if owner == &self.owner) {
            return Err(p::Error(
                "only the verified owner can revoke a device".into(),
            ));
        }
        let event = adapter.revoke_with_event(lineage)?;
        self.harness.append_ingress_events(
            p::RunId(format!("device-revocation:{}", adapter.id.0)),
            vec![self.ingress_authority.stamp(
                event,
                p::Provenance {
                    source: p::Source::Communication,
                    actor: p::Actor::Owner,
                    trust_tier: p::TrustTier::OwnerInput,
                    caused_by: None,
                },
            )],
        )?;
        Ok(())
    }

    fn persist_fabric_events(
        &self,
        fabric: &InMemoryCommunicationFabric,
        session: &SessionId,
    ) -> p::Result<()> {
        let events = fabric
            .take_events()
            .into_iter()
            .map(|payload| {
                let provenance =
                    if matches!(payload, p::EventPayload::ExternalCommunicationGranted(_)) {
                        p::Provenance {
                            source: p::Source::Communication,
                            actor: p::Actor::Owner,
                            trust_tier: p::TrustTier::OwnerInput,
                            caused_by: None,
                        }
                    } else {
                        p::Provenance {
                            source: p::Source::Communication,
                            actor: p::Actor::System,
                            trust_tier: p::TrustTier::VerifiedProcess,
                            caused_by: None,
                        }
                    };
                self.ingress_authority.stamp(payload, provenance)
            })
            .collect();
        self.harness
            .append_ingress_events(p::RunId(format!("communication:{}", session.0)), events)?;
        Ok(())
    }
}

impl<H> LocalGateway<H>
where
    H: HarnessActionIngress + Send + Sync,
{
    pub fn deliver_external_message(
        &self,
        fabric: &InMemoryCommunicationFabric,
        adapter: &LoopbackChannelAdapter,
        message: OutboundMessage,
        auth: AuthContext,
    ) -> p::Result<p::RunId> {
        auth.validate(now_ms())?;
        if !matches!(&auth.principal, AuthPrincipal::Owner(owner) if owner == &self.owner) {
            return Err(p::Error(
                "only the verified owner can submit an external delivery proposal".into(),
            ));
        }
        let session = message.session.clone();
        let prepared = match adapter.prepare_delivery(fabric, message) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.persist_fabric_events(fabric, &session)?;
                return Err(error);
            }
        };
        let prelude = fabric
            .take_events()
            .into_iter()
            .map(|payload| {
                self.ingress_authority.stamp(
                    payload,
                    p::Provenance {
                        source: p::Source::Communication,
                        actor: p::Actor::System,
                        trust_tier: p::TrustTier::VerifiedProcess,
                        caused_by: None,
                    },
                )
            })
            .collect::<Vec<_>>();
        if !prelude
            .iter()
            .any(|event| matches!(event.payload(), p::EventPayload::DisclosurePolicyApplied(_)))
        {
            return Err(p::Error(
                "external delivery has no persisted disclosure decision".into(),
            ));
        }
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::Communication,
            session: p::SessionRef(prepared.session.0.clone()),
            agent_profile: self.agent_profile.clone(),
            input: p::RunInput(format!(
                "deliver approved external message {}",
                prepared.message_id
            )),
            budget: Some(prepared.envelope.budget.clone()),
            idempotency_key: Some(p::IdempotencyKey(format!(
                "external-delivery:{}:{}",
                adapter.id.0, prepared.message_id
            ))),
        };
        self.harness
            .submit_action(request, prepared.intent, prepared.envelope, prelude)
    }
}

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessGatewayControl + Send + Sync,
{
    pub fn start_run(&self, request: p::RunRequest, auth: AuthContext) -> p::Result<p::RunId> {
        self.validate_run_submission(&request, auth)?;
        HarnessGatewayControl::start_run(self.harness.clone(), request)
    }

    pub fn control_profile(
        &self,
        surface: p::SurfaceRef,
        auth: AuthContext,
    ) -> p::Result<p::ControlProfile> {
        self.validate_owner_control(auth)?;
        let profile = p::ControlProfile {
            schema_version: p::SchemaVersion(1),
            owner: self.owner.clone(),
            gateway: HarnessGatewayControl::gateway_profile(self.harness.as_ref(), surface)?,
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn event_page(&self, cursor: p::EventCursor, auth: AuthContext) -> p::Result<p::EventPage> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::stream_event_page(self.harness.as_ref(), cursor)
    }

    pub fn list_runs(&self, auth: AuthContext) -> p::Result<Vec<p::RunSummary>> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::list_runs(self.harness.as_ref())
    }

    pub fn run_summary(&self, run: p::RunId, auth: AuthContext) -> p::Result<p::RunSummary> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::run_summary(self.harness.as_ref(), run)
    }

    pub fn pending_approvals(
        &self,
        session: p::SessionId,
        auth: AuthContext,
    ) -> p::Result<Vec<p::PendingApproval>> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::pending_approvals(self.harness.as_ref(), session)
    }

    pub fn control_run(
        &self,
        run: p::RunId,
        control: p::RunControl,
        auth: AuthContext,
    ) -> p::Result<()> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::control(self.harness.as_ref(), run, control)
    }

    pub fn trace_view(&self, run: p::RunId, auth: AuthContext) -> p::Result<p::TraceView> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::trace_view(self.harness.as_ref(), run)
    }

    pub fn review_candidate(
        &self,
        command: p::CandidateReviewCommand,
        auth: AuthContext,
    ) -> p::Result<()> {
        self.validate_owner_control(auth)?;
        HarnessGatewayControl::review_candidate(self.harness.as_ref(), command)
    }

    fn validate_owner_control(&self, auth: AuthContext) -> p::Result<()> {
        auth.validate(now_ms())?;
        match auth.principal {
            AuthPrincipal::Owner(owner) if owner == self.owner => Ok(()),
            _ => Err(p::Error(
                "gateway control requires the authenticated owner".into(),
            )),
        }
    }
}

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessFederationGatewayControl + Send + Sync,
{
    pub fn owner_client_gateway(&self) -> OwnerClientGateway<H> {
        OwnerClientGateway {
            harness: self.harness.clone(),
            owner: self.owner.clone(),
            sequence: AtomicU64::new(1),
        }
    }
}

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessGatewayControl + HarnessEcosystemGatewayControl + Send + Sync,
{
    pub fn ecosystem_snapshot(
        &self,
        scope: p::Scope,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityEcosystemSnapshot> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::capability_ecosystem_snapshot(self.harness.as_ref(), scope)
    }

    pub fn provision_capability_publisher(
        &self,
        run: p::RunId,
        grant: p::CapabilityPublisherGrant,
        previous: Option<p::CapabilityPublisherGrantRef>,
        expected: p::EcosystemAggregateVersion,
        auth: AuthContext,
    ) -> p::Result<p::ExpectedAppend> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::provision_capability_publisher(
            self.harness.as_ref(),
            run,
            grant,
            previous,
            expected,
            self.owner.clone(),
        )
    }

    pub fn admit_capability_package(
        &self,
        run: p::RunId,
        catalog_run: p::RunId,
        package: p::SignedCapabilityPackage,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityPackageAdmission> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::admit_capability_package(
            self.harness.as_ref(),
            run,
            catalog_run,
            package,
            now_ms(),
            self.owner.clone(),
        )
    }

    pub fn prepare_capability_change(
        &self,
        operation: p::CapabilityPackageOperation,
        package: p::CapabilityPackageRef,
        release: p::CapabilityReleaseRef,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityInstallPlan> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::prepare_capability_change(
            self.harness.as_ref(),
            operation,
            package,
            release,
            self.owner.clone(),
        )
    }

    pub fn apply_capability_change(
        &self,
        run: p::RunId,
        plan: p::CapabilityInstallPlan,
        approval: p::CapabilityPackageApproval,
        reason: p::ReasonRef,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityPackageState> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::apply_capability_change(
            self.harness.as_ref(),
            run,
            plan,
            approval,
            reason,
            now_ms(),
        )
    }

    pub fn prepare_capability_distribution(
        &self,
        plan: p::CapabilityInstallPlan,
        target_peer: p::FederatedPeerRef,
        authority_epoch: p::AuthorityEpoch,
        auth: AuthContext,
    ) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
        self.validate_owner_control(auth)?;
        HarnessEcosystemGatewayControl::prepare_capability_distribution(
            self.harness.as_ref(),
            plan,
            target_peer,
            authority_epoch,
            self.owner.clone(),
            now_ms(),
        )
    }
}

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessFederationActionGateway + Send + Sync,
{
    pub fn submit_remote_action(
        &self,
        request: p::RunRequest,
        intent: p::ActionIntent,
        auth: AuthContext,
    ) -> p::Result<forme_harness::RemoteActionSubmission> {
        self.validate_run_submission(&request, auth)?;
        if request.source != p::Source::UserTurn || request.idempotency_key.is_none() {
            return Err(p::Error(
                "remote action gateway submission requires an idempotent owner turn".into(),
            ));
        }
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        self.harness.submit_federated_remote_action(
            p::RunId(format!("gateway:remote-action:{sequence}")),
            request,
            intent,
        )
    }
}

pub trait EvolutionRunGateway: Send + Sync {
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
    ) -> p::Result<EvolutionActivationResult>;
    fn set_auto_activation_paused(&self, paused: bool) -> p::Result<()>;
    fn auto_activation_paused(&self) -> p::Result<bool>;
}

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessGatewayControl + HarnessEvolutionGatewayControl + Send + Sync,
{
    pub fn evolution_snapshot(
        &self,
        scope: p::Scope,
        auth: AuthContext,
    ) -> p::Result<p::EvolutionSnapshot> {
        self.validate_owner_control(auth)?;
        HarnessEvolutionGatewayControl::evolution_snapshot(self.harness.as_ref(), scope)
    }

    pub fn record_strategy_candidate(
        &self,
        run: p::RunId,
        candidate: p::StrategyCandidate,
        auth: AuthContext,
    ) -> p::Result<p::EventId> {
        self.validate_owner_control(auth)?;
        if !matches!(candidate.provenance.actor, p::Actor::Owner)
            || candidate.provenance.trust_tier != p::TrustTier::OwnerInput
        {
            return Err(p::Error(
                "gateway strategy candidate must carry owner provenance".into(),
            ));
        }
        HarnessEvolutionGatewayControl::record_strategy_candidate(
            self.harness.as_ref(),
            run,
            candidate,
        )
    }

    pub fn evaluate_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        comparison: p::EvolutionComparison,
        auth: AuthContext,
    ) -> p::Result<EvolutionEvaluationResult> {
        self.validate_owner_control(auth)?;
        HarnessEvolutionGatewayControl::evaluate_strategy(
            self.harness.as_ref(),
            run,
            candidate,
            comparison,
        )
    }

    pub fn promote_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        auth: AuthContext,
    ) -> p::Result<p::EventId> {
        self.validate_owner_control(auth)?;
        HarnessEvolutionGatewayControl::promote_strategy(
            self.harness.as_ref(),
            run,
            candidate,
            evaluation,
        )
    }

    pub fn activate_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        promotion: p::EventId,
        auth: AuthContext,
    ) -> p::Result<EvolutionActivationResult> {
        self.validate_owner_control(auth)?;
        let owner_confirmation = p::OwnerControlRef(format!(
            "owner-control:{}:{}",
            self.owner.0,
            self.sequence.fetch_add(1, Ordering::SeqCst)
        ));
        HarnessEvolutionGatewayControl::activate_strategy(
            self.harness.as_ref(),
            run,
            aggregate,
            candidate,
            evaluation,
            promotion,
            Some(owner_confirmation),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rollback_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        domain: p::StrategyDomain,
        scope: p::Scope,
        restored: p::StrategyVersionRef,
        triggers: Vec<p::EvidenceRef>,
        in_flight: p::InFlightDisposition,
        auth: AuthContext,
    ) -> p::Result<EvolutionActivationResult> {
        self.validate_owner_control(auth)?;
        let owner_confirmation = p::OwnerControlRef(format!(
            "owner-control:{}:{}",
            self.owner.0,
            self.sequence.fetch_add(1, Ordering::SeqCst)
        ));
        HarnessEvolutionGatewayControl::rollback_strategy(
            self.harness.as_ref(),
            run,
            aggregate,
            domain,
            scope,
            restored,
            triggers,
            in_flight,
            Some(owner_confirmation),
        )
    }

    pub fn set_auto_activation_paused(&self, paused: bool, auth: AuthContext) -> p::Result<()> {
        self.validate_owner_control(auth)?;
        HarnessEvolutionGatewayControl::set_auto_activation_paused(self.harness.as_ref(), paused);
        Ok(())
    }

    pub fn auto_activation_paused(&self, auth: AuthContext) -> p::Result<bool> {
        self.validate_owner_control(auth)?;
        Ok(HarnessEvolutionGatewayControl::auto_activation_paused(
            self.harness.as_ref(),
        ))
    }
}

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessManualEvaluator + Send + Sync,
{
    pub fn run_eval(
        &self,
        case: p::ManualEvalCase,
        profile: p::EvalProfile,
        auth: AuthContext,
    ) -> p::Result<p::ManualEvalReport> {
        self.validate_owner_control(auth)?;
        HarnessManualEvaluator::run_case(self.harness.as_ref(), case, profile)
    }

    pub fn export_eval(
        &self,
        eval_ref: p::EvalRef,
        auth: AuthContext,
    ) -> p::Result<p::ManualEvalReport> {
        self.validate_owner_control(auth)?;
        HarnessManualEvaluator::export_report(self.harness.as_ref(), eval_ref)
    }
}

impl<H> LocalGateway<H>
where
    H: HarnessIngress + HarnessSchedulerGatewayControl + HarnessSchedulerService + Send + Sync,
{
    pub fn schedule_job(
        &self,
        command: p::ScheduleCommand,
        auth: AuthContext,
    ) -> p::Result<p::IntentionId> {
        self.validate_owner_control(auth)?;
        HarnessSchedulerGatewayControl::schedule(self.harness.as_ref(), command)
    }

    pub fn list_jobs(&self, auth: AuthContext) -> p::Result<Vec<p::ScheduledJob>> {
        self.validate_owner_control(auth)?;
        HarnessSchedulerGatewayControl::list_jobs(self.harness.as_ref())
    }

    pub fn cancel_job(&self, intention: p::IntentionId, auth: AuthContext) -> p::Result<()> {
        self.validate_owner_control(auth)?;
        HarnessSchedulerService::cancel(self.harness.as_ref(), intention, p::Actor::Owner)
    }
}

impl<H> Gateway for LocalGateway<H>
where
    H: HarnessIngress + Send + Sync,
{
    fn submit(&self, event: CommunicationEvent) -> p::Result<p::RunId> {
        let auth = match &event.participant {
            ParticipantProfile::Owner => {
                AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000))
            }
            ParticipantProfile::External(participant)
            | ParticipantProfile::ExternalAgent(participant) => AuthContext {
                schema_version: p::SchemaVersion(1),
                principal: AuthPrincipal::External(participant.clone()),
                method: forme_communication::AuthMethod::MutualChannel,
                expires_at: now_ms().saturating_add(60_000),
            },
            ParticipantProfile::AgentRepresentingOwner => AuthContext {
                schema_version: p::SchemaVersion(1),
                principal: AuthPrincipal::AgentRepresentingOwner(self.owner.clone()),
                method: forme_communication::AuthMethod::LocalSession,
                expires_at: now_ms().saturating_add(60_000),
            },
        };
        self.submit_event_with_auth(event, auth)
    }

    fn stream_events(&self, run: p::RunId) -> EventStream {
        self.harness.stream_events(run)
    }

    fn approve(&self, ticket: ApprovalTicket, grant: ApprovalGrant) -> p::Result<()> {
        if grant.approver != self.owner {
            return Err(p::Error(
                "approval principal does not match the local owner".into(),
            ));
        }
        self.harness.resolve_approval(ticket, grant)
    }
}

impl<H> RunGateway for LocalGateway<H>
where
    H: HarnessIngress + Send + Sync,
{
    fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId> {
        LocalGateway::submit_run(
            self,
            request,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn stream_run(&self, run: p::RunId) -> EventStream {
        Gateway::stream_events(self, run)
    }

    fn wait_run(&self, run: p::RunId) -> p::Result<p::RunResult> {
        self.wait(run)
    }

    fn answer(&self, run: p::RunId) -> p::Result<Option<String>> {
        LocalGateway::answer(self, run)
    }
}

impl<H> EvolutionRunGateway for LocalGateway<H>
where
    H: HarnessIngress + HarnessGatewayControl + HarnessEvolutionGatewayControl + Send + Sync,
{
    fn evolution_snapshot(&self, scope: p::Scope) -> p::Result<p::EvolutionSnapshot> {
        LocalGateway::evolution_snapshot(
            self,
            scope,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn record_strategy_candidate(
        &self,
        run: p::RunId,
        candidate: p::StrategyCandidate,
    ) -> p::Result<p::EventId> {
        LocalGateway::record_strategy_candidate(
            self,
            run,
            candidate,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn evaluate_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        comparison: p::EvolutionComparison,
    ) -> p::Result<EvolutionEvaluationResult> {
        LocalGateway::evaluate_strategy(
            self,
            run,
            candidate,
            comparison,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn promote_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
    ) -> p::Result<p::EventId> {
        LocalGateway::promote_strategy(
            self,
            run,
            candidate,
            evaluation,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn activate_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        promotion: p::EventId,
    ) -> p::Result<EvolutionActivationResult> {
        LocalGateway::activate_strategy(
            self,
            run,
            aggregate,
            candidate,
            evaluation,
            promotion,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
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
    ) -> p::Result<EvolutionActivationResult> {
        LocalGateway::rollback_strategy(
            self,
            run,
            aggregate,
            domain,
            scope,
            restored,
            triggers,
            in_flight,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn set_auto_activation_paused(&self, paused: bool) -> p::Result<()> {
        LocalGateway::set_auto_activation_paused(
            self,
            paused,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }

    fn auto_activation_paused(&self) -> p::Result<bool> {
        LocalGateway::auto_activation_paused(
            self,
            AuthContext::local_owner(self.owner.clone(), now_ms().saturating_add(60_000)),
        )
    }
}

fn validate_principal(
    participant: &ParticipantProfile,
    principal: &AuthPrincipal,
    owner: &p::VerifiedPrincipal,
) -> p::Result<()> {
    let valid = match (participant, principal) {
        (ParticipantProfile::Owner, AuthPrincipal::Owner(authenticated)) => authenticated == owner,
        (
            ParticipantProfile::AgentRepresentingOwner,
            AuthPrincipal::AgentRepresentingOwner(authenticated),
        ) => authenticated == owner,
        (ParticipantProfile::External(expected), AuthPrincipal::External(actual))
        | (ParticipantProfile::ExternalAgent(expected), AuthPrincipal::External(actual)) => {
            expected == actual
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(p::Error(
            "gateway authentication does not match the event participant".into(),
        ))
    }
}

fn gateway_provenance(
    participant: &ParticipantProfile,
    principal: &AuthPrincipal,
) -> p::Provenance {
    match (participant, principal) {
        (ParticipantProfile::Owner, AuthPrincipal::Owner(_)) => p::Provenance {
            source: p::Source::UserTurn,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
        (ParticipantProfile::AgentRepresentingOwner, AuthPrincipal::AgentRepresentingOwner(_)) => {
            p::Provenance {
                source: p::Source::Communication,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            }
        }
        (ParticipantProfile::External(participant), AuthPrincipal::External(_))
        | (ParticipantProfile::ExternalAgent(participant), AuthPrincipal::External(_)) => {
            p::Provenance {
                source: p::Source::Communication,
                actor: p::Actor::External(participant.clone()),
                trust_tier: p::TrustTier::Untrusted,
                caused_by: None,
            }
        }
        _ => p::Provenance {
            source: p::Source::Communication,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::Untrusted,
            caused_by: None,
        },
    }
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    use forme_communication::{
        Carrier, CommunicationBudget, CommunicationPurpose, DeviceGrant, DeviceObservation,
        DisclosurePolicy, DisclosureRule, ObservationScope, RepresentationRule, RetentionPolicy,
        TerminationPolicy, TextChannelAdapter, TranscriptPolicy,
    };
    use forme_harness::{
        AgentHarness, EcosystemGatewayControl, EvolutionGatewayControl, HarnessIngress,
        IngressEvent, ResumeInput,
    };

    use super::*;

    #[derive(Clone)]
    struct FakeRun {
        events: Vec<p::Event>,
        result: p::RunResult,
        output: String,
    }

    type EcosystemDistributionCall = (
        p::CapabilityInstallPlan,
        p::FederatedPeerRef,
        p::AuthorityEpoch,
        p::VerifiedPrincipal,
        p::Timestamp,
    );

    struct FakeHarness {
        sequence: AtomicU64,
        ingress_authority: IngressAuthority,
        responses: Mutex<VecDeque<String>>,
        runs: Mutex<BTreeMap<p::RunId, FakeRun>>,
        requests: Mutex<Vec<p::RunRequest>>,
        evolution_paused: AtomicBool,
        federation_commands: Mutex<
            Vec<(
                p::FederatedControlEnvelope,
                p::FederatedOwnerCommand,
                p::Timestamp,
            )>,
        >,
        federation_actions: Mutex<Vec<(p::RunId, p::RunRequest, p::ActionIntent)>>,
        federation_acks: Mutex<Vec<(p::FederatedPeerRef, p::ReplicationAck)>>,
        federation_retention_receipts:
            Mutex<Vec<(p::FederatedPeerRef, p::FederatedRetentionReceipt)>>,
        federation_signals: Mutex<Vec<(p::FederatedPeerRef, p::FederatedDeviceSignal)>>,
        ecosystem_distributions: Mutex<Vec<EcosystemDistributionCall>>,
    }

    impl FakeHarness {
        fn new(responses: Vec<String>) -> Self {
            Self {
                sequence: AtomicU64::new(1),
                ingress_authority: IngressAuthority::default(),
                responses: Mutex::new(responses.into()),
                runs: Mutex::new(BTreeMap::new()),
                requests: Mutex::new(Vec::new()),
                evolution_paused: AtomicBool::new(false),
                federation_commands: Mutex::new(Vec::new()),
                federation_actions: Mutex::new(Vec::new()),
                federation_acks: Mutex::new(Vec::new()),
                federation_retention_receipts: Mutex::new(Vec::new()),
                federation_signals: Mutex::new(Vec::new()),
                ecosystem_distributions: Mutex::new(Vec::new()),
            }
        }

        fn federated_grant(peer: &p::FederatedPeerRef) -> Option<p::FederatedPeerGrant> {
            let (roles, identity, version) = match peer.0.as_str() {
                "peer:owner-device-b" => (
                    vec![p::FederatedPeerRole::OwnerClient],
                    "sha256:owner-device-b",
                    1,
                ),
                "peer:replica-b" => (vec![p::FederatedPeerRole::Replica], "sha256:replica-b", 2),
                _ => return None,
            };
            Some(p::FederatedPeerGrant {
                schema_version: p::M4_SCHEMA_VERSION,
                peer: peer.clone(),
                owner: p::VerifiedPrincipal("owner:test".into()),
                roles,
                scopes: vec![p::Scope("workspace:m4".into())],
                capabilities: Vec::new(),
                transport_identity: p::TransportIdentityDigest(identity.into()),
                authority_epoch: p::AuthorityEpoch(1),
                grant_version: p::PeerGrantVersion(version),
                expires_at: i64::MAX,
                created_by: p::OwnerControlRef(format!("owner-control:{}", peer.0)),
            })
        }

        fn event(
            run: &p::RunId,
            sequence: u64,
            payload: p::EventPayload,
            provenance: p::Provenance,
        ) -> p::Event {
            let mut event = p::Event::new(
                p::EventId(format!("fake-event:{sequence}")),
                run.clone(),
                None,
                payload,
                p::SchemaVersion(1),
                now_ms(),
                provenance,
            );
            event.stream_seq = sequence;
            event
        }
    }

    fn m5_gateway_package() -> p::SignedCapabilityPackage {
        let content = "gateway-owned declarative skill".to_owned();
        let payload_digest = p::sha256_content_digest(content.as_bytes());
        let mut package = p::SignedCapabilityPackage {
            schema_version: p::M5_SCHEMA_VERSION,
            manifest: p::CapabilityPackageManifest {
                schema_version: p::M5_SCHEMA_VERSION,
                package: p::CapabilityPackageRef("package:m5-gateway".into()),
                release: p::CapabilityReleaseRef("release:m5-gateway:v1".into()),
                version: p::Version(1),
                kind: p::CapabilityPackageKind::Skill,
                publisher: p::CapabilityPublisherRef("publisher:m5-gateway".into()),
                scope: p::Scope("workspace:m5-gateway".into()),
                contributions: vec![p::CapabilityContributionDescriptor {
                    schema_version: p::M5_SCHEMA_VERSION,
                    kind: p::CapabilityPackageKind::Skill,
                    capability: p::CapabilityRef("skill:m5-gateway".into()),
                    payload_digest: payload_digest.clone(),
                    required_permissions: vec![p::PermissionRef("permission:read".into())],
                    risk: p::Risk::Low,
                    network: false,
                    hook: false,
                }],
                dependencies: Vec::new(),
                sbom_digest: p::sha256_content_digest(b"m5 gateway SBOM"),
                license_expression: "MIT".into(),
                body_digest: p::SchemaDigest(String::new()),
                max_unpacked_bytes: 1024,
                contains_executable: false,
            },
            resources: vec![p::CapabilityPackageResource {
                schema_version: p::M5_SCHEMA_VERSION,
                relative_path: "skill.txt".into(),
                content,
                digest: payload_digest,
            }],
            package_digest: p::SchemaDigest(String::new()),
            signature: p::PackageSignature(format!("ed25519:{}", "00".repeat(64))),
        };
        package.refresh_digests().unwrap();
        package.validate().unwrap();
        package
    }

    fn m5_gateway_admission(package: &p::SignedCapabilityPackage) -> p::CapabilityPackageAdmission {
        p::CapabilityPackageAdmission {
            schema_version: p::M5_SCHEMA_VERSION,
            reference: p::CapabilityAdmissionRef("admission:m5-gateway:v1".into()),
            package: package.manifest.package.clone(),
            release: package.manifest.release.clone(),
            package_digest: package.package_digest.clone(),
            publisher_grant: p::CapabilityPublisherGrantRef("grant:m5-gateway:v1".into()),
            publisher_version: p::Version(1),
            policy: p::CapabilityPolicyRef("policy:m5-gateway".into()),
            policy_version: p::Version(1),
            checks: p::CapabilityAdmissionCheckKind::ALL
                .into_iter()
                .map(|kind| p::CapabilityAdmissionCheck {
                    schema_version: p::M5_SCHEMA_VERSION,
                    kind,
                    verdict: p::CapabilityAdmissionVerdict::Pass,
                    evidence: p::EvidenceRef(format!("evidence:m5-gateway:{kind:?}")),
                })
                .collect(),
            dependencies: Vec::new(),
            admitted_at: 1,
        }
    }

    fn m5_gateway_publisher() -> p::CapabilityPublisherGrant {
        p::CapabilityPublisherGrant {
            schema_version: p::M5_SCHEMA_VERSION,
            reference: p::CapabilityPublisherGrantRef("grant:m5-gateway:v1".into()),
            publisher: p::CapabilityPublisherRef("publisher:m5-gateway".into()),
            public_key_digest: p::sha256_content_digest(b"m5 gateway publisher key"),
            allowed_kinds: vec![p::CapabilityPackageKind::Skill],
            scope: p::Scope("workspace:m5-gateway".into()),
            expires_at: i64::MAX,
            version: p::Version(1),
            status: p::CapabilityPublisherStatus::Active,
        }
    }

    fn m5_gateway_policy() -> p::CapabilityAdmissionPolicy {
        p::CapabilityAdmissionPolicy {
            schema_version: p::M5_SCHEMA_VERSION,
            reference: p::CapabilityPolicyRef("policy:m5-gateway".into()),
            version: p::Version(1),
            allowed_kinds: vec![p::CapabilityPackageKind::Skill],
            allowed_licenses: vec!["MIT".into()],
            max_package_bytes: 1024,
            max_dependencies: 1,
            max_depth: 1,
            allow_network: false,
            allow_hooks: false,
        }
    }

    fn m5_gateway_distribution_envelope(
        plan: p::CapabilityInstallPlan,
        target_peer: p::FederatedPeerRef,
        authority_epoch: p::AuthorityEpoch,
    ) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
        let package = m5_gateway_package();
        let mut envelope = p::CapabilityPackageDistributionEnvelope {
            schema_version: p::M5_SCHEMA_VERSION,
            install_plan: plan,
            admission: m5_gateway_admission(&package),
            package,
            dependency_admissions: Vec::new(),
            publisher_grant: m5_gateway_publisher(),
            authority_policy: m5_gateway_policy(),
            target_peer,
            authority_epoch,
            content_digest: p::SchemaDigest(String::new()),
        };
        envelope.refresh_digest()?;
        envelope.validate()?;
        Ok(envelope)
    }

    impl AgentHarness for FakeHarness {
        fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId> {
            self.submit_ingress(request, Vec::new())
        }

        fn stream_events(&self, run: p::RunId) -> EventStream {
            EventStream::new(
                self.runs
                    .lock()
                    .ok()
                    .and_then(|runs| runs.get(&run).cloned())
                    .map(|run| run.events)
                    .unwrap_or_default(),
            )
        }

        fn wait(&self, run: p::RunId) -> p::Result<p::RunResult> {
            self.runs
                .lock()
                .map_err(|_| p::Error("fake run state unavailable".into()))?
                .get(&run)
                .map(|run| run.result.clone())
                .ok_or_else(|| p::Error("fake run not found".into()))
        }

        fn cancel(&self, _run: p::RunId) -> p::Result<()> {
            Ok(())
        }

        fn resume(&self, _run: p::RunId, _input: ResumeInput) -> p::Result<()> {
            Ok(())
        }

        fn drain(&self, _session: p::SessionId) -> p::Result<()> {
            Ok(())
        }
    }

    impl HarnessIngress for FakeHarness {
        fn ingress_authority(&self) -> IngressAuthority {
            self.ingress_authority.clone()
        }

        fn submit_ingress(
            &self,
            request: p::RunRequest,
            prelude: Vec<IngressEvent>,
        ) -> p::Result<p::RunId> {
            self.requests
                .lock()
                .map_err(|_| p::Error("fake request capture unavailable".into()))?
                .push(request.clone());
            let id = self.sequence.fetch_add(1, Ordering::SeqCst);
            let run = p::RunId(format!("fake-run:{id}"));
            let mut events = prelude
                .into_iter()
                .enumerate()
                .map(|(index, event)| {
                    if !event.is_authorized_by(&self.ingress_authority) {
                        return Err(p::Error("fake harness rejected an ingress stamp".into()));
                    }
                    let (payload, provenance) = event.into_parts();
                    Ok(Self::event(&run, index as u64 + 1, payload, provenance))
                })
                .collect::<p::Result<Vec<_>>>()?;
            let provenance = if request.source == p::Source::UserTurn {
                p::Provenance {
                    source: request.source,
                    actor: p::Actor::Owner,
                    trust_tier: p::TrustTier::OwnerInput,
                    caused_by: None,
                }
            } else {
                p::Provenance {
                    source: request.source,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::Untrusted,
                    caused_by: None,
                }
            };
            let final_path = vec![
                p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                    source: request.source,
                    session_ref: p::SessionId(request.session.0),
                    input_ref: p::InputRef(request.input.0),
                    idempotency_key: request.idempotency_key,
                }),
                p::EventPayload::SessionBound(p::SessionBoundPayload {
                    policy_profile: p::PolicyProfileRef("policy:fake".into()),
                    model_profile: p::ModelProfileRef("model:fake".into()),
                    toolset_ref: p::ToolsetRef("toolset:fake".into()),
                    workspace: p::WorkspaceRef("workspace:fake".into()),
                    effect_mode: None,
                    evolution_snapshot: None,
                    federation_snapshot: None,
                }),
                p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 0 }),
                p::EventPayload::ContextBuildStarted(p::ContextBuildStartedPayload {
                    sources: Vec::new(),
                    slice_refs: Vec::new(),
                }),
                p::EventPayload::ContextBuildFinished(p::ContextBuildFinishedPayload {
                    sources: Vec::new(),
                    slice_refs: Vec::new(),
                }),
                p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
                    call_id: p::ModelCallId("model-call:fake".into()),
                    model_profile: p::ModelProfileRef("model:fake".into()),
                }),
                p::EventPayload::ModelCallFinished(p::ModelCallFinishedPayload {
                    call_id: p::ModelCallId("model-call:fake".into()),
                    model_profile: p::ModelProfileRef("model:fake".into()),
                    usage: p::ModelUsage {
                        input_tokens: 1,
                        output_tokens: 1,
                    },
                    finish_reason: p::FinishReason("stop".into()),
                }),
                p::EventPayload::OutputClassified(p::OutputClassifiedPayload {
                    kind: p::OutputKind::Final,
                }),
                p::EventPayload::VerificationStarted(p::VerificationStartedPayload {
                    verifier_kind: p::VerifierKind("deterministic".into()),
                    against: p::DoneContractRef("done:fake".into()),
                }),
                p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                    verifier_kind: p::VerifierKind("deterministic".into()),
                    outcome: p::VerificationOutcome::Pass,
                    against: p::DoneContractRef("done:fake".into()),
                }),
                p::EventPayload::TurnComplete(p::TurnCompletePayload { turn_index: 0 }),
                p::EventPayload::RunComplete(p::RunCompletePayload {
                    stop_reason: p::StopReason("final_output".into()),
                    result_ref: None,
                }),
            ];
            for payload in final_path {
                let sequence = events.len() as u64 + 1;
                events.push(Self::event(&run, sequence, payload, provenance.clone()));
            }
            let output = self
                .responses
                .lock()
                .map_err(|_| p::Error("fake responses unavailable".into()))?
                .pop_front()
                .ok_or_else(|| p::Error("fake response missing".into()))?;
            self.runs
                .lock()
                .map_err(|_| p::Error("fake run state unavailable".into()))?
                .insert(
                    run.clone(),
                    FakeRun {
                        events,
                        result: p::RunResult {
                            schema_version: p::SchemaVersion(1),
                            status: p::RunStatus::Complete,
                            stop_reason: p::StopReason("final_output".into()),
                            outputs: Vec::new(),
                            evidence_refs: Vec::new(),
                        },
                        output,
                    },
                );
            Ok(run)
        }

        fn resolve_approval(
            &self,
            _ticket: ApprovalTicket,
            _grant: ApprovalGrant,
        ) -> p::Result<()> {
            Ok(())
        }

        fn append_ingress_events(
            &self,
            run: p::RunId,
            events: Vec<IngressEvent>,
        ) -> p::Result<Vec<p::EventId>> {
            let mut runs = self
                .runs
                .lock()
                .map_err(|_| p::Error("fake run state unavailable".into()))?;
            let record = runs.entry(run.clone()).or_insert_with(|| FakeRun {
                events: Vec::new(),
                result: p::RunResult {
                    schema_version: p::SchemaVersion(1),
                    status: p::RunStatus::Complete,
                    stop_reason: p::StopReason("audit_only".into()),
                    outputs: Vec::new(),
                    evidence_refs: Vec::new(),
                },
                output: String::new(),
            });
            let mut ids = Vec::new();
            for event in events {
                if !event.is_authorized_by(&self.ingress_authority) {
                    return Err(p::Error("fake harness rejected an ingress stamp".into()));
                }
                let sequence = record.events.len() as u64 + 1;
                let (payload, provenance) = event.into_parts();
                let stored = Self::event(&run, sequence, payload, provenance);
                ids.push(stored.event_id.clone());
                record.events.push(stored);
            }
            Ok(ids)
        }

        fn result_text(&self, run: p::RunId) -> p::Result<Option<String>> {
            Ok(self
                .runs
                .lock()
                .map_err(|_| p::Error("fake run state unavailable".into()))?
                .get(&run)
                .map(|run| run.output.clone()))
        }
    }

    impl HarnessGatewayControl for FakeHarness {
        fn start_run(self: Arc<Self>, request: p::RunRequest) -> p::Result<p::RunId> {
            HarnessIngress::submit_ingress(self.as_ref(), request, Vec::new())
        }

        fn gateway_profile(&self, surface: p::SurfaceRef) -> p::Result<p::GatewayProfile> {
            Ok(p::GatewayProfile {
                schema_version: p::SchemaVersion(1),
                surface,
                policy: p::PolicyProfileRef("policy:fake".into()),
                model: p::ModelProfileRef("model:fake".into()),
                toolset: p::ToolsetRef("toolset:fake".into()),
                workspace: p::WorkspaceRef("workspace:fake".into()),
            })
        }

        fn list_runs(&self) -> p::Result<Vec<p::RunSummary>> {
            let run_ids = self
                .runs
                .lock()
                .map_err(|_| p::Error("fake run state unavailable".into()))?
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            run_ids
                .into_iter()
                .map(|run| HarnessGatewayControl::run_summary(self, run))
                .collect()
        }

        fn stream_event_page(&self, cursor: p::EventCursor) -> p::Result<p::EventPage> {
            cursor.validate()?;
            let events = self
                .runs
                .lock()
                .map_err(|_| p::Error("fake run state unavailable".into()))?
                .get(&cursor.run)
                .map(|run| run.events.clone())
                .ok_or_else(|| p::Error("fake run not found".into()))?;
            let snapshot_upper_bound = events.last().map(|event| event.stream_seq).unwrap_or(0);
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
            let runs = self
                .runs
                .lock()
                .map_err(|_| p::Error("fake run state unavailable".into()))?;
            let record = runs
                .get(&run)
                .ok_or_else(|| p::Error("fake run not found".into()))?;
            let (source, session) = record
                .events
                .iter()
                .find_map(|event| match &event.payload {
                    p::EventPayload::RunAccepted(payload) => {
                        Some((payload.source, payload.session_ref.clone()))
                    }
                    _ => None,
                })
                .ok_or_else(|| p::Error("fake run acceptance missing".into()))?;
            Ok(p::RunSummary {
                schema_version: p::SchemaVersion(1),
                run,
                source,
                session,
                workspace: Some(p::WorkspaceRef("workspace:fake".into())),
                status: record.result.status,
                last_stream_seq: record
                    .events
                    .last()
                    .map(|event| event.stream_seq)
                    .unwrap_or(0),
                result: Some(record.result.clone()),
            })
        }

        fn pending_approvals(&self, _session: p::SessionId) -> p::Result<Vec<p::PendingApproval>> {
            Ok(Vec::new())
        }

        fn control(&self, _run: p::RunId, _control: p::RunControl) -> p::Result<()> {
            Ok(())
        }

        fn trace_view(&self, run: p::RunId) -> p::Result<p::TraceView> {
            let events = self.stream_events(run.clone()).events();
            Ok(p::TraceView {
                schema_version: p::SchemaVersion(1),
                run,
                snapshot_upper_bound: events.last().map(|event| event.stream_seq).unwrap_or(0),
                failure_refs: Vec::new(),
                verification_outcomes: events
                    .iter()
                    .filter_map(|event| match &event.payload {
                        p::EventPayload::VerificationFinished(payload) => {
                            Some(payload.outcome.clone())
                        }
                        _ => None,
                    })
                    .collect(),
                events,
            })
        }

        fn review_candidate(&self, _command: p::CandidateReviewCommand) -> p::Result<()> {
            Ok(())
        }
    }

    impl EcosystemGatewayControl for FakeHarness {
        fn provision_capability_publisher(
            &self,
            _run: p::RunId,
            _grant: p::CapabilityPublisherGrant,
            _previous: Option<p::CapabilityPublisherGrantRef>,
            _expected: p::EcosystemAggregateVersion,
            _owner: p::VerifiedPrincipal,
        ) -> p::Result<p::ExpectedAppend> {
            Err(p::Error(
                "fake ecosystem publisher mutation is unsupported".into(),
            ))
        }

        fn admit_capability_package(
            &self,
            _run: p::RunId,
            _catalog_run: p::RunId,
            _package: p::SignedCapabilityPackage,
            _now: p::Timestamp,
            _owner: p::VerifiedPrincipal,
        ) -> p::Result<p::CapabilityPackageAdmission> {
            Err(p::Error(
                "fake ecosystem admission mutation is unsupported".into(),
            ))
        }

        fn prepare_capability_change(
            &self,
            _operation: p::CapabilityPackageOperation,
            _package: p::CapabilityPackageRef,
            _release: p::CapabilityReleaseRef,
            _owner: p::VerifiedPrincipal,
        ) -> p::Result<p::CapabilityInstallPlan> {
            Err(p::Error(
                "fake ecosystem lifecycle planning is unsupported".into(),
            ))
        }

        fn apply_capability_change(
            &self,
            _run: p::RunId,
            _plan: p::CapabilityInstallPlan,
            _approval: p::CapabilityPackageApproval,
            _reason: p::ReasonRef,
            _now: p::Timestamp,
        ) -> p::Result<p::CapabilityPackageState> {
            Err(p::Error(
                "fake ecosystem lifecycle mutation is unsupported".into(),
            ))
        }

        fn prepare_capability_distribution(
            &self,
            plan: p::CapabilityInstallPlan,
            target_peer: p::FederatedPeerRef,
            authority_epoch: p::AuthorityEpoch,
            owner: p::VerifiedPrincipal,
            now: p::Timestamp,
        ) -> p::Result<p::CapabilityPackageDistributionEnvelope> {
            self.ecosystem_distributions
                .lock()
                .map_err(|_| p::Error("fake ecosystem capture is unavailable".into()))?
                .push((
                    plan.clone(),
                    target_peer.clone(),
                    authority_epoch,
                    owner,
                    now,
                ));
            m5_gateway_distribution_envelope(plan, target_peer, authority_epoch)
        }

        fn capability_ecosystem_snapshot(
            &self,
            scope: p::Scope,
        ) -> p::Result<p::CapabilityEcosystemSnapshot> {
            Ok(p::CapabilityEcosystemSnapshot::empty(scope))
        }
    }

    impl EvolutionGatewayControl for FakeHarness {
        fn evolution_snapshot(&self, _scope: p::Scope) -> p::Result<p::EvolutionSnapshot> {
            Err(p::Error("fake evolution projection is empty".into()))
        }

        fn record_strategy_candidate(
            &self,
            _run: p::RunId,
            _candidate: p::StrategyCandidate,
        ) -> p::Result<p::EventId> {
            Err(p::Error("fake evolution candidate is unsupported".into()))
        }

        fn evaluate_strategy(
            &self,
            _run: p::RunId,
            _candidate: &p::StrategyCandidate,
            _comparison: p::EvolutionComparison,
        ) -> p::Result<EvolutionEvaluationResult> {
            Err(p::Error("fake evolution evaluation is unsupported".into()))
        }

        fn promote_strategy(
            &self,
            _run: p::RunId,
            _candidate: &p::StrategyCandidate,
            _evaluation: &p::EvolutionEvaluation,
        ) -> p::Result<p::EventId> {
            Err(p::Error("fake evolution promotion is unsupported".into()))
        }

        fn activate_strategy(
            &self,
            _run: p::RunId,
            _aggregate: p::EvolutionAggregateRef,
            _candidate: &p::StrategyCandidate,
            _evaluation: &p::EvolutionEvaluation,
            _promotion: p::EventId,
            _owner_confirmation: Option<p::OwnerControlRef>,
        ) -> p::Result<EvolutionActivationResult> {
            Err(p::Error("fake evolution activation is unsupported".into()))
        }

        fn rollback_strategy(
            &self,
            _run: p::RunId,
            _aggregate: p::EvolutionAggregateRef,
            _domain: p::StrategyDomain,
            _scope: p::Scope,
            _restored: p::StrategyVersionRef,
            _triggers: Vec<p::EvidenceRef>,
            _in_flight: p::InFlightDisposition,
            _owner_confirmation: Option<p::OwnerControlRef>,
        ) -> p::Result<EvolutionActivationResult> {
            Err(p::Error("fake evolution rollback is unsupported".into()))
        }

        fn set_auto_activation_paused(&self, paused: bool) {
            self.evolution_paused.store(paused, Ordering::SeqCst);
        }

        fn auto_activation_paused(&self) -> bool {
            self.evolution_paused.load(Ordering::SeqCst)
        }
    }

    impl HarnessFederationGatewayControl for FakeHarness {
        fn federation_snapshot(&self, _scope: p::Scope) -> p::Result<p::FederationSnapshot> {
            let mut grants = ["peer:owner-device-b", "peer:replica-b"]
                .into_iter()
                .map(|peer| {
                    Self::federated_grant(&p::FederatedPeerRef(peer.into()))
                        .expect("fake grant")
                        .reference()
                })
                .collect::<p::Result<Vec<_>>>()?;
            grants.sort_by(|left, right| left.0.cmp(&right.0));
            let authority = p::AuthorityRef("authority:fake".into());
            let authority_epoch = p::AuthorityEpoch(1);
            let registry_version = p::FederationAggregateVersion {
                schema_version: p::M4_SCHEMA_VERSION,
                aggregate: p::FederationAggregateRef("federation".into()),
                version: 2,
            };
            let digest =
                p::canonical_digest(&(&authority, authority_epoch, &registry_version, &grants))?;
            Ok(p::FederationSnapshot {
                schema_version: p::M4_SCHEMA_VERSION,
                authority,
                authority_epoch,
                registry_version,
                grants,
                digest,
            })
        }

        fn federated_peer(
            &self,
            peer: &p::FederatedPeerRef,
        ) -> p::Result<Option<p::FederatedPeerState>> {
            Ok(
                Self::federated_grant(peer).map(|grant| p::FederatedPeerState {
                    schema_version: p::M4_SCHEMA_VERSION,
                    grant,
                    revoked: false,
                    last_seen_at: None,
                }),
            )
        }

        fn register_federated_peer(
            &self,
            _run: p::RunId,
            _grant: p::FederatedPeerGrant,
            _previous: Option<p::FederatedPeerGrantRef>,
            _expected: p::FederationAggregateVersion,
            _owner: p::VerifiedPrincipal,
        ) -> p::Result<p::ExpectedAppend> {
            Err(p::Error("fake federation enrollment is unsupported".into()))
        }

        fn revoke_federated_peer(
            &self,
            _run: p::RunId,
            _peer: p::FederatedPeerRef,
            _grant: p::FederatedPeerGrantRef,
            _in_flight: p::InFlightDisposition,
            _expected: p::FederationAggregateVersion,
            _owner: p::VerifiedPrincipal,
        ) -> p::Result<p::ExpectedAppend> {
            Err(p::Error("fake federation revocation is unsupported".into()))
        }

        fn apply_federated_owner_command(
            &self,
            envelope: p::FederatedControlEnvelope,
            command: p::FederatedOwnerCommand,
            now: p::Timestamp,
        ) -> p::Result<FederationControlResult> {
            let result = match &command {
                p::FederatedOwnerCommand::ResolveApproval { .. } => {
                    FederationControlResult::ApprovalResolved {
                        run: p::RunId("run:fake-federation".into()),
                        terminal: false,
                    }
                }
                p::FederatedOwnerCommand::Cancel { lease, .. } => {
                    FederationControlResult::Cancelled {
                        lease: lease.clone(),
                        terminal: false,
                    }
                }
                p::FederatedOwnerCommand::RequestRetention(request) => {
                    FederationControlResult::RetentionRequested(request.request.clone())
                }
                p::FederatedOwnerCommand::Register(_) | p::FederatedOwnerCommand::Revoke { .. } => {
                    return Err(p::Error(
                        "fake owner-client enrollment is unsupported".into(),
                    ));
                }
            };
            self.federation_commands
                .lock()
                .map_err(|_| p::Error("fake federation capture is unavailable".into()))?
                .push((envelope, command, now));
            Ok(result)
        }

        fn federated_lease(
            &self,
            _lease: &p::RemoteExecutionLeaseRef,
        ) -> p::Result<Option<p::RemoteExecutionLease>> {
            Ok(None)
        }

        fn federated_retention_state(
            &self,
            _request: &p::RetentionRequestRef,
        ) -> p::Result<Option<forme_harness::RetentionState>> {
            Ok(None)
        }

        fn recover_federated_remote_action(
            &self,
            _run: &p::RunId,
        ) -> p::Result<forme_harness::RemoteRecoveryResult> {
            Err(p::Error("fake remote recovery is unsupported".into()))
        }

        fn acknowledge_federated_replication(
            &self,
            authenticated_peer: &p::FederatedPeerRef,
            batch: &p::ReplicationBatch,
            ack: p::ReplicationAck,
            expected: p::FederationAggregateVersion,
            _now: p::Timestamp,
        ) -> p::Result<p::ExpectedAppend> {
            batch.validate()?;
            ack.validate()?;
            let committed = expected.next()?;
            self.federation_acks
                .lock()
                .map_err(|_| p::Error("fake federation ack capture is unavailable".into()))?
                .push((authenticated_peer.clone(), ack));
            Ok(p::ExpectedAppend {
                schema_version: p::M4_SCHEMA_VERSION,
                status: p::ExpectedAppendStatus::Applied,
                expected_version: expected.version,
                actual_version: expected.version,
                resulting_version: committed.version,
                event_id: Some(p::EventId("fake:replication-checkpoint".into())),
            })
        }

        fn accept_federated_retention_receipt(
            &self,
            authenticated_peer: &p::FederatedPeerRef,
            receipt: p::FederatedRetentionReceipt,
            _now: p::Timestamp,
        ) -> p::Result<()> {
            self.federation_retention_receipts
                .lock()
                .map_err(|_| p::Error("fake retention capture is unavailable".into()))?
                .push((authenticated_peer.clone(), receipt));
            Ok(())
        }

        fn accept_federated_device_signal(
            &self,
            authenticated_peer: &p::FederatedPeerRef,
            signal: p::FederatedDeviceSignal,
            _now: p::Timestamp,
        ) -> p::Result<bool> {
            self.federation_signals
                .lock()
                .map_err(|_| p::Error("fake signal capture is unavailable".into()))?
                .push((authenticated_peer.clone(), signal));
            Ok(true)
        }
    }

    impl HarnessFederationActionGateway for FakeHarness {
        fn submit_federated_remote_action(
            &self,
            run: p::RunId,
            request: p::RunRequest,
            intent: p::ActionIntent,
        ) -> p::Result<forme_harness::RemoteActionSubmission> {
            self.federation_actions
                .lock()
                .map_err(|_| p::Error("fake federation action capture is unavailable".into()))?
                .push((run.clone(), request, intent));
            Ok(forme_harness::RemoteActionSubmission {
                run,
                approval: p::ApprovalId("approval:fake-federation".into()),
                plan_digest: p::PlanDigest("sha256:fake-federation-plan".into()),
                federation_snapshot: p::FederationSnapshotRef(
                    "sha256:fake-federation-snapshot".into(),
                ),
                placement_decision: p::FederatedPlacementDecision {
                    schema_version: p::M4_SCHEMA_VERSION,
                    decision: p::PlacementDecisionRef("decision:fake-federation".into()),
                    federation_snapshot: p::FederationSnapshotRef(
                        "sha256:fake-federation-snapshot".into(),
                    ),
                    scope: p::Scope("workspace:m4".into()),
                    capability: p::CapabilityRef("fixture.mutate".into()),
                    evaluated_at: 1,
                    candidates: Vec::new(),
                    chosen: None,
                    placement: None,
                    digest: p::SchemaDigest("sha256:fake-placement-decision".into()),
                },
            })
        }
    }

    fn response(text: &str) -> String {
        text.into()
    }

    fn gateway(responses: Vec<String>) -> LocalGateway<FakeHarness> {
        let harness = Arc::new(FakeHarness::new(responses));
        LocalGateway::new(
            harness,
            p::VerifiedPrincipal("owner:test".into()),
            p::AgentProfileRef("agent:test".into()),
        )
        .unwrap()
    }

    fn external_grant() -> ExternalCommunicationGrant {
        ExternalCommunicationGrant {
            schema_version: p::SchemaVersion(1),
            grant_ref: p::GrantRef("gateway-external-grant".into()),
            participant: ParticipantProfile::External(p::ParticipantId("person-a".into())),
            purpose: CommunicationPurpose {
                schema_version: p::SchemaVersion(1),
                id: p::PurposeRef("bounded-support".into()),
                description: "Answer one scoped support question".into(),
            },
            modality_set: vec![p::Modality::Text],
            disclosure: DisclosurePolicy {
                schema_version: p::SchemaVersion(1),
                id: p::DisclosurePolicyRef("disclosure:gateway-test".into()),
                allow: vec![DisclosureRule {
                    schema_version: p::SchemaVersion(1),
                    category: "public".into(),
                    outcome: DisclosureOutcome::Answer,
                    high_impact_requires_approval: true,
                }],
                representation: RepresentationRule::AgentOnly,
            },
            termination: TerminationPolicy {
                schema_version: p::SchemaVersion(1),
                max_rounds: 1,
                terminate_on_purpose_drift: true,
                terminate_on_risk_escalation: true,
            },
            autonomy_envelope: p::AutonomyEnvelope {
                schema_version: p::SchemaVersion(1),
                scope: p::Scope("communication:gateway-test".into()),
                capability: p::CapabilitySet {
                    schema_version: p::SchemaVersion(1),
                    capabilities: vec![p::CapabilityRef("channel:text".into())],
                    permissions: vec![p::PermissionRef("communicate".into())],
                },
                action_type: vec![p::ActionType::Deliver],
                risk_limit: p::Risk::Medium,
                approval_rule: p::ApprovalRule::Ask,
                budget: p::Budget("bounded".into()),
                timebox: p::Timebox {
                    schema_version: p::SchemaVersion(1),
                    starts_at: 100,
                    expires_at: 10_100,
                    max_turns: 4,
                },
                rollback: p::RollbackReq {
                    schema_version: p::SchemaVersion(1),
                    required: false,
                    boundary: None,
                },
            },
            ttl: p::DurationMs(1_000),
            budget: CommunicationBudget {
                schema_version: p::SchemaVersion(1),
                max_messages: 4,
                max_tokens: 100,
            },
            transcript_policy: TranscriptPolicy {
                schema_version: p::SchemaVersion(1),
                id: p::TranscriptPolicyRef("candidate-only".into()),
                retain: true,
                candidate_only: true,
            },
            issued_at: 100,
        }
    }

    #[test]
    fn s12_adapter_enters_through_gateway_and_gateway_stamps_external_provenance() {
        let gateway = gateway(vec![response("external answer")]);
        let adapter = TextChannelAdapter {
            schema_version: p::SchemaVersion(1),
            id: p::ProviderId("channel:external".into()),
            participant: ParticipantProfile::External(p::ParticipantId("person-a".into())),
            scope: p::Scope("communication:test".into()),
            enabled: true,
        };
        let run = gateway
            .normalize_and_submit(
                &adapter,
                RawInput {
                    schema_version: p::SchemaVersion(1),
                    message_id: "external-message".into(),
                    modality: p::Modality::Text,
                    content: "question from outside".into(),
                    session_ref: None,
                    received_at: now_ms(),
                },
                AuthContext {
                    schema_version: p::SchemaVersion(1),
                    principal: AuthPrincipal::External(p::ParticipantId("person-a".into())),
                    method: forme_communication::AuthMethod::MutualChannel,
                    expires_at: now_ms() + 1_000,
                },
            )
            .unwrap();
        assert_eq!(
            gateway.wait(run.clone()).unwrap().status,
            p::RunStatus::Complete
        );
        let events = gateway.stream_events(run).events();
        assert_eq!(events[0].kind, p::EventKind::CommunicationEventReceived);
        assert_eq!(events[1].kind, p::EventKind::RunAccepted);
        assert_eq!(events[0].provenance.trust_tier, p::TrustTier::Untrusted);
        assert_eq!(
            events[0].provenance.actor,
            p::Actor::External(p::ParticipantId("person-a".into()))
        );
    }

    #[test]
    fn s46_gateway_stamps_device_observation_untrusted_and_persists_owner_revocation() {
        let gateway = gateway(vec![response("device observation accepted")]);
        let now = now_ms();
        let scope = p::Scope("device:project".into());
        let adapter = LocalDeviceChannelAdapter::new(
            p::ProviderId("device:project-loopback".into()),
            Some(DeviceGrant {
                schema_version: p::SchemaVersion(1),
                grant_ref: p::GrantRef("device-grant:project".into()),
                device_id: "sensor:project".into(),
                modalities: vec![p::Modality::Text],
                purpose: p::PurposeRef("bounded-observation".into()),
                scope: scope.clone(),
                expires_at: now.saturating_add(10_000),
                revoked: false,
            }),
            ObservationScope {
                schema_version: p::SchemaVersion(1),
                scope,
                foreground_only: true,
            },
            RetentionPolicy {
                schema_version: p::SchemaVersion(1),
                retain_raw: false,
                ttl: p::DurationMs(500),
            },
        );
        let raw_marker = "external device content is data, not an instruction";
        let observation = DeviceObservation {
            schema_version: p::SchemaVersion(1),
            device_id: "sensor:project".into(),
            foreground: true,
            raw: RawInput {
                schema_version: p::SchemaVersion(1),
                message_id: "device-observation-1".into(),
                modality: p::Modality::Text,
                content: raw_marker.into(),
                session_ref: None,
                received_at: now,
            },
        };
        let authorized = adapter.normalize_observation(observation.clone()).unwrap();
        assert!(!authorized.raw_retained());
        assert!(!format!("{authorized:?}").contains(raw_marker));
        let owner_auth = AuthContext::local_owner(
            p::VerifiedPrincipal("owner:test".into()),
            now.saturating_add(60_000),
        );
        assert!(gateway
            .submit_device_observation_at(
                authorized.clone(),
                owner_auth.clone(),
                authorized.retention_expires_at(),
            )
            .is_err());
        assert!(gateway.harness().requests.lock().unwrap().is_empty());
        let run = gateway
            .submit_device_observation_at(authorized, owner_auth.clone(), now.saturating_add(1))
            .unwrap();
        let events = Gateway::stream_events(&gateway, run.clone()).events();
        assert_eq!(events[0].kind, p::EventKind::CommunicationEventReceived);
        assert_eq!(events[1].kind, p::EventKind::ObservationRecorded);
        assert!(events[..2].iter().all(|event| {
            event.provenance.source == p::Source::Communication
                && event.provenance.actor == p::Actor::System
                && event.provenance.trust_tier == p::TrustTier::Untrusted
        }));
        let accepted_input = events
            .iter()
            .find_map(|event| match &event.payload {
                p::EventPayload::RunAccepted(payload) => Some(payload.input_ref.0.as_str()),
                _ => None,
            })
            .unwrap();
        assert!(!accepted_input.contains(raw_marker));
        let projection = gateway.run_summary(run, owner_auth.clone()).unwrap();
        let persisted = serde_json::to_string(&(events, projection)).unwrap();
        assert!(!persisted.contains(raw_marker));

        assert!(gateway
            .revoke_device(
                &adapter,
                p::LineageRef("external:forged-revocation".into()),
                AuthContext {
                    schema_version: p::SchemaVersion(1),
                    principal: AuthPrincipal::External(p::ParticipantId("person-a".into())),
                    method: AuthMethod::MutualChannel,
                    expires_at: now.saturating_add(60_000),
                },
            )
            .is_err());
        gateway
            .revoke_device(
                &adapter,
                p::LineageRef("owner:device-revocation".into()),
                owner_auth,
            )
            .unwrap();
        let revocation = Gateway::stream_events(
            &gateway,
            p::RunId("device-revocation:device:project-loopback".into()),
        )
        .events();
        assert_eq!(revocation.len(), 1);
        assert_eq!(revocation[0].kind, p::EventKind::RevocationEvent);
        assert_eq!(revocation[0].provenance.actor, p::Actor::Owner);
        assert_eq!(
            revocation[0].provenance.trust_tier,
            p::TrustTier::OwnerInput
        );
        assert!(adapter.normalize_observation(observation).is_err());
    }

    #[test]
    fn s47_gateway_requires_mutual_channel_and_stamps_external_agent_untrusted() {
        let gateway = gateway(vec![response("bounded peer input accepted")]);
        let now = now_ms();
        let participant = p::ParticipantId("peer:project-agent".into());
        let event = CommunicationEvent {
            schema_version: p::SchemaVersion(1),
            message_id: "a2a-message-1".into(),
            modality: p::Modality::Text,
            carrier: Carrier::Software,
            channel_adapter: p::ProviderId("channel:project-a2a".into()),
            participant: ParticipantProfile::ExternalAgent(participant.clone()),
            scope: p::Scope("communication:a2a-project".into()),
            session_ref: Some(p::CommunicationSessionId("a2a:bounded-session".into())),
            content: "peer-provided data".into(),
            received_at: now,
        };
        assert!(gateway
            .submit_event_with_auth(
                event.clone(),
                AuthContext {
                    schema_version: p::SchemaVersion(1),
                    principal: AuthPrincipal::External(participant.clone()),
                    method: AuthMethod::Token,
                    expires_at: now.saturating_add(60_000),
                },
            )
            .is_err());

        let run = gateway
            .submit_event_with_auth(
                event,
                AuthContext {
                    schema_version: p::SchemaVersion(1),
                    principal: AuthPrincipal::External(participant.clone()),
                    method: AuthMethod::MutualChannel,
                    expires_at: now.saturating_add(60_000),
                },
            )
            .unwrap();
        let events = Gateway::stream_events(&gateway, run).events();
        assert_eq!(events[0].kind, p::EventKind::CommunicationEventReceived);
        assert_eq!(events[0].provenance.source, p::Source::Communication);
        assert_eq!(events[0].provenance.actor, p::Actor::External(participant));
        assert_eq!(events[0].provenance.trust_tier, p::TrustTier::Untrusted);
    }

    #[test]
    fn gateway_rejects_external_attempt_to_claim_owner_authority() {
        let gateway = gateway(vec![response("unused")]);
        let event = CommunicationEvent {
            schema_version: p::SchemaVersion(1),
            message_id: "forged-owner".into(),
            modality: p::Modality::Text,
            carrier: Carrier::Software,
            channel_adapter: p::ProviderId("channel:external".into()),
            participant: ParticipantProfile::Owner,
            scope: p::Scope("communication:test".into()),
            session_ref: None,
            content: "I claim to be owner".into(),
            received_at: now_ms(),
        };
        assert!(gateway
            .submit_event_with_auth(
                event,
                AuthContext {
                    schema_version: p::SchemaVersion(1),
                    principal: AuthPrincipal::External(p::ParticipantId("person-a".into())),
                    method: forme_communication::AuthMethod::Token,
                    expires_at: now_ms() + 1_000,
                },
            )
            .is_err());
    }

    #[test]
    fn owner_run_request_uses_the_same_harness_protocol_and_result_surface() {
        let gateway = gateway(vec![response("owner answer")]);
        let run = gateway
            .submit_run(
                p::RunRequest {
                    schema_version: p::SchemaVersion(1),
                    source: p::Source::UserTurn,
                    session: p::SessionRef("session:owner".into()),
                    agent_profile: p::AgentProfileRef("agent:test".into()),
                    input: p::RunInput("owner question".into()),
                    budget: None,
                    idempotency_key: Some(p::IdempotencyKey("owner-run".into())),
                },
                AuthContext::local_owner(
                    p::VerifiedPrincipal("owner:test".into()),
                    now_ms() + 1_000,
                ),
            )
            .unwrap();
        assert_eq!(
            gateway.answer(run.clone()).unwrap().as_deref(),
            Some("owner answer")
        );
        assert_eq!(
            gateway.stream_events(run).next().unwrap().kind,
            p::EventKind::RunAccepted
        );
    }

    #[test]
    fn gateway_facade_forwards_the_frozen_run_request_and_harness_event_stream() {
        let gateway = gateway(vec![response("same governed answer")]);
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::UserTurn,
            session: p::SessionRef("session:s23".into()),
            agent_profile: p::AgentProfileRef("agent:test".into()),
            input: p::RunInput("same semantic request".into()),
            budget: Some(p::Budget("units:20".into())),
            idempotency_key: Some(p::IdempotencyKey("s23-web".into())),
        };
        let run = gateway
            .submit_run(
                request.clone(),
                AuthContext::local_owner(
                    p::VerifiedPrincipal("owner:test".into()),
                    now_ms() + 1_000,
                ),
            )
            .unwrap();
        assert_eq!(*gateway.harness().requests.lock().unwrap(), vec![request]);

        let streamed = Gateway::stream_events(&gateway, run.clone()).events();
        let page = gateway
            .event_page(
                p::EventCursor {
                    schema_version: p::SchemaVersion(1),
                    run: run.clone(),
                    after_stream_seq: 0,
                },
                AuthContext::local_owner(
                    p::VerifiedPrincipal("owner:test".into()),
                    now_ms() + 1_000,
                ),
            )
            .unwrap();
        assert_eq!(page.events, streamed);
        assert_eq!(
            page.events
                .iter()
                .map(|event| event.kind)
                .collect::<Vec<_>>(),
            vec![
                p::EventKind::RunAccepted,
                p::EventKind::SessionBound,
                p::EventKind::TurnStarted,
                p::EventKind::ContextBuildStarted,
                p::EventKind::ContextBuildFinished,
                p::EventKind::ModelCallStarted,
                p::EventKind::ModelCallFinished,
                p::EventKind::OutputClassified,
                p::EventKind::VerificationStarted,
                p::EventKind::VerificationFinished,
                p::EventKind::TurnComplete,
                p::EventKind::RunComplete,
            ]
        );
        assert_eq!(
            gateway
                .run_summary(
                    run.clone(),
                    AuthContext::local_owner(
                        p::VerifiedPrincipal("owner:test".into()),
                        now_ms() + 1_000,
                    ),
                )
                .unwrap()
                .status,
            p::RunStatus::Complete
        );
        assert!(gateway
            .trace_view(
                run,
                AuthContext {
                    schema_version: p::SchemaVersion(1),
                    principal: AuthPrincipal::External(p::ParticipantId("outside".into())),
                    method: forme_communication::AuthMethod::Token,
                    expires_at: now_ms() + 1_000,
                },
            )
            .is_err());
    }

    #[test]
    fn s44_external_session_requires_owner_auth_and_records_owner_grant_provenance() {
        let gateway = gateway(Vec::new());
        let fabric = InMemoryCommunicationFabric::with_clock(|| 100);
        assert!(gateway
            .open_external_session(
                &fabric,
                external_grant(),
                AuthContext {
                    schema_version: p::SchemaVersion(1),
                    principal: AuthPrincipal::External(p::ParticipantId("person-a".into())),
                    method: AuthMethod::MutualChannel,
                    expires_at: now_ms().saturating_add(1_000),
                },
            )
            .is_err());
        assert!(fabric.take_events().is_empty());
        let session = gateway
            .open_external_session(
                &fabric,
                external_grant(),
                AuthContext::local_owner(
                    p::VerifiedPrincipal("owner:test".into()),
                    now_ms().saturating_add(1_000),
                ),
            )
            .unwrap();
        let run = p::RunId(format!("communication:{}", session.0));
        let events = gateway.stream_events(run).events();

        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            vec![
                p::EventKind::ExternalCommunicationGranted,
                p::EventKind::CommunicationSessionOpened,
            ]
        );
        assert_eq!(
            events
                .iter()
                .map(|event| event.stream_seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(events[0].provenance.source, p::Source::Communication);
        assert_eq!(events[0].provenance.actor, p::Actor::Owner);
        assert_eq!(events[0].provenance.trust_tier, p::TrustTier::OwnerInput);
        assert_eq!(events[1].provenance.source, p::Source::Communication);
        assert_eq!(events[1].provenance.actor, p::Actor::System);
        assert_eq!(
            events[1].provenance.trust_tier,
            p::TrustTier::VerifiedProcess
        );
        assert!(fabric.take_events().is_empty());
    }

    #[test]
    fn m3_evolution_controls_require_authenticated_owner_and_expose_pause_to_cli() {
        let gateway = gateway(Vec::new());
        let external = AuthContext {
            schema_version: p::SchemaVersion(1),
            principal: AuthPrincipal::External(p::ParticipantId("outside".into())),
            method: AuthMethod::Token,
            expires_at: now_ms().saturating_add(1_000),
        };
        assert!(gateway.set_auto_activation_paused(true, external).is_err());
        let owner = || {
            AuthContext::local_owner(
                p::VerifiedPrincipal("owner:test".into()),
                now_ms().saturating_add(1_000),
            )
        };
        assert!(!gateway.auto_activation_paused(owner()).unwrap());
        gateway.set_auto_activation_paused(true, owner()).unwrap();
        assert!(gateway.auto_activation_paused(owner()).unwrap());
        assert!(EvolutionRunGateway::auto_activation_paused(&gateway).unwrap());
    }

    #[test]
    fn s79_owner_client_requires_peer_channel_and_independent_owner_before_harness_control() {
        let gateway = gateway(Vec::new());
        let owner_client = gateway.owner_client_gateway();
        let now = now_ms();
        let command = p::FederatedOwnerCommand::Cancel {
            lease: p::RemoteExecutionLeaseRef("lease:s79-gateway".into()),
            reason: p::ReasonRef("owner cancellation".into()),
        };
        let envelope = p::FederatedControlEnvelope {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: p::FederatedPeerRef("peer:owner-device-b".into()),
            session: p::FederatedSessionRef("session:owner-device-b".into()),
            owner: p::VerifiedPrincipal("owner:test".into()),
            nonce: p::Nonce("nonce:s79-gateway".into()),
            expires_at: now.saturating_add(60_000),
            command_digest: p::canonical_digest(&command).unwrap(),
        };
        assert!(owner_client
            .authenticate_owner_client_channel(
                envelope.peer.clone(),
                envelope.session.clone(),
                p::TransportIdentityDigest("sha256:wrong-device".into()),
                now.saturating_add(60_000),
            )
            .is_err());
        assert!(owner_client
            .authenticate_owner_client_channel(
                envelope.peer.clone(),
                envelope.session.clone(),
                p::TransportIdentityDigest("sha256:owner-device-b".into()),
                now.saturating_sub(1),
            )
            .is_err());
        let channel = owner_client
            .authenticate_owner_client_channel(
                envelope.peer.clone(),
                envelope.session.clone(),
                p::TransportIdentityDigest("sha256:owner-device-b".into()),
                now.saturating_add(60_000),
            )
            .unwrap();
        let owner = || {
            AuthContext::local_owner(
                p::VerifiedPrincipal("owner:test".into()),
                now.saturating_add(60_000),
            )
        };

        assert!(owner_client
            .apply(
                &channel,
                envelope.clone(),
                command.clone(),
                AuthContext {
                    schema_version: p::M4_SCHEMA_VERSION,
                    principal: AuthPrincipal::Owner(p::VerifiedPrincipal("owner:test".into())),
                    method: AuthMethod::FederatedPeer,
                    expires_at: now.saturating_add(60_000),
                },
            )
            .is_err());
        assert!(owner_client
            .apply(
                &channel,
                envelope.clone(),
                command.clone(),
                AuthContext {
                    schema_version: p::M4_SCHEMA_VERSION,
                    principal: AuthPrincipal::External(p::ParticipantId("outside".into())),
                    method: AuthMethod::Token,
                    expires_at: now.saturating_add(60_000),
                },
            )
            .is_err());
        let mut wrong_owner = envelope.clone();
        wrong_owner.owner = p::VerifiedPrincipal("owner:other".into());
        assert!(owner_client
            .apply(&channel, wrong_owner, command.clone(), owner())
            .is_err());
        let mut wrong_session = envelope.clone();
        wrong_session.session = p::FederatedSessionRef("session:other".into());
        assert!(owner_client
            .apply(&channel, wrong_session, command.clone(), owner())
            .is_err());
        assert!(gateway
            .harness()
            .federation_commands
            .lock()
            .unwrap()
            .is_empty());

        assert!(matches!(
            owner_client
                .apply(&channel, envelope, command, owner(),)
                .unwrap(),
            FederationControlResult::Cancelled {
                terminal: false,
                ..
            }
        ));
        assert_eq!(
            gateway.harness().federation_commands.lock().unwrap().len(),
            1
        );
    }

    #[test]
    fn s75_s78_s82_peer_ingress_is_channel_bound_before_harness() {
        let gateway = gateway(Vec::new());
        let peer_gateway = gateway.owner_client_gateway();
        let now = now_ms();
        let replica = p::FederatedPeerRef("peer:replica-b".into());
        assert!(peer_gateway
            .authenticate_retention_receipt_channel(
                replica.clone(),
                p::FederatedSessionRef("session:replica-b".into()),
                p::TransportIdentityDigest("sha256:wrong-replica".into()),
                now.saturating_add(60_000),
            )
            .is_err());
        let retention_channel = peer_gateway
            .authenticate_retention_receipt_channel(
                replica.clone(),
                p::FederatedSessionRef("session:replica-b".into()),
                p::TransportIdentityDigest("sha256:replica-b".into()),
                now.saturating_add(60_000),
            )
            .unwrap();
        let replica_channel = peer_gateway
            .authenticate_replica_channel(
                replica.clone(),
                p::FederatedSessionRef("session:replica-b".into()),
                p::TransportIdentityDigest("sha256:replica-b".into()),
                now.saturating_add(60_000),
            )
            .unwrap();
        let aggregate = p::RunId("run:gateway-replication".into());
        let from =
            p::ReplicationCursor::zero(replica.clone(), aggregate.clone(), p::AuthorityEpoch(1));
        let to = p::ReplicationCursor {
            stream_seq: 1,
            ..from.clone()
        };
        let mut batch = p::ReplicationBatch {
            schema_version: p::M4_SCHEMA_VERSION,
            batch: p::ReplicationBatchRef("batch:gateway".into()),
            peer: replica.clone(),
            peer_grant: FakeHarness::federated_grant(&replica)
                .unwrap()
                .reference()
                .unwrap(),
            aggregate: aggregate.clone(),
            from,
            to: to.clone(),
            redaction: p::RedactionPolicyRef("redaction:gateway".into()),
            events: vec![p::SyncTransferEvent {
                schema_version: p::M4_SCHEMA_VERSION,
                event_id: p::EventId("event:gateway-replication".into()),
                aggregate: aggregate.clone(),
                source_stream_seq: 1,
                turn_id: None,
                kind: p::EventKind::TurnStarted,
                payload: p::SyncTransferPayload::Full(Box::new(p::EventPayload::TurnStarted(
                    p::TurnStartedPayload { turn_index: 0 },
                ))),
                event_schema_version: p::M4_SCHEMA_VERSION,
                ts_unix_ms: now,
                provenance: p::Provenance {
                    source: p::Source::Internal,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::VerifiedProcess,
                    caused_by: None,
                },
            }],
            content_digest: p::SchemaDigest(String::new()),
        };
        batch.refresh_digest().unwrap();
        let ack = p::ReplicationAck {
            schema_version: p::M4_SCHEMA_VERSION,
            batch: batch.batch.clone(),
            peer: replica.clone(),
            aggregate,
            applied: to,
            projection_digest: p::SchemaDigest("sha256:gateway-projection".into()),
        };
        let expected = p::FederationAggregateVersion {
            schema_version: p::M4_SCHEMA_VERSION,
            aggregate: p::FederationAggregateRef("federation".into()),
            version: 2,
        };
        assert!(peer_gateway
            .acknowledge_replication(&retention_channel, &batch, ack.clone(), expected.clone(),)
            .is_err());
        peer_gateway
            .acknowledge_replication(&replica_channel, &batch, ack, expected)
            .unwrap();
        assert_eq!(gateway.harness().federation_acks.lock().unwrap().len(), 1);

        let mut receipt = p::FederatedRetentionReceipt {
            schema_version: p::M4_SCHEMA_VERSION,
            receipt: p::RetentionReceiptRef("retention-receipt:gateway".into()),
            request: p::RetentionRequestRef("retention:gateway".into()),
            peer: replica.clone(),
            authority_epoch: p::AuthorityEpoch(1),
            deleted_projection: p::ReplicaProjectionDigestRef(
                "sha256:deleted-replica-projection".into(),
            ),
            evidence: vec![p::EvidenceRef("remote-delete:gateway".into())],
            observed_at: now,
            digest: p::SchemaDigest(String::new()),
        };
        receipt.refresh_digest().unwrap();
        let mut spoofed_receipt = receipt.clone();
        spoofed_receipt.peer = p::FederatedPeerRef("peer:owner-device-b".into());
        spoofed_receipt.refresh_digest().unwrap();
        assert!(peer_gateway
            .accept_retention_receipt(&retention_channel, spoofed_receipt)
            .is_err());
        assert!(gateway
            .harness()
            .federation_retention_receipts
            .lock()
            .unwrap()
            .is_empty());
        peer_gateway
            .accept_retention_receipt(&retention_channel, receipt)
            .unwrap();
        assert_eq!(
            gateway
                .harness()
                .federation_retention_receipts
                .lock()
                .unwrap()
                .len(),
            1
        );

        let owner_peer = p::FederatedPeerRef("peer:owner-device-b".into());
        let owner_session = p::FederatedSessionRef("session:owner-device-b".into());
        let signal_channel = peer_gateway
            .authenticate_device_signal_channel(
                owner_peer.clone(),
                owner_session.clone(),
                p::TransportIdentityDigest("sha256:owner-device-b".into()),
                now.saturating_add(60_000),
            )
            .unwrap();
        let mut signal = p::FederatedDeviceSignal {
            schema_version: p::M4_SCHEMA_VERSION,
            signal: p::FederatedDeviceSignalRef("signal:gateway".into()),
            peer: owner_peer,
            session: owner_session,
            kind: p::FederatedSignalKind::Tick,
            nonce: p::Nonce("nonce:gateway-signal".into()),
            observed_at: now,
            expires_at: now.saturating_add(60_000),
            digest: p::SchemaDigest(String::new()),
        };
        signal.refresh_digest().unwrap();
        let mut spoofed_signal = signal.clone();
        spoofed_signal.session = p::FederatedSessionRef("session:spoofed".into());
        spoofed_signal.refresh_digest().unwrap();
        assert!(peer_gateway
            .accept_device_signal(&signal_channel, spoofed_signal)
            .is_err());
        assert!(gateway
            .harness()
            .federation_signals
            .lock()
            .unwrap()
            .is_empty());
        assert!(peer_gateway
            .accept_device_signal(&signal_channel, signal)
            .unwrap());
        assert_eq!(
            gateway.harness().federation_signals.lock().unwrap().len(),
            1
        );
    }

    #[test]
    fn s74_gateway_remote_action_requires_owner_auth_and_routes_only_through_harness() {
        let gateway = gateway(Vec::new());
        let now = now_ms();
        let request = p::RunRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            source: p::Source::UserTurn,
            session: p::SessionRef("session:s74-gateway".into()),
            agent_profile: p::AgentProfileRef("agent:forme".into()),
            input: p::RunInput("perform one remote mutation".into()),
            budget: Some(p::Budget("units:1".into())),
            idempotency_key: Some(p::IdempotencyKey("s74-gateway-once".into())),
        };
        let mut operation = p::RemoteOperation {
            schema_version: p::M4_SCHEMA_VERSION,
            backend: p::BackendKind::File,
            parameters: p::ActionParameters::File {
                operation: p::FileOperation::Write,
                path: "golden/state".into(),
                content: Some(b"mutation".to_vec()),
            },
            capability: p::CapabilityRef("fixture.mutate".into()),
            scope: p::Scope("workspace:m4".into()),
            action_type: p::ActionType::ExternalCommit,
            expected_effect: p::ExpectedEffect::Outward,
            rollback_boundary: p::RollbackBoundary("fixture-record-only".into()),
            credential_slot: None,
            digest: p::SchemaDigest(String::new()),
        };
        operation.refresh_digest().unwrap();
        let mut placement = p::RemotePlacementPlan {
            schema_version: p::M4_SCHEMA_VERSION,
            executor: p::FederatedPeerRef("peer:s74-executor".into()),
            peer_grant: p::FederatedPeerGrantRef("sha256:s74-grant".into()),
            grant_version: p::PeerGrantVersion(1),
            authority_epoch: p::AuthorityEpoch(1),
            executor_profile: p::ExecutorProfileRef("profile:s74".into()),
            operation,
            digest: p::SchemaDigest(String::new()),
        };
        placement.refresh_digest().unwrap();
        let intent = p::ActionIntent {
            schema_version: p::M4_SCHEMA_VERSION,
            intent_id: p::ActionId("intent:s74-gateway".into()),
            source: p::Source::UserTurn,
            goal: p::GoalRef("one governed remote mutation".into()),
            backend_hint: p::BackendKind::Remote,
            capability_ref: p::CapabilityRef("fixture.mutate".into()),
            action_type: p::ActionType::ExternalCommit,
            scope: p::Scope("workspace:m4".into()),
            risk_hint: p::Risk::High,
            expected_effect: p::ExpectedEffect::Outward,
            rollback_expectation: p::RollbackBoundary("fixture-record-only".into()),
            parameters: p::ActionParameters::Remote(Box::new(p::RemoteActionSpec {
                schema_version: p::M4_SCHEMA_VERSION,
                placement,
            })),
            requested_permissions: vec![p::PermissionRef("permission:remote-execute".into())],
            requested_at: now,
            estimated_output_bytes: 1,
            estimated_duration: p::DurationMs(1),
        };

        assert!(gateway
            .submit_remote_action(
                request.clone(),
                intent.clone(),
                AuthContext {
                    schema_version: p::M4_SCHEMA_VERSION,
                    principal: AuthPrincipal::External(p::ParticipantId("outside".into())),
                    method: AuthMethod::Token,
                    expires_at: now.saturating_add(60_000),
                },
            )
            .is_err());
        assert!(gateway
            .harness()
            .federation_actions
            .lock()
            .unwrap()
            .is_empty());

        let submission = gateway
            .submit_remote_action(
                request.clone(),
                intent.clone(),
                AuthContext::local_owner(
                    p::VerifiedPrincipal("owner:test".into()),
                    now.saturating_add(60_000),
                ),
            )
            .unwrap();
        let captured = gateway.harness().federation_actions.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, submission.run);
        assert_eq!(captured[0].1, request);
        assert_eq!(captured[0].2, intent);
        assert!(submission.run.0.starts_with("gateway:remote-action:"));
    }

    #[test]
    fn s95_gateway_distribution_prepare_is_owner_only_and_has_no_execution_side_effect() {
        let harness = Arc::new(FakeHarness::new(Vec::new()));
        let gateway = LocalGateway::new(
            harness.clone(),
            p::VerifiedPrincipal("owner:test".into()),
            p::AgentProfileRef("agent:test".into()),
        )
        .unwrap();
        let package = m5_gateway_package();
        let admission = m5_gateway_admission(&package);
        let mut plan = p::CapabilityInstallPlan {
            schema_version: p::M5_SCHEMA_VERSION,
            reference: p::CapabilityInstallPlanRef("plan:m5-gateway:distribute".into()),
            operation: p::CapabilityPackageOperation::Distribute,
            package: package.manifest.package.clone(),
            release: package.manifest.release.clone(),
            package_digest: package.package_digest.clone(),
            admission: admission.reference,
            scope: package.manifest.scope.clone(),
            policy: p::CapabilityPolicyRef("policy:m5-gateway".into()),
            policy_version: p::Version(1),
            expected_version: p::EcosystemAggregateVersion {
                schema_version: p::M5_SCHEMA_VERSION,
                value: 4,
            },
            previous_release: None,
            rollback_boundary: p::RollbackBoundary("remote package bytes remain historical".into()),
            digest: p::PlanDigest(String::new()),
        };
        plan.refresh_digest().unwrap();
        plan.validate().unwrap();
        let peer = p::FederatedPeerRef("peer:m5-gateway-executor".into());
        let epoch = p::AuthorityEpoch(2);
        let now = now_ms();

        assert!(gateway
            .prepare_capability_distribution(
                plan.clone(),
                peer.clone(),
                epoch,
                AuthContext {
                    schema_version: p::M5_SCHEMA_VERSION,
                    principal: AuthPrincipal::External(p::ParticipantId("outside".into())),
                    method: AuthMethod::Token,
                    expires_at: now.saturating_add(60_000),
                },
            )
            .is_err());
        assert!(harness.ecosystem_distributions.lock().unwrap().is_empty());

        let envelope = gateway
            .prepare_capability_distribution(
                plan.clone(),
                peer.clone(),
                epoch,
                AuthContext::local_owner(
                    p::VerifiedPrincipal("owner:test".into()),
                    now.saturating_add(60_000),
                ),
            )
            .unwrap();
        envelope.validate().unwrap();
        assert_eq!(envelope.install_plan, plan);
        assert_eq!(envelope.target_peer, peer);
        assert_eq!(envelope.authority_epoch, epoch);

        let captured = harness.ecosystem_distributions.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, envelope.install_plan);
        assert_eq!(captured[0].1, envelope.target_peer);
        assert_eq!(captured[0].2, envelope.authority_epoch);
        assert_eq!(captured[0].3, p::VerifiedPrincipal("owner:test".into()));
        assert!(captured[0].4 > 0);
        drop(captured);
        assert!(harness.federation_actions.lock().unwrap().is_empty());
        assert!(harness.runs.lock().unwrap().is_empty());
    }
}
