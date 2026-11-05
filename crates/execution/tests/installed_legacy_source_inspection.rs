//! An actual offline executable observes only its bound original installation.
use forme_execution::installed::ProductJournal;
use forme_protocol as p;
use std::{
    path::Path,
    process::{Command, Output, Stdio},
};

fn hash(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}

fn installation(root: &Path) -> p::InstallationManifest {
    let target = p::ProductTarget::WindowsX86_64;
    p::InstallationManifest {
        schema_version: p::SchemaVersion(1),
        install_id: "install:offline-source".into(),
        target,
        release_id: "original-0.1".into(),
        release_digest: hash("original release"),
        paths: p::ProductPaths {
            schema_version: p::SchemaVersion(1),
            releases: root.join("program"),
            config: root.join("config"),
            brain: root.join("brain"),
            web_auth: root.join("web-auth"),
            host_state: root.join("host"),
            keys: root.join("keys"),
            logs: root.join("logs"),
            backups: root.join("backups"),
            workspaces: root.join("workspaces"),
            ipc: root.join("ipc"),
        },
        services: [
            p::BuiltinServiceId::Host,
            p::BuiltinServiceId::Web,
            p::BuiltinServiceId::Brain,
        ]
        .into_iter()
        .map(|service| p::ServiceIdentityBinding {
            schema_version: p::SchemaVersion(1),
            service,
            account: service.required_account(target).into(),
        })
        .collect(),
        created_at: 1,
    }
}

fn source(installation: &p::InstallationManifest) -> ProductJournal {
    let journal = ProductJournal::open(
        &installation.paths.host_state.join("host.journal"),
        &installation.install_id,
    )
    .unwrap();
    journal
        .record_installation_verification_with_active_release(
            installation.digest().unwrap(),
            installation.release_digest.clone(),
            Some((&installation.release_id, &installation.release_digest)),
        )
        .unwrap();
    let operation = journal
        .plan(
            p::MaintenanceCommand {
                schema_version: p::SchemaVersion(1),
                operation_id: "upgrade:original".into(),
                idempotency_key: "key:original".into(),
                expected_revision: journal.revision().unwrap(),
                actor_proof: p::MaintenanceProofRef {
                    schema_version: p::SchemaVersion(1),
                    proof_ref: "proof:fixture".into(),
                },
                kind: p::MaintenanceKind::Upgrade {
                    release_id: "target-0.2".into(),
                    release_digest: hash("target release"),
                    backup_ref: "backup:original".into(),
                },
                requested_deadline: 10_000,
            },
            p::MaintenanceActorClass::OwnerReauthenticated,
            p::MaintenanceReferences {
                schema_version: p::SchemaVersion(1),
                original_release_ref: installation.release_id.clone(),
                target_release_ref: "target-0.2".into(),
                data_candidate_ref: None,
                backup_ref: Some("backup:original".into()),
            },
            1,
        )
        .unwrap();
    journal
        .begin_stage(
            "upgrade:original",
            journal.revision().unwrap(),
            p::MaintenanceStage::Validated,
            p::MaintenanceActorClass::OwnerReauthenticated,
            2,
        )
        .unwrap();
    journal
        .finish_stage(
            "upgrade:original",
            journal.revision().unwrap(),
            p::MaintenanceStage::Validated,
            vec![hash("validation receipt")],
            3,
        )
        .unwrap();
    let takeover = p::ProductReleaseTakeover {
        schema_version: p::SchemaVersion(1),
        release: p::ProductReleaseRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: installation.install_id.clone(),
            operation_id: "upgrade:original".into(),
            command_digest: operation.command_digest,
            change: p::ProductReleaseChange::Upgrade,
            original_release_ref: installation.release_id.clone(),
            original_release_digest: installation.release_digest.clone(),
            target_release_ref: "target-0.2".into(),
            target_release_digest: hash("target release"),
            backup_ref: "backup:original".into(),
        },
        installation_digest: installation.digest().unwrap(),
        owner: p::OwnerPrincipalRef("owner:offline-source".into()),
        expected_config_revision: 1,
        expected_config_digest: hash("configuration"),
        deadline: 10_000,
        readiness_nonce: "b".repeat(64),
    };
    // Original on-disk fixture, not a new public API for the private writer.
    let sequence = journal.revision().unwrap() + 1;
    let path = installation.paths.host_state.join("host.journal");
    drop(journal);
    let mut bytes = std::fs::read(&path).unwrap();
    let last: serde_json::Value = serde_json::from_slice(
        bytes
            .split(|byte| *byte == b'\n')
            .rfind(|line| !line.is_empty())
            .unwrap(),
    )
    .unwrap();
    let previous: p::SchemaDigest = serde_json::from_value(last["digest"].clone()).unwrap();
    let fact =
        serde_json::json!({"record":"release_takeover_prepared", "takeover":takeover, "at":4});
    let digest = p::canonical_digest(&(1u32, sequence, &previous, &fact)).unwrap();
    let envelope = serde_json::json!({"schema_version":1, "product_seq":sequence,
        "previous":previous, "fact":fact, "digest":digest});
    bytes.extend(serde_json::to_vec(&envelope).unwrap());
    bytes.push(b'\n');
    std::fs::write(&path, bytes).unwrap();
    let journal = ProductJournal::open(&path, &installation.install_id).unwrap();
    journal
        .begin_stage(
            "upgrade:original",
            journal.revision().unwrap(),
            p::MaintenanceStage::Quiescing,
            p::MaintenanceActorClass::OwnerReauthenticated,
            5,
        )
        .unwrap();
    journal
}

