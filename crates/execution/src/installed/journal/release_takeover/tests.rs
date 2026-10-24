use super::*;
fn hash(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}
fn bytes(journal: &ProductJournal) -> Vec<u8> {
    let mut writer = journal.writer.lock().unwrap();
    let at = writer.file.stream_position().unwrap();
    writer.file.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    writer.file.read_to_end(&mut bytes).unwrap();
    writer.file.seek(SeekFrom::Start(at)).unwrap();
    bytes
}
fn fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    ProductJournal,
    p::ProductReleaseTakeover,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("product.jsonl");
    let journal = ProductJournal::open(&path, "install:takeover").unwrap();
    journal
        .record_installation_verification_with_active_release(
            hash("installation"),
            hash("old"),
            Some(("0.1.0", &hash("old"))),
        )
        .unwrap();
    let command = p::MaintenanceCommand {
        schema_version: p::SchemaVersion(1),
        operation_id: "upgrade:takeover".into(),
        idempotency_key: "key:takeover".into(),
        expected_revision: journal.revision().unwrap(),
        actor_proof: p::MaintenanceProofRef {
            schema_version: p::SchemaVersion(1),
            proof_ref: "proof:owner".into(),
        },
        kind: p::MaintenanceKind::Upgrade {
            release_id: "0.2.0".into(),
            release_digest: hash("target"),
            backup_ref: "backup:takeover".into(),
        },
        requested_deadline: 10000,
    };
    let op = journal
        .plan(
            command,
            p::MaintenanceActorClass::OwnerReauthenticated,
            p::MaintenanceReferences {
                schema_version: p::SchemaVersion(1),
                original_release_ref: "0.1.0".into(),
                target_release_ref: "0.2.0".into(),
                backup_ref: Some("backup:takeover".into()),
                data_candidate_ref: None,
            },
            1,
        )
        .unwrap();
    stage(&journal, &op.operation_id, p::MaintenanceStage::Validated);
    let takeover = p::ProductReleaseTakeover {
        schema_version: p::SchemaVersion(1),
        release: p::ProductReleaseRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: "install:takeover".into(),
            operation_id: op.operation_id,
            command_digest: op.command_digest,
            change: p::ProductReleaseChange::Upgrade,
            original_release_ref: "0.1.0".into(),
            original_release_digest: hash("old"),
            target_release_ref: "0.2.0".into(),
            target_release_digest: hash("target"),
            backup_ref: "backup:takeover".into(),
        },
        installation_digest: hash("installation"),
        owner: p::OwnerPrincipalRef("owner:test".into()),
        expected_config_revision: 1,
        expected_config_digest: hash("config"),
        deadline: 10000,
        readiness_nonce: "a".repeat(64),
    };
    (root, path, journal, takeover)
}
fn stage(journal: &ProductJournal, id: &str, stage: p::MaintenanceStage) {
    journal
        .begin_stage(
            id,
            journal.revision().unwrap(),
            stage,
            p::MaintenanceActorClass::OwnerReauthenticated,
            2,
        )
        .unwrap();
    journal
        .finish_stage(
            id,
            journal.revision().unwrap(),
            stage,
            vec![hash("receipt")],
            3,
        )
        .unwrap();
}
fn checking(t: &p::ProductReleaseTakeover) -> p::ProductReleaseChecking {
    let core = hash("core-image");
    p::ProductReleaseChecking {
        schema_version: p::SchemaVersion(1),
        takeover: t.clone(),
        binding: p::ProductReleaseBinding {
            schema_version: p::SchemaVersion(1),
            install_ref: t.release.install_ref.clone(),
            operation_id: t.release.operation_id.clone(),
            command_digest: t.release.command_digest.clone(),
            target_release_ref: t.release.target_release_ref.clone(),
            target_release_digest: t.release.target_release_digest.clone(),
            core_image_digest: core.clone(),
            owner_digest: p::sha256_content_digest(t.owner.0.as_bytes()),
            store_identity: hash("store"),
            config_revision: 2,
            readiness_nonce: t.readiness_nonce.clone(),
        },
        data_identity: p::ProductReleaseDataIdentity {
            schema_version: p::SchemaVersion(1),
            config_schema_version: 1,
            database_schema_version: 13,
            owner_digest: p::sha256_content_digest(t.owner.0.as_bytes()),
            store_identity: hash("store"),
            schema_digest: hash("schema"),
            database_digest: hash("db"),
            event_count: 10,
            last_stream_seq: 7,
        },
        service_receipt: p::ProductReleaseServiceReceipt {
            schema_version: p::SchemaVersion(1),
            operation_id: t.release.operation_id.clone(),
            command_digest: t.release.command_digest.clone(),
            images: [
                p::BuiltinServiceId::Host,
                p::BuiltinServiceId::Web,
                p::BuiltinServiceId::Brain,
            ]
            .into_iter()
            .map(|id| p::ProductReleaseServiceImageReceipt {
                schema_version: p::SchemaVersion(1),
                service: id,
                source_image_digest: hash("old-image"),
                target_image_digest: core.clone(),
                configuration_digest: hash("service-config"),
            })
            .collect(),
        },
    }
}

