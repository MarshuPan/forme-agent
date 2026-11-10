#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forme_execution::installed::{HostCredentials, IsolatedExecutorHost, WorkerRuntimeConfig};
use forme_protocol as p;

pub(super) fn default_configuration_bind() -> SocketAddr {
    "127.0.0.1:18768"
        .parse()
        .expect("static configuration bind")
}

pub(super) fn loopback_address(key: &str, default: &str) -> p::Result<SocketAddr> {
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

pub(super) fn reject_path(path: &Path) -> p::Result<()> {
    forme_execution::installed::reject_linked_ancestors(path)
}

pub(crate) struct WorkerHostContext<'a> {
    #[cfg(windows)]
    pub managed_mcp: Option<Arc<forme_execution::installed::InstalledManagedMcpPrograms>>,
    pub managed_service_bound: bool,
    pub desktop_authority: Option<Arc<forme_execution::installed::InstalledDesktopAuthority>>,
    pub protected_paths: &'a [PathBuf],
    pub active_release: Option<&'a forme_execution::installed::VerifiedStagedRelease>,
    pub initial_release_root: Option<&'a Path>,
}

pub(crate) fn load_worker_host(
    host_journal: &Path,
    agent_digest: Option<p::SchemaDigest>,
    worker_config_path: Option<&Path>,
    development_workers: bool,
    context: WorkerHostContext<'_>,
) -> p::Result<Arc<dyn IsolatedExecutorHost>> {
    if let Some(config_path) = worker_config_path {
        let digest = agent_digest.ok_or_else(|| {
            p::Error("worker configuration requires an agent execution credential".into())
        })?;
        let original = WorkerRuntimeConfig::load(config_path)?;
        let config = match context.active_release {
            Some(release) => original.for_active_release(release)?,
            None => original,
        };
        let mut protected = vec![host_journal.to_path_buf()];
        protected.extend(context.protected_paths.iter().cloned());
        config.validate_protected_paths(
            &protected.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
        )?;
        #[cfg(windows)]
        if context.managed_service_bound {
            let mut host = match context.active_release {
                Some(release) => config
                    .open_managed_windows_host_with_verified_desktop_helper(digest, release)?,
                None => {
                    let helper = context
                        .initial_release_root
                        .map(|root| root.join("bin/forme-executord.exe"));
                    config
                        .open_managed_windows_host_with_desktop_helper(digest, helper.as_deref())?
                }
            };
            if let Some(authority) = context.desktop_authority {
                host = host.with_installed_desktop_authority(authority);
            }
            if let Some(programs) = context.managed_mcp {
                host = host.with_managed_mcp_programs(programs, config.runtime_root.join("mcp"))?;
            }
            return Ok(Arc::new(host));
        }
        #[cfg(target_os = "linux")]
        if context.managed_service_bound {
            return Ok(Arc::new(config.open_managed_linux_systemd_host(digest)?));
        }
        #[cfg(target_os = "macos")]
        if context.managed_service_bound {
            return Ok(Arc::new(config.open_managed_macos_launchd_host(digest)?));
        }
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

pub(crate) fn load_bootstrap_authority(
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
    let has_sources =
        config.historical_brain_directory.is_some() || config.verified_backup_directory.is_some();
    let mut authority = BootstrapAuthority::open(config, maintenance, agent.as_ref())
        .map_err(|e| p::Error(e.to_string()))?;
    if has_sources {
        let manifest_path = path
            .parent()
            .ok_or_else(|| p::Error("setup layout is invalid".into()))?
            .join("installation.json");
        let mut custody = forme_execution::installed::bootstrap::BootstrapSourceCustody::default();
        let manifest: p::InstallationManifest =
            serde_json::from_slice(&custody.read(&manifest_path, 256 * 1024)?)
                .map_err(|_| p::Error("setup installation is invalid".into()))?;
        if manifest.paths.config.join("installation.json") != manifest_path
            || manifest.paths.config.join("bootstrap.json") != path
        {
            return Err(p::Error("setup layout is invalid".into()));
        }
        authority = authority
            .with_installed_candidates(manifest)
            .map_err(|e| p::Error(e.to_string()))?;
    }
    Ok(Some(Arc::new(authority)))
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

pub(super) fn read_managed_credential(path: &Path, domain: &str) -> p::Result<String> {
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
        let mode = metadata.permissions().mode() & 0o7777;
        // The installed AgentExecution bearer is the sole Host credential
        // deliberately shared read-only with Brain. Manifest validation pins
        // this exact path before this reader is entered; management/setup
        // credentials keep their private-only rule.
        let service_read = domain == "agent execution" && mode == 0o640;
        if mode & 0o077 != 0 && !service_read {
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

pub(super) fn paths_overlap(left: &Path, right: &Path) -> bool {
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

pub(super) fn validate_native_target(target: p::ProductTarget) -> p::Result<()> {
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

/// The owner may be claimed after Host startup. Journal opening is deferred by
/// InstalledDesktopAuthority until an authenticated request sees that owner.
pub(crate) fn load_desktop_authority(
    journal: &Path,
    installation: &str,
    configuration: Option<Arc<std::sync::Mutex<forme_execution::installed::ProductConfigStore>>>,
    worker_config: Option<&Path>,
    managed: bool,
    release: Option<&forme_execution::installed::VerifiedStagedRelease>,
    initial_root: Option<&Path>,
) -> p::Result<Option<Arc<forme_execution::installed::InstalledDesktopAuthority>>> {
    #[cfg(windows)]
    {
        use forme_execution::installed::{InstalledDesktopAuthority, WindowsDesktopSessionCatalog};
        if !managed {
            return Ok(None);
        }
        let (Some(configuration), Some(worker_config)) = (configuration, worker_config) else {
            return Ok(None);
        };
        let worker = WorkerRuntimeConfig::load(worker_config)?;
        let catalog = match release {
            Some(release) => WindowsDesktopSessionCatalog::from_verified_release(release)?,
            None => WindowsDesktopSessionCatalog::new(
                initial_root
                    .ok_or_else(|| p::Error("installed desktop helper root is unavailable".into()))?
                    .join("bin/forme-executord.exe"),
            )?,
        };
        Ok(Some(Arc::new(InstalledDesktopAuthority::new(
            journal.with_extension("desktop.jsonl"),
            installation.into(),
            worker.authority_epoch,
            configuration,
            Arc::new(catalog),
        )?)))
    }
    #[cfg(not(windows))]
    {
        let _ = (
            journal,
            installation,
            configuration,
            worker_config,
            managed,
            release,
            initial_root,
        );
        Ok(None)
    }
}
