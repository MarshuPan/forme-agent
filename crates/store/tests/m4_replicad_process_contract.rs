use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use forme_protocol as p;
use forme_store::{
    EventStore, FederationEventStore, FederationProjection, ReplicaProjectionStore,
    ReplicaSqliteStore, SqliteEventStore, StoreOptions,
};

const PEER: &str = "peer:replica-process";
const SCOPE: &str = "workspace:replica-process";

fn unique_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "forme-m4-replicad-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
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

fn replica_grant() -> p::FederatedPeerGrant {
    p::FederatedPeerGrant {
        schema_version: p::M4_SCHEMA_VERSION,
        peer: p::FederatedPeerRef(PEER.into()),
        owner: p::VerifiedPrincipal("owner:forme".into()),
        roles: vec![p::FederatedPeerRole::Replica],
        scopes: vec![p::Scope(SCOPE.into())],
        capabilities: vec![p::CapabilityRef("replication:read".into())],
        transport_identity: p::TransportIdentityDigest("sha256:replica-process".into()),
        authority_epoch: p::AuthorityEpoch(1),
        grant_version: p::PeerGrantVersion(1),
        expires_at: i64::MAX - 1,
        created_by: p::OwnerControlRef("owner-control:replica-process".into()),
    }
}

fn append_registration(store: &SqliteEventStore, grant: &p::FederatedPeerGrant) {
    let registered = event(
        "event:replica-process-register",
        "owner-control:replica-process",
        p::EventPayload::FederatedPeerRegistered(p::FederatedPeerRegisteredPayload {
            grant: grant.clone(),
            previous: None,
            committed_version: version(1),
        }),
        owner_provenance(),
    );
    assert_eq!(
        store
            .append_federation_expected(
                registered,
                &p::FederationAggregateRef("federation".into()),
                version(0),
            )
            .unwrap()
            .status,
        p::ExpectedAppendStatus::Applied
    );
}

fn run_event(id: &str, run: &str, turn: u32) -> p::Event {
    event(
        id,
        run,
        p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: turn }),
        authority_provenance(),
    )
}

fn session_bound_event(id: &str, run: &str) -> p::Event {
    event(
        id,
        run,
        p::EventPayload::SessionBound(p::SessionBoundPayload {
            policy_profile: p::PolicyProfileRef("policy:replicad".into()),
            model_profile: p::ModelProfileRef("model:replicad".into()),
            toolset_ref: p::ToolsetRef("toolset:replicad".into()),
            workspace: p::WorkspaceRef(SCOPE.into()),
            effect_mode: None,
            evolution_snapshot: None,
            federation_snapshot: None,
        }),
        authority_provenance(),
    )
}

fn export(
    store: &SqliteEventStore,
    grant: &p::FederatedPeerGrant,
    aggregate: &p::RunId,
    after: p::ReplicationCursor,
) -> p::ReplicationBatch {
    store
        .export_replication(p::ReplicationExportRequest {
            schema_version: p::M4_SCHEMA_VERSION,
            peer: grant.peer.clone(),
            grant: grant.reference().unwrap(),
            aggregate: aggregate.clone(),
            after,
            limit: 2,
            redaction: p::RedactionPolicyRef("redaction:m4-process".into()),
        })
        .unwrap()
}

