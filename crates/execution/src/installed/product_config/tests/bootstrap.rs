//! Revision-zero CAS consumes only the exact trusted first-adoption result.
use super::*;
use std::sync::Arc;

fn hash(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}

fn prepared(
    t: &p::ProductConfigTemplate,
    operation: &str,
    owner: &str,
) -> (
    p::BootstrapDataIntentV2,
    p::BootstrapDataPreparedResultV2,
    p::BootstrapClaimCommitV2,
) {
    let owner = p::OwnerPrincipalRef(owner.into());
    let mut candidate = p::BootstrapCandidateViewV2 {
        schema_version: p::SchemaVersion(2),
        candidate_ref: "candidate:original".into(),
        kind: p::BootstrapCandidateKind::ExistingBrain,
        status: p::BootstrapCandidateStatus::Available,
        candidate_digest: hash("pending"),
        verification_epoch: 1,
        owner_source: p::BootstrapOwnerSource::VerifiedFromCandidate,
        owner_ref: Some(owner.clone()),
        owner_binding_digest: Some(
            p::canonical_digest(&("forme-bootstrap-owner-binding-v2", &owner)).unwrap(),
        ),
        store_identity_digest: Some(hash("source-store")),
        schema_digest: Some(hash("source-schema")),
        protection_state: p::BootstrapProtectionState::Protected,
        source_observation_digest: hash("observation"),
        issue_ref: None,
    };
    candidate.candidate_digest = candidate.expected_digest(&t.install_ref).unwrap();
    let intent = p::BootstrapDataIntentV2 {
        schema_version: p::SchemaVersion(2),
        install_ref: t.install_ref.clone(),
        request: p::BootstrapClaimRequestV2 {
            schema_version: p::SchemaVersion(2),
            session: p::BootstrapSessionBindingV2 {
                schema_version: p::SchemaVersion(2),
                operation_id: operation.into(),
                setup_digest: hash("setup"),
            },
            candidate_ref: candidate.candidate_ref.clone(),
            expected_candidate_digest: candidate.candidate_digest.clone(),
            account_commitment: hash("original-account"),
        },
        candidate: candidate.clone(),
    };
    let root = t
        .brain_store_path
        .parent()
        .unwrap()
        .join("bootstrap-data")
        .join(intent.target_ref().unwrap());
    let mut data = p::BootstrapDataPreparedResultV2 {
        schema_version: p::SchemaVersion(2),
        prepared: p::BootstrapClaimDataPreparedV2 {
            schema_version: p::SchemaVersion(2),
            operation_id: operation.into(),
            candidate_ref: candidate.candidate_ref.clone(),
            candidate_digest: candidate.candidate_digest.clone(),
            verification_epoch: 1,
            target_binding_digest: hash("pending"),
            migration_outcome: p::BootstrapMigrationOutcome::Recovered,
            worker_receipt_digest: hash("pending"),
        },
        binding: p::ProductDataBinding {
            schema_version: p::PRODUCT_DATA_BINDING_SCHEMA_VERSION,
            candidate_ref: candidate.candidate_ref.clone(),
            store_path: root.join("forme.db"),
            content_key_directory: root.join("forme.content-keys"),
            owner_digest: hash(owner.0.as_str()),
            store_identity: hash("source-store"),
            manifest_digest: hash("target-manifest"),
            execution_fence_digest: Some(hash("fence")),
        },
        artifact_digest: hash("artifacts"),
        verification_digest: hash("actual-target-verification"),
    };
    data.prepared.target_binding_digest = data.target_digest().unwrap();
    data.prepared.worker_receipt_digest = data.worker_digest(&intent).unwrap();
    let commit = p::BootstrapClaimCommitV2 {
        schema_version: p::SchemaVersion(2),
        receipt: p::BootstrapClaimReceiptV2 {
            schema_version: p::SchemaVersion(2),
            install_ref: t.install_ref.clone(),
            operation_id: operation.into(),
            candidate_ref: candidate.candidate_ref,
            candidate_kind: candidate.kind,
            candidate_digest: candidate.candidate_digest,
            owner_source: candidate.owner_source,
            owner_ref: owner,
            owner_binding_digest: candidate.owner_binding_digest.unwrap(),
            account_commitment: intent.request.account_commitment.clone(),
            target_binding_digest: data.prepared.target_binding_digest.clone(),
            migration_outcome: p::BootstrapMigrationOutcome::Recovered,
            expires_at: 10000,
        },
    };
    data.validate_for(&intent).unwrap();
    (intent, data, commit)
}

