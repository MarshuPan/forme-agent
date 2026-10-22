#![forbid(unsafe_code)]

#[path = "hostctl_install_filesystem.rs"]
mod hostctl_install_filesystem;
#[path = "hostctl_install_layout.rs"]
mod hostctl_install_layout;

use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;
use forme_gateway::web::{WebBootstrapBridgeConfig, WebServerConfig};
use forme_protocol as p;

use hostctl_install_filesystem::*;

const HOST_BIND: &str = "127.0.0.1:8766";
const AGENT_BIND: &str = "127.0.0.1:8767";
const CONFIG_BIND: &str = "127.0.0.1:8768";
const CORE_BIND: &str = "127.0.0.1:8769";
const WEB_BIND: &str = "127.0.0.1:8765";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRequest {
    pub install_root: PathBuf,
    pub state_root: PathBuf,
    pub config_root: PathBuf,
    pub log_root: PathBuf,
    pub ipc_root: PathBuf,
    pub target: p::ProductTarget,
    pub release_id: String,
    pub release_digest: p::SchemaDigest,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparationReport {
    pub install_ref: String,
    pub target: p::ProductTarget,
    pub release_id: String,
    pub manifest_path: PathBuf,
    pub host_config_path: PathBuf,
    pub web_config_path: PathBuf,
    pub bootstrap_config_path: PathBuf,
    pub core_config_path: Option<PathBuf>,
    pub created: bool,
}
pub fn run(args: &[String]) -> Result<(), String> {
    let request = parse_args(args)?;
    let report = prepare(request)?;
    println!(
        "{}",
        serde_json::json!({
            "schema_version": 1,
            "install_ref": report.install_ref,
            "target": report.target,
            "release_id": report.release_id,
            "manifest_path": report.manifest_path,
            "host_config_path": report.host_config_path,
            "web_config_path": report.web_config_path,
            "bootstrap_config_path": report.bootstrap_config_path,
            "core_config_path": report.core_config_path,
            "core_state": "pending_authenticated_owner_claim",
            "created": report.created,
        })
    );
    Ok(())
}
pub fn prepare(request: PrepareRequest) -> Result<PreparationReport, String> {
    validate_request(&request)?;
    let paths = product_paths(&request)?;
    let manifest_path = request.config_root.join("installation.json");
    reject_linked_ancestors(&manifest_path)?;

    if manifest_path.is_file() {
        return reopen_existing(&request, &paths, &manifest_path);
    }
    if manifest_path.exists() {
        return Err("installation manifest is not a regular file".into());
    }

    ensure_fresh_domains(&request, &paths)?;
    for path in product_directories(&paths) {
        ensure_directory(path)?;
    }
    ensure_directory(&paths.config.join("core"))?;
    ensure_directory(&paths.keys.join("model-credentials"))?;

    let install_ref = random_ref("installation")?;
    let bootstrap_key = random_key()?;
    let bootstrap_signing = SigningKey::from_bytes(&bootstrap_key);
    let web_assertion_key = random_key()?;
    let web_assertion_signing = SigningKey::from_bytes(&web_assertion_key);
    let bootstrap_web_credential = random_token()?;
    let maintenance_credential = random_token()?;
    let agent_credential = random_token()?;
    let web_configuration_credential = random_token()?;
    let core_owner_credential = random_token()?;
    let core_web_credential = random_token()?;
    let bootstrap_config_path = request.config_root.join("bootstrap.json");
    let host_config_path = request.config_root.join("host.json");
    let web_config_path = request.config_root.join("web.json");
    let core_config_path = request.config_root.join("core").join("core.json");
    let template_path = request.config_root.join("product-template.json");
    let web_assertion_key_path = paths.web_auth.join("web-assertion.key");
    let web_configuration_credential_path = paths.web_auth.join("configuration.token");
    let core_owner_credential_path = paths.keys.join("core-owner.token");
    let core_web_credential_path = paths.keys.join("core-web.token");
    let core_session_fence_path = paths.brain.join("runtime").join("core-auth.sqlite");

    let manifest = installation_manifest(&request, &paths, &install_ref)?;
    let bootstrap = serde_json::json!({
        "schema_version": 1,
        "install_ref": install_ref,
        "journal_path": paths.host_state.join("bootstrap.journal"),
        "signing_key_path": paths.keys.join("bootstrap.key"),
        "web_credential_path": paths.web_auth.join("bootstrap-web.token"),
        "new_brain_directory": paths.brain,
        "candidate_ref": "candidate:new-brain"
    });
    let host = serde_json::json!({
        "schema_version": 1,
        "install_ref": install_ref,
        "active_release": request.release_id,
        "product_target": request.target,
        "installation_manifest_path": manifest_path,
        "journal_path": paths.host_state.join("host.journal"),
        "maintenance_credential_path": paths.keys.join("host-maintenance.token"),
        "agent_execution_credential_path": paths.keys.join("agent-execution.token"),
        "web_configuration_credential_path": web_configuration_credential_path,
        "product_config_template_path": template_path,
        "worker_config_path": serde_json::Value::Null,
        "bootstrap_config_path": bootstrap_config_path,
        "host_bind": HOST_BIND,
        "agent_execution_bind": AGENT_BIND,
        "configuration_bind": CONFIG_BIND
    });
    let template = p::ProductConfigTemplate {
        schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
        install_ref: install_ref.clone(),
        core_config_path: core_config_path.clone(),
        configuration_ack_path: paths.brain.join("runtime").join("core-config-ack.json"),
        model_credentials_directory: paths.keys.join("model-credentials"),
        brain_store_path: paths.brain.join("forme.db"),
        artifact_root: paths.brain.join("artifacts"),
        core_owner_credential_path: core_owner_credential_path.clone(),
        core_web_credential_path: core_web_credential_path.clone(),
        core_web_assertion_verifying_key: web_assertion_signing.verifying_key().to_bytes(),
        core_session_fence_path,
        core_bind: CORE_BIND
            .parse()
            .map_err(|_| "core bind is invalid".to_owned())?,
        workspace: p::WorkspaceRef("workspace:default".into()),
        agent_profile: p::AgentProfileRef("agent:forme-local".into()),
        scheduler_tick: p::DurationMs(1_000),
        scheduler_lease: p::DurationMs(30_000),
        scheduler_max_claims: 1,
        cognition_tick: p::DurationMs(60_000),
        stable_evidence_max_age: p::DurationMs(30 * 24 * 60 * 60 * 1_000),
        authority_lease: p::DurationMs(30_000),
        drain_timeout: p::DurationMs(30_000),
    };
    template.validate().map_err(|error| error.0)?;
    let web = WebServerConfig {
        schema_version: p::SchemaVersion(1),
        install_ref: install_ref.clone(),
        identity_directory: paths.web_auth.clone(),
        claim_verifying_key: bootstrap_signing.verifying_key().to_bytes(),
        bind: WEB_BIND
            .parse()
            .map_err(|_| "Web bind is invalid".to_owned())?,
        bootstrap_bridge: Some(WebBootstrapBridgeConfig {
            schema_version: p::SchemaVersion(1),
            address: HOST_BIND
                .parse()
                .map_err(|_| "host bind is invalid".to_owned())?,
            credential_path: paths.web_auth.join("bootstrap-web.token"),
        }),
        configuration_bridge: Some(forme_gateway::web::WebConfigurationBridgeConfig {
            schema_version: p::SchemaVersion(1),
            address: CONFIG_BIND
                .parse()
                .map_err(|_| "configuration bind is invalid".to_owned())?,
            credential_path: web_configuration_credential_path.clone(),
        }),
        core_bridge: Some(forme_gateway::web::WebCoreBridgeConfig {
            schema_version: p::SchemaVersion(1),
            address: CORE_BIND
                .parse()
                .map_err(|_| "core bind is invalid".to_owned())?,
            credential_path: core_web_credential_path.clone(),
            signing_key_path: web_assertion_key_path.clone(),
        }),
    };
    web.validate().map_err(|error| error.0)?;

    let files = [
        (
            paths.keys.join("bootstrap.key"),
            bootstrap_key.to_vec(),
            0o600,
        ),
        (
            paths.web_auth.join("bootstrap-web.token"),
            bootstrap_web_credential.into_bytes(),
            0o600,
        ),
        (
            paths.keys.join("host-maintenance.token"),
            maintenance_credential.into_bytes(),
            0o600,
        ),
        (
            paths.keys.join("agent-execution.token"),
            agent_credential.into_bytes(),
            0o600,
        ),
        (
            web_configuration_credential_path,
            web_configuration_credential.into_bytes(),
            0o600,
        ),
        (
            core_owner_credential_path,
            core_owner_credential.into_bytes(),
            0o600,
        ),
        (
            core_web_credential_path,
            core_web_credential.into_bytes(),
            0o600,
        ),
        (web_assertion_key_path, web_assertion_key.to_vec(), 0o600),
        (
            bootstrap_config_path.clone(),
            json_bytes(&bootstrap)?,
            0o600,
        ),
        (host_config_path.clone(), json_bytes(&host)?, 0o600),
        (
            web_config_path.clone(),
            serde_json::to_vec_pretty(&web)
                .map_err(|_| "Web configuration encoding failed".to_owned())?,
            0o600,
        ),
        (
            template_path.clone(),
            serde_json::to_vec_pretty(&template)
                .map_err(|_| "product configuration template encoding failed".to_owned())?,
            0o600,
        ),
    ];
    let result = write_transaction(&files, &manifest_path, &manifest);
    result?;
    Ok(PreparationReport {
        install_ref,
        target: request.target,
        release_id: request.release_id,
        manifest_path,
        host_config_path,
        web_config_path,
        bootstrap_config_path,
        // Installation writes the template only. The core configuration is
        // created after an authenticated owner claim completes.
        core_config_path: None,
        created: true,
    })
}
fn parse_args(args: &[String]) -> Result<PrepareRequest, String> {
    let mut values = std::collections::BTreeMap::<String, String>::new();
    let mut index = 0;
    while index < args.len() {
        let key = args[index].strip_prefix("--").ok_or_else(usage)?.to_owned();
        if !matches!(
            key.as_str(),
            "install-root"
                | "state-root"
                | "config-root"
                | "log-root"
                | "ipc-root"
                | "target"
                | "release-id"
                | "release-digest"
                | "bundle-inventory"
        ) || values.contains_key(&key)
        {
            return Err(usage());
        }
        let value = args
            .get(index + 1)
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(usage)?;
        values.insert(key, value.clone());
        index += 2;
    }
    let target = parse_target(&take_required(&mut values, "target")?)?;
    let (release_id, release_digest) = match values.remove("bundle-inventory") {
        Some(path) => read_bundle_inventory(Path::new(&path))?,
        None => {
            let release_id = take_required(&mut values, "release-id")?;
            let digest = take_required(&mut values, "release-digest")?;
            (release_id, p::SchemaDigest(digest))
        }
    };
    let install_root = absolute(take_required(&mut values, "install-root")?, "install-root")?;
    let state_root = absolute(take_required(&mut values, "state-root")?, "state-root")?;
    let config_root = absolute(take_required(&mut values, "config-root")?, "config-root")?;
    let log_root = absolute(take_required(&mut values, "log-root")?, "log-root")?;
    let ipc_root = absolute(take_required(&mut values, "ipc-root")?, "ipc-root")?;
    if !values.is_empty() {
        return Err(usage());
    }
    Ok(PrepareRequest {
        install_root,
        state_root,
        config_root,
        log_root,
        ipc_root,
        target,
        release_id,
        release_digest,
    })
}
fn take_required(
    values: &mut std::collections::BTreeMap<String, String>,
    name: &str,
) -> Result<String, String> {
    values.remove(name).ok_or_else(usage)
}
fn usage() -> String {
    "usage: forme-hostctl prepare-installation --install-root <abs> --state-root <abs> --config-root <abs> --log-root <abs> --ipc-root <abs> --target <product-target> (--bundle-inventory <abs> | --release-id <id> --release-digest sha256:<hex>)".into()
}
fn parse_target(value: &str) -> Result<p::ProductTarget, String> {
    match value {
        "windows_x86_64" => Ok(p::ProductTarget::WindowsX86_64),
        "macos_arm64" => Ok(p::ProductTarget::MacosArm64),
        "macos_x86_64" => Ok(p::ProductTarget::MacosX86_64),
        "ubuntu2404_x86_64" => Ok(p::ProductTarget::Ubuntu2404X86_64),
        "ubuntu2404_arm64" => Ok(p::ProductTarget::Ubuntu2404Arm64),
        "rocky9_x86_64" => Ok(p::ProductTarget::Rocky9X86_64),
        _ => Err("target is not a supported forme product target".into()),
    }
}
fn read_bundle_inventory(path: &Path) -> Result<(String, p::SchemaDigest), String> {
    let bytes = read_bounded(path, 256 * 1024)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "bundle inventory is invalid".to_owned())?;
    let release_id = value
        .get("release_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "bundle inventory has no release id".to_owned())?
        .to_owned();
    let digest = value
        .get("content_digest")
        .and_then(serde_json::Value::as_str)
        .map(|value| format!("sha256:{value}"))
        .ok_or_else(|| "bundle inventory has no content digest".to_owned())?;
    Ok((release_id, p::SchemaDigest(digest)))
}
fn validate_request(request: &PrepareRequest) -> Result<(), String> {
    for (name, path) in [
        ("install-root", &request.install_root),
        ("state-root", &request.state_root),
        ("config-root", &request.config_root),
        ("log-root", &request.log_root),
        ("ipc-root", &request.ipc_root),
    ] {
        absolute_path(path, name)?;
    }
    p::validate_release_id(&request.release_id).map_err(|error| error.0)?;
    p::validate_product_digest(&request.release_digest).map_err(|error| error.0)?;
    Ok(())
}
fn product_paths(request: &PrepareRequest) -> Result<p::ProductPaths, String> {
    let paths = p::ProductPaths {
        schema_version: p::SchemaVersion(1),
        releases: request.install_root.clone(),
        config: request.config_root.clone(),
        brain: request.state_root.join("brain"),
        web_auth: request.state_root.join("web-auth"),
        host_state: request.state_root.join("host-state"),
        keys: request.state_root.join("keys"),
        logs: request.log_root.clone(),
        backups: request.state_root.join("backups"),
        workspaces: request.state_root.join("workspaces"),
        ipc: request.ipc_root.clone(),
    };
    paths.validate().map_err(|error| error.0)?;
    for path in [
        &paths.releases,
        &paths.config,
        &paths.brain,
        &paths.web_auth,
        &paths.host_state,
        &paths.keys,
        &paths.logs,
        &paths.backups,
        &paths.workspaces,
        &paths.ipc,
    ] {
        reject_linked_ancestors(path)?;
    }
    Ok(paths)
}
fn installation_manifest(
    request: &PrepareRequest,
    paths: &p::ProductPaths,
    install_ref: &str,
) -> Result<p::InstallationManifest, String> {
    let manifest = p::InstallationManifest {
        schema_version: p::SchemaVersion(1),
        install_id: install_ref.to_owned(),
        target: request.target,
        release_id: request.release_id.clone(),
        release_digest: request.release_digest.clone(),
        paths: paths.clone(),
        services: [
            p::BuiltinServiceId::Host,
            p::BuiltinServiceId::Web,
            p::BuiltinServiceId::Brain,
        ]
        .into_iter()
        .map(|service| p::ServiceIdentityBinding {
            schema_version: p::SchemaVersion(1),
            account: service.required_account(request.target).to_owned(),
            service,
        })
        .collect(),
        created_at: now_ms()?,
    };
    manifest.validate().map_err(|error| error.0)?;
    Ok(manifest)
}

fn reopen_existing(
    request: &PrepareRequest,
    paths: &p::ProductPaths,
    manifest_path: &Path,
) -> Result<PreparationReport, String> {
    let manifest: p::InstallationManifest =
        serde_json::from_slice(&read_bounded(manifest_path, 256 * 1024)?)
            .map_err(|_| "existing installation manifest is invalid".to_owned())?;
    manifest.validate().map_err(|error| error.0)?;
    if manifest.target != request.target
        || manifest.release_id != request.release_id
        || manifest.release_digest != request.release_digest
        || manifest.paths != *paths
    {
        return Err(
            "existing installation belongs to another release or path set; refusing replacement"
                .into(),
        );
    }
    let core_config_path = hostctl_install_layout::existing_core_path(&manifest)?;
    let core_config_path = match fs::symlink_metadata(&core_config_path) {
        Ok(metadata) if metadata.file_type().is_file() => Some(core_config_path),
        Ok(_) => return Err("existing core configuration is not a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("cannot inspect core configuration: {error}")),
    };
    Ok(PreparationReport {
        install_ref: manifest.install_id,
        target: manifest.target,
        release_id: manifest.release_id,
        manifest_path: manifest_path.to_owned(),
        host_config_path: request.config_root.join("host.json"),
        web_config_path: request.config_root.join("web.json"),
        bootstrap_config_path: request.config_root.join("bootstrap.json"),
        core_config_path,
        created: false,
    })
}

fn ensure_fresh_domains(request: &PrepareRequest, paths: &p::ProductPaths) -> Result<(), String> {
    if request.config_root.exists() && !is_empty_directory(&request.config_root)? {
        return Err("configuration directory is non-empty; explicit recovery is required".into());
    }
    for path in [
        &paths.brain,
        &paths.web_auth,
        &paths.host_state,
        &paths.keys,
        &paths.backups,
        &paths.workspaces,
        &paths.ipc,
    ] {
        if path.exists() && !is_empty_directory(path)? {
            return Err(format!(
                "existing product data at {} requires explicit claim or recovery",
                path.display()
            ));
        }
    }
    Ok(())
}

fn product_directories(paths: &p::ProductPaths) -> [&Path; 10] {
    [
        &paths.releases,
        &paths.config,
        &paths.brain,
        &paths.web_auth,
        &paths.host_state,
        &paths.keys,
        &paths.logs,
        &paths.backups,
        &paths.workspaces,
        &paths.ipc,
    ]
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    reject_linked_ancestors(path)?;
    fs::create_dir_all(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!("{} is not a real directory", path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("cannot protect {}: {error}", path.display()))?;
    }
    Ok(())
}
