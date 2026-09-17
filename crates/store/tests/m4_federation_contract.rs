use forme_protocol as p;
use forme_store::{
    EventStore, FederationEventStore, FederationProjection, RemoteDispatchLedger,
    ReplicaProjectionStore, ReplicaSqliteStore, SqliteEventStore, StoreOptions,
};

fn store() -> SqliteEventStore {
    SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap()
}

fn owner_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::OwnerControl,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}

fn authority_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn event(id: &str, run: &str, payload: p::EventPayload, provenance: p::Provenance) -> p::Event {
    p::Event::new(
        p::EventId(id.into()),
        p::RunId(run.into()),
        None,
        payload,
        p::M4_SCHEMA_VERSION,
        1,
        provenance,
    )
}

fn version(value: u64) -> p::FederationAggregateVersion {
    p::FederationAggregateVersion {
        schema_version: p::M4_SCHEMA_VERSION,
        aggregate: p::FederationAggregateRef("federation".into()),
        version: value,
    }
}

fn grant(
    peer: &str,
    role: p::FederatedPeerRole,
    epoch: u64,
    grant_version: u64,
) -> p::FederatedPeerGrant {
    grant_in_scope(peer, role, epoch, grant_version, "workspace:alpha")
}

fn grant_in_scope(
    peer: &str,
    role: p::FederatedPeerRole,
    epoch: u64,
    grant_version: u64,
    scope: &str,
) -> p::FederatedPeerGrant {
    p::FederatedPeerGrant {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: p::FederatedPeerRef(peer.into()),
        owner: p::VerifiedPrincipal("owner:forme".into()),
        roles: vec![role],
        scopes: vec![p::Scope(scope.into())],
        capabilities: vec![p::CapabilityRef("fixture.mutate".into())],
        transport_identity: p::TransportIdentityDigest(format!("sha256:identity:{peer}")),
        authority_epoch: p::AuthorityEpoch(epoch),
        grant_version: p::PeerGrantVersion(grant_version),
        expires_at: i64::MAX - 1,
        created_by: p::OwnerControlRef(format!("owner-control:{peer}:{grant_version}")),
    }
}

fn register_event(
    id: &str,
    peer_grant: p::FederatedPeerGrant,
    previous: Option<p::FederatedPeerGrantRef>,
    expected: u64,
) -> p::Event {
    event(
        id,
        "owner-control",
        p::EventPayload::FederatedPeerRegistered(p::FederatedPeerRegisteredPayload {
            grant: peer_grant,
            previous,
            committed_version: version(expected + 1),
        }),
        owner_provenance(),
    )
}

fn append_registration(
    store: &SqliteEventStore,
    id: &str,
    peer_grant: p::FederatedPeerGrant,
    previous: Option<p::FederatedPeerGrantRef>,
    expected: u64,
) -> p::ExpectedAppend {
    store
        .append_federation_expected(
            register_event(id, peer_grant, previous, expected),
            &p::FederationAggregateRef("federation".into()),
            version(expected),
        )
        .unwrap()
}

fn remote_lease(peer_grant: &p::FederatedPeerGrant) -> p::RemoteExecutionLease {
    p::RemoteExecutionLease {
        schema_version: p::M4_SCHEMA_VERSION,
        lease: p::RemoteExecutionLeaseRef("lease:one".into()),
        dispatch: p::RemoteDispatchId("dispatch:one".into()),
        intent: p::ActionId("intent:remote".into()),
        plan_digest: p::PlanDigest("sha256:approved-plan".into()),
        placement: p::RemotePlacementPlanRef("sha256:placement".into()),
        executor: peer_grant.peer.clone(),
        peer_grant: peer_grant.reference().unwrap(),
        grant_version: peer_grant.grant_version,
        authority_epoch: peer_grant.authority_epoch,
        fence: p::FenceToken(1),
        expires_at: i64::MAX - 1,
        state: p::RemoteLeaseState::Acquired,
    }
}

