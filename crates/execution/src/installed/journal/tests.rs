use super::*;
use std::path::PathBuf;

const INSTALL: &str = "journal-release-install";

fn digest(value: &str) -> p::SchemaDigest {
    p::canonical_digest(&value).unwrap()
}

fn references(target: &str, backup: Option<&str>) -> p::MaintenanceReferences {
    p::MaintenanceReferences {
        schema_version: p::SchemaVersion(1),
        original_release_ref: "0.1.0".into(),
        target_release_ref: target.into(),
        data_candidate_ref: None,
        backup_ref: backup.map(str::to_owned),
    }
}

fn upgrade_command(
    operation_id: &str,
    expected_revision: u64,
    target: &str,
) -> p::MaintenanceCommand {
    p::MaintenanceCommand {
        schema_version: p::SchemaVersion(1),
        operation_id: operation_id.into(),
        idempotency_key: format!("{operation_id}-key"),
        expected_revision,
        actor_proof: p::MaintenanceProofRef {
            schema_version: p::SchemaVersion(1),
            proof_ref: "proof:journal-fixture".into(),
        },
        kind: p::MaintenanceKind::Upgrade {
            release_id: target.into(),
            release_digest: digest("target-release"),
            backup_ref: "backup:journal-fixture".into(),
        },
        requested_deadline: 10_000,
    }
}

fn prepare_upgrade(journal: &ProductJournal) -> (p::MaintenanceCommand, p::SchemaDigest) {
    let current_digest = digest("current-release");
    journal
        .record_installation_verification_with_active_release(
            digest("installation"),
            current_digest.clone(),
            Some(("0.1.0", &current_digest)),
        )
        .unwrap();
    let command = upgrade_command("upgrade:journal", journal.revision().unwrap(), "0.2.0");
    let refs = references("0.2.0", Some("backup:journal-fixture"));
    journal
        .plan(
            command.clone(),
            p::MaintenanceActorClass::LocalAdministrator,
            refs,
            1,
        )
        .unwrap();
    for stage in [
        p::MaintenanceStage::Validated,
        p::MaintenanceStage::Quiescing,
        p::MaintenanceStage::BackedUp,
    ] {
        journal
            .begin_stage(
                &command.operation_id,
                journal.revision().unwrap(),
                stage,
                p::MaintenanceActorClass::LocalAdministrator,
                2,
            )
            .unwrap();
        journal
            .finish_stage(
                &command.operation_id,
                journal.revision().unwrap(),
                stage,
                vec![digest("receipt")],
                3,
            )
            .unwrap();
    }
    (command, current_digest)
}

fn open_journal() -> (tempfile::TempDir, PathBuf, ProductJournal) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("product.jsonl");
    let journal = ProductJournal::open(&path, INSTALL).unwrap();
    (directory, path, journal)
}

#[test]
fn historical_journal_hashes_preserve_absent_and_explicit_null_fields() {
    for explicit_null in [false, true] {
        let (_directory, path, journal) = open_journal();
        let (command, _) = prepare_upgrade(&journal);
        drop(journal);
        let bytes = std::fs::read(&path).unwrap();
        let mut previous = digest("forme-product-journal-v1");
        let mut historical = Vec::new();
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let mut envelope: serde_json::Value = serde_json::from_slice(line).unwrap();
            let fact = envelope["fact"].as_object_mut().unwrap();
            match fact["record"].as_str().unwrap() {
                "installation_verified" => {
                    fact.remove("active_release");
                    if explicit_null {
                        fact.insert("active_release".into(), serde_json::Value::Null);
                    }
                }
                "planned" => {
                    fact.remove("network_contract");
                }
                "stage_receipt" => {
                    fact.remove("network_result");
                }
                _ => {}
            }
            envelope["previous"] = serde_json::to_value(&previous).unwrap();
            previous = p::canonical_digest(&(
                p::SchemaVersion(1),
                envelope["product_seq"].as_u64().unwrap(),
                &previous,
                &envelope["fact"],
            ))
            .unwrap();
            envelope["digest"] = serde_json::to_value(&previous).unwrap();
            serde_json::to_writer(&mut historical, &envelope).unwrap();
            historical.push(b'\n');
        }
        std::fs::write(&path, &historical).unwrap();
        let reopened = ProductJournal::open(&path, INSTALL).unwrap();
        assert_eq!(
            reopened.operation(&command.operation_id).unwrap().stage,
            p::MaintenanceStage::BackedUp
        );
        assert_eq!(reopened.active_release().unwrap(), None);
        assert!(
            reopened
                .record_installation_verification_with_active_release(
                    digest("installation"),
                    digest("current-release"),
                    Some(("0.1.0", &digest("current-release"))),
                )
                .is_err(),
            "an interrupted operation must block a late initial binding"
        );
        drop(reopened);
        assert!(std::fs::read(&path).unwrap().starts_with(&historical));
        assert_eq!(
            ProductJournal::open(&path, INSTALL)
                .unwrap()
                .active_release()
                .unwrap(),
            None
        );
    }
}

