use forme_protocol as p;

fn grant() -> p::FederatedPeerGrant {
    p::FederatedPeerGrant {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: p::FederatedPeerRef("peer:executor-a".into()),
        owner: p::VerifiedPrincipal("owner:forme".into()),
        roles: vec![p::FederatedPeerRole::Executor],
        scopes: vec![p::Scope("workspace:alpha".into())],
        capabilities: vec![p::CapabilityRef("fixture.mutate".into())],
        transport_identity: p::TransportIdentityDigest("sha256:executor-cert".into()),
        authority_epoch: p::AuthorityEpoch(2),
        grant_version: p::PeerGrantVersion(1),
        expires_at: 50_000,
        created_by: p::OwnerControlRef("owner-control:grant-a".into()),
    }
}

fn placement() -> p::RemotePlacementPlan {
    let grant = grant();
    let mut operation = p::RemoteOperation {
        schema_version: p::M4_SCHEMA_VERSION,
        backend: p::BackendKind::File,
        parameters: p::ActionParameters::File {
            operation: p::FileOperation::Write,
            path: "golden/state".into(),
            content: Some(b"commit".to_vec()),
        },
        capability: p::CapabilityRef("fixture.mutate".into()),
        scope: p::Scope("workspace:alpha".into()),
        action_type: p::ActionType::ExternalCommit,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_boundary: p::RollbackBoundary("fixture-record-only".into()),
        credential_slot: None,
        digest: p::SchemaDigest(String::new()),
    };
    operation.refresh_digest().unwrap();
    let mut placement = p::RemotePlacementPlan {
        schema_version: p::M4_SCHEMA_VERSION,
        executor: grant.peer.clone(),
        peer_grant: grant.reference().unwrap(),
        grant_version: grant.grant_version,
        authority_epoch: grant.authority_epoch,
        executor_profile: p::ExecutorProfileRef("profile:fixture-v1".into()),
        operation,
        digest: p::SchemaDigest(String::new()),
    };
    placement.refresh_digest().unwrap();
    placement
}

fn lease(plan: &p::RemotePlacementPlan) -> p::RemoteExecutionLease {
    p::RemoteExecutionLease {
        schema_version: p::M4_SCHEMA_VERSION,
        lease: p::RemoteExecutionLeaseRef("lease:one-shot".into()),
        dispatch: p::RemoteDispatchId("dispatch:one-shot".into()),
        intent: p::ActionId("intent:remote".into()),
        plan_digest: p::PlanDigest("sha256:authority-plan".into()),
        placement: plan.reference().unwrap(),
        executor: plan.executor.clone(),
        peer_grant: plan.peer_grant.clone(),
        grant_version: plan.grant_version,
        authority_epoch: plan.authority_epoch,
        fence: p::FenceToken(7),
        expires_at: 40_000,
        state: p::RemoteLeaseState::Acquired,
    }
}

#[test]
fn m4_event_taxonomy_is_additive_after_the_frozen_m3_prefix() {
    assert_eq!(p::EventKind::ALL.len(), 99);
    assert_eq!(
        &p::EventKind::ALL[89..93],
        &[
            p::EventKind::FederatedPeerRegistered,
            p::EventKind::FederatedPeerRevoked,
            p::EventKind::RemoteExecutionLeaseChanged,
            p::EventKind::ReplicationCheckpointAdvanced,
        ]
    );
    assert!(!p::EventKind::ALL[..89].iter().any(|kind| matches!(
        kind,
        p::EventKind::FederatedPeerRegistered
            | p::EventKind::FederatedPeerRevoked
            | p::EventKind::RemoteExecutionLeaseChanged
            | p::EventKind::ReplicationCheckpointAdvanced
    )));
}

#[test]
fn remote_wire_round_trip_is_closed_plan_bound_and_tamper_evident() {
    let plan = placement();
    let lease = lease(&plan);
    let mut envelope = p::RemoteWireEnvelope {
        schema_version: p::M4_SCHEMA_VERSION,
        request: p::RemoteWireRequestRef("wire:dispatch:1".into()),
        authority: p::AuthorityRef("authority:local".into()),
        peer: plan.executor.clone(),
        nonce: p::Nonce("nonce:dispatch:1".into()),
        expires_at: 30_000,
        command: p::RemoteWireCommand::Dispatch {
            plan: Box::new(plan.clone()),
            lease: lease.clone(),
        },
        digest: p::SchemaDigest(String::new()),
    };
    envelope.refresh_digest().unwrap();
    envelope.validate(20_000).unwrap();
    let encoded = serde_json::to_vec(&envelope).unwrap();
    let decoded: p::RemoteWireEnvelope = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, envelope);

    let mut tampered = decoded;
    let p::RemoteWireCommand::Dispatch { lease, .. } = &mut tampered.command else {
        panic!("dispatch fixture changed")
    };
    lease.fence = p::FenceToken(8);
    assert!(tampered.validate(20_000).is_err());
    assert!(envelope.validate(30_000).is_err());

    let mut value = serde_json::to_value(&envelope).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<p::RemoteWireEnvelope>(value).is_err());
}

