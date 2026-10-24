//! Protected installation layout shared by offline maintenance lanes.
use super::*;
use serde::de::DeserializeOwned;

pub(super) fn read_json<T: DeserializeOwned>(path: &Path) -> p::Result<T> {
    crate::web::config::check_path(path)?;
    let file = std::fs::File::open(path).map_err(|_| invalid())?;
    if !file.metadata().map_err(|_| invalid())?.is_file() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > 256 * 1024 {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

pub(super) fn validate_template(
    m: &p::InstallationManifest,
    t: &p::ProductConfigTemplate,
) -> p::Result<()> {
    t.validate()?;
    let legacy = t.core_config_path == m.paths.config.join("core.json")
        && t.core_session_fence_path == m.paths.host_state.join("core-auth.sqlite")
        && t.configuration_ack_path == m.paths.config.join("core-config-ack.json");
    let partitioned = t.core_config_path == m.paths.config.join("core/core.json")
        && t.core_session_fence_path == m.paths.brain.join("runtime/core-auth.sqlite")
        && t.configuration_ack_path == m.paths.brain.join("runtime/core-config-ack.json");
    if t.install_ref != m.install_id
        || !(legacy || partitioned)
        || t.brain_store_path != m.paths.brain.join("forme.db")
        || t.artifact_root != m.paths.brain.join("artifacts")
        || t.model_credentials_directory != m.paths.keys.join("model-credentials")
    {
        return Err(invalid());
    }
    Ok(())
}