#[test]
fn active_release_projection_replays_after_a_complete_switch() {
    let (_directory, path, journal) = open_journal();
    let (command, current_digest) = prepare_upgrade(&journal);
    journal
        .begin_stage(
            &command.operation_id,
            journal.revision().unwrap(),
            p::MaintenanceStage::Switching,
            p::MaintenanceActorClass::LocalAdministrator,
            4,
        )
        .unwrap();
    let target_digest = digest("target-release");
    journal
        .change_active_release(
            &command.operation_id,
            journal.revision().unwrap(),
            "0.1.0",
            &current_digest,
            "0.2.0",
            &target_digest,
        )
        .unwrap();
    journal
        .finish_stage(
            &command.operation_id,
            journal.revision().unwrap(),
            p::MaintenanceStage::Switching,
            vec![digest("switch-receipt")],
            5,
        )
        .unwrap();
    drop(journal);

    let reopened = ProductJournal::open(&path, INSTALL).unwrap();
    assert_eq!(
        reopened.active_release().unwrap(),
        Some(("0.2.0".into(), target_digest))
    );
    assert_eq!(
        reopened.operation(&command.operation_id).unwrap().stage,
        p::MaintenanceStage::Switching
    );
}

#[test]
fn active_release_change_rejects_stale_target_and_non_lifecycle_operations() {
    for (from_release, from_digest, to_release, to_digest) in [
        (
            "0.0.9",
            digest("current-release"),
            "0.2.0",
            digest("target-release"),
        ),
        (
            "0.1.0",
            digest("wrong-current-digest"),
            "0.2.0",
            digest("target-release"),
        ),
        (
            "0.1.0",
            digest("current-release"),
            "0.3.0",
            digest("target-release"),
        ),
        (
            "0.1.0",
            digest("current-release"),
            "0.2.0",
            digest("wrong-target-digest"),
        ),
    ] {
        let (_directory, _path, journal) = open_journal();
        let (command, _) = prepare_upgrade(&journal);
        let revision = journal.revision().unwrap();
        assert!(journal
            .change_active_release(
                &command.operation_id,
                revision,
                from_release,
                &from_digest,
                to_release,
                &to_digest,
            )
            .is_err());
        assert_eq!(journal.revision().unwrap(), revision);
    }

    let (_directory, _path, journal) = open_journal();
    let current_digest = digest("current-release");
    journal
        .record_installation_verification_with_active_release(
            digest("installation"),
            current_digest.clone(),
            Some(("0.1.0", &current_digest)),
        )
        .unwrap();
    let command = p::MaintenanceCommand {
        schema_version: p::SchemaVersion(1),
        operation_id: "restore:journal".into(),
        idempotency_key: "restore:journal-key".into(),
        expected_revision: journal.revision().unwrap(),
        actor_proof: p::MaintenanceProofRef {
            schema_version: p::SchemaVersion(1),
            proof_ref: "proof:journal-fixture".into(),
        },
        kind: p::MaintenanceKind::CommitRestore {
            candidate_ref: "candidate-journal".into(),
            candidate_digest: digest("candidate"),
            owner_digest: digest("owner"),
            config_revision: 1,
        },
        requested_deadline: 10_000,
    };
    journal
        .plan(
            command.clone(),
            p::MaintenanceActorClass::LocalAdministrator,
            p::MaintenanceReferences {
                schema_version: p::SchemaVersion(1),
                original_release_ref: "0.1.0".into(),
                target_release_ref: "0.1.0".into(),
                data_candidate_ref: Some("candidate-journal".into()),
                backup_ref: None,
            },
            1,
        )
        .unwrap();
    for stage in [
        p::MaintenanceStage::Validated,
        p::MaintenanceStage::Quiescing,
        p::MaintenanceStage::BackedUp,
    ] {
        journal
            .begin_stage(
                &command.operation_id,
                journal.revision().unwrap(),
                stage,
                p::MaintenanceActorClass::LocalAdministrator,
                2,
            )
            .unwrap();
        journal
            .finish_stage(
                &command.operation_id,
                journal.revision().unwrap(),
                stage,
                vec![digest("receipt")],
                3,
            )
            .unwrap();
    }
    journal
        .begin_stage(
            &command.operation_id,
            journal.revision().unwrap(),
            p::MaintenanceStage::Switching,
            p::MaintenanceActorClass::LocalAdministrator,
            4,
        )
        .unwrap();
    let revision = journal.revision().unwrap();
    assert!(journal
        .change_active_release(
            &command.operation_id,
            revision,
            "0.1.0",
            &current_digest,
            "0.2.0",
            &digest("target-release"),
        )
        .is_err());
    assert_eq!(journal.revision().unwrap(), revision);
}

#[test]
fn active_release_change_requires_switching_intent() {
    let (_directory, _path, journal) = open_journal();
    let (command, current_digest) = prepare_upgrade(&journal);
    let revision = journal.revision().unwrap();
    assert!(journal
        .change_active_release(
            &command.operation_id,
            revision,
            "0.1.0",
            &current_digest,
            "0.2.0",
            &digest("target-release"),
        )
        .is_err());
    assert_eq!(journal.revision().unwrap(), revision);
}