#[test]
fn remote_recovery_and_retention_state_are_versioned_and_digest_bound() {
    let plan = placement();
    let lease = lease(&plan);
    let intent = p::ActionIntent {
        schema_version: p::M4_SCHEMA_VERSION,
        intent_id: lease.intent.clone(),
        source: p::Source::UserTurn,
        goal: p::GoalRef("recover one remote action".into()),
        backend_hint: p::BackendKind::Remote,
        capability_ref: plan.operation.capability.clone(),
        action_type: plan.operation.action_type,
        scope: plan.operation.scope.clone(),
        risk_hint: p::Risk::High,
        expected_effect: plan.operation.expected_effect,
        rollback_expectation: plan.operation.rollback_boundary.clone(),
        parameters: p::ActionParameters::Remote(Box::new(p::RemoteActionSpec {
            schema_version: p::M4_SCHEMA_VERSION,
            placement: plan.clone(),
        })),
        requested_permissions: Vec::new(),
        requested_at: 20_000,
        estimated_output_bytes: 1,
        estimated_duration: p::DurationMs(1),
    };
    let mut recovery = p::RemoteActionRecoveryRecord {
        schema_version: p::M4_SCHEMA_VERSION,
        run: p::RunId("run:recovery".into()),
        source: intent.source,
        intent,
        placement: plan,
        lease,
        driver_receipt: None,
        digest: p::SchemaDigest(String::new()),
    };
    recovery.refresh_digest().unwrap();
    recovery.validate().unwrap();
    recovery.driver_receipt = Some(p::RemoteDriverReceiptRef("receipt:original".into()));
    assert!(recovery.validate().is_err());
    recovery.refresh_digest().unwrap();
    recovery.validate().unwrap();

    let mut request = p::FederatedRetentionRequest {
        schema_version: p::M4_SCHEMA_VERSION,
        request: p::RetentionRequestRef("retention:1".into()),
        peer: p::FederatedPeerRef("peer:replica".into()),
        scope: p::Scope("workspace:alpha".into()),
        authority_epoch: p::AuthorityEpoch(2),
        requested_by: p::OwnerControlRef("owner-control:retention".into()),
        expires_at: 40_000,
        digest: p::SchemaDigest(String::new()),
    };
    request.refresh_digest().unwrap();
    let mut state = p::FederatedRetentionState {
        schema_version: p::M4_SCHEMA_VERSION,
        request,
        status: p::RetentionStatus::Requested,
        receipt: None,
        digest: p::SchemaDigest(String::new()),
    };
    state.refresh_digest().unwrap();
    state.validate().unwrap();
    state.status = p::RetentionStatus::Verified;
    state.refresh_digest().unwrap();
    assert!(state.validate().is_err());
}

#[test]
fn remote_operation_rejects_recursive_remote_and_host_paths() {
    let mut absolute = placement();
    absolute.operation.parameters = p::ActionParameters::File {
        operation: p::FileOperation::Write,
        path: r"C:\private\state".into(),
        content: Some(vec![1]),
    };
    absolute.operation.refresh_digest().unwrap();
    absolute.refresh_digest().unwrap();
    assert!(absolute.validate().is_err());

    let mut recursive = placement();
    recursive.operation.backend = p::BackendKind::Remote;
    recursive.operation.parameters = p::ActionParameters::Remote(Box::new(p::RemoteActionSpec {
        schema_version: p::M4_SCHEMA_VERSION,
        placement: placement(),
    }));
    recursive.operation.refresh_digest().unwrap();
    recursive.refresh_digest().unwrap();
    assert!(recursive.validate().is_err());
}

#[test]
fn owner_control_and_retention_are_nonce_expiry_and_digest_bound() {
    let command = p::FederatedOwnerCommand::Revoke {
        peer: grant().peer,
        grant: grant().reference().unwrap(),
        in_flight: p::InFlightDisposition::WaitForOwner,
    };
    let envelope = p::FederatedControlEnvelope {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: p::FederatedPeerRef("peer:owner-device".into()),
        session: p::FederatedSessionRef("session:owner-device".into()),
        owner: p::VerifiedPrincipal("owner:forme".into()),
        nonce: p::Nonce("nonce:owner:1".into()),
        expires_at: 30_000,
        command_digest: p::canonical_digest(&command).unwrap(),
    };
    envelope.validate_command(&command, 20_000).unwrap();
    assert!(envelope.validate_command(&command, 30_000).is_err());
    assert!(envelope
        .validate_command(
            &p::FederatedOwnerCommand::Cancel {
                lease: p::RemoteExecutionLeaseRef("lease:other".into()),
                reason: p::ReasonRef("owner".into()),
            },
            20_000,
        )
        .is_err());
}