fn checkpoint(
    store: &SqliteEventStore,
    batch: &p::ReplicationBatch,
    report: &p::ReplicaApplyReport,
    expected: u64,
) {
    let ack = p::ReplicationAck {
        schema_version: p::M4_SCHEMA_VERSION,
        batch: batch.batch.clone(),
        peer: batch.peer.clone(),
        aggregate: batch.aggregate.clone(),
        applied: report.cursor.clone(),
        projection_digest: report.projection_digest.clone(),
    };
    let event = event(
        &format!("event:replica-checkpoint:{expected}"),
        "owner-control:replication",
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
    assert_eq!(
        store
            .acknowledge_replication(event, ack, version(expected))
            .unwrap()
            .status,
        p::ExpectedAppendStatus::Applied
    );
}

fn run_replicad(
    database: &Path,
    grant: &p::FederatedPeerGrant,
    batch: &p::ReplicationBatch,
) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_forme-replicad"))
        .env("FORME_REPLICA_PEER", PEER)
        .env("FORME_REPLICA_GRANT", grant.reference().unwrap().0)
        .env("FORME_AUTHORITY_EPOCH", "1")
        .env("FORME_REPLICA_SCOPES", SCOPE)
        .env("FORME_REPLICA_ALLOW_OWNER_VIEW", "false")
        .env("FORME_REPLICA_DB_PATH", database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(batch).unwrap())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn decode_report(output: &Output) -> p::ReplicaApplyReport {
    assert!(
        output.status.success(),
        "replicad stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn s76_replicad_process_is_atomic_restart_idempotent_and_rejects_reorder_or_tamper() {
    let root = unique_root();
    std::fs::create_dir_all(&root).unwrap();
    let authority = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let grant = replica_grant();
    append_registration(&authority, &grant);
    let aggregate = p::RunId("run:replicad-process".into());
    authority
        .append(session_bound_event(
            "event:replicad-process:bound",
            &aggregate.0,
        ))
        .unwrap();
    for turn in 1..=3 {
        authority
            .append(run_event(
                &format!("event:replicad-process:{turn}"),
                &aggregate.0,
                turn,
            ))
            .unwrap();
    }
    let zero =
        p::ReplicationCursor::zero(grant.peer.clone(), aggregate.clone(), p::AuthorityEpoch(1));
    let first = export(&authority, &grant, &aggregate, zero.clone());

    let primary_db = root.join("primary.sqlite3");
    let applied = decode_report(&run_replicad(&primary_db, &grant, &first));
    assert_eq!(applied.status, p::ReplicaApplyStatus::Applied);
    assert_eq!(applied.cursor.stream_seq, 2);
    let duplicate = decode_report(&run_replicad(&primary_db, &grant, &first));
    assert_eq!(duplicate.status, p::ReplicaApplyStatus::Duplicate);
    assert_eq!(duplicate.cursor, applied.cursor);

    checkpoint(&authority, &first, &applied, 1);
    let second_cursor = authority.checkpoint(&grant.peer, &aggregate).unwrap();
    let second = export(&authority, &grant, &aggregate, second_cursor);
    assert_eq!(second.from.stream_seq, 2);
    assert_eq!(second.to.stream_seq, 4);

    let reordered_db = root.join("reordered.sqlite3");
    let reordered = run_replicad(&reordered_db, &grant, &second);
    assert!(!reordered.status.success());
    let reordered_store = ReplicaSqliteStore::open(
        &reordered_db,
        grant.peer.clone(),
        grant.reference().unwrap(),
        p::AuthorityEpoch(1),
        p::ReplicaScope {
            schema_version: p::M4_SCHEMA_VERSION,
            scopes: vec![p::Scope(SCOPE.into())],
            allow_owner_view: false,
        },
    )
    .unwrap();
    assert_eq!(
        reordered_store.cursor(&grant.peer, &aggregate).unwrap(),
        zero
    );
    drop(reordered_store);

    let mut changed_semantics = first.clone();
    changed_semantics.redaction = p::RedactionPolicyRef("redaction:changed".into());
    changed_semantics.refresh_digest().unwrap();
    let changed = run_replicad(&primary_db, &grant, &changed_semantics);
    assert!(!changed.status.success());
    let mut tampered = second.clone();
    tampered.content_digest = p::SchemaDigest("sha256:tampered".into());
    let tampered_output = run_replicad(&primary_db, &grant, &tampered);
    assert!(!tampered_output.status.success());

    let reopened = ReplicaSqliteStore::open(
        &primary_db,
        grant.peer.clone(),
        grant.reference().unwrap(),
        p::AuthorityEpoch(1),
        p::ReplicaScope {
            schema_version: p::M4_SCHEMA_VERSION,
            scopes: vec![p::Scope(SCOPE.into())],
            allow_owner_view: false,
        },
    )
    .unwrap();
    assert_eq!(reopened.cursor(&grant.peer, &aggregate).unwrap(), first.to);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}
