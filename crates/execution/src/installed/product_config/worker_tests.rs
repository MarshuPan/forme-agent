//! Configuration evidence only; these fixtures do not establish service identity.
use super::*;
use std::fs;

pub(super) fn placement(
    store: &ProductConfigStore,
    root: &Path,
) -> super::super::InstalledFileProvisioning {
    let workspace = root.join("workspaces/default");
    fs::create_dir_all(&workspace).unwrap();
    let install = &store.template.install_ref;
    let digest = p::canonical_digest(&("forme-agent-execution-pipe-v1", install)).unwrap();
    serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "binding": {
            "schema_version": 1,
            "endpoint": { "transport": "windows_named_pipe", "name": format!(r"\\.\pipe\forme-agent-execution-v1-{}", &digest.0[7..]) },
            "credential_path": store.template.model_credentials_directory.parent().unwrap().join("agent-execution.token"),
            "authority_epoch": 1, "sandbox_profile_ref": "sandbox:installed-file-v1",
            "backend_kinds": ["File"]
        },
        "workspace_roots": { "workspace:default": workspace }
    })).unwrap()
}

#[test]
fn installed_worker_is_persisted_only_on_a_new_owner_save_and_preserves_history() {
    let (directory, path, store) = tests::fixture();
    let worker = placement(&store.lock().unwrap(), directory.path());
    store
        .lock()
        .unwrap()
        .apply(tests::patch("save:legacy", 0))
        .unwrap();
    let core_path = store.lock().unwrap().template.core_config_path.clone();
    let old_core = fs::read(&core_path).unwrap();
    let current = store.lock().unwrap().journal.current().unwrap().clone();
    let old_revision =
        materialize::revision_directory(&store.lock().unwrap().template, &current.id);
    let old_state = fs::read(old_revision.join("state.json")).unwrap();
    drop(store);
    let store = ProductConfigStore::open_with_installed_file(&path, Some(worker.clone())).unwrap();
    assert_eq!(fs::read(&core_path).unwrap(), old_core);
    let replay = store
        .lock()
        .unwrap()
        .apply(tests::patch("save:legacy", 0))
        .unwrap();
    assert!(!replay.applied);
    assert_eq!(fs::read(&core_path).unwrap(), old_core);
    assert!(store.lock().unwrap().state.worker.is_none());
    store
        .lock()
        .unwrap()
        .apply(tests::patch("save:worker", 1))
        .unwrap();
    {
        let mut guard = store.lock().unwrap();
        assert_eq!(guard.state.schema_version, p::SchemaVersion(9));
        assert_eq!(guard.state.worker, Some(worker.clone()));
        let core: serde_json::Value =
            serde_json::from_slice(&fs::read(&core_path).unwrap()).unwrap();
        assert_eq!(
            core["brain"]["worker_binding"]["backend_kinds"],
            serde_json::json!(["File"])
        );
        assert!(core["brain"]["worker_file_roots"]["workspace:default"].is_string());
        let public = serde_json::to_string(&guard.snapshot_settings_v2().unwrap()).unwrap();
        assert!(!public.contains("worker_binding"));
        assert!(!public.contains("worker_file_roots"));
        assert!(!public.contains("agent-execution.token"));
        assert!(!public.contains(directory.path().to_str().unwrap()));
    }
    assert_eq!(
        fs::read(old_revision.join("state.json")).unwrap(),
        old_state
    );
    assert_eq!(fs::read(old_revision.join("core.json")).unwrap(), old_core);
    drop(store);
    // Losing optional live worker configuration does not erase committed
    // history or prevent maintenance. It does not prove a live worker exists.
    let reopened = ProductConfigStore::open(&path).unwrap();
    let mut guard = reopened.lock().unwrap();
    guard.apply(tests::patch("save:later", 2)).unwrap();
    assert_eq!(guard.state.worker, Some(worker));
    assert_eq!(guard.state.schema_version, p::SchemaVersion(9));
}

