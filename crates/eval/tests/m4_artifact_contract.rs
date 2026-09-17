use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use forme_eval::{
    FederationArtifactBundle, FederationArtifactStore, RemoteAuthorityVerifier, RemoteGroundTruth,
};
use forme_protocol as p;

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "forme-m4-artifact-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn artifact_path(root: &Path, prefix: &str) -> PathBuf {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
        })
        .unwrap()
}

fn plan() -> p::RemotePlacementPlan {
    let mut operation = p::RemoteOperation {
        schema_version: p::M4_SCHEMA_VERSION,
        backend: p::BackendKind::File,
        parameters: p::ActionParameters::File {
            operation: p::FileOperation::Write,
            path: "state.json".into(),
            content: Some(b"one".to_vec()),
        },
        capability: p::CapabilityRef("fixture.mutate".into()),
        scope: p::Scope("workspace:m4-artifact".into()),
        action_type: p::ActionType::ExternalCommit,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_boundary: p::RollbackBoundary("fixture reset only".into()),
        credential_slot: None,
        digest: p::SchemaDigest(String::new()),
    };
    operation.refresh_digest().unwrap();
    let mut plan = p::RemotePlacementPlan {
        schema_version: p::M4_SCHEMA_VERSION,
        executor: p::FederatedPeerRef("peer:m4-artifact-executor".into()),
        peer_grant: p::FederatedPeerGrantRef("grant:m4-artifact-executor".into()),
        grant_version: p::PeerGrantVersion(1),
        authority_epoch: p::AuthorityEpoch(3),
        executor_profile: p::ExecutorProfileRef("profile:m4-artifact".into()),
        operation,
        digest: p::SchemaDigest(String::new()),
    };
    plan.refresh_digest().unwrap();
    plan
}

fn lease(plan: &p::RemotePlacementPlan) -> p::RemoteExecutionLease {
    p::RemoteExecutionLease {
        schema_version: p::M4_SCHEMA_VERSION,
        lease: p::RemoteExecutionLeaseRef("lease:m4-artifact".into()),
        dispatch: p::RemoteDispatchId("dispatch:m4-artifact".into()),
        intent: p::ActionId("intent:m4-artifact".into()),
        plan_digest: p::PlanDigest("sha256:execution-plan".into()),
        placement: plan.reference().unwrap(),
        executor: plan.executor.clone(),
        peer_grant: plan.peer_grant.clone(),
        grant_version: plan.grant_version,
        authority_epoch: plan.authority_epoch,
        fence: p::FenceToken(9),
        expires_at: 9_999,
        state: p::RemoteLeaseState::Acquired,
    }
}

fn driver(
    plan: &p::RemotePlacementPlan,
    lease: &p::RemoteExecutionLease,
) -> p::RemoteDriverReceipt {
    p::RemoteDriverReceipt {
        schema_version: p::M4_SCHEMA_VERSION,
        receipt: p::RemoteDriverReceiptRef("driver-receipt:m4-artifact".into()),
        lease: lease.lease.clone(),
        dispatch: lease.dispatch.clone(),
        intent: lease.intent.clone(),
        plan_digest: lease.plan_digest.clone(),
        operation_digest: plan.operation.digest.clone(),
        executor: lease.executor.clone(),
        authority_epoch: lease.authority_epoch,
        fence: lease.fence,
        outcome: p::RemoteReceiptOutcome::Completed,
        result_digest: Some(p::SchemaDigest("sha256:result".into())),
        observations: vec![p::EvidenceRef("worker:mutation-observed".into())],
        observed_at: 1,
    }
}

fn ground_truth() -> RemoteGroundTruth {
    RemoteGroundTruth {
        schema_version: p::M4_SCHEMA_VERSION,
        outcome: p::RemoteReceiptOutcome::Completed,
        result_digest: Some(p::SchemaDigest("sha256:result".into())),
        evidence: vec![p::EvidenceRef("ground-truth:mutation-ordinal:1".into())],
        verification: vec![p::EvidenceRef("verification:repository-state".into())],
    }
}