#[test]
fn takeover_rejects_stale_cas_foreign_binding_duplicate_and_expiry_without_writes() {
    let (_root, path, journal, takeover) = fixture();
    let baseline = bytes(&journal);
    let revision = journal.revision().unwrap();
    for i in 0..7 {
        let mut altered = takeover.clone();
        match i {
            0 => altered.installation_digest = hash("wrong"),
            1 => altered.release.command_digest = hash("wrong"),
            2 => altered.release.original_release_digest = hash("wrong"),
            3 => altered.release.backup_ref = "backup:wrong".into(),
            4 => altered.deadline = 10001,
            5 => altered.release.target_release_ref = "0.3.0".into(),
            _ => {}
        }
        let at = if i == 6 { 10000 } else { 4 };
        assert!(journal
            .prepare_release_takeover(revision, altered, at)
            .is_err());
        assert_eq!(bytes(&journal), baseline);
    }
    assert!(journal
        .prepare_release_takeover(revision - 1, takeover.clone(), 4)
        .is_err());
    journal
        .prepare_release_takeover(revision, takeover.clone(), 4)
        .unwrap();
    let saved = bytes(&journal);
    assert!(journal
        .prepare_release_takeover(journal.revision().unwrap(), takeover.clone(), 5)
        .is_err());
    assert_eq!(bytes(&journal), saved);
    assert!(journal.release_is_unresolved().unwrap());
    drop(journal);
    let replayed = ProductJournal::open(&path, "install:takeover").unwrap();
    assert_eq!(
        replayed
            .release_takeover(&takeover.release.operation_id)
            .unwrap(),
        Some(takeover)
    );
    assert_eq!(bytes(&replayed), saved);
}

#[test]
fn checking_requires_images_switched_active_target_and_original_takeover_then_replays_read_only() {
    let (_root, path, journal, takeover) = fixture();
    let id = &takeover.release.operation_id;
    journal
        .prepare_release_takeover(journal.revision().unwrap(), takeover.clone(), 4)
        .unwrap();
    let checking = checking(&takeover);
    assert!(journal
        .prepare_release_checking(journal.revision().unwrap(), checking.clone(), 5)
        .is_err());
    stage(&journal, id, p::MaintenanceStage::Quiescing);
    stage(&journal, id, p::MaintenanceStage::BackedUp);
    journal
        .begin_stage(
            id,
            journal.revision().unwrap(),
            p::MaintenanceStage::Switching,
            p::MaintenanceActorClass::OwnerReauthenticated,
            5,
        )
        .unwrap();
    journal
        .claim_release_images(
            journal.revision().unwrap(),
            id,
            takeover.release.command_digest.clone(),
            hash("prepared"),
            5,
        )
        .unwrap();
    journal
        .change_active_release(
            id,
            journal.revision().unwrap(),
            "0.1.0",
            &hash("old"),
            "0.2.0",
            &hash("target"),
        )
        .unwrap();
    journal
        .finish_stage(
            id,
            journal.revision().unwrap(),
            p::MaintenanceStage::Switching,
            vec![hash("switched")],
            6,
        )
        .unwrap();
    journal
        .begin_stage(
            id,
            journal.revision().unwrap(),
            p::MaintenanceStage::Checking,
            p::MaintenanceActorClass::OwnerReauthenticated,
            7,
        )
        .unwrap();
    let mut wrong = checking.clone();
    wrong.binding.readiness_nonce = "b".repeat(64);
    assert!(journal
        .prepare_release_checking(journal.revision().unwrap(), wrong, 7)
        .is_err());
    journal
        .prepare_release_checking(journal.revision().unwrap(), checking.clone(), 7)
        .unwrap();
    let before = bytes(&journal);
    drop(journal);
    let journal = ProductJournal::open(&path, "install:takeover").unwrap();
    assert_eq!(journal.release_checking(id).unwrap(), Some(checking));
    assert_eq!(
        journal.operation(id).unwrap().pending_stage,
        Some(p::MaintenanceStage::Checking)
    );
    assert!(journal.release_images_claimed(id).unwrap());
    assert_eq!(bytes(&journal), before);
}
