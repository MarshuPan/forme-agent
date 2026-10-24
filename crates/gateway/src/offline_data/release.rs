//! Read-only schema/identity inspection and one-shot backup under the store fence.
use super::{
    bindings::{read_json, validate_template},
    *,
};

pub struct OfflineReleaseWorker {
    current: Option<OfflineCoreMaintenance>,
    request: p::ProductReleaseDataRequest,
    installation: p::InstallationManifest,
    identity: p::ProductReleaseDataIdentity,
    backup_attempted: bool,
}

impl OfflineReleaseWorker {
    /// A local private-pipe worker, never an HTTP handler or model runtime.
    /// Paths come only from the protected installation and core configuration.
    pub fn open(manifest_path: &Path, request: p::ProductReleaseDataRequest) -> p::Result<Self> {
        request.validate()?;
        let installation: p::InstallationManifest = read_json(manifest_path)?;
        installation.digest()?;
        if installation.install_id != request.release.install_ref
            || installation.paths.config.join("installation.json") != manifest_path
        {
            return Err(invalid());
        }
        let template: p::ProductConfigTemplate =
            read_json(&installation.paths.config.join("product-template.json"))?;
        validate_template(&installation, &template)?;
        let config = StorageConfig::load(&template.core_config_path)?;
        let keys = config
            .brain
            .content_key_directory
            .clone()
            .unwrap_or_else(|| template.brain_store_path.with_extension("content-keys"));
        if config.revision != request.expected_config_revision
            || config.install_ref != request.release.install_ref
            || config.brain.owner != request.owner
            || config.brain.artifact_root != template.artifact_root
            || !config
                .brain
                .store_path
                .starts_with(&installation.paths.brain)
            || keys != template.brain_store_path.with_extension("content-keys")
        {
            return Err(invalid());
        }
        let current = OfflineCoreMaintenance::open(&template.core_config_path, None)?;
        let observed = current.identity();
        let identity = p::ProductReleaseDataIdentity {
            schema_version: p::SchemaVersion(1),
            config_schema_version: config.schema_version.0,
            database_schema_version: current.database_schema_version()?,
            owner_digest: observed.owner_digest.clone(),
            store_identity: observed.store_identity.clone(),
            schema_digest: observed.schema_digest.clone(),
            database_digest: observed.database_digest.clone(),
            event_count: observed.event_count,
            last_stream_seq: observed.last_stream_seq,
        };
        identity.validate()?;
        Ok(Self {
            current: Some(current),
            request,
            installation,
            identity,
            backup_attempted: false,
        })
    }

    pub fn inspected(&self) -> p::Result<p::ProductReleaseDataResponse> {
        if self.current.is_none() {
            return Err(invalid());
        }
        self.response(p::ProductReleaseDataResult::Inspected {
            identity: self.identity.clone(),
        })
    }

    /// The caller persists BackedUp intent before requesting Backup. A failed
    /// attempt consumes this session's attempt, retaining any partial target.
    pub fn step(
        &mut self,
        command: p::ProductReleaseDataStepRequest,
    ) -> p::Result<p::ProductReleaseDataResponse> {
        command.validate_for(&self.request)?;
        let current = self.current.as_ref().ok_or_else(invalid)?;
        let result = match command.step {
            p::ProductReleaseDataStep::Backup if !self.backup_attempted => {
                self.backup_attempted = true;
                let hash = p::canonical_digest(&(
                    "forme-release-backup-v1",
                    &self.request.release.operation_id,
                    &self.request.release.command_digest,
                ))?;
                let path = self
                    .installation
                    .paths
                    .backups
                    .join(format!("release-{}.db", &hash.0[7..]));
                crate::web::config::check_path(&path)?;
                let verification = current.backup(&path)?;
                if !verification.is_valid()
                    || verification.manifest.owner_digest.as_ref()
                        != Some(&self.identity.owner_digest)
                    || verification.manifest.schema_digest != self.identity.schema_digest
                    || verification.manifest.event_count != self.identity.event_count
                    || verification.manifest.last_stream_seq != self.identity.last_stream_seq
                {
                    return Err(invalid());
                }
                p::ProductReleaseDataResult::BackedUp {
                    backup_ref: self.request.release.backup_ref.clone(),
                    backup_digest: p::canonical_digest(&verification.manifest)?,
                    identity: self.identity.clone(),
                }
            }
            p::ProductReleaseDataStep::Release => {
                // Drop the exclusive source fence before producing the receipt.
                self.current.take();
                p::ProductReleaseDataResult::Released {}
            }
            _ => return Err(invalid()),
        };
        self.response(result)
    }

    fn response(
        &self,
        result: p::ProductReleaseDataResult,
    ) -> p::Result<p::ProductReleaseDataResponse> {
        let response = p::ProductReleaseDataResponse {
            schema_version: p::SchemaVersion(1),
            operation_id: self.request.release.operation_id.clone(),
            command_digest: self.request.release.command_digest.clone(),
            result,
        };
        response.validate_for(&self.request)?;
        Ok(response)
    }
}
