//! Protected storage projection for the Host's one-shot read-only verifier.
use super::bindings::{read_json, validate_template};
use super::*;

pub fn verify_installed_backup(
    manifest_path: &Path,
    request: p::ProductBackupVerificationRequest,
) -> p::Result<p::ProductBackupVerificationResponse> {
    request.validate()?;
    let installation: p::InstallationManifest = read_json(manifest_path)?;
    installation.digest()?;
    if installation.install_id != request.install_ref
        || installation.paths.config.join("installation.json") != manifest_path
    {
        return Err(invalid());
    }
    let template: p::ProductConfigTemplate =
        read_json(&installation.paths.config.join("product-template.json"))?;
    validate_template(&installation, &template)?;
    let config = StorageConfig::load(&template.core_config_path)?;
    let keys = template.brain_store_path.with_extension("content-keys");
    if config.revision != request.expected_config_revision
        || config.install_ref != request.install_ref
        || config.brain.owner != request.owner
        || config.brain.artifact_root != template.artifact_root
        || !config
            .brain
            .store_path
            .starts_with(&installation.paths.brain)
        || config
            .brain
            .content_key_directory
            .as_ref()
            .is_some_and(|path| path != &keys)
    {
        return Err(invalid());
    }
    let source = installation
        .paths
        .brain
        .join("data-operations")
        .join(&request.source);
    crate::web::config::check_path(&source)?;
    let data = OfflineBrainMaintenance::open_storage(
        &p::VerifiedPrincipal(request.owner.0.clone()),
        p::Scope(config.brain.workspace.0),
        &config.brain.store_path,
        &template.artifact_root,
        &keys,
        Some(&source),
    )?;
    let verification = data.verify()?;
    if !verification.is_valid() {
        return Err(invalid());
    }
    let response = p::ProductBackupVerificationResponse {
        schema_version: p::SchemaVersion(1),
        request_digest: request.digest()?,
        config_revision: config.revision,
        verification: p::DataBackupView {
            schema_version: p::SchemaVersion(1),
            object: request.source.clone(),
            digest: verification.manifest.database_digest.clone(),
            events: verification.manifest.event_count,
            created_at: verification.manifest.created_at,
            verified: true,
            restoration: None,
        },
    };
    response.validate_for(&request)?;
    Ok(response)
}