fn inspect(manifest: &Path, root: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forme-hostd"));
    command
        .args(["--inspect-legacy-repair-source", "--installation"])
        .arg(manifest)
        .args(["--operation", "upgrade:original"])
        .env_clear()
        .env(
            "FORME_HOST_JOURNAL_PATH",
            root.join("never-created.journal"),
        )
        .env("FORME_HOST_BIND", "invalid-must-not-be-read")
        .current_dir(root)
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.output().unwrap()
}

#[test]
fn offline_source_child_uses_fixed_original_and_rejects_writer_alias_or_manifest_drift() {
    let root = tempfile::tempdir().unwrap();
    let installation = installation(root.path());
    for directory in [&installation.paths.config, &installation.paths.host_state] {
        std::fs::create_dir(directory).unwrap();
    }
    let path = installation.paths.config.join("installation.json");
    let original_manifest = serde_json::to_vec(&installation).unwrap();
    std::fs::write(&path, &original_manifest).unwrap();
    let journal = source(&installation);
    let revision = journal.revision().unwrap();
    let source_path = installation.paths.host_state.join("host.journal");
    assert!(!inspect(&path, root.path()).status.success());
    assert_eq!(journal.revision().unwrap(), revision);
    drop(journal);
    let original_journal = std::fs::read(&source_path).unwrap();
    let output = inspect(&path, root.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(snapshot["journal_seq"], revision);
    assert_eq!(
        snapshot["journal_digest"],
        serde_json::json!(p::sha256_content_digest(&original_journal))
    );
    assert_eq!(snapshot["owner"], "owner:offline-source");
    assert_eq!(snapshot["stage"], "validated");
    assert_eq!(snapshot["pending_stage"], "quiescing");
    assert_eq!(snapshot["recovery_baseline_present"], false);
    let alias = installation.paths.config.join("copied-installation.json");
    std::fs::write(&alias, &original_manifest).unwrap();
    assert!(!inspect(&alias, root.path()).status.success());
    let mut changed = installation.clone();
    changed.created_at += 1;
    let changed_bytes = serde_json::to_vec(&changed).unwrap();
    std::fs::write(&path, &changed_bytes).unwrap();
    let rejected = inspect(&path, root.path());
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), changed_bytes);
    assert_eq!(std::fs::read(&source_path).unwrap(), original_journal);
    assert!(!root.path().join("never-created.journal").exists());
    for directory in [
        &installation.paths.brain,
        &installation.paths.keys,
        &installation.paths.logs,
    ] {
        assert!(!directory.exists());
    }
}