fn event_kinds() -> Vec<p::EventKind> {
    vec![
        p::EventKind::FederatedPeerRegistered,
        p::EventKind::RunAccepted,
        p::EventKind::SessionBound,
        p::EventKind::ToolCallProposed,
        p::EventKind::ToolPolicyEvaluated,
        p::EventKind::ActionPlanned,
        p::EventKind::ApprovalRequested,
        p::EventKind::RunWaiting,
        p::EventKind::ApprovalResolved,
        p::EventKind::RunResumed,
        p::EventKind::CompetenceGateEvaluated,
        p::EventKind::RemoteExecutionLeaseChanged,
        p::EventKind::ActionStarted,
        p::EventKind::ActionCompleted,
        p::EventKind::RemoteExecutionLeaseChanged,
        p::EventKind::VerificationStarted,
        p::EventKind::VerificationFinished,
        p::EventKind::ReplicationCheckpointAdvanced,
        p::EventKind::FederatedPeerRevoked,
    ]
}

fn bundle() -> FederationArtifactBundle {
    let plan = plan();
    let lease = lease(&plan);
    let receipt = RemoteAuthorityVerifier
        .verify(&plan, &lease, &driver(&plan, &lease), ground_truth())
        .unwrap();
    let replica = p::FederatedPeerRef("peer:m4-artifact-replica".into());
    let run = p::RunId("run:m4-artifact".into());
    let from = p::ReplicationCursor::zero(replica.clone(), run.clone(), p::AuthorityEpoch(3));
    let to = p::ReplicationCursor {
        stream_seq: 2,
        ..from.clone()
    };
    let trace = p::FederatedTraceManifest {
        schema_version: p::M4_SCHEMA_VERSION,
        run: run.clone(),
        owner_commands: vec![p::EventId("event:m4:owner-register".into())],
        approvals: vec![
            p::EventId("event:m4:approval-requested".into()),
            p::EventId("event:m4:approval-resolved".into()),
        ],
        leases: vec![
            p::EventId("event:m4:lease-acquired".into()),
            p::EventId("event:m4:lease-released".into()),
        ],
        actions: vec![
            p::EventId("event:m4:action-started".into()),
            p::EventId("event:m4:action-completed".into()),
        ],
        verifications: vec![
            p::EventId("event:m4:verification-started".into()),
            p::EventId("event:m4:verification-finished".into()),
        ],
        replication: vec![p::EventId("event:m4:replication-checkpoint".into())],
        recovery: vec![p::EventId("event:m4:recovery".into())],
        revocations: vec![p::EventId("event:m4:peer-revoked".into())],
    };
    let trace_digest = p::canonical_digest(&trace).unwrap();
    FederationArtifactBundle {
        peer: p::FederatedPeerManifest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: plan.executor.clone(),
            roles: vec![p::FederatedPeerRole::Executor],
            scopes: vec![plan.operation.scope.clone()],
            transport_identity: p::TransportIdentityDigest("sha256:public-identity".into()),
            authority_epoch: plan.authority_epoch,
            grant_version: plan.grant_version,
            expires_at: 9_999,
            grant_ref: plan.peer_grant,
        },
        receipt,
        replication: p::ReplicationManifest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: replica,
            aggregate: run,
            from,
            to: to.clone(),
            batch: p::ReplicationBatchRef("batch:m4-artifact".into()),
            batch_digest: p::SchemaDigest("sha256:batch".into()),
            event_digests: vec![
                p::SchemaDigest("sha256:event-1".into()),
                p::SchemaDigest("sha256:event-2".into()),
            ],
            redaction: p::RedactionPolicyRef("redaction:m4-artifact".into()),
            authority_epoch: p::AuthorityEpoch(3),
        },
        trace,
        report: p::FederationGoldenReport {
            schema_version: p::M4_SCHEMA_VERSION,
            scenario: "S83 three-process governed federation fixture".into(),
            event_kinds: event_kinds(),
            authority_driver_calls: 1,
            mutation_ordinal: 1,
            replica_cursor: Some(to),
            secret_scan_matches: 0,
            negative_assertions: vec![
                "approval preceded dispatch".into(),
                "unknown outcome was not retried".into(),
                "revocation blocked the second mutation".into(),
            ],
            trace_digest,
        },
    }
}

