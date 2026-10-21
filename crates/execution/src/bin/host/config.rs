//! Explicit configuration, credential and worker composition boundaries.
mod manifest;

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forme_execution::installed::{HostCredentials, IsolatedExecutorHost, WorkerRuntimeConfig};
use forme_protocol as p;
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostServiceConfig {
    pub schema_version: p::SchemaVersion,
    pub install_ref: String,
    pub active_release: String,
    pub product_target: p::ProductTarget,
    pub installation_manifest_path: PathBuf,
    pub journal_path: PathBuf,
    pub maintenance_credential_path: PathBuf,
    pub agent_execution_credential_path: PathBuf,
    pub worker_config_path: Option<PathBuf>,
    pub bootstrap_config_path: Option<PathBuf>,
    pub host_bind: SocketAddr,
    pub agent_execution_bind: SocketAddr,
}

impl HostServiceConfig {
    pub(crate) fn load(path: &Path) -> p::Result<Self> {
        reject_path(path)?;
        let file = std::fs::File::open(path)
            .map_err(|_| p::Error("managed host configuration is unavailable".into()))?;
        let metadata = file
            .metadata()
            .map_err(|_| p::Error("managed host configuration is unavailable".into()))?;
        if !metadata.is_file() || metadata.len() > 64 * 1024 {
            return Err(p::Error(
                "managed host configuration is unavailable or oversized".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o022 != 0 {
                return Err(p::Error(
                    "managed host configuration is writable by another account".into(),
                ));
            }
        }
        let mut bytes = Vec::new();
        file.take(metadata.len() + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| p::Error("managed host configuration cannot be read".into()))?;
        if bytes.len() as u64 != metadata.len() {
            return Err(p::Error(
                "managed host configuration changed during read".into(),
            ));
        }
        let config: Self = serde_json::from_slice(&bytes)
            .map_err(|_| p::Error("managed host configuration is invalid".into()))?;
        config.validate()?;
        if config
            .paths()
            .into_iter()
            .any(|other| paths_overlap(path, other))
        {
            return Err(p::Error(
                "managed host configuration aliases a state or credential path".into(),
            ));
        }
        config.preflight_references(path)?;
        Ok(config)
    }

    pub(crate) fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1)
            || !self.host_bind.ip().is_loopback()
            || self.host_bind.port() == 0
            || !self.agent_execution_bind.ip().is_loopback()
            || self.agent_execution_bind.port() == 0
            || self.host_bind == self.agent_execution_bind
        {
            return Err(p::Error(
                "managed host configuration has an invalid loopback binding".into(),
            ));
        }
        p::validate_product_id(&self.install_ref)?;
        p::validate_release_id(&self.active_release)?;
        validate_native_target(self.product_target)?;
        reject_path(&self.installation_manifest_path)?;
        let paths = self.paths();
        let fallback = self.journal_path.with_extension("worker.jsonl");
        for (index, path) in paths.iter().enumerate() {
            reject_path(path)?;
            if paths
                .iter()
                .skip(index + 1)
                .any(|other| paths_overlap(path, other))
                || (self.worker_config_path.is_none() && paths_overlap(&fallback, path))
            {
                return Err(p::Error(
                    "managed host state and credential paths overlap".into(),
                ));
            }
        }
        Ok(())
    }

    fn paths(&self) -> Vec<&Path> {
        let mut paths = vec![
            self.installation_manifest_path.as_path(),
            self.journal_path.as_path(),
            self.maintenance_credential_path.as_path(),
            self.agent_execution_credential_path.as_path(),
        ];
        if let Some(path) = &self.worker_config_path {
            paths.push(path);
        }
        if let Some(path) = &self.bootstrap_config_path {
            paths.push(path);
        }
        paths
    }

    fn preflight_references(&self, source: &Path) -> p::Result<Vec<PathBuf>> {
        let mut protected = self
            .paths()
            .into_iter()
            .map(Path::to_path_buf)
            .collect::<Vec<_>>();
        protected.push(source.to_path_buf());
        if let Some(path) = &self.bootstrap_config_path {
            let setup = forme_execution::installed::bootstrap::BootstrapAuthorityConfig::load(path)
                .map_err(|e| p::Error(e.to_string()))?;
            if setup.install_ref != self.install_ref {
                return Err(p::Error(
                    "setup configuration belongs to another installation".into(),
                ));
            }
            setup
                .reject_protected_paths(&protected)
                .map_err(|e| p::Error(e.to_string()))?;
            protected.extend([
                setup.journal_path,
                setup.signing_key_path,
                setup.web_credential_path,
                setup.new_brain_directory,
            ]);
        }
        if let Some(path) = &self.worker_config_path {
            let worker = WorkerRuntimeConfig::load(path)?;
            worker.validate_protected_paths(
                &protected.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
            )?;
        }
        Ok(protected)
    }
}

