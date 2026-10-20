//! Managed configuration validation; all files are repository-owned fixtures.
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use forme_execution::installed::{WorkerLaunchProfileConfig, WorkerRuntimeConfig};
use forme_protocol as p;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-worker-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("workspace")).unwrap();
        Self(root)
    }

    fn config(&self) -> WorkerRuntimeConfig {
        WorkerRuntimeConfig {
            schema_version: p::SchemaVersion(1),
            authority_epoch: p::AuthorityEpoch(1),
            journal_path: self.0.join("worker.jsonl"),
            runtime_root: self.0.join("runtime"),
            allowed_scopes: vec![p::Scope("workspace:one".into())],
            workspace_roots: BTreeMap::from([("workspace:one".into(), self.0.join("workspace"))]),
            launch_profiles: vec![WorkerLaunchProfileConfig {
                schema_version: p::SchemaVersion(1),
                sandbox_profile_ref: "sandbox:fixture".into(),
                program: PathBuf::from(env!("CARGO_BIN_EXE_forme-worker-fixture")),
                args: Vec::new(),
            }],
        }
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn managed_worker_config_rejects_schema_scope_and_profile_ambiguity() {
    let temp = Temp::new();
    let config = temp.config();
    config.validate().unwrap();
    let mut invalid = Vec::new();
    let mut changed = config.clone();
    changed.schema_version = p::SchemaVersion(2);
    invalid.push(changed);
    let mut changed = config.clone();
    changed.authority_epoch = p::AuthorityEpoch(0);
    invalid.push(changed);
    let mut changed = config.clone();
    changed
        .allowed_scopes
        .push(changed.allowed_scopes[0].clone());
    invalid.push(changed);
    let mut changed = config.clone();
    changed.allowed_scopes.clear();
    invalid.push(changed);
    let mut changed = config.clone();
    changed.workspace_roots.clear();
    invalid.push(changed);
    let mut changed = config.clone();
    changed
        .workspace_roots
        .insert("workspace:two".into(), temp.0.join("other"));
    invalid.push(changed);
    let mut changed = config.clone();
    changed
        .launch_profiles
        .push(changed.launch_profiles[0].clone());
    invalid.push(changed);
    let mut changed = config.clone();
    changed.launch_profiles.clear();
    invalid.push(changed);
    let mut changed = config.clone();
    changed.launch_profiles[0].schema_version = p::SchemaVersion(2);
    invalid.push(changed);
    for changed in invalid {
        assert!(changed.validate().is_err());
    }
    assert!(!config.journal_path.exists());
    assert!(!config.runtime_root.exists());
}

#[test]
fn managed_worker_config_rejects_writable_path_overlap_and_protected_files() {
    let temp = Temp::new();
    let config = temp.config();
    let mut changed = config.clone();
    changed.journal_path = temp.0.join("workspace/journal.jsonl");
    assert!(changed.validate().is_err());
    let mut changed = config.clone();
    changed.runtime_root = temp.0.join("workspace/runtime");
    assert!(changed.validate().is_err());
    let mut changed = config.clone();
    changed
        .allowed_scopes
        .push(p::Scope("workspace:two".into()));
    changed
        .workspace_roots
        .insert("workspace:two".into(), temp.0.join("workspace"));
    assert!(changed.validate().is_err());
    let mut changed = config.clone();
    changed.journal_path = temp.0.join("journal.jsonl:stream");
    assert!(changed.validate().is_err());
    let mut changed = config.clone();
    changed.journal_path = PathBuf::from("relative.jsonl");
    assert!(changed.validate().is_err());
    assert!(config
        .validate_protected_paths(&[&config.journal_path])
        .is_err());
    assert!(config
        .validate_protected_paths(&[&temp.0.join("runtime/token")])
        .is_err());
    assert!(config
        .validate_protected_paths(&[&temp.0.join("workspace/config")])
        .is_err());
    config
        .validate_protected_paths(&[&temp.0.join("host.jsonl")])
        .unwrap();
    #[cfg(windows)]
    {
        let alias = PathBuf::from(config.journal_path.to_string_lossy().to_ascii_uppercase());
        assert!(config.validate_protected_paths(&[&alias]).is_err());
        let mut changed = config.clone();
        changed.runtime_root = temp.0.join("rÜntime");
        changed.journal_path = temp.0.join("RüNTIME/journal.jsonl");
        assert!(changed.validate().is_err());
    }
}

#[test]
fn managed_worker_config_load_is_bounded_closed_and_outside_worker_paths() {
    let temp = Temp::new();
    let config = temp.config();
    let path = temp.0.join("config.json");
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(WorkerRuntimeConfig::load(&path).unwrap(), config);
    let mut value = serde_json::to_value(&config).unwrap();
    value["enable_unsandboxed"] = true.into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(WorkerRuntimeConfig::load(&path).is_err());
    fs::write(&path, b"{").unwrap();
    assert!(WorkerRuntimeConfig::load(&path).is_err());
    fs::File::create(&path)
        .unwrap()
        .set_len(4 * 1024 * 1024 + 1)
        .unwrap();
    assert!(WorkerRuntimeConfig::load(&path).is_err());
    let writable_path = temp.0.join("workspace/config.json");
    fs::write(&writable_path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(WorkerRuntimeConfig::load(&writable_path).is_err());
    assert!(WorkerRuntimeConfig::load(&temp.0).is_err());
    assert!(WorkerRuntimeConfig::load(std::path::Path::new("relative.json")).is_err());
}

#[cfg(unix)]
#[test]
fn managed_worker_config_rejects_linked_files_and_workspaces() {
    let temp = Temp::new();
    let config = temp.config();
    let path = temp.0.join("config.json");
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let link = temp.0.join("link.json");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(WorkerRuntimeConfig::load(&link).is_err());
    let workspace_link = temp.0.join("workspace-link");
    std::os::unix::fs::symlink(temp.0.join("workspace"), &workspace_link).unwrap();
    let mut changed = config;
    changed
        .workspace_roots
        .insert("workspace:one".into(), workspace_link);
    assert!(changed.validate().is_err());
}