#[test]
fn s74_s83_five_file_artifact_set_is_content_addressed_and_offline_verifiable() {
    let root = temp_root("pass");
    let store = FederationArtifactStore::new(&root).unwrap();
    let receipt = store.write(&bundle()).unwrap();
    assert!(receipt.digest.0.starts_with("sha256:"));
    assert_eq!(
        receipt.content_ref.0,
        format!("artifact:{}", receipt.digest.0)
    );
    assert_eq!(store.verify_complete_set().unwrap().digest, receipt.digest);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 5);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn s84_artifact_gate_rejects_tamper_unknown_fields_extra_and_missing_files() {
    let root = temp_root("body-tamper");
    let store = FederationArtifactStore::new(&root).unwrap();
    store.write(&bundle()).unwrap();
    let path = artifact_path(&root, "receipt-");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["body"]["rollback_boundary"] = serde_json::Value::String("changed".into());
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(&root).unwrap();

    let root = temp_root("digest-tamper");
    let store = FederationArtifactStore::new(&root).unwrap();
    store.write(&bundle()).unwrap();
    let path = artifact_path(&root, "peer-");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["digest"] = serde_json::Value::String("sha256:changed".into());
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(&root).unwrap();

    let root = temp_root("unknown-field");
    let store = FederationArtifactStore::new(&root).unwrap();
    store.write(&bundle()).unwrap();
    let path = artifact_path(&root, "trace-");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["body"]["unchecked"] = serde_json::Value::String("not-digest-bound".into());
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(&root).unwrap();

    let root = temp_root("extra");
    let store = FederationArtifactStore::new(&root).unwrap();
    store.write(&bundle()).unwrap();
    fs::write(root.join("unexpected.txt"), b"unexpected").unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(&root).unwrap();

    let root = temp_root("missing");
    let store = FederationArtifactStore::new(&root).unwrap();
    store.write(&bundle()).unwrap();
    fs::remove_file(artifact_path(&root, "replication-")).unwrap();
    assert!(store.verify_complete_set().is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn s84_artifact_gate_rejects_secrets_host_paths_and_endpoints() {
    for (label, mutate) in [("secret", 0_u8), ("host-path", 1_u8), ("endpoint", 2_u8)] {
        let root = temp_root(label);
        let store = FederationArtifactStore::new(&root).unwrap();
        let mut artifact = bundle();
        match mutate {
            0 => {
                artifact.report.negative_assertions[0] =
                    "Authorization: Bearer raw-fixture-value".into();
            }
            1 => artifact.peer.scopes[0] = p::Scope("C:\\Users\\owner\\private".into()),
            _ => {
                artifact.report.negative_assertions[0] = "https://10.0.0.7:8443/private".into();
            }
        }
        assert!(store.write(&artifact).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn s84_plan_lease_fence_cursor_and_trace_mismatches_fail_closed() {
    let plan = plan();
    let lease = lease(&plan);
    let driver = driver(&plan, &lease);
    let verifier = RemoteAuthorityVerifier;
    assert!(verifier
        .verify(&plan, &lease, &driver, ground_truth())
        .is_ok());

    let mut wrong_lease = lease.clone();
    wrong_lease.placement = p::RemotePlacementPlanRef("placement:wrong".into());
    assert!(verifier
        .verify(&plan, &wrong_lease, &driver, ground_truth())
        .is_err());

    let mut wrong_plan = driver.clone();
    wrong_plan.plan_digest = p::PlanDigest("sha256:wrong-plan".into());
    assert!(verifier
        .verify(&plan, &lease, &wrong_plan, ground_truth())
        .is_err());

    let mut wrong_fence = driver;
    wrong_fence.fence = p::FenceToken(10);
    assert!(verifier
        .verify(&plan, &lease, &wrong_fence, ground_truth())
        .is_err());

    let mut cursor_mismatch = bundle();
    cursor_mismatch
        .report
        .replica_cursor
        .as_mut()
        .unwrap()
        .stream_seq += 1;
    assert!(cursor_mismatch.validate().is_err());

    let mut trace_mismatch = bundle();
    trace_mismatch.report.trace_digest = p::SchemaDigest("sha256:wrong-trace".into());
    assert!(trace_mismatch.validate().is_err());
}