#[derive(Clone)]
pub(crate) struct HostRuntimeConfig {
    pub journal_path: PathBuf,
    pub install_id: String,
    pub credentials: HostCredentials,
    pub target: p::ProductTarget,
    pub active_release: String,
    pub installation_manifest_digest: Option<p::SchemaDigest>,
    pub release_digest: Option<p::SchemaDigest>,
    pub bootstrap_config_path: Option<PathBuf>,
    pub worker_config_path: Option<PathBuf>,
    pub host_bind: SocketAddr,
    pub agent_execution_bind: SocketAddr,
    pub has_agent_endpoint: bool,
    pub development_workers: bool,
    pub protected_paths: Vec<PathBuf>,
    pub worker_protected_paths: Vec<PathBuf>,
    pub managed: bool,
}

pub(crate) enum Launch {
    Development { process_workers: bool },
    Managed(PathBuf),
    InstallService(PathBuf),
    UninstallService(PathBuf),
}

pub(crate) fn launch_from_args(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> p::Result<Launch> {
    let args: Vec<_> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(Launch::Development {
            process_workers: false,
        }),
        [flag] if flag == "--development-process-workers" => Ok(Launch::Development {
            process_workers: true,
        }),
        [flag, path] if flag == "--config" => {
            let path = PathBuf::from(path);
            reject_path(&path)?;
            Ok(Launch::Managed(path))
        }
        [install, flag, path] if install == "--install-service" && flag == "--config" => {
            let path = PathBuf::from(path);
            reject_path(&path)?;
            Ok(Launch::InstallService(path))
        }
        [uninstall, flag, path] if uninstall == "--uninstall-service" && flag == "--config" => {
            let path = PathBuf::from(path);
            reject_path(&path)?;
            Ok(Launch::UninstallService(path))
        }
        _ => Err(p::Error(
            "host arguments are invalid for this execution mode".into(),
        )),
    }
}

#[cfg(windows)]
pub(crate) fn managed_from_process_args() -> p::Result<HostRuntimeConfig> {
    match launch_from_args(std::env::args_os().skip(1))? {
        Launch::Managed(path) => from_managed_config(&path),
        _ => Err(p::Error(
            "Windows service requires --config with managed host configuration".into(),
        )),
    }
}

