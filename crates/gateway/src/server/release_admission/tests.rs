use super::*;

fn digest(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}
fn binding() -> p::ProductReleaseBinding {
    p::ProductReleaseBinding {
        schema_version: p::SchemaVersion(1),
        install_ref: "installation:release-test".into(),
        operation_id: "upgrade:test".into(),
        command_digest: digest("command"),
        target_release_ref: "0.0.2".into(),
        target_release_digest: digest("release"),
        core_image_digest: digest("image"),
        owner_digest: digest("owner:test"),
        store_identity: digest("store"),
        config_revision: 2,
        readiness_nonce: "a".repeat(64),
    }
}

#[test]
fn release_and_restore_guards_both_recheck_every_claim_and_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let release_path = root.path().join("release.json");
    let restore_path = root.path().join("restore.json");
    let binding = binding();
    let mut release = p::ProductReleaseAdmission {
        schema_version: p::SchemaVersion(1),
        binding_digest: binding.digest().unwrap(),
        released: false,
        at: 100,
    };
    let fence = digest("restore-fence");
    let mut restore = p::ProductWorkAdmission {
        schema_version: p::SchemaVersion(1),
        install_ref: binding.install_ref.clone(),
        operation_id: "restore:previous".into(),
        command_digest: digest("restore-command"),
        config_revision: 1,
        owner_digest: binding.owner_digest.clone(),
        execution_fence_digest: fence.clone(),
        released: true,
    };
    let identity = p::ProductDataAcknowledgement {
        schema_version: p::SchemaVersion(1),
        owner_digest: binding.owner_digest.clone(),
        store_identity: binding.store_identity.clone(),
        execution_fence_digest: Some(fence),
        composition_digest: digest("composition"),
    };
    fs::write(&restore_path, serde_json::to_vec(&restore).unwrap()).unwrap();
    fs::write(&release_path, serde_json::to_vec(&release).unwrap()).unwrap();
    let guard = Arc::new(
        super::super::maintenance_admission::managed_admission(
            restore_path.clone(),
            binding.install_ref.clone(),
            2,
            identity,
        )
        .and(managed_admission(release_path.clone(), binding.clone())),
    );
    assert!(guard.acquire().is_err());
    release.released = true;
    fs::write(&release_path, serde_json::to_vec(&release).unwrap()).unwrap();
    assert!(guard.acquire().is_ok());
    restore.released = false;
    fs::write(&restore_path, serde_json::to_vec(&restore).unwrap()).unwrap();
    assert!(guard.acquire().is_err());
    restore.released = true;
    fs::write(&restore_path, serde_json::to_vec(&restore).unwrap()).unwrap();
    assert!(guard.acquire().is_ok());
    release.binding_digest = digest("other-operation");
    fs::write(&release_path, serde_json::to_vec(&release).unwrap()).unwrap();
    assert!(guard.acquire().is_err());
    fs::write(&release_path, b"{").unwrap();
    assert!(guard.acquire().is_err());
    fs::remove_file(&release_path).unwrap();
    assert!(guard.acquire().is_err());
}

#[test]
fn actual_running_image_is_checked_before_the_brain_can_be_opened() {
    let mut binding = binding();
    binding.core_image_digest =
        p::sha256_reader_digest(fs::File::open(std::env::current_exe().unwrap()).unwrap()).unwrap();
    assert_eq!(
        current_image(Some(&binding)).unwrap(),
        Some(binding.core_image_digest.clone())
    );
    binding.core_image_digest = digest("wrong-image");
    assert!(current_image(Some(&binding)).is_err());
    assert!(current_image(None).unwrap().is_none());
}

#[test]
fn partial_or_foreign_release_transport_is_rejected_without_opening_a_store() {
    let root = tempfile::tempdir().unwrap();
    let mut value = super::super::managed_config::tests::fixture(root.path());
    value["brain"]["cognition"]["relevance_mode"] =
        serde_json::to_value(p::RelevanceMode::DirectModel).unwrap();
    let mut config: CoreServiceConfig = serde_json::from_value(value).unwrap();
    config.transport.release_binding = Some(binding());
    assert!(config.validate().is_err());
    config.transport.release_admission_path = Some(root.path().join("release.json"));
    config.transport.release_readiness_path = Some(root.path().join("readiness.json"));
    config.transport.configuration_ack_path = Some(root.path().join("ack.json"));
    assert!(config.validate().is_err());
    let b = config.transport.release_binding.as_mut().unwrap();
    b.install_ref = config.install_ref.clone();
    b.config_revision = config.revision;
    b.owner_digest = p::sha256_content_digest(config.brain.owner.0.as_bytes());
    assert!(config.validate().is_ok());
}
