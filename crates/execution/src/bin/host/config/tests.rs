use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-managed-host-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn document(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1, "install_ref": "managed-host-test", "active_release": "0.0.1",
            "product_target": native_target(),
            "installation_manifest_path": self.0.join("config/installation.json"),
            "journal_path": self.journal(),
            "maintenance_credential_path": self.maintenance_key(),
            "agent_execution_credential_path": self.agent_key(),
            "host_bind": "127.0.0.1:28766", "agent_execution_bind": "127.0.0.1:28767",
        })
    }
    fn save(&self, doc: &serde_json::Value) -> PathBuf {
        let path = self.0.join("host.json");
        std::fs::create_dir_all(self.0.join("config")).unwrap();
        std::fs::write(
            self.0.join("config/installation.json"),
            serde_json::to_vec(&self.manifest()).unwrap(),
        )
        .unwrap();
        std::fs::write(&path, serde_json::to_vec(doc).unwrap()).unwrap();
        path
    }
    fn key(&self, name: &str, bytes: &[u8]) {
        let path = match name {
            "maintenance.key" => self.maintenance_key(),
            "agent.key" => self.agent_key(),
            _ => self.0.join(name),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).unwrap().write_all(bytes).unwrap();
    }
    fn journal(&self) -> PathBuf {
        self.0.join("host-state/host.journal")
    }
    fn maintenance_key(&self) -> PathBuf {
        self.0.join("keys/host-maintenance.token")
    }
    fn agent_key(&self) -> PathBuf {
        self.0.join("keys/agent-execution.token")
    }
    fn manifest(&self) -> serde_json::Value {
        let root = &self.0;
        serde_json::json!({
            "schema_version": 1,
            "install_id": "managed-host-test",
            "target": native_target(),
            "release_id": "0.0.1",
            "release_digest": format!("sha256:{}", "a".repeat(64)),
            "paths": {
                "schema_version": 1,
                "releases": root.join("releases"),
                "config": root.join("config"),
                "brain": root.join("brain"),
                "web_auth": root.join("web-auth"),
                "host_state": root.join("host-state"),
                "keys": root.join("keys"),
                "logs": root.join("logs"),
                "backups": root.join("backups"),
                "workspaces": root.join("workspaces"),
                "ipc": root.join("ipc")
            },
            "services": [
                {"schema_version": 1, "service": "host", "account": native_account("host")},
                {"schema_version": 1, "service": "web", "account": native_account("web")},
                {"schema_version": 1, "service": "brain", "account": native_account("brain")}
            ],
            "created_at": 1
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn native_target() -> p::ProductTarget {
    use p::ProductTarget::*;
    if cfg!(windows) {
        WindowsX86_64
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            MacosArm64
        } else {
            MacosX86_64
        }
    } else if cfg!(target_arch = "aarch64") {
        Ubuntu2404Arm64
    } else {
        Ubuntu2404X86_64
    }
}

fn native_account(service: &str) -> &'static str {
    use p::BuiltinServiceId::*;
    let service = match service {
        "host" => Host,
        "web" => Web,
        "brain" => Brain,
        _ => unreachable!(),
    };
    service.required_account(native_target())
}

#[cfg(windows)]
#[test]
fn installed_file_projection_requires_the_exact_manifest_profile_epoch_and_roots() {
    let fixture = Fixture::new();
    fixture.key(
        "maintenance.key",
        b"fixture-maintenance-credential-0123456789",
    );
    fixture.key("agent.key", b"fixture-agent-credential-0123456789");
    let root = &fixture.0;
    std::fs::create_dir_all(root.join("releases/bin")).unwrap();
    std::fs::create_dir_all(root.join("workspaces/default")).unwrap();
    std::fs::write(
        root.join("releases/bin/forme-executord.exe"),
        b"registered-program-metadata-fixture",
    )
    .unwrap();
    let worker_path = root.join("config/worker.json");
    let worker = serde_json::json!({
        "schema_version": 1, "authority_epoch": 1,
        "journal_path": root.join("host-state/file-worker.journal"),
        "runtime_root": root.join("host-state/file-worker-runtime"),
        "allowed_scopes": ["workspace:default"],
        "workspace_roots": { "workspace:default": root.join("workspaces/default") },
        "launch_profiles": [{ "schema_version": 1, "sandbox_profile_ref": "sandbox:installed-file-v1",
            "program": root.join("releases/bin/forme-executord.exe"), "args": ["--installed-file-worker"] }]
    });
    let mut doc = fixture.document();
    doc["worker_config_path"] = serde_json::to_value(&worker_path).unwrap();
    let path = fixture.save(&doc);
    std::fs::write(&worker_path, serde_json::to_vec(&worker).unwrap()).unwrap();
    let runtime = from_managed_config(&path).unwrap();
    assert!(runtime.installed_file.is_some());
    assert!(!fixture.journal().exists());
    assert!(from_development_config(&path)
        .unwrap()
        .installed_file
        .is_none());
    let headless = root.join("releases/bin/forme-file-worker.exe");
    std::fs::write(&headless, b"headless-program-metadata-fixture").unwrap();
    let mut current = worker.clone();
    current["launch_profiles"][0]["program"] = serde_json::to_value(headless).unwrap();
    std::fs::write(&worker_path, serde_json::to_vec(&current).unwrap()).unwrap();
    assert!(from_managed_config(&path).unwrap().installed_file.is_some());
    for case in ["epoch", "profile", "args", "program", "root", "scope"] {
        let mut changed = worker.clone();
        match case {
            "epoch" => changed["authority_epoch"] = 2.into(),
            "profile" => {
                changed["launch_profiles"][0]["sandbox_profile_ref"] = "sandbox:foreign".into()
            }
            "args" => changed["launch_profiles"][0]["args"] = serde_json::json!([]),
            "program" => {
                let foreign = root.join("releases/bin/other.exe");
                std::fs::write(&foreign, b"foreign fixture").unwrap();
                changed["launch_profiles"][0]["program"] = serde_json::to_value(foreign).unwrap();
            }
            "root" => {
                changed["workspace_roots"]["workspace:default"] =
                    serde_json::to_value(root.join("workspaces")).unwrap()
            }
            "scope" => {
                changed["allowed_scopes"] = serde_json::json!(["workspace:foreign"]);
                changed["workspace_roots"] =
                    serde_json::json!({ "workspace:foreign": root.join("workspaces/default") });
            }
            _ => unreachable!(),
        }
        std::fs::write(&worker_path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(from_managed_config(&path).is_err(), "accepted {case}");
        assert!(!fixture.journal().exists());
        assert!(!root.join("host-state/file-worker.journal").exists());
        assert!(!root.join("brain/forme.db").exists());
    }
    let mut custom = worker;
    custom["launch_profiles"][0]["sandbox_profile_ref"] = "sandbox:legacy-custom".into();
    custom["launch_profiles"][0]["args"] = serde_json::json!(["--custom-entry"]);
    std::fs::write(&worker_path, serde_json::to_vec(&custom).unwrap()).unwrap();
    assert!(from_managed_config(&path).unwrap().installed_file.is_none());
    assert!(from_development_config(&path)
        .unwrap()
        .installed_file
        .is_none());
    assert!(!fixture.journal().exists());
}

#[test]
fn managed_host_load_is_closed_bounded_and_side_effect_free() {
    let fixture = Fixture::new();
    let original = fixture.document();
    let path = fixture.save(&original);
    HostServiceConfig::load(&path).unwrap();
    assert!(!fixture.journal().exists());
    for (key, value) in [
        ("schema_version", serde_json::json!(2)),
        ("unrecognized", serde_json::json!(true)),
        ("host_bind", serde_json::json!("0.0.0.0:28766")),
        ("host_bind", serde_json::json!("127.0.0.1:0")),
        ("agent_execution_bind", serde_json::json!("127.0.0.1:28766")),
        ("journal_path", serde_json::json!("relative.jsonl")),
        (
            "maintenance_credential_path",
            original["journal_path"].clone(),
        ),
        (
            "product_target",
            serde_json::json!(if cfg!(windows) {
                "macos_arm64"
            } else {
                "windows_x86_64"
            }),
        ),
    ] {
        let mut changed = original.clone();
        changed[key] = value;
        fixture.save(&changed);
        assert!(HostServiceConfig::load(&path).is_err(), "accepted {key}");
    }
    std::fs::write(&path, vec![b' '; 64 * 1024 + 1]).unwrap();
    assert!(HostServiceConfig::load(&path).is_err());
    assert!(!fixture.journal().exists());
}

#[test]
fn managed_host_rejects_product_configuration_without_web_credential_without_side_effects() {
    let fixture = Fixture::new();
    let mut doc = fixture.document();
    doc["product_config_template_path"] =
        serde_json::to_value(fixture.0.join("config/product-config.json")).unwrap();
    let path = fixture.save(&doc);

    let error = match HostServiceConfig::load(&path) {
        Ok(_) => panic!("accepted an incomplete managed product configuration"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("product configuration and Web configuration credential must be paired"));
    assert!(!fixture.journal().exists());
    assert!(!fixture.0.join("brain/forme.db").exists());
}

#[test]
fn managed_host_needs_private_distinct_file_credentials_and_cannot_enable_process_workers() {
    let fixture = Fixture::new();
    let doc = fixture.document();
    let path = fixture.save(&doc);
    assert!(from_managed_config(&path).is_err());
    fixture.key(
        "maintenance.key",
        b"test-maintenance-only-token-0000000001\n",
    );
    fixture.key("agent.key", b"test-agent-execution-token-0000000002\n");
    let runtime = from_managed_config(&path).unwrap();
    assert!(runtime.managed && runtime.has_agent_endpoint);
    assert!(!runtime.development_workers);
    assert_eq!(
        runtime.initial_release_root,
        Some(fixture.0.join("releases"))
    );
    assert_eq!(
        runtime.credentials.maintenance(),
        "test-maintenance-only-token-0000000001"
    );
    for bytes in [
        b"short".as_slice(),
        b"test-maintenance-only-token-0000000001",
        &vec![b'a'; 260],
    ] {
        std::fs::write(fixture.agent_key(), bytes).unwrap();
        assert!(from_managed_config(&path).is_err());
    }
    assert!(!fixture.journal().exists());
}

#[test]
fn host_configuration_cannot_alias_itself_or_the_fallback_worker_journal() {
    let fixture = Fixture::new();
    let mut doc = fixture.document();
    doc["journal_path"] = serde_json::json!(fixture.0.join("host.json"));
    let path = fixture.save(&doc);
    assert!(HostServiceConfig::load(&path).is_err());
    doc = fixture.document();
    doc["agent_execution_credential_path"] =
        serde_json::json!(fixture.0.join("host-state/host.worker.jsonl"));
    fixture.save(&doc);
    assert!(HostServiceConfig::load(&path).is_err());
}

#[test]
fn managed_bootstrap_reference_is_preflighted_without_opening_its_authority() {
    let fixture = Fixture::new();
    fixture.key("maintenance.key", b"test-maintenance-only-token-0000000001");
    fixture.key("agent.key", b"test-agent-execution-token-0000000002");
    let mut doc = fixture.document();
    let setup_path = fixture.0.join("bootstrap.json");
    doc["bootstrap_config_path"] = serde_json::json!(setup_path);
    let path = fixture.save(&doc);
    let mut setup = serde_json::json!({
        "schema_version": 1, "install_ref": "managed-host-test",
        "journal_path": fixture.0.join("bootstrap.jsonl"),
        "signing_key_path": fixture.0.join("bootstrap.key"),
        "web_credential_path": fixture.0.join("web.key"),
        "new_brain_directory": fixture.0.join("brain"),
        "candidate_ref": "new-managed-brain",
        "historical_brain_directory": null,
        "verified_backup_directory": null,
    });
    fixture.key("bootstrap.json", &serde_json::to_vec(&setup).unwrap());
    let runtime = from_managed_config(&path).unwrap();
    for name in ["bootstrap.jsonl", "bootstrap.key", "web.key", "brain"] {
        assert!(runtime
            .worker_protected_paths
            .contains(&fixture.0.join(name)));
        assert!(!runtime.protected_paths.contains(&fixture.0.join(name)));
        assert!(!fixture.0.join(name).exists());
    }
    setup["install_ref"] = serde_json::json!("different-install");
    std::fs::write(&setup_path, serde_json::to_vec(&setup).unwrap()).unwrap();
    assert!(HostServiceConfig::load(&path).is_err());
    setup["install_ref"] = serde_json::json!("managed-host-test");
    setup["journal_path"] = doc["journal_path"].clone();
    std::fs::write(setup_path, serde_json::to_vec(&setup).unwrap()).unwrap();
    assert!(HostServiceConfig::load(&path).is_err());
    assert!(!fixture.journal().exists());
}

#[test]
fn administrative_arguments_need_an_explicit_config_and_never_start_development() {
    let fixture = Fixture::new();
    for command in ["--install-service", "--uninstall-service"] {
        assert!(launch_from_args(vec![command.into()]).is_err());
        let args = vec![
            command.into(),
            "--config".into(),
            fixture.0.join("host.json").into_os_string(),
        ];
        assert!(matches!(
            launch_from_args(args).unwrap(),
            Launch::InstallService(_) | Launch::UninstallService(_)
        ));
    }
}

#[cfg(unix)]
#[test]
fn broad_credential_permissions_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.key("maintenance.key", b"test-maintenance-only-token-0000000001");
    std::fs::set_permissions(
        fixture.0.join("maintenance.key"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(read_managed_credential(&fixture.0.join("maintenance.key"), "maintenance").is_err());
}

#[cfg(unix)]
#[test]
fn only_agent_credentials_allow_the_installed_read_only_service_group() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let key = fixture.0.join("agent-execution.token");
    fixture.key(
        "agent-execution.token",
        b"test-agent-only-token-00000000000001",
    );
    for mode in [0o640, 0o660, 0o644, 0o2640] {
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            read_managed_credential(&key, "agent execution").is_ok(),
            mode == 0o640
        );
        assert!(read_managed_credential(&key, "maintenance").is_err());
        assert!(read_managed_credential(&key, "Web configuration").is_err());
    }
}

#[test]
fn release_staging_roots_are_checked_before_credentials_and_journal() {
    let fixture = Fixture::new();
    let mut doc = fixture.document();
    let staging_path = fixture.0.join("release-staging.json");
    doc["release_staging_config_path"] = serde_json::json!(staging_path);
    let path = fixture.save(&doc);
    let key = ed25519_dalek::SigningKey::from_bytes(&[0x22; 32]);
    let mut staging = serde_json::json!({ "schema_version": 1,
        "install_ref": "another-install", "pinned_keys": { "test-key": key.verifying_key().to_bytes() } });
    std::fs::write(&staging_path, serde_json::to_vec(&staging).unwrap()).unwrap();
    assert!(from_managed_config(&path).is_err());
    assert!(!fixture.journal().exists());
    assert!(!fixture.maintenance_key().exists());
    staging["install_ref"] = serde_json::json!("managed-host-test");
    std::fs::write(&staging_path, serde_json::to_vec(&staging).unwrap()).unwrap();
    fixture.key("maintenance.key", b"test-maintenance-only-token-0000000001");
    fixture.key("agent.key", b"test-agent-only-token-000000000000002");
    // Installed release takeover only accepts the immutable service config
    // location recorded by the installation manifest.
    let installed_path = fixture.0.join("config/host.json");
    std::fs::copy(&path, &installed_path).unwrap();
    let runtime = from_managed_config(&installed_path).unwrap();
    assert!(runtime.release_stager.is_some());
    assert!(!fixture.journal().exists());
    doc["release_staging_config_path"] = doc["journal_path"].clone();
    assert!(HostServiceConfig::load(&fixture.save(&doc)).is_err());
}