pub(super) fn from_environment(development_workers: bool) -> p::Result<HostRuntimeConfig> {
    let journal_path = required_path("FORME_HOST_JOURNAL_PATH")?;
    let install_id = required("FORME_INSTALL_ID")?;
    let credentials = HostCredentials::new(
        load_or_create_token()?
            .ok_or_else(|| p::Error("host management token is unavailable".into()))?,
        std::env::var("FORME_AGENT_EXECUTION_TOKEN").ok(),
    )?;
    let has_agent_endpoint = credentials.agent_execution().is_some();
    let host_bind = loopback_address("FORME_HOST_BIND", "127.0.0.1:8766")?;
    let agent_execution_bind = loopback_address("FORME_AGENT_EXECUTION_BIND", "127.0.0.1:8767")?;
    let mut protected_paths = vec![journal_path.clone()];
    for key in [
        "FORME_HOST_MAINTENANCE_TOKEN_PATH",
        "FORME_HOST_AGENT_TOKEN_PATH",
    ] {
        if let Some(path) = std::env::var_os(key) {
            protected_paths.push(PathBuf::from(path));
        }
    }
    if let Some(path) = token_path() {
        protected_paths.push(path);
    }
    if let Some(path) = std::env::var_os("FORME_WORKER_CONFIG_PATH") {
        protected_paths.push(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("FORME_BOOTSTRAP_CONFIG_PATH") {
        protected_paths.push(PathBuf::from(path));
    }
    Ok(HostRuntimeConfig {
        journal_path,
        install_id,
        credentials,
        target: target_from_environment()?,
        active_release: std::env::var("FORME_ACTIVE_RELEASE")
            .unwrap_or_else(|_| "unconfigured".into()),
        installation_manifest_digest: None,
        release_digest: None,
        bootstrap_config_path: std::env::var_os("FORME_BOOTSTRAP_CONFIG_PATH").map(PathBuf::from),
        worker_config_path: std::env::var_os("FORME_WORKER_CONFIG_PATH").map(PathBuf::from),
        host_bind,
        agent_execution_bind,
        has_agent_endpoint,
        development_workers,
        worker_protected_paths: protected_paths.clone(),
        protected_paths,
        managed: false,
    })
}

pub(super) fn from_managed_config(path: &Path) -> p::Result<HostRuntimeConfig> {
    let config = HostServiceConfig::load(path)?;
    let (manifest, manifest_digest) = manifest::load_installation_manifest(&config)?;
    let worker_protected_paths = config.preflight_references(path)?;
    let credentials = HostCredentials::new(
        read_managed_credential(&config.maintenance_credential_path, "maintenance")?,
        Some(read_managed_credential(
            &config.agent_execution_credential_path,
            "agent execution",
        )?),
    )?;
    let mut protected_paths = config
        .paths()
        .into_iter()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    protected_paths.push(path.to_path_buf());
    Ok(HostRuntimeConfig {
        journal_path: config.journal_path,
        install_id: config.install_ref,
        credentials,
        target: config.product_target,
        active_release: config.active_release,
        installation_manifest_digest: Some(manifest_digest),
        release_digest: Some(manifest.release_digest),
        bootstrap_config_path: config.bootstrap_config_path,
        worker_config_path: config.worker_config_path,
        host_bind: config.host_bind,
        agent_execution_bind: config.agent_execution_bind,
        has_agent_endpoint: true,
        development_workers: false,
        protected_paths,
        worker_protected_paths,
        managed: true,
    })
}

fn loopback_address(key: &str, default: &str) -> p::Result<SocketAddr> {
    let address = std::env::var(key).unwrap_or_else(|_| default.into());
    let address: SocketAddr = address
        .parse()
        .map_err(|_| p::Error(format!("{key} must be a numeric loopback address")))?;
    if !address.ip().is_loopback() {
        return Err(p::Error(format!(
            "{key} must be a loopback address with a port"
        )));
    }
    Ok(address)
}

fn reject_path(path: &Path) -> p::Result<()> {
    forme_execution::installed::reject_linked_ancestors(path)
}

pub(crate) fn load_worker_host(
    host_journal: &Path,
    agent_digest: Option<p::SchemaDigest>,
    worker_config_path: Option<&Path>,
    development_workers: bool,
    protected_paths: &[PathBuf],
) -> p::Result<Arc<dyn IsolatedExecutorHost>> {
    if let Some(config_path) = worker_config_path {
        let digest = agent_digest.ok_or_else(|| {
            p::Error("worker configuration requires an agent execution credential".into())
        })?;
        let config = WorkerRuntimeConfig::load(config_path)?;
        let mut protected = vec![host_journal.to_path_buf()];
        protected.extend(protected_paths.iter().cloned());
        config.validate_protected_paths(
            &protected.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
        )?;
        return if development_workers {
            Ok(Arc::new(config.open_development_process_host(digest)?))
        } else {
            Ok(Arc::new(config.open_host(digest)?))
        };
    }

    // Development and maintenance-only hosts still expose the typed route,
    // but it remains fail-closed until a managed worker config is installed.
    let digest = agent_digest
        .or_else(|| p::canonical_digest(&"unconfigured-agent-execution").ok())
        .ok_or_else(|| p::Error("worker execution digest is unavailable".into()))?;
    let worker_journal = host_journal.with_extension("worker.jsonl");
    Ok(Arc::new(
        forme_execution::installed::WorkerExecutorHost::open(
            &worker_journal,
            digest,
            p::AuthorityEpoch(1),
            [p::Scope("workspace:unconfigured".into())]
                .into_iter()
                .collect::<BTreeSet<_>>(),
        )?,
    ))
}
pub(crate) fn required(key: &str) -> p::Result<String> {
    std::env::var(key).map_err(|_| p::Error(format!("required configuration {key} is missing")))
}

pub(super) fn load_bootstrap_authority(
    host_journal: &Path,
    install: &str,
    credentials: &HostCredentials,
    bootstrap_config_path: Option<&Path>,
    protected_paths: &[PathBuf],
) -> p::Result<Option<Arc<forme_execution::installed::bootstrap::BootstrapAuthority>>> {
    use forme_execution::installed::bootstrap::{BootstrapAuthority, BootstrapAuthorityConfig};
    let Some(path) = bootstrap_config_path else {
        return Ok(None);
    };
    let config = BootstrapAuthorityConfig::load(path).map_err(|e| p::Error(e.to_string()))?;
    if config.install_ref != install {
        return Err(p::Error(
            "setup configuration belongs to another installation".into(),
        ));
    }
    let mut protected = vec![host_journal.to_path_buf()];
    protected.extend(protected_paths.iter().cloned());
    config
        .reject_protected_paths(&protected)
        .map_err(|e| p::Error(e.to_string()))?;
    let maintenance = p::canonical_digest(&credentials.maintenance())?;
    let agent = credentials
        .agent_execution()
        .map(|value| p::canonical_digest(&value))
        .transpose()?;
    BootstrapAuthority::open(config, maintenance, agent.as_ref())
        .map(Arc::new)
        .map(Some)
        .map_err(|e| p::Error(e.to_string()))
}

pub(crate) fn required_path(key: &str) -> p::Result<PathBuf> {
    let path = PathBuf::from(required(key)?);
    if !path.is_absolute() {
        return Err(p::Error(format!("{key} must be an absolute path")));
    }
    Ok(path)
}

pub(crate) fn token_path() -> Option<PathBuf> {
    std::env::var_os("FORME_HOST_TOKEN_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("FORME_HOST_STATE_ROOT")
                .map(|root| PathBuf::from(root).join("host.token"))
        })
}

pub(crate) fn load_or_create_token() -> p::Result<Option<String>> {
    if let Ok(token) = std::env::var("FORME_HOST_TOKEN") {
        return Ok(Some(token));
    }
    let Some(path) = token_path() else {
        return Ok(None);
    };
    forme_execution::installed::reject_linked_ancestors(&path)?;
    match std::fs::read_to_string(&path) {
        Ok(token) => return Ok(Some(token.trim().to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(p::Error(
                "host token cannot be read; refusing to replace it".into(),
            ))
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| p::Error("host state directory cannot be created".into()))?;
    }
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| p::Error("host token entropy is unavailable".into()))?;
    let token = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|_| p::Error("host token could not be stored".into()))?;
    file.write_all(token.as_bytes())
        .map_err(|_| p::Error("host token could not be stored".into()))?;
    file.write_all(b"\n")
        .map_err(|_| p::Error("host token could not be stored".into()))?;
    file.sync_all()
        .map_err(|_| p::Error("host token could not be synced".into()))?;
    Ok(Some(token))
}

