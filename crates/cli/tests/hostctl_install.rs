use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_cli::hostctl_install::{prepare, PrepareRequest};
use forme_gateway::web::WebServerConfig;
use forme_protocol as p;

fn temporary_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("forme-hostctl-{label}-{nonce}"));
    fs::create_dir(&path).unwrap();
    path
}

fn request(root: &Path) -> PrepareRequest {
    PrepareRequest {
        install_root: root.join("install"),
        state_root: root.join("state"),
        config_root: root.join("config"),
        log_root: root.join("logs"),
        ipc_root: root.join("ipc"),
        target: p::ProductTarget::WindowsX86_64,
        release_id: "dev-release".into(),
        release_digest: p::SchemaDigest(format!("sha256:{}", "a".repeat(64))),
    }
}

#[test]
fn first_run_creates_unclaimed_web_setup_without_core_config() {
    let root = temporary_root("first");
    let request = request(&root);
    let report = prepare(request.clone()).unwrap();

    assert!(report.created);
    assert!(report.manifest_path.is_file());
    assert!(report.host_config_path.is_file());
    assert!(report.bootstrap_config_path.is_file());
    assert!(report.core_config_path.is_none());
    assert!(WebServerConfig::load(&report.web_config_path).is_ok());
    let manifest: p::InstallationManifest =
        serde_json::from_slice(&fs::read(&report.manifest_path).unwrap()).unwrap();
    assert_eq!(manifest.install_id, report.install_ref);
    assert_eq!(manifest.services.len(), 3);
    assert_eq!(
        fs::read(root.join("state/keys/bootstrap.key"))
            .unwrap()
            .len(),
        32
    );
    assert_eq!(
        fs::read_to_string(root.join("state/web-auth/bootstrap-web.token"))
            .unwrap()
            .trim()
            .len(),
        64
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeated_prepare_is_idempotent_and_different_release_is_rejected() {
    let root = temporary_root("repeat");
    let initial = request(&root);
    let first = prepare(initial.clone()).unwrap();
    let key = fs::read(root.join("state/keys/bootstrap.key")).unwrap();
    let second = prepare(initial).unwrap();
    assert!(!second.created);
    assert_eq!(first.install_ref, second.install_ref);
    assert_eq!(
        key,
        fs::read(root.join("state/keys/bootstrap.key")).unwrap()
    );

    let mut changed = request(&root);
    changed.release_id = "another-release".into();
    let error = prepare(changed).unwrap_err();
    assert!(error.contains("another release or path set"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn non_empty_brain_directory_requires_explicit_claim_or_recovery() {
    let root = temporary_root("existing");
    let brain = root.join("state/brain");
    fs::create_dir_all(&brain).unwrap();
    fs::write(brain.join("existing.sqlite"), b"owner-data").unwrap();
    let error = prepare(request(&root)).unwrap_err();
    assert!(error.contains("explicit claim or recovery"));
    assert_eq!(
        fs::read(brain.join("existing.sqlite")).unwrap(),
        b"owner-data"
    );
    fs::remove_dir_all(root).unwrap();
}
