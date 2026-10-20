use forme_protocol as p;

fn command() -> p::MaintenanceCommand {
    p::MaintenanceCommand {
        schema_version: p::SchemaVersion(1),
        operation_id: "op-1".into(),
        idempotency_key: "command-1".into(),
        expected_revision: 1,
        actor_proof: p::MaintenanceProofRef {
            schema_version: p::SchemaVersion(1),
            proof_ref: "proof-1".into(),
        },
        kind: p::MaintenanceKind::StartCore,
        requested_deadline: 1000,
    }
}

#[test]
fn product_commands_are_closed_and_never_accept_shell_or_owner_reassignment() {
    let valid = serde_json::to_value(command()).unwrap();
    assert!(serde_json::from_value::<p::MaintenanceCommand>(valid.clone()).is_ok());
    for injected in ["principal", "owner", "program", "parameters", "env"] {
        let mut value = valid.clone();
        value[injected] = "forged".into();
        assert!(serde_json::from_value::<p::MaintenanceCommand>(value).is_err());
    }
    for kind in [
        "execute_shell",
        "set_owner",
        "run_sql",
        "write_arbitrary_file",
    ] {
        let mut value = valid.clone();
        value["kind"] = serde_json::json!({"kind":kind});
        assert!(serde_json::from_value::<p::MaintenanceCommand>(value).is_err());
    }
    let mut value = command();
    value.schema_version = p::SchemaVersion(2);
    assert!(value.validate().is_err());
}

#[test]
fn uninstall_cannot_smuggle_owner_data_deletion_into_maintenance() {
    assert!(p::MaintenanceKind::UninstallPlan {
        preserve_owner_data: true
    }
    .validate()
    .is_ok());
    assert!(p::MaintenanceKind::UninstallPlan {
        preserve_owner_data: false
    }
    .validate()
    .is_err());
    assert!(!p::MaintenanceKind::StartCore.setup_allowed());
    assert!(p::MaintenanceKind::PrepareRestore {
        backup_ref: "backup-1".into()
    }
    .setup_allowed());
}

#[test]
fn release_paths_reject_cross_platform_escapes_aliases_and_device_names() {
    for bad in [
        "/root/file",
        "../file",
        "a/../file",
        "a//file",
        "a/./file",
        "C:/private",
        "a\\file",
        "//server/share",
        "web/CON.txt",
        "aux",
        "LPT1.dat",
        "COM9",
        "web/file.",
        "web/file ",
        "file:stream",
        "web/<file>",
        "web/file\nname",
    ] {
        assert!(
            p::validate_release_relative_path(bad).is_err(),
            "accepted: {bad}"
        );
    }
    for good in ["bin/forme-hostd.exe", "web/assets/app-0123.js", "LICENSE"] {
        assert!(p::validate_release_relative_path(good).is_ok());
    }
}

#[test]
fn release_ids_and_compatibility_ranges_are_validated_before_use() {
    for bad in ["release:one", "..", "CON", "bad?name", "release/one"] {
        assert!(p::validate_release_id(bad).is_err());
    }
    assert!(p::validate_release_id("0.1.0-rc.1").is_ok());
    let invalid = p::SchemaReadWriteRange {
        schema_version: p::SchemaVersion(1),
        read_min: 2,
        read_max: 4,
        write_version: 1,
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn manifest_paths_and_accounts_cannot_alias_privileged_domains() {
    let root = std::env::temp_dir().join("forme-product-path-contract");
    let paths = p::ProductPaths {
        schema_version: p::SchemaVersion(1),
        releases: root.join("releases"),
        config: root.join("config"),
        brain: root.join("brain"),
        web_auth: root.join("web-auth"),
        host_state: root.join("host-state"),
        keys: root.join("keys"),
        logs: root.join("logs"),
        backups: root.join("backups"),
        workspaces: root.join("workspaces"),
        ipc: root.join("ipc"),
    };
    let mut manifest = p::InstallationManifest {
        schema_version: p::SchemaVersion(1),
        install_id: "installation-1".into(),
        target: p::ProductTarget::WindowsX86_64,
        release_id: "0.1.0".into(),
        release_digest: p::canonical_digest(&"release").unwrap(),
        paths,
        created_at: 1,
        services: [
            p::BuiltinServiceId::Host,
            p::BuiltinServiceId::Web,
            p::BuiltinServiceId::Brain,
        ]
        .into_iter()
        .map(|service| p::ServiceIdentityBinding {
            schema_version: p::SchemaVersion(1),
            service,
            account: service
                .required_account(p::ProductTarget::WindowsX86_64)
                .into(),
        })
        .collect(),
    };
    assert!(manifest.validate().is_ok());
    let original = manifest.clone();
    manifest.services[1].account = "LocalSystem".into();
    assert!(manifest.validate().is_err());
    manifest = original.clone();
    manifest.paths.keys = manifest.paths.brain.join("keys");
    assert!(manifest.validate().is_err());
    #[cfg(windows)]
    {
        manifest = original;
        manifest.paths.keys = root.join("BRAIN/keys");
        assert!(manifest.validate().is_err());
    }
}

#[test]
fn lifecycle_cannot_skip_drain_backup_or_health_checks() {
    use p::MaintenanceStage::*;
    assert!(!Validated.permits(Switching, true));
    assert!(!Validated.permits(Checking, true));
    assert!(!Quiescing.permits(Switching, true));
    assert!(!Switching.permits(Committed, true));
    assert!(!Committed.permits(Planned, true));
    assert!(Validated.permits(Checking, false));
    assert!(Switching.permits(RecoveryRequired, true));
    assert!(!RecoveryRequired.permits(Switching, true));
}

#[test]
fn maintenance_authentication_can_renew_without_changing_bound_command_or_references() {
    let original = command();
    let mut renewed = original.clone();
    renewed.actor_proof.proof_ref = "renewed-proof".into();
    assert_eq!(original.digest().unwrap(), renewed.digest().unwrap());
    renewed.kind = p::MaintenanceKind::StopCore;
    assert_ne!(original.digest().unwrap(), renewed.digest().unwrap());
    let mut refs = p::MaintenanceReferences {
        schema_version: p::SchemaVersion(1),
        original_release_ref: "0.1.0".into(),
        target_release_ref: "0.1.0".into(),
        data_candidate_ref: None,
        backup_ref: None,
    };
    assert!(refs.validate(&original.kind).is_ok());
    refs.target_release_ref = "unrelated-release".into();
    assert!(refs.validate(&original.kind).is_err());
    refs.target_release_ref = "0.1.0".into();
    refs.data_candidate_ref = Some("unexpected-candidate".into());
    assert!(refs.validate(&original.kind).is_err());
}

#[test]
fn productization_keeps_brain_taxonomy_unchanged() {
    assert_eq!(p::EventKind::ALL.len(), 99);
    assert_eq!(
        p::EventKind::ALL.last(),
        Some(&p::EventKind::DataLifecycleApplied)
    );
}
