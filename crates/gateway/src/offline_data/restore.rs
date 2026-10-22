//! Holds both offline fences across the Host's backup/configuration transaction.
use super::*;
use crate::web::{identity::fence_installed_sessions, WebServerConfig};
use serde::de::DeserializeOwned;

pub struct OfflineRestoreWorker {
    current: OfflineCoreMaintenance,
    _candidate: OfflineBrainMaintenance,
    request: p::ProductRestoreRequest,
    installation: p::InstallationManifest,
    template: p::ProductConfigTemplate,
    binding: p::ProductDataBinding,
    backed_up: bool,
    fenced: bool,
}

impl OfflineRestoreWorker {
    /// Local installed worker only. All data paths are derived from protected
    /// installation/configuration records; the request carries no paths.
    pub fn open(manifest_path: &Path, request: p::ProductRestoreRequest) -> p::Result<Self> {
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
        if config.revision != request.expected_config_revision
            || config.install_ref != request.install_ref
            || config.brain.owner != request.owner
            || config.brain.artifact_root != template.artifact_root
            || !config
                .brain
                .store_path
                .starts_with(&installation.paths.brain)
        {
            return Err(invalid());
        }
        let keys = config
            .brain
            .content_key_directory
            .clone()
            .unwrap_or_else(|| template.brain_store_path.with_extension("content-keys"));
        if keys != template.brain_store_path.with_extension("content-keys") {
            return Err(invalid());
        }
        let current = OfflineCoreMaintenance::open(&template.core_config_path, None)?;
        let candidate_path = installation
            .paths
            .brain
            .join("data-operations")
            .join(&request.candidate_ref);
        let candidate = OfflineBrainMaintenance::open_storage(
            &p::VerifiedPrincipal(request.owner.0.clone()),
            p::Scope(config.brain.workspace.0),
            &config.brain.store_path,
            &template.artifact_root,
            &keys,
            Some(&candidate_path),
        )?;
        let verification = candidate.verify()?;
        let manifest_digest = p::canonical_digest(&verification.manifest)?;
        if manifest_digest != request.candidate_digest
            || candidate.identity().store_identity != current.identity().store_identity
            || candidate.identity().owner_digest != current.identity().owner_digest
        {
            return Err(invalid());
        }
        let binding = p::ProductDataBinding {
            schema_version: p::PRODUCT_DATA_BINDING_SCHEMA_VERSION,
            candidate_ref: request.candidate_ref.clone(),
            store_path: candidate_path,
            content_key_directory: keys,
            owner_digest: candidate.identity().owner_digest.clone(),
            store_identity: candidate.identity().store_identity.clone(),
            manifest_digest,
            execution_fence_digest: Some(candidate.restored_candidate_fence_digest()?),
        };
        binding.validate()?;
        Ok(Self {
            current,
            _candidate: candidate,
            request,
            installation,
            template,
            binding,
            backed_up: false,
            fenced: false,
        })
    }

    pub fn validated(&self) -> p::Result<p::ProductRestoreWorkerResponse> {
        self.response(p::ProductRestoreWorkerResult::Validated {
            binding: self.binding.clone(),
            current_store_identity: self.current.identity().store_identity.clone(),
            current_database_digest: self.current.identity().database_digest.clone(),
        })
    }

    /// The caller must persist the matching stage intent before requesting IO.
    pub fn step(
        &mut self,
        command: p::ProductRestoreStepRequest,
    ) -> p::Result<p::ProductRestoreWorkerResponse> {
        if command.schema_version != p::SchemaVersion(1)
            || command.operation_id != self.request.operation_id
            || command.command_digest != self.request.command_digest
        {
            return Err(invalid());
        }
        let result = match command.step {
            p::ProductRestoreStep::Backup if !self.backed_up && !self.fenced => {
                let hash = p::canonical_digest(&(
                    &self.request.operation_id,
                    &self.request.command_digest,
                ))?;
                let backup_ref = format!("restore-{}.db", &hash.0[7..]);
                let path = self.installation.paths.backups.join(&backup_ref);
                crate::web::config::check_path(&path)?;
                let verification = self.current.backup(&path)?;
                if !verification.is_valid() {
                    return Err(invalid());
                }
                self.backed_up = true;
                p::ProductRestoreWorkerResult::BackedUp {
                    backup_ref,
                    manifest_digest: p::canonical_digest(&verification.manifest)?,
                }
            }
            p::ProductRestoreStep::FenceSessions if self.backed_up && !self.fenced => {
                let web = WebServerConfig::load(&self.installation.paths.config.join("web.json"))?;
                if web.install_ref != self.request.install_ref
                    || web.identity_directory != self.installation.paths.web_auth
                {
                    return Err(invalid());
                }
                let revision = fence_installed_sessions(
                    &web.identity_directory,
                    &web.install_ref,
                    &web.claim_verifying_key,
                    &self.request.owner,
                    &self.request.operation_id,
                    &self.request.command_digest,
                )
                .map_err(|_| invalid())?;
                crate::server::apply_offline_session_fence(
                    &self.template.core_session_fence_path,
                    &p::WebSessionFenceUpdate {
                        schema_version: p::SchemaVersion(1),
                        install_ref: self.request.install_ref.clone(),
                        owner: self.request.owner.clone(),
                        session_digest: None,
                        minimum_auth_epoch: revision.auth_epoch,
                    },
                )?;
                self.fenced = true;
                p::ProductRestoreWorkerResult::SessionsFenced { revision }
            }
            p::ProductRestoreStep::Release if self.backed_up && self.fenced => {
                p::ProductRestoreWorkerResult::Released {}
            }
            _ => return Err(invalid()),
        };
        self.response(result)
    }

    fn response(
        &self,
        result: p::ProductRestoreWorkerResult,
    ) -> p::Result<p::ProductRestoreWorkerResponse> {
        let response = p::ProductRestoreWorkerResponse {
            schema_version: p::SchemaVersion(1),
            operation_id: self.request.operation_id.clone(),
            command_digest: self.request.command_digest.clone(),
            result,
        };
        response.validate_for(&self.request)?;
        Ok(response)
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> p::Result<T> {
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

fn validate_template(m: &p::InstallationManifest, t: &p::ProductConfigTemplate) -> p::Result<()> {
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