#[test]
fn s70_s71_s77_peer_epoch_lease_and_dispatch_claim_are_single_writer() {
    let store = store();
    let executor = grant("peer:executor", p::FederatedPeerRole::Executor, 1, 1);
    assert_eq!(
        append_registration(&store, "event:register", executor.clone(), None, 0).status,
        p::ExpectedAppendStatus::Applied
    );
    let snapshot = store.snapshot(p::Scope("workspace:alpha".into())).unwrap();
    assert_eq!(snapshot.authority_epoch, p::AuthorityEpoch(1));
    assert_eq!(snapshot.grants, vec![executor.reference().unwrap()]);

    let stale = append_registration(
        &store,
        "event:stale-register",
        grant("peer:other", p::FederatedPeerRole::Executor, 1, 1),
        None,
        0,
    );
    assert_eq!(stale.status, p::ExpectedAppendStatus::Conflict);
    assert!(store
        .peer(&p::FederatedPeerRef("peer:other".into()))
        .unwrap()
        .is_none());

    let lease = remote_lease(&executor);
    let acquired = event(
        "event:lease-acquired",
        "run:remote",
        p::EventPayload::RemoteExecutionLeaseChanged(p::RemoteExecutionLeaseChangedPayload {
            lease: lease.clone(),
            reason: p::ReasonRef("approved and rechecked".into()),
            committed_version: version(2),
        }),
        authority_provenance(),
    );
    let appended = store
        .append_federation_expected(
            acquired,
            &p::FederationAggregateRef("federation".into()),
            version(1),
        )
        .unwrap();
    assert_eq!(appended.status, p::ExpectedAppendStatus::Applied);
    assert_eq!(
        store
            .remote_dispatch_claim(&lease.dispatch)
            .unwrap()
            .unwrap()
            .status,
        p::RemoteDispatchClaimStatus::Reserved
    );
    let first = store
        .claim_remote_dispatch(&lease.lease, &lease.plan_digest, lease.authority_epoch)
        .unwrap();
    assert_eq!(first.status, p::RemoteDispatchClaimStatus::Claimed);
    let duplicate = store
        .claim_remote_dispatch(&lease.lease, &lease.plan_digest, lease.authority_epoch)
        .unwrap();
    assert_eq!(
        duplicate.status,
        p::RemoteDispatchClaimStatus::AlreadyAttempted
    );
    assert!(store
        .claim_remote_dispatch(
            &lease.lease,
            &p::PlanDigest("sha256:changed".into()),
            lease.authority_epoch,
        )
        .is_err());

    let updated = grant("peer:executor", p::FederatedPeerRole::Executor, 2, 2);
    let previous = executor.reference().unwrap();
    assert_eq!(
        append_registration(&store, "event:update", updated, Some(previous), 2).status,
        p::ExpectedAppendStatus::Applied
    );
    assert_eq!(
        store.lease(&lease.lease).unwrap().unwrap().state,
        p::RemoteLeaseState::Fenced
    );
    assert!(store
        .claim_remote_dispatch(&lease.lease, &lease.plan_digest, p::AuthorityEpoch(1))
        .is_err());
}

fn session_bound_event(id: &str, run: &str, workspace: &str) -> p::Event {
    event(
        id,
        run,
        p::EventPayload::SessionBound(p::SessionBoundPayload {
            policy_profile: p::PolicyProfileRef("policy:replication".into()),
            model_profile: p::ModelProfileRef("model:replication".into()),
            toolset_ref: p::ToolsetRef("toolset:replication".into()),
            workspace: p::WorkspaceRef(workspace.into()),
            effect_mode: None,
            evolution_snapshot: None,
            federation_snapshot: None,
        }),
        authority_provenance(),
    )
}

fn sensitive_tool_event(id: &str, run: &str) -> p::Event {
    event(
        id,
        run,
        p::EventPayload::ToolCallProposed(p::ToolCallProposedPayload {
            call_id: p::ToolCallId("call:sensitive".into()),
            tool: p::ToolRef("tool:private".into()),
            args: serde_json::json!({"private": "not exported"}),
        }),
        authority_provenance(),
    )
}