#[test]
fn bootstrap_data_binding_is_exact_idempotent_revision_zero_and_survives_restart_without_runtime_ack(
) {
    let directory = tempfile::tempdir().unwrap();
    let t = template(directory.path());
    let path = directory.path().join("template.json");
    fs::write(&path, storage::encode(&t).unwrap()).unwrap();
    let store = ProductConfigStore::open(&path).unwrap();
    let (intent, data, commit) = prepared(&t, "claim:original", "owner:original");
    let snapshot = {
        let mut store = store.lock().unwrap();
        store.bootstrap_admission(&t.install_ref).unwrap();
        let snapshot = store
            .publish_bootstrap_data(&intent, &data, &commit)
            .unwrap();
        assert_eq!(
            store
                .publish_bootstrap_data(&intent, &data, &commit)
                .unwrap(),
            snapshot
        );
        assert_eq!(store.state.revision, 0);
        assert_eq!(store.state.owner, Some(commit.receipt.owner_ref.clone()));
        assert_eq!(store.state.active_data, Some(data.binding.clone()));
        assert!(store.state.models.is_empty() && store.state.credentials.is_empty());
        assert!(store.journal.acknowledged().is_none());
        assert!(store.bootstrap_admission(&t.install_ref).is_err());
        let mut replaced = commit.clone();
        replaced.receipt.account_commitment = hash("replaced-account");
        assert!(store
            .publish_bootstrap_data(&intent, &data, &replaced)
            .is_err());
        snapshot
    };
    drop(store);
    let reopened = ProductConfigStore::open(&path).unwrap();
    assert_eq!(
        reopened
            .lock()
            .unwrap()
            .publish_bootstrap_data(&intent, &data, &commit)
            .unwrap(),
        snapshot
    );
    assert!(!t.core_config_path.exists() && !t.configuration_ack_path.exists());
    assert!(!t.brain_store_path.exists());
}

#[test]
fn concurrent_first_adoptions_allow_one_owner_and_reject_foreign_target_before_publication() {
    let directory = tempfile::tempdir().unwrap();
    let t = template(directory.path());
    let path = directory.path().join("template.json");
    fs::write(&path, storage::encode(&t).unwrap()).unwrap();
    let store = Arc::new(ProductConfigStore::open(&path).unwrap());
    let (intent, mut data, mut commit) = prepared(&t, "claim:wrong-path", "owner:original");
    data.binding.store_path = directory.path().join("outside-authorized-target.db");
    data.prepared.target_binding_digest = data.target_digest().unwrap();
    data.prepared.worker_receipt_digest = data.worker_digest(&intent).unwrap();
    commit.receipt.target_binding_digest = data.prepared.target_binding_digest.clone();
    assert!(store
        .lock()
        .unwrap()
        .publish_bootstrap_data(&intent, &data, &commit)
        .is_err());
    assert!(store
        .lock()
        .unwrap()
        .bootstrap_admission(&t.install_ref)
        .is_ok());
    let a = prepared(&t, "claim:first", "owner:first");
    let b = prepared(&t, "claim:second", "owner:second");
    let concurrent = store.clone();
    let other = std::thread::spawn(move || {
        concurrent
            .lock()
            .unwrap()
            .publish_bootstrap_data(&a.0, &a.1, &a.2)
    });
    let local = store
        .lock()
        .unwrap()
        .publish_bootstrap_data(&b.0, &b.1, &b.2);
    assert_ne!(local.is_ok(), other.join().unwrap().is_ok());
    let store = store.lock().unwrap();
    assert_eq!(store.state.revision, 0);
    assert!(store.journal.acknowledged().is_none());
    assert!(!t.core_config_path.exists() && !t.configuration_ack_path.exists());
}
