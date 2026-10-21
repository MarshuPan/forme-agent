use std::io::Read;
use std::path::Path;

use forme_protocol as p;

use super::{reject_path, HostServiceConfig};

pub(super) fn load_installation_manifest(
    config: &HostServiceConfig,
) -> p::Result<(p::InstallationManifest, p::SchemaDigest)> {
    let path = &config.installation_manifest_path;
    reject_path(path)?;
    let file = std::fs::File::open(path)
        .map_err(|_| p::Error("installation manifest is unavailable".into()))?;
    let metadata = file
        .metadata()
        .map_err(|_| p::Error("installation manifest is unavailable".into()))?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 {
        return Err(p::Error(
            "installation manifest is unavailable or oversized".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take(metadata.len() + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| p::Error("installation manifest cannot be read".into()))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(p::Error("installation manifest changed during read".into()));
    }
    let manifest: p::InstallationManifest = serde_json::from_slice(&bytes)
        .map_err(|_| p::Error("installation manifest is invalid".into()))?;
    let digest = manifest.digest()?;
    forme_execution::installed::validate_installation_paths(&manifest)?;
    if manifest.install_id != config.install_ref
        || manifest.target != config.product_target
        || manifest.release_id != config.active_release
        || !same_path(&manifest.paths.config.join("installation.json"), path)
        || !same_path(
            &manifest.paths.host_state.join("host.journal"),
            &config.journal_path,
        )
        || !same_path(
            &manifest.paths.keys.join("host-maintenance.token"),
            &config.maintenance_credential_path,
        )
        || !same_path(
            &manifest.paths.keys.join("agent-execution.token"),
            &config.agent_execution_credential_path,
        )
    {
        return Err(p::Error(
            "managed host configuration is not bound to its installation manifest".into(),
        ));
    }
    Ok((manifest, digest))
}

fn same_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy().replace('/', "\\").to_lowercase()
            == right.to_string_lossy().replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}
