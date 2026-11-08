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
    let template: p::ProductConfigTemplate =
        serde_json::from_slice(&fs::read(root.join("config/product-template.json")).unwrap())
            .unwrap();
    assert_eq!(
        template.core_config_path,
        root.join("config/core/core.json")
    );
    assert_eq!(
        template.core_session_fence_path,
        root.join("state/brain/runtime/core-auth.sqlite")
    );
    assert_eq!(
        template.configuration_ack_path,
        root.join("state/brain/runtime/core-config-ack.json")
    );
    #[cfg(windows)]
    for path in [
        &template.core_config_path,
        &template.configuration_ack_path,
        &template.core_session_fence_path,
    ] {
        // The native installer compares serialized paths, not Path components.
        assert!(!path.to_string_lossy().contains('/'));
    }
    assert!(root.join("state/keys/model-credentials").is_dir());
    // Preserve the empty-new-brain admission contract before owner claim.
    assert!(fs::read_dir(root.join("state/brain"))
        .unwrap()
        .next()
        .is_none());
    assert!(WebServerConfig::load(&report.web_config_path).is_ok());
    let manifest: p::InstallationManifest =
        serde_json::from_slice(&fs::read(&report.manifest_path).unwrap()).unwrap();
    assert_eq!(manifest.install_id, report.install_ref);
    assert_eq!(manifest.services.len(), 3);
    let host: serde_json::Value =
        serde_json::from_slice(&fs::read(&report.host_config_path).unwrap()).unwrap();
    let worker_path = root.join("config/worker.json");
    assert_eq!(
        serde_json::from_value::<PathBuf>(host["worker_config_path"].clone()).unwrap(),
        worker_path
    );
    let worker: serde_json::Value =
        serde_json::from_slice(&fs::read(&worker_path).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_value::<PathBuf>(worker["launch_profiles"][0]["program"].clone()).unwrap(),
        manifest.paths.releases.join("bin/forme-file-worker.exe")
    );
    assert_eq!(worker["authority_epoch"], 1);
    assert_eq!(
        worker["allowed_scopes"],
        serde_json::json!(["workspace:default"])
    );
    assert_eq!(
        serde_json::from_value::<PathBuf>(worker["workspace_roots"]["workspace:default"].clone())
            .unwrap(),
        root.join("state/workspaces/default")
    );
    assert_eq!(
        worker["launch_profiles"][0]["args"],
        serde_json::json!(["--installed-file-worker"])
    );
    assert_eq!(
        worker["launch_profiles"][0]["sandbox_profile_ref"],
        "sandbox:installed-file-v1"
    );
    assert!(root.join("state/workspaces/default").is_dir());
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
fn reopening_legacy_installation_does_not_provision_a_worker() {
    let root = temporary_root("no-worker-migration");
    let initial = request(&root);
    let first = prepare(initial.clone()).unwrap();
    let worker_path = root.join("config/worker.json");
    fs::remove_file(&worker_path).unwrap();
    let mut host: serde_json::Value =
        serde_json::from_slice(&fs::read(&first.host_config_path).unwrap()).unwrap();
    host["worker_config_path"] = serde_json::Value::Null;
    let bytes = serde_json::to_vec(&host).unwrap();
    fs::write(&first.host_config_path, &bytes).unwrap();
    let reopened = prepare(initial).unwrap();
    assert!(!reopened.created);
    assert!(!worker_path.exists());
    assert_eq!(fs::read(&reopened.host_config_path).unwrap(), bytes);
    assert!(fs::read_dir(root.join("state/brain"))
        .unwrap()
        .next()
        .is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn posix_preparation_keeps_file_worker_unprovisioned() {
    for target in [
        p::ProductTarget::Ubuntu2404X86_64,
        p::ProductTarget::MacosArm64,
    ] {
        let root = temporary_root("posix-no-worker");
        let mut request = request(&root);
        request.target = target;
        let report = prepare(request).unwrap();
        let host: serde_json::Value =
            serde_json::from_slice(&fs::read(report.host_config_path).unwrap()).unwrap();
        assert!(host["worker_config_path"].is_null());
        assert!(!root.join("config/worker.json").exists());
        assert!(!root.join("state/workspaces/default").exists());
        assert!(fs::read_dir(root.join("state/brain"))
            .unwrap()
            .next()
            .is_none());
        fs::remove_dir_all(root).unwrap();
    }
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

#[test]
fn repeat_reports_published_config_from_its_template_without_rewriting_it() {
    let root = temporary_root("published");
    let initial = request(&root);
    prepare(initial.clone()).unwrap();
    let core = root.join("config/core/core.json");
    fs::write(&core, b"published-config-fixture").unwrap();
    let template = fs::read(root.join("config/product-template.json")).unwrap();
    let second = prepare(initial).unwrap();
    assert!(!second.created);
    assert_eq!(second.core_config_path, Some(core.clone()));
    assert_eq!(fs::read(core).unwrap(), b"published-config-fixture");
    assert_eq!(
        fs::read(root.join("config/product-template.json")).unwrap(),
        template
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_installation_keeps_original_config_fence_ack_and_keys() {
    let root = temporary_root("legacy");
    let initial = request(&root);
    prepare(initial.clone()).unwrap();
    let template_path = root.join("config/product-template.json");
    let mut template: p::ProductConfigTemplate =
        serde_json::from_slice(&fs::read(&template_path).unwrap()).unwrap();
    template.core_config_path = root.join("config/core.json");
    template.core_session_fence_path = root.join("state/host-state/core-auth.sqlite");
    template.configuration_ack_path = root.join("config/core-config-ack.json");
    let original = serde_json::to_vec(&template).unwrap();
    fs::write(&template_path, &original).unwrap();
    for path in [
        &template.core_config_path,
        &template.core_session_fence_path,
        &template.configuration_ack_path,
    ] {
        fs::write(path, b"legacy-data").unwrap();
    }
    let key = fs::read(root.join("state/keys/bootstrap.key")).unwrap();
    let second = prepare(initial).unwrap();
    assert_eq!(
        second.core_config_path,
        Some(template.core_config_path.clone())
    );
    assert_eq!(fs::read(&template_path).unwrap(), original);
    assert_eq!(
        fs::read(root.join("state/keys/bootstrap.key")).unwrap(),
        key
    );
    for path in [
        template.core_config_path,
        template.core_session_fence_path,
        template.configuration_ack_path,
    ] {
        assert_eq!(fs::read(path).unwrap(), b"legacy-data");
    }
    assert!(!root.join("state/brain/runtime").exists());
    assert!(!root.join("config/core/core.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeat_refuses_missing_foreign_or_escaping_template_without_new_writes() {
    for change in ["missing", "foreign", "escape", "mixed", "unknown-version"] {
        let root = temporary_root(change);
        let initial = request(&root);
        let first = prepare(initial.clone()).unwrap();
        let manifest = fs::read(&first.manifest_path).unwrap();
        let key = fs::read(root.join("state/keys/bootstrap.key")).unwrap();
        let path = root.join("config/product-template.json");
        if change == "missing" {
            fs::remove_file(&path).unwrap();
        } else {
            let mut template: p::ProductConfigTemplate =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            match change {
                "foreign" => template.install_ref = "installation:another".into(),
                "escape" => template.core_config_path = root.join("outside.json"),
                "mixed" => {
                    template.core_session_fence_path =
                        root.join("state/host-state/core-auth.sqlite")
                }
                "unknown-version" => template.schema_version = p::SchemaVersion(99),
                _ => unreachable!(),
            }
            fs::write(&path, serde_json::to_vec(&template).unwrap()).unwrap();
        }
        assert!(prepare(initial).is_err());
        assert_eq!(fs::read(first.manifest_path).unwrap(), manifest);
        assert_eq!(
            fs::read(root.join("state/keys/bootstrap.key")).unwrap(),
            key
        );
        assert!(!root.join("config/core/core.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
