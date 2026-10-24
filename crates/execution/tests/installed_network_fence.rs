//! Durable management admission, with real files and no native side effects.
use forme_execution::installed::ProductJournal;
use forme_protocol as p;

fn network() -> p::MaintenanceKind {
    p::MaintenanceKind::ConfigureLan {
        config_revision: 1,
        validation_digest: p::sha256_content_digest(b"network-test-material"),
    }
}

fn transitions() -> Vec<p::MaintenanceKind> {
    vec![
        network(),
        p::MaintenanceKind::ApplyValidatedConfig {
            config_revision: 1,
            validation_digest: p::sha256_content_digest(b"network-test-config"),
        },
        p::MaintenanceKind::Upgrade {
            release_id: "0.2.0".into(),
            release_digest: p::sha256_content_digest(b"network-test-upgrade"),
            backup_ref: "backup:network-test".into(),
        },
        p::MaintenanceKind::RollbackCompatibleRelease {
            release_id: "0.0.1".into(),
            release_digest: p::sha256_content_digest(b"network-test-rollback"),
            backup_ref: "backup:network-test".into(),
        },
        p::MaintenanceKind::CommitRestore {
            candidate_ref: "restored-network.db".into(),
            candidate_digest: p::sha256_content_digest(b"network-test-restore"),
            owner_digest: p::sha256_content_digest(b"owner:network-test"),
            config_revision: 1,
        },
        p::MaintenanceKind::UninstallPlan {
            preserve_owner_data: true,
        },
    ]
}

fn request(
    journal: &ProductJournal,
    label: &str,
    kind: p::MaintenanceKind,
) -> (p::MaintenanceCommand, p::MaintenanceReferences) {
    let mut references = p::MaintenanceReferences {
        schema_version: p::SchemaVersion(1),
        original_release_ref: "0.1.0".into(),
        target_release_ref: "0.1.0".into(),
        backup_ref: None,
        data_candidate_ref: None,
    };
    match &kind {
        p::MaintenanceKind::Upgrade {
            release_id,
            backup_ref,
            ..
        }
        | p::MaintenanceKind::RollbackCompatibleRelease {
            release_id,
            backup_ref,
            ..
        } => {
            references.target_release_ref = release_id.clone();
            references.backup_ref = Some(backup_ref.clone());
        }
        p::MaintenanceKind::CommitRestore { candidate_ref, .. } => {
            references.data_candidate_ref = Some(candidate_ref.clone());
        }
        _ => {}
    }
    (
        p::MaintenanceCommand {
            schema_version: p::SchemaVersion(1),
            operation_id: label.into(),
            idempotency_key: format!("key:{label}"),
            expected_revision: journal.revision().unwrap(),
            actor_proof: p::MaintenanceProofRef {
                schema_version: p::SchemaVersion(1),
                proof_ref: "proof:network-fence".into(),
            },
            kind,
            requested_deadline: 100_000,
        },
        references,
    )
}

fn plan(
    journal: &ProductJournal,
    label: &str,
    kind: p::MaintenanceKind,
) -> p::MaintenanceOperation {
    let (command, references) = request(journal, label, kind);
    journal
        .plan(
            command,
            p::MaintenanceActorClass::LocalAdministrator,
            references,
            1,
        )
        .unwrap()
}

fn reject_without_writes(journal: &ProductJournal, kind: p::MaintenanceKind) {
    let revision = journal.revision().unwrap();
    let count = journal.operations(0).unwrap().len();
    let (command, references) = request(journal, "new:network-transition", kind);
    let error = journal
        .plan(
            command,
            p::MaintenanceActorClass::LocalAdministrator,
            references,
            2,
        )
        .unwrap_err();
    assert!(error.0.contains("requires recovery"), "{error}");
    assert_eq!(journal.revision().unwrap(), revision);
    assert_eq!(journal.operations(0).unwrap().len(), count);
}

fn stage(journal: &ProductJournal, next: p::MaintenanceStage) {
    journal
        .begin_stage(
            "network:original",
            journal.revision().unwrap(),
            next,
            p::MaintenanceActorClass::LocalAdministrator,
            3,
        )
        .unwrap();
    journal
        .finish_stage(
            "network:original",
            journal.revision().unwrap(),
            next,
            vec![p::sha256_content_digest(b"network-test-stage")],
            4,
        )
        .unwrap();
}

