//! Explicit configuration, credential and worker composition boundaries.
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use forme_execution::installed::{HostCredentials, IsolatedExecutorHost, WorkerRuntimeConfig};
use forme_protocol as p;

pub(crate) fn development_process_workers(
    args: impl IntoIterator<Item = String>,
    managed: bool,
) -> p::Result<bool> {
    let args: Vec<_> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(false),
        [flag] if flag == "--development-process-workers" && !managed => Ok(true),
        _ => Err(p::Error(
            "host arguments are invalid for this execution mode".into(),
        )),
    }
}
pub(crate) fn load_worker_host(
    host_journal: &Path,
    agent_digest: Option<p::SchemaDigest>,
    development_workers: bool,
) -> p::Result<Arc<dyn IsolatedExecutorHost>> {
    if let Some(config_path) = std::env::var_os("FORME_WORKER_CONFIG_PATH") {
        let digest = agent_digest.ok_or_else(|| {
            p::Error("worker configuration requires an agent execution credential".into())
        })?;
        let config = WorkerRuntimeConfig::load(&PathBuf::from(config_path))?;
        let mut protected = vec![host_journal.to_path_buf()];
        for key in [
            "FORME_HOST_MAINTENANCE_TOKEN_PATH",
            "FORME_HOST_AGENT_TOKEN_PATH",
        ] {
            if let Some(path) = std::env::var_os(key) {
                protected.push(PathBuf::from(path));
            }
        }
        if let Some(path) = token_path() {
            protected.push(path);
        }
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

pub(crate) fn load_managed_credentials() -> p::Result<HostCredentials> {
    let maintenance_path = required_path("FORME_HOST_MAINTENANCE_TOKEN_PATH")?;
    let agent_path = required_path("FORME_HOST_AGENT_TOKEN_PATH")?;
    if same_path(&maintenance_path, &agent_path) {
        return Err(p::Error(
            "maintenance and agent credential paths must be distinct".into(),
        ));
    }
    HostCredentials::new(
        read_managed_credential(&maintenance_path, "maintenance")?,
        Some(read_managed_credential(&agent_path, "agent execution")?),
    )
}

fn read_managed_credential(path: &Path, domain: &str) -> p::Result<String> {
    forme_execution::installed::reject_linked_ancestors(path)?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| p::Error(format!("{domain} credential file is unavailable")))?;
    if !metadata.is_file() {
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
    let value = std::fs::read_to_string(path)
        .map_err(|_| p::Error(format!("{domain} credential file cannot be read")))?;
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err(p::Error(format!("{domain} credential file is malformed")));
    }
    Ok(value)
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.to_string_lossy().replace('\\', "/");
    let right = right.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(&right)
    }
    #[cfg(not(windows))]
    {
        left == right
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