#[test]
fn installed_worker_conflict_and_unbound_projection_cannot_rewrite_a_revision() {
    let (directory, path, store) = tests::fixture();
    let worker = placement(&store.lock().unwrap(), directory.path());
    drop(store);
    let store = ProductConfigStore::open_with_installed_file(&path, Some(worker.clone())).unwrap();
    assert!(!store.lock().unwrap().template.core_config_path.exists());
    store
        .lock()
        .unwrap()
        .apply(tests::patch("save:worker", 0))
        .unwrap();
    let core_path = store.lock().unwrap().template.core_config_path.clone();
    let core = fs::read(&core_path).unwrap();
    drop(store);
    let mut changed = serde_json::to_value(&worker).unwrap();
    let other = directory.path().join("workspaces/other");
    fs::create_dir_all(&other).unwrap();
    changed["workspace_roots"]["workspace:default"] = serde_json::to_value(other).unwrap();
    let changed = serde_json::from_value(changed).unwrap();
    assert!(ProductConfigStore::open_with_installed_file(&path, Some(changed)).is_err());
    assert_eq!(fs::read(&core_path).unwrap(), core);
    for field in ["authority_epoch", "sandbox_profile_ref", "credential_path"] {
        let mut changed = serde_json::to_value(&worker).unwrap();
        changed["binding"][field] = match field {
            "authority_epoch" => serde_json::json!(2),
            "sandbox_profile_ref" => serde_json::json!("sandbox:other"),
            _ => serde_json::to_value(directory.path().join("foreign.token")).unwrap(),
        };
        let changed = serde_json::from_value(changed).unwrap();
        assert!(ProductConfigStore::open_with_installed_file(&path, Some(changed)).is_err());
        assert_eq!(fs::read(&core_path).unwrap(), core);
    }
    ProductConfigStore::open_with_installed_file(&path, Some(worker)).unwrap();
}

#[test]
fn installed_worker_survives_skills_proxy_restore_and_subsequent_model_save() {
    let (directory, path, store) = tests::fixture();
    let mut store = store.into_inner().unwrap();
    let worker = placement(&store, directory.path());
    store.installed_file = Some(worker.clone());
    let mut save = tests::patch("worker:with-skills", 0);
    save.capabilities = Some(
        serde_json::from_value(serde_json::json!({
            "schema_version": 1, "skills": [{
                "schema_version": 1, "id": "skill:worker-test", "summary": "worker test",
                "scope": "workspace:test", "version": 1, "body": "repository-owned fixture",
                "enabled": true, "trust": "owner_input"
            }]
        }))
        .unwrap(),
    );
    let skills = save.capabilities.clone();
    store
        .apply_internal(save, Some(p::ProductProxyConfig::direct()))
        .unwrap();
    let binding = p::ProductDataBinding {
        schema_version: p::SchemaVersion(1),
        candidate_ref: "restored-worker.db".into(),
        store_path: store
            .template
            .brain_store_path
            .parent()
            .unwrap()
            .join("data-operations/restored-worker.db"),
        content_key_directory: store
            .template
            .brain_store_path
            .with_extension("content-keys"),
        owner_digest: p::sha256_content_digest(b"owner:test"),
        store_identity: p::sha256_content_digest(b"worker:store"),
        manifest_digest: p::sha256_content_digest(b"worker:manifest"),
        execution_fence_digest: Some(p::sha256_content_digest(b"worker:fence")),
    };
    let request = p::ProductRestoreRequest {
        schema_version: p::SchemaVersion(1),
        install_ref: store.template.install_ref.clone(),
        operation_id: "restore:worker".into(),
        command_digest: p::sha256_content_digest(b"restore:worker"),
        expected_config_revision: 1,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        candidate_ref: binding.candidate_ref.clone(),
        candidate_digest: binding.manifest_digest.clone(),
    };
    store
        .publish_restored_data(&request, binding.clone())
        .unwrap();
    assert_eq!(store.state.schema_version, p::SchemaVersion(9));
    assert_eq!(store.state.worker, Some(worker.clone()));
    assert_eq!(store.state.capabilities, skills);
    assert_eq!(store.state.proxy, Some(p::ProductProxyConfig::direct()));
    drop(store);
    let mut reopened = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    reopened
        .apply(tests::patch("worker:after-restore", 2))
        .unwrap();
    assert_eq!(reopened.state.worker, Some(worker));
    assert_eq!(reopened.state.active_data, Some(binding));
    assert_eq!(reopened.state.schema_version, p::SchemaVersion(9));
}
