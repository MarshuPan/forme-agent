//! Real encrypted storage checks without a model, core or data switch.
use super::{installed_restore::RestoreFixture, *};
use crate::offline_data::{verify_installed_backup, OfflineCoreMaintenance};

fn request(f: &RestoreFixture) -> p::ProductBackupVerificationRequest {
    p::ProductBackupVerificationRequest {
        schema_version: p::SchemaVersion(1),
        install_ref: f.request.install_ref.clone(),
        owner: f.request.owner.clone(),
        expected_config_revision: f.request.expected_config_revision,
        source: "verification-source.sqlite".into(),
    }
}
fn backup(f: &RestoreFixture, request: &p::ProductBackupVerificationRequest) -> PathBuf {
    let source =
        f.f.config
            .store_path
            .parent()
            .unwrap()
            .join("data-operations")
            .join(&request.source);
    let data = OfflineCoreMaintenance::open(&f.core, None).unwrap();
    data.backup(&source).unwrap();
    source
}

#[test]
fn offline_backup_verification_checks_real_encrypted_snapshot_without_loading_runtime() {
    let f = RestoreFixture::new();
    let request = request(&f);
    let source = backup(&f, &request);
    let original = fs::read(&f.f.config.store_path).unwrap();
    let original_backup = fs::read(&source).unwrap();
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&f.core).unwrap()).unwrap();
    config["model_proxy"] = serde_json::json!({"unavailable":"fixture-only"});
    config["brain"]["worker_binding"] = serde_json::json!({"unavailable":true});
    config["brain"]["worker_file_roots"] = serde_json::Value::Null;
    fs::write(&f.core, serde_json::to_vec(&config).unwrap()).unwrap();
    let response = verify_installed_backup(&f.manifest, request.clone()).unwrap();
    response.validate_for(&request).unwrap();
    assert!(response.verification.verified);
    assert!(response.verification.restoration.is_none());
    assert_eq!(fs::read(&f.f.config.store_path).unwrap(), original);
    assert_eq!(fs::read(&source).unwrap(), original_backup);
    assert_eq!(f.f.calls.load(Ordering::SeqCst), 0);
    let wire = serde_json::to_string(&response).unwrap();
    assert!(!wire.contains(f.f.root.to_str().unwrap()));
    assert!(!wire.contains("owner:"));
}

#[test]
fn offline_backup_verification_refuses_owner_revision_installation_and_path_substitution() {
    let f = RestoreFixture::new();
    let request = request(&f);
    backup(&f, &request);
    for case in ["owner", "revision", "installation", "path", "missing"] {
        let mut changed = request.clone();
        match case {
            "owner" => changed.owner = p::OwnerPrincipalRef("owner:foreign".into()),
            "revision" => changed.expected_config_revision += 1,
            "installation" => changed.install_ref = "install:other".into(),
            "path" => changed.source = "../source.db".into(),
            _ => changed.source = "missing.sqlite".into(),
        }
        assert!(
            verify_installed_backup(&f.manifest, changed).is_err(),
            "accepted {case}"
        );
    }
    assert!(!f
        .f
        .config
        .store_path
        .parent()
        .unwrap()
        .join("data-operations/missing.sqlite")
        .exists());
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&f.core).unwrap()).unwrap();
    let missing_keys = f.f.root.join("absent-keys");
    config["brain"]["content_key_directory"] = serde_json::to_value(&missing_keys).unwrap();
    fs::write(&f.core, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(verify_installed_backup(&f.manifest, request).is_err());
    assert!(!missing_keys.exists());
    assert_eq!(f.f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn offline_backup_verification_refuses_lock_contention_and_corruption_without_repair() {
    let f = RestoreFixture::new();
    let request = request(&f);
    let source = backup(&f, &request);
    let held = OfflineCoreMaintenance::open(&f.core, Some(&source)).unwrap();
    assert!(verify_installed_backup(&f.manifest, request.clone()).is_err());
    drop(held);
    verify_installed_backup(&f.manifest, request.clone()).unwrap();
    let mut corrupt = fs::read(&source).unwrap();
    corrupt[0] ^= 1;
    fs::write(&source, &corrupt).unwrap();
    assert!(verify_installed_backup(&f.manifest, request).is_err());
    assert_eq!(fs::read(&source).unwrap(), corrupt);
}