#[test]
fn planned_network_survives_restart_and_rejects_new_transition_keys_with_fresh_cas() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("product.jsonl");
    let journal = ProductJournal::open(&path, "install:network-fence").unwrap();
    let original = request(&journal, "network:original", network());
    journal
        .plan(
            original.0.clone(),
            p::MaintenanceActorClass::LocalAdministrator,
            original.1.clone(),
            1,
        )
        .unwrap();
    drop(journal);
    let journal = ProductJournal::open(&path, "install:network-fence").unwrap();
    for transition in transitions() {
        reject_without_writes(&journal, transition);
    }
    // Read-only projections do not create a second management transaction.
    let before = journal.operation("network:original").unwrap();
    assert_eq!(journal.operations(0).unwrap(), vec![before.clone()]);
    assert_eq!(journal.operation("network:original").unwrap(), before);
    let revision = journal.revision().unwrap();
    let same = journal
        .plan(
            original.0,
            p::MaintenanceActorClass::LocalAdministrator,
            original.1,
            9,
        )
        .unwrap();
    assert_eq!(same, before);
    assert_eq!(journal.revision().unwrap(), revision);
    reject_without_writes(&journal, network());
}

#[test]
fn unknown_network_intent_retains_its_fence_after_recovery_required_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("product.jsonl");
    let journal = ProductJournal::open(&path, "install:network-fence").unwrap();
    plan(&journal, "network:original", network());
    stage(&journal, p::MaintenanceStage::Validated);
    journal
        .begin_stage(
            "network:original",
            journal.revision().unwrap(),
            p::MaintenanceStage::Checking,
            p::MaintenanceActorClass::LocalAdministrator,
            5,
        )
        .unwrap();
    journal
        .require_recovery(
            "network:original",
            journal.revision().unwrap(),
            p::sha256_content_digest(b"unknown-native-network-result"),
            6,
        )
        .unwrap();
    drop(journal);
    let journal = ProductJournal::open(&path, "install:network-fence").unwrap();
    let original = journal.operation("network:original").unwrap();
    assert_eq!(original.stage, p::MaintenanceStage::RecoveryRequired);
    assert_eq!(original.pending_stage, Some(p::MaintenanceStage::Checking));
    reject_without_writes(&journal, network());
}

#[test]
fn unfinished_lifecycle_operations_block_network_admission_in_the_reverse_direction() {
    for transition in transitions().into_iter().skip(1) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("product.jsonl");
        let journal = ProductJournal::open(&path, "install:network-fence").unwrap();
        plan(&journal, "lifecycle:original", transition);
        drop(journal);
        let journal = ProductJournal::open(&path, "install:network-fence").unwrap();
        reject_without_writes(&journal, network());
    }
}

#[test]
fn committed_network_releases_admission_without_changing_the_stage_protocol() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("product.jsonl");
    let journal = ProductJournal::open(&path, "install:network-fence").unwrap();
    plan(&journal, "network:original", network());
    stage(&journal, p::MaintenanceStage::Validated);
    journal
        .begin_stage(
            "network:original",
            journal.revision().unwrap(),
            p::MaintenanceStage::Checking,
            p::MaintenanceActorClass::LocalAdministrator,
            5,
        )
        .unwrap();
    let requested_listener_digest = p::sha256_content_digest(b"requested-listener");
    journal
        .record_network_progress(
            "network:original",
            journal.revision().unwrap(),
            p::ProductNetworkProgress {
                schema_version: p::SchemaVersion(1),
                phase: p::ProductNetworkProgressPhase::TargetIntent,
                requested_listener_digest: requested_listener_digest.clone(),
                active_bind: None,
                active_listener_digest: None,
                health_receipt_digest: None,
                failure_ref: None,
            },
            6,
        )
        .unwrap();
    let health_receipt_digest = p::sha256_content_digest(b"health-receipt");
    journal
        .record_network_progress(
            "network:original",
            journal.revision().unwrap(),
            p::ProductNetworkProgress {
                schema_version: p::SchemaVersion(1),
                phase: p::ProductNetworkProgressPhase::TargetVerified,
                requested_listener_digest,
                active_bind: Some("127.0.0.1:8765".parse().unwrap()),
                active_listener_digest: Some(p::sha256_content_digest(b"active-listener")),
                health_receipt_digest: Some(health_receipt_digest.clone()),
                failure_ref: None,
            },
            7,
        )
        .unwrap();
    journal
        .finish_stage(
            "network:original",
            journal.revision().unwrap(),
            p::MaintenanceStage::Checking,
            vec![health_receipt_digest],
            8,
        )
        .unwrap();
    stage(&journal, p::MaintenanceStage::Committed);
    assert_eq!(
        plan(&journal, "network:next", network()).stage,
        p::MaintenanceStage::Planned
    );
}
