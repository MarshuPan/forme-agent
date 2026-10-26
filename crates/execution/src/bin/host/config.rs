//! Explicit configuration, credential and worker composition boundaries.
mod manifest;
#[path = "runtime_inputs.rs"]
mod runtime_inputs;

use runtime_inputs::{
    default_configuration_bind, loopback_address, paths_overlap, read_managed_credential,
    reject_path, validate_native_target,
};
pub(crate) use runtime_inputs::{
    load_bootstrap_authority, load_or_create_token, load_worker_host, required, required_path,
    target_from_environment, token_path, WorkerHostContext,
};

use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use forme_execution::installed::{
    HostCredentials, InstalledFileProvisioning, ReleaseStager, ReleaseStagingConfig,
    WorkerRuntimeConfig,
};
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
    #[serde(default)]
    pub web_configuration_credential_path: Option<PathBuf>,
    #[serde(default)]
    pub product_config_template_path: Option<PathBuf>,
    #[serde(default)]
    pub release_staging_config_path: Option<PathBuf>,
    pub worker_config_path: Option<PathBuf>,
    pub bootstrap_config_path: Option<PathBuf>,
    pub host_bind: SocketAddr,
    pub agent_execution_bind: SocketAddr,
    #[serde(default = "default_configuration_bind")]
    pub configuration_bind: SocketAddr,
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
            || self.configuration_bind == self.host_bind
            || self.configuration_bind == self.agent_execution_bind
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
        if let Some(path) = &self.web_configuration_credential_path {
            paths.push(path);
        }
        if let Some(path) = &self.product_config_template_path {
            paths.push(path);
        }
        if let Some(path) = &self.release_staging_config_path {
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
    pub product_config_template_path: Option<PathBuf>,
    pub installed_file: Option<InstalledFileProvisioning>,
    pub release_stager: Option<std::sync::Arc<ReleaseStager>>,
    pub restore_worker: Option<std::sync::Arc<forme_execution::installed::InstalledRestoreWorker>>,
    pub release_updater:
        Option<std::sync::Arc<forme_execution::installed::InstalledReleaseUpdater>>,
    pub host_bind: SocketAddr,
    pub agent_execution_bind: SocketAddr,
    pub configuration_bind: SocketAddr,
    pub core_image_path: Option<PathBuf>,
    pub has_agent_endpoint: bool,
    pub development_workers: bool,
    pub protected_paths: Vec<PathBuf>,
    pub worker_protected_paths: Vec<PathBuf>,
    pub managed: bool,
}