fn acknowledge_batch(
    store: &SqliteEventStore,
    event_id: &str,
    batch: &p::ReplicationBatch,
    report: &p::ReplicaApplyReport,
    expected: u64,
) -> p::ExpectedAppend {
    let ack = p::ReplicationAck {
        schema_version: p::M4_SCHEMA_VERSION,
        batch: batch.batch.clone(),
        peer: batch.peer.clone(),
        aggregate: batch.aggregate.clone(),
        applied: report.cursor.clone(),
        projection_digest: report.projection_digest.clone(),
    };
    let checkpoint = event(
        event_id,
        "owner-control",
        p::EventPayload::ReplicationCheckpointAdvanced(p::ReplicationCheckpointAdvancedPayload {
            peer: batch.peer.clone(),
            aggregate: batch.aggregate.clone(),
            from_stream_seq: batch.from.stream_seq,
            to_stream_seq: batch.to.stream_seq,
            batch_digest: batch.content_digest.clone(),
            redaction: batch.redaction.clone(),
            authority_epoch: batch.to.authority_epoch,
            committed_version: version(expected + 1),
        }),
        authority_provenance(),
    );
    store
        .acknowledge_replication(checkpoint, ack, version(expected))
        .unwrap()
}

#[test]
fn s75_s76_replication_is_filtered_contiguous_atomic_and_idempotent() {
    let store = store();
    let alpha_grant = grant_in_scope(
        "peer:replica-alpha",
        p::FederatedPeerRole::Replica,
        1,
        1,
        "workspace:alpha",
    );
    let beta_grant = grant_in_scope(
        "peer:replica-beta",
        p::FederatedPeerRole::Replica,
        2,
        1,
        "workspace:beta",
    );
    append_registration(
        &store,
        "event:register-replica-alpha",
        alpha_grant.clone(),
        None,
        0,
    );
    append_registration(
        &store,
        "event:register-replica-beta",
        beta_grant.clone(),
        None,
        1,
    );
    store
        .append(session_bound_event(
            "event:alpha-one",
            "run:alpha",
            "workspace:alpha",
        ))
        .unwrap();
    store
        .append(sensitive_tool_event("event:alpha-two", "run:alpha"))
        .unwrap();
    store
        .append(session_bound_event(
            "event:beta-one",
            "run:beta",
            "workspace:beta",
        ))
        .unwrap();
    store
        .append(sensitive_tool_event("event:beta-two", "run:beta"))
        .unwrap();

    let alpha_aggregate = p::RunId("run:alpha".into());
    let beta_aggregate = p::RunId("run:beta".into());
    let alpha_zero = store
        .checkpoint(&alpha_grant.peer, &alpha_aggregate)
        .unwrap();
    let beta_zero = store.checkpoint(&beta_grant.peer, &beta_aggregate).unwrap();
    assert_eq!(alpha_zero.authority_epoch, p::AuthorityEpoch(2));
    assert_eq!(beta_zero.authority_epoch, p::AuthorityEpoch(2));
    let alpha_batch = store
        .export_replication(p::ReplicationExportRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: alpha_grant.peer.clone(),
            grant: alpha_grant.reference().unwrap(),
            aggregate: alpha_aggregate.clone(),
            after: alpha_zero.clone(),
            limit: 10,
            redaction: p::RedactionPolicyRef("redaction:alpha".into()),
        })
        .unwrap();
    let beta_batch = store
        .export_replication(p::ReplicationExportRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: beta_grant.peer.clone(),
            grant: beta_grant.reference().unwrap(),
            aggregate: beta_aggregate.clone(),
            after: beta_zero.clone(),
            limit: 10,
            redaction: p::RedactionPolicyRef("redaction:beta".into()),
        })
        .unwrap();

    assert_eq!(alpha_batch.events.len(), 2);
    assert_eq!(beta_batch.events.len(), 2);
    assert_eq!(alpha_batch.from.stream_seq, 0);
    assert_eq!(beta_batch.from.stream_seq, 0);
    assert_eq!(alpha_batch.to.stream_seq, 2);
    assert_eq!(beta_batch.to.stream_seq, 2);
    assert_ne!(alpha_batch.batch, beta_batch.batch);
    assert_ne!(alpha_batch.content_digest, beta_batch.content_digest);
    assert!(matches!(
        alpha_batch.events[1].payload,
        p::SyncTransferPayload::Redacted { .. }
    ));
    assert!(matches!(
        beta_batch.events[1].payload,
        p::SyncTransferPayload::Redacted { .. }
    ));

    let alpha_to_beta = store
        .checkpoint(&alpha_grant.peer, &beta_aggregate)
        .unwrap();
    assert!(store
        .export_replication(p::ReplicationExportRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: alpha_grant.peer.clone(),
            grant: alpha_grant.reference().unwrap(),
            aggregate: beta_aggregate.clone(),
            after: alpha_to_beta.clone(),
            limit: 10,
            redaction: p::RedactionPolicyRef("redaction:alpha".into()),
        })
        .is_err());
    let beta_to_alpha = store
        .checkpoint(&beta_grant.peer, &alpha_aggregate)
        .unwrap();
    assert!(store
        .export_replication(p::ReplicationExportRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: beta_grant.peer.clone(),
            grant: beta_grant.reference().unwrap(),
            aggregate: alpha_aggregate.clone(),
            after: beta_to_alpha.clone(),
            limit: 10,
            redaction: p::RedactionPolicyRef("redaction:beta".into()),
        })
        .is_err());

    let alpha_scope = p::ReplicaScope {
        schema_version: p::M4_SCHEMA_VERSION,
        scopes: vec![p::Scope("workspace:alpha".into())],
        allow_owner_view: false,
    };
    let beta_scope = p::ReplicaScope {
        schema_version: p::M4_SCHEMA_VERSION,
        scopes: vec![p::Scope("workspace:beta".into())],
        allow_owner_view: false,
    };
    let alpha_replica = ReplicaSqliteStore::open_in_memory(
        alpha_grant.peer.clone(),
        alpha_grant.reference().unwrap(),
        p::AuthorityEpoch(2),
        alpha_scope.clone(),
    )
    .unwrap();
    let beta_replica = ReplicaSqliteStore::open_in_memory(
        beta_grant.peer.clone(),
        beta_grant.reference().unwrap(),
        p::AuthorityEpoch(2),
        beta_scope.clone(),
    )
    .unwrap();
    let alpha_applied = alpha_replica
        .apply(alpha_batch.clone(), alpha_zero.clone())
        .unwrap();
    let beta_applied = beta_replica
        .apply(beta_batch.clone(), beta_zero.clone())
        .unwrap();
    assert_eq!(alpha_applied.status, p::ReplicaApplyStatus::Applied);
    assert_eq!(beta_applied.status, p::ReplicaApplyStatus::Applied);
    assert_eq!(alpha_applied.cursor.stream_seq, 2);
    assert_eq!(beta_applied.cursor.stream_seq, 2);
    let duplicate = alpha_replica
        .apply(alpha_batch.clone(), alpha_zero)
        .unwrap();
    assert_eq!(duplicate.status, p::ReplicaApplyStatus::Duplicate);
    assert_eq!(
        alpha_replica.rebuild(alpha_scope).unwrap().0,
        alpha_applied.projection_digest.0
    );
    assert_eq!(
        beta_replica.rebuild(beta_scope).unwrap().0,
        beta_applied.projection_digest.0
    );

    let mut tampered = beta_batch.clone();
    tampered.to.stream_seq = 3;
    assert!(beta_replica.apply(tampered, beta_batch.to.clone()).is_err());
    assert_eq!(
        beta_replica
            .cursor(&beta_grant.peer, &beta_aggregate)
            .unwrap()
            .stream_seq,
        2
    );

    assert_eq!(
        acknowledge_batch(
            &store,
            "event:checkpoint-alpha",
            &alpha_batch,
            &alpha_applied,
            2
        )
        .status,
        p::ExpectedAppendStatus::Applied
    );
    assert_eq!(
        acknowledge_batch(
            &store,
            "event:checkpoint-beta",
            &beta_batch,
            &beta_applied,
            3
        )
        .status,
        p::ExpectedAppendStatus::Applied
    );
    assert_eq!(
        store
            .checkpoint(&alpha_grant.peer, &alpha_aggregate)
            .unwrap()
            .stream_seq,
        2
    );
    assert_eq!(
        store
            .checkpoint(&beta_grant.peer, &beta_aggregate)
            .unwrap()
            .stream_seq,
        2
    );
    assert_eq!(
        store
            .checkpoint(&alpha_grant.peer, &beta_aggregate)
            .unwrap()
            .stream_seq,
        0
    );
    assert_eq!(
        store
            .checkpoint(&beta_grant.peer, &alpha_aggregate)
            .unwrap()
            .stream_seq,
        0
    );
}
