#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::SigningKey;
use forme_gateway::web::{WebBootstrapBridgeConfig, WebServerConfig};
use forme_protocol as p;

const HOST_BIND: &str = "127.0.0.1:8766";
const AGENT_BIND: &str = "127.0.0.1:8767";
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

    let install_ref = random_ref("installation")?;
    let bootstrap_key = random_key()?;
    let bootstrap_signing = SigningKey::from_bytes(&bootstrap_key);
    let bootstrap_web_credential = random_token()?;
    let maintenance_credential = random_token()?;
    let agent_credential = random_token()?;
    let bootstrap_config_path = request.config_root.join("bootstrap.json");
    let host_config_path = request.config_root.join("host.json");
    let web_config_path = request.config_root.join("web.json");

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
        "worker_config_path": serde_json::Value::Null,
        "bootstrap_config_path": bootstrap_config_path,
        "host_bind": HOST_BIND,
        "agent_execution_bind": AGENT_BIND
    });
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
        core_bridge: None,
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
    Ok(PreparationReport {
        install_ref: manifest.install_id,
        target: manifest.target,
        release_id: manifest.release_id,
        manifest_path: manifest_path.to_owned(),
        host_config_path: request.config_root.join("host.json"),
        web_config_path: request.config_root.join("web.json"),
        bootstrap_config_path: request.config_root.join("bootstrap.json"),
        core_config_path: None,
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

fn write_transaction(
    files: &[(PathBuf, Vec<u8>, u32)],
    manifest_path: &Path,
    manifest: &p::InstallationManifest,
) -> Result<(), String> {
    let mut created = Vec::new();
    let result = (|| {
        for (path, bytes, mode) in files {
            write_new(path, bytes, *mode)?;
            created.push(path.clone());
        }
        let manifest_bytes = serde_json::to_vec_pretty(manifest)
            .map_err(|_| "installation manifest encoding failed".to_owned())?;
        write_new(manifest_path, &manifest_bytes, 0o600)?;
        created.push(manifest_path.to_owned());
        Ok(())
    })();
    if result.is_err() {
        for path in created.into_iter().rev() {
            let _ = fs::remove_file(path);
        }
    }
    result
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    reject_linked_ancestors(parent)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("cannot commit {}: {error}", path.display()))
}

fn json_bytes(value: &serde_json::Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(value).map_err(|_| "configuration encoding failed".into())
}

fn random_key() -> Result<[u8; 32], String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "secure random source is unavailable".to_owned())?;
    Ok(bytes)
}

fn random_token() -> Result<String, String> {
    let bytes = random_key()?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn random_ref(prefix: &str) -> Result<String, String> {
    Ok(format!("{prefix}:{}", random_token()?))
}

fn now_ms() -> Result<p::Timestamp, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .map_err(|_| "system clock is before the Unix epoch".into())
}

fn absolute(value: String, name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    absolute_path(&path, name)?;
    Ok(path)
}

fn absolute_path(path: &Path, name: &str) -> Result<(), String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(format!("{name} must be an absolute normalized path"));
    }
    reject_linked_ancestors(path)
}

fn reject_linked_ancestors(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("product path must be absolute".into());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "product path contains a link: {}",
                    current.display()
                ))
            }
            Ok(metadata) if is_reparse_point(&metadata) => {
                return Err(format!(
                    "product path contains a reparse point: {}",
                    current.display()
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect {}: {error}", current.display())),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_: &fs::Metadata) -> bool {
    false
}

fn is_empty_directory(path: &Path) -> Result<bool, String> {
    reject_linked_ancestors(path)?;
    if !path.exists() {
        return Ok(true);
    }
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    Ok(fs::read_dir(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?
        .next()
        .is_none())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    reject_linked_ancestors(path)?;
    let file =
        File::open(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(format!("{} is not a bounded regular file", path.display()));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds its size limit", path.display()));
    }
    Ok(bytes)
}