fn candidate(peer: &str, allowed: bool, score: u16) -> p::FederatedExecutorCandidate {
    p::FederatedExecutorCandidate {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: p::FederatedPeerRef(peer.into()),
        grant: p::FederatedPeerGrantRef(format!("grant:{peer}")),
        profile: p::ExecutorProfileRef("profile:fixture-v1".into()),
        scope: p::Scope("workspace:alpha".into()),
        capability: p::CapabilityRef("fixture.mutate".into()),
        expires_at: 40_000,
        health: p::ExecutorHealthState::Healthy,
        health_observed_at: 20_000,
        capability_evidence: vec![p::CapabilityEvidenceRef(format!("evidence:{peer}"))],
        failure_evidence: Vec::new(),
        managed_policy_allowed: allowed,
        score_basis_points: score,
    }
}

#[test]
fn placement_score_cannot_revive_a_filtered_executor() {
    let denied = p::PlacementCandidateTrace {
        schema_version: p::M4_SCHEMA_VERSION,
        candidate: candidate("peer:a", false, 10_000),
        eligible: false,
        reasons: vec![p::PlacementFilterReason::ManagedPolicyDenied],
    };
    let allowed = p::PlacementCandidateTrace {
        schema_version: p::M4_SCHEMA_VERSION,
        candidate: candidate("peer:b", true, 100),
        eligible: true,
        reasons: Vec::new(),
    };
    let mut decision = p::FederatedPlacementDecision {
        schema_version: p::M4_SCHEMA_VERSION,
        decision: p::PlacementDecisionRef("placement:1".into()),
        federation_snapshot: p::FederationSnapshotRef("snapshot:1".into()),
        scope: p::Scope("workspace:alpha".into()),
        capability: p::CapabilityRef("fixture.mutate".into()),
        evaluated_at: 20_000,
        candidates: vec![denied, allowed],
        chosen: Some(p::FederatedPeerRef("peer:a".into())),
        placement: Some(placement().reference().unwrap()),
        digest: p::SchemaDigest(String::new()),
    };
    decision.refresh_digest().unwrap();
    assert!(decision.validate().is_err());
    decision.chosen = Some(p::FederatedPeerRef("peer:b".into()));
    decision.refresh_digest().unwrap();
    decision.validate().unwrap();
}

fn checkpoint() -> p::FederatedCheckpointArtifact {
    let mut checkpoint = p::FederatedCheckpointArtifact {
        schema_version: p::M4_SCHEMA_VERSION,
        reference: p::FederatedCheckpointArtifactRef("checkpoint-artifact:1".into()),
        checkpoint: p::GoalCheckpoint {
            schema_version: p::M4_SCHEMA_VERSION,
            reference: p::GoalCheckpointRef("checkpoint:1".into()),
            goal_frame: p::GoalFrameRef("goal:long".into()),
            intention: p::IntentionId("intention:long".into()),
            route: p::ExecutionRouteRef("route:segment-a".into()),
            artifact: p::ContentRef("artifact:checkpoint".into()),
            situation_digest: p::SchemaDigest("sha256:situation".into()),
            evidence_refs: vec![p::EventId("event:verified".into())],
            created_at: 20_000,
        },
        done_contract: p::DoneContractRef("done:segment-a".into()),
        verification_outcome: p::VerificationOutcome::Pass,
        verification_events: vec![p::EventId("event:verified".into())],
        artifacts: vec![p::ContentRef("artifact:result".into())],
        spent_budget: p::Budget("turns=1".into()),
        remaining_budget: p::Budget("turns=2".into()),
        external_effects: vec![p::EvidenceRef("effect:ordinal:1".into())],
        scope: p::Scope("workspace:alpha".into()),
        policy: p::PolicyProfileRef("policy:v1".into()),
        toolset: p::ToolsetRef("toolset:v1".into()),
        model: p::ModelProfileRef("model:v1".into()),
        evolution_snapshot: p::EvolutionSnapshotRef("evolution:v1".into()),
        federation_snapshot: p::FederationSnapshotRef("federation:v1".into()),
        digest: p::SchemaDigest(String::new()),
    };
    checkpoint.refresh_digest().unwrap();
    checkpoint
}

#[test]
fn handoff_requires_a_passing_durable_checkpoint_and_a_new_run() {
    let mut artifact = checkpoint();
    artifact.validate().unwrap();
    artifact.verification_outcome =
        p::VerificationOutcome::Unverifiable(p::ReasonRef("none".into()));
    artifact.refresh_digest().unwrap();
    assert!(artifact.validate().is_err());

    let mut handoff = p::FederatedHandoffPlan {
        schema_version: p::M4_SCHEMA_VERSION,
        checkpoint: p::FederatedCheckpointArtifactRef("checkpoint-artifact:1".into()),
        from_run: p::RunId("run:a".into()),
        next_run: p::RunId("run:a".into()),
        target: p::FederatedPeerRef("peer:executor-b".into()),
        placement: placement().reference().unwrap(),
        evolution_snapshot: p::EvolutionSnapshotRef("evolution:v2".into()),
        federation_snapshot: p::FederationSnapshotRef("federation:v2".into()),
        budget: p::Budget("turns=2".into()),
        digest: p::SchemaDigest(String::new()),
    };
    handoff.refresh_digest().unwrap();
    assert!(handoff.validate().is_err());
    handoff.next_run = p::RunId("run:b".into());
    handoff.refresh_digest().unwrap();
    handoff.validate().unwrap();
}