fn read_managed_credential(path: &Path, domain: &str) -> p::Result<String> {
    forme_execution::installed::reject_linked_ancestors(path)?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| p::Error(format!("{domain} credential file is unavailable")))?;
    if !metadata.is_file() || metadata.len() > 258 {
        return Err(p::Error(format!(
            "{domain} credential path is not a regular file"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(p::Error(format!(
                "{domain} credential file permissions are too broad"
            )));
        }
    }
    let mut value = String::new();
    std::fs::File::open(path)
        .map_err(|_| p::Error(format!("{domain} credential file cannot be read")))?
        .take(259)
        .read_to_string(&mut value)
        .map_err(|_| p::Error(format!("{domain} credential file cannot be read")))?;
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    if !(32..=256).contains(&value.len()) || value.chars().any(char::is_whitespace) {
        return Err(p::Error(format!("{domain} credential file is malformed")));
    }
    Ok(value)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    fn normalized(path: &Path) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(path.to_string_lossy().replace('\\', "/").to_lowercase())
        }
        #[cfg(not(windows))]
        {
            path.to_path_buf()
        }
    }
    let left = normalized(left);
    let right = normalized(right);
    left.starts_with(&right) || right.starts_with(&left)
}

fn validate_native_target(target: p::ProductTarget) -> p::Result<()> {
    use p::ProductTarget::*;
    let compatible = match target {
        WindowsX86_64 => cfg!(all(windows, target_arch = "x86_64")),
        MacosArm64 => cfg!(all(target_os = "macos", target_arch = "aarch64")),
        MacosX86_64 => cfg!(all(target_os = "macos", target_arch = "x86_64")),
        Ubuntu2404X86_64 | Rocky9X86_64 => cfg!(all(target_os = "linux", target_arch = "x86_64")),
        Ubuntu2404Arm64 => cfg!(all(target_os = "linux", target_arch = "aarch64")),
    };
    if compatible {
        Ok(())
    } else {
        Err(p::Error(
            "managed host target differs from the running binary".into(),
        ))
    }
}

pub(crate) fn target_from_environment() -> p::Result<p::ProductTarget> {
    let value = std::env::var("FORME_PRODUCT_TARGET").or_else(|_| {
        if cfg!(all(windows, target_arch = "x86_64")) {
            Ok("windows_x86_64".into())
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Ok("macos_arm64".into())
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Ok("macos_x86_64".into())
        } else {
            Err(p::Error(
                "FORME_PRODUCT_TARGET must identify the verified native target".into(),
            ))
        }
    })?;
    match value.as_str() {
        "windows_x86_64" => Ok(p::ProductTarget::WindowsX86_64),
        "macos_arm64" => Ok(p::ProductTarget::MacosArm64),
        "macos_x86_64" => Ok(p::ProductTarget::MacosX86_64),
        "ubuntu_2404_x86_64" => Ok(p::ProductTarget::Ubuntu2404X86_64),
        "ubuntu_2404_arm64" => Ok(p::ProductTarget::Ubuntu2404Arm64),
        "rocky9_x86_64" => Ok(p::ProductTarget::Rocky9X86_64),
        _ => Err(p::Error("FORME_PRODUCT_TARGET is unsupported".into())),
    }
}

#[cfg(test)]
mod tests;
