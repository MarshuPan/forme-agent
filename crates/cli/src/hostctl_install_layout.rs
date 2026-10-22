//! Preserve the originally prepared layout; never migrate an existing brain.
use std::path::PathBuf;

use forme_protocol as p;

use super::hostctl_install_filesystem::{absolute_path, read_bounded};

pub(super) fn existing_core_path(manifest: &p::InstallationManifest) -> Result<PathBuf, String> {
    let paths = &manifest.paths;
    let template: p::ProductConfigTemplate = serde_json::from_slice(&read_bounded(
        &paths.config.join("product-template.json"),
        256 * 1024,
    )?)
    .map_err(|_| "existing product configuration template is invalid".to_owned())?;
    template.validate().map_err(|error| error.0)?;
    let legacy = template.core_config_path == paths.config.join("core.json")
        && template.configuration_ack_path == paths.config.join("core-config-ack.json")
        && template.core_session_fence_path == paths.host_state.join("core-auth.sqlite");
    let partitioned = template.core_config_path == paths.config.join("core/core.json")
        && template.configuration_ack_path == paths.brain.join("runtime/core-config-ack.json")
        && template.core_session_fence_path == paths.brain.join("runtime/core-auth.sqlite");
    if template.install_ref != manifest.install_id
        || (!legacy && !partitioned)
        || template.model_credentials_directory != paths.keys.join("model-credentials")
        || template.brain_store_path != paths.brain.join("forme.db")
        || template.artifact_root != paths.brain.join("artifacts")
        || template.core_owner_credential_path != paths.keys.join("core-owner.token")
        || template.core_web_credential_path != paths.keys.join("core-web.token")
    {
        return Err("existing configuration layout requires explicit recovery".into());
    }
    for path in [
        &template.core_config_path,
        &template.configuration_ack_path,
        &template.core_session_fence_path,
        &template.model_credentials_directory,
        &template.brain_store_path,
        &template.artifact_root,
        &template.core_owner_credential_path,
        &template.core_web_credential_path,
    ] {
        absolute_path(path, "existing template path")?;
    }
    Ok(template.core_config_path)
}
