//! Gateway federation boundary; public facade remains in the crate root.
use super::*;

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
    pub(super) fn validate_owner(&self, auth: AuthContext, now: p::Timestamp) -> p::Result<()> {
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

    pub(super) fn bind_channel(
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

    pub(super) fn validate_channel(
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
