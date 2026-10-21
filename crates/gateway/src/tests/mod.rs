use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use forme_communication::{
    Carrier, CommunicationBudget, CommunicationPurpose, DeviceGrant, DeviceObservation,
    DisclosurePolicy, DisclosureRule, ObservationScope, RepresentationRule, RetentionPolicy,
    TerminationPolicy, TextChannelAdapter, TranscriptPolicy,
};
use forme_harness::{
    AgentHarness, EcosystemGatewayControl, EvolutionGatewayControl, HarnessIngress, IngressEvent,
    ResumeInput,
};

use super::*;
mod fixtures;
use fixtures::*;

#[test]
fn local_gateway_rejects_an_approval_principal_that_is_not_the_authenticated_owner() {
    let gateway = gateway(Vec::new());
    let decision = p::ApprovalDecision {
        schema_version: p::SchemaVersion(1),
        approval_id: p::ApprovalId("approval:wrong-principal".into()),
        outcome: p::ApprovalOutcome::Granted,
        approver: p::VerifiedPrincipal("owner:other".into()),
        bound_plan_digest: p::PlanDigest("sha256:plan".into()),
        normalized_action_digest: p::SchemaDigest("sha256:action".into()),
        affected_resources: Vec::new(),
        policy_version: p::Version(1),
        tool_schema_version: p::Version(1),
        nonce: p::Nonce("nonce:wrong-principal".into()),
        use_by: now_ms().saturating_add(60_000),
    };
    let error = gateway
        .control_run(
            p::RunId("run:wrong-principal".into()),
            p::RunControl::ResolveApproval(decision),
            AuthContext::local_owner(
                p::VerifiedPrincipal("owner:test".into()),
                now_ms().saturating_add(60_000),
            ),
        )
        .expect_err("gateway must reject a mismatched approval principal");
    assert!(error
        .0
        .contains("approval principal does not match the authenticated owner"));
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
            AuthContext::local_owner(p::VerifiedPrincipal("owner:test".into()), now_ms() + 1_000),
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
            AuthContext::local_owner(p::VerifiedPrincipal("owner:test".into()), now_ms() + 1_000),
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
            AuthContext::local_owner(p::VerifiedPrincipal("owner:test".into()), now_ms() + 1_000),
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
    let from = p::ReplicationCursor::zero(replica.clone(), aggregate.clone(), p::AuthorityEpoch(1));
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