pub(crate) enum Launch {
    Development {
        process_workers: bool,
    },
    DevelopmentConfig(PathBuf),
    Managed(PathBuf),
    InstallService(PathBuf),
    UninstallService(PathBuf),
    ReleaseUpdater(PathBuf),
    VerifyRelease {
        root: PathBuf,
        trust: PathBuf,
        target: p::ProductTarget,
    },
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
        [flag, path] if flag == "--development-config" => {
            let path = PathBuf::from(path);
            reject_path(&path)?;
            Ok(Launch::DevelopmentConfig(path))
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
        [updater, flag, path] if updater == "--release-updater" && flag == "--config" => {
            let path = PathBuf::from(path);
            reject_path(&path)?;
            Ok(Launch::ReleaseUpdater(path))
        }
        [verify, root, trust_flag, trust, target_flag, target]
            if verify == "--verify-release"
                && trust_flag == "--trust-config"
                && target_flag == "--target" =>
        {
            let (root, trust) = (PathBuf::from(root), PathBuf::from(trust));
            reject_path(&root)?;
            reject_path(&trust)?;
            let target = serde_json::from_value(serde_json::Value::String(
                target
                    .to_str()
                    .ok_or_else(|| p::Error("release target is invalid".into()))?
                    .to_owned(),
            ))
            .map_err(|_| p::Error("release target is invalid".into()))?;
            Ok(Launch::VerifyRelease {
                root,
                trust,
                target,
            })
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
    let configuration_bind = loopback_address("FORME_CONFIGURATION_BIND", "127.0.0.1:8768")?;
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
        product_config_template_path: None,
        installed_file: None,
        release_stager: None,
        restore_worker: None,
        release_updater: None,
        host_bind,
        agent_execution_bind,
        configuration_bind,
        core_image_path: None,
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
    let restore_worker = config
        .product_config_template_path
        .as_ref()
        .map(|_| forme_execution::installed::InstalledRestoreWorker::new(manifest.clone()))
        .transpose()?
        .map(std::sync::Arc::new);
    let worker_protected_paths = config.preflight_references(path)?;
    let core_image_path = Some(manifest.paths.releases.join("bin").join(format!(
        "forme-gatewayd{}",
        if manifest.target == p::ProductTarget::WindowsX86_64 {
            ".exe"
        } else {
            ""
        }
    )));
    // Validate all trust/layout inputs before opening credentials or any journal.
    let release_stager = config
        .release_staging_config_path
        .as_deref()
        .map(|path| ReleaseStager::open(manifest.clone(), ReleaseStagingConfig::load(path)?))
        .transpose()?
        .map(std::sync::Arc::new);
    let release_updater = release_stager
        .as_ref()
        .map(|stager| {
            forme_execution::installed::InstalledReleaseUpdater::new(
                manifest.clone(),
                path.to_owned(),
                stager.clone(),
                std::sync::Arc::new(forme_execution::installed::SystemReleaseUpdaterPlatform),
            )
            .map(std::sync::Arc::new)
        })
        .transpose()?;
    let credentials = HostCredentials::new(
        read_managed_credential(&config.maintenance_credential_path, "maintenance")?,
        Some(read_managed_credential(
            &config.agent_execution_credential_path,
            "agent execution",
        )?),
    )?
    .with_web_configuration(
        config
            .web_configuration_credential_path
            .as_deref()
            .map(|path| read_managed_credential(path, "Web configuration"))
            .transpose()?,
    )?;
    let mut protected_paths = config
        .paths()
        .into_iter()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    protected_paths.push(path.to_path_buf());
    #[cfg(windows)]
    let installed_file = config
        .worker_config_path
        .as_deref()
        .map(|worker_path| {
            let worker = WorkerRuntimeConfig::load(worker_path)?;
            let claims_file = worker.launch_profiles.iter().any(|profile| {
                profile.sandbox_profile_ref == forme_execution::installed::INSTALLED_FILE_PROFILE
                    || profile
                        .args
                        .iter()
                        .any(|arg| arg == "--installed-file-worker")
            });
            if !claims_file {
                return Ok(None);
            }
            InstalledFileProvisioning::derive(
                &manifest,
                &worker,
                &config.agent_execution_credential_path,
            )
            .map(Some)
        })
        .transpose()?
        .flatten();
    #[cfg(not(windows))]
    let installed_file = None;
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
        product_config_template_path: config.product_config_template_path,
        installed_file,
        release_stager,
        release_updater,
        restore_worker,
        host_bind: config.host_bind,
        agent_execution_bind: config.agent_execution_bind,
        configuration_bind: config.configuration_bind,
        core_image_path,
        has_agent_endpoint: true,
        development_workers: false,
        protected_paths,
        worker_protected_paths,
        managed: true,
    })
}

/// Explicit foreground compatibility for repository-owned integration and
/// development runs. It reuses the closed file schema and credential/path
/// validation, but it never claims SCM service identity or enables the native
/// AgentExecution peer transport.
pub(super) fn from_development_config(path: &Path) -> p::Result<HostRuntimeConfig> {
    let mut runtime = from_managed_config(path)?;
    runtime.managed = false;
    runtime.development_workers = false;
    runtime.installed_file = None;
    Ok(runtime)
}

#[cfg(windows)]
pub(super) fn installation(path: &Path) -> p::Result<p::InstallationManifest> {
    let config = HostServiceConfig::load(path)?;
    manifest::load_installation_manifest(&config).map(|pair| pair.0)
}

#[cfg(test)]
mod tests;
