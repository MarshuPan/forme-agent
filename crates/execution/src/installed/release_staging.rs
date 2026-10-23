//! Typed, offline release staging. Publication never changes the running image.
mod storage;
mod verified;
pub use verified::VerifiedStagedRelease;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{
    reject_linked_ancestors, validate_installation_paths, ReleaseKeyring, VerifiedRelease,
};
use storage::{directory, immutable, invalid, json, read, sync_directory};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseStagingConfig {
    pub schema_version: p::SchemaVersion,
    pub install_ref: String,
    pub pinned_keys: BTreeMap<String, [u8; 32]>,
}

impl ReleaseStagingConfig {
    pub fn load(path: &Path) -> p::Result<Self> {
        let config: Self = json(path, 64 * 1024)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1) {
            return Err(invalid());
        }
        p::validate_product_id(&self.install_ref)?;
        ReleaseKeyring::from_pinned_keys(self.pinned_keys.clone())?;
        Ok(())
    }
}

/// Administrator-installed layout and trust roots; no request-supplied paths.
pub struct ReleaseStager {
    installation: p::InstallationManifest,
    keys: ReleaseKeyring,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagedReleaseRecord {
    schema_version: p::SchemaVersion,
    install_ref: String,
    operation_id: String,
    command_digest: p::SchemaDigest,
    manifest: p::ReleaseManifest,
    signature: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UninstallScope {
    pub schema_version: p::SchemaVersion,
    pub install_ref: String,
    pub installation_digest: p::SchemaDigest,
    pub operation_id: String,
    pub command_digest: p::SchemaDigest,
    pub services: Vec<p::BuiltinServiceId>,
    pub program_releases: PathBuf,
    pub preserved_paths: Vec<PathBuf>,
}

impl ReleaseStager {
    /// Read-only construction. OS ACL protection of these roots is a prerequisite.
    pub fn open(
        installation: p::InstallationManifest,
        config: ReleaseStagingConfig,
    ) -> p::Result<Self> {
        validate_installation_paths(&installation)?;
        config.validate()?;
        if installation.install_id != config.install_ref {
            return Err(invalid());
        }
        Ok(Self {
            installation,
            keys: ReleaseKeyring::from_pinned_keys(config.pinned_keys)?,
        })
    }

    pub fn install_ref(&self) -> &str {
        &self.installation.install_id
    }

    pub fn target(&self) -> p::ProductTarget {
        self.installation.target
    }

    pub fn installation_digest(&self) -> p::Result<p::SchemaDigest> {
        self.installation.digest()
    }

    /// Offline build/operator verification. No installation state, journal or
    /// service is opened. Trust roots must be supplied independently of the pack.
    pub fn verify_directory(
        root: &Path,
        config: &ReleaseStagingConfig,
        target: p::ProductTarget,
    ) -> p::Result<p::SchemaDigest> {
        config.validate()?;
        reject_linked_ancestors(root)?;
        let manifest: p::ReleaseManifest = json(&root.join("manifest.json"), 8 * 1024 * 1024)?;
        let signature = read(&root.join("signature.ed25519"), 64)?;
        ReleaseKeyring::from_pinned_keys(config.pinned_keys.clone())?
            .verify(&manifest, &signature, &root.join("content"), target)
            .map(|release| release.digest().clone())
    }

    pub(crate) fn preflight(&self, kind: &p::MaintenanceKind) -> p::Result<()> {
        match kind {
            p::MaintenanceKind::StageSignedRelease {
                release_id,
                release_digest,
            } => {
                self.source(release_id, release_digest)?;
                let destination = self.destination(release_id)?;
                if destination.try_exists().map_err(|_| invalid())? {
                    return Err(p::Error(
                        "release destination already exists; inspect its original operation".into(),
                    ));
                }
                Ok(())
            }
            p::MaintenanceKind::UninstallPlan {
                preserve_owner_data: true,
            } => validate_installation_paths(&self.installation),
            _ => Err(p::Error(
                "maintenance operation has no installed lifecycle worker".into(),
            )),
        }
    }

    /// Called only after the broker persisted the exact operation's stage intent.
    pub(crate) fn execute(
        &self,
        command: &p::MaintenanceCommand,
        operation: &p::MaintenanceOperation,
    ) -> p::Result<p::SchemaDigest> {
        self.preflight(&command.kind)?;
        match &command.kind {
            p::MaintenanceKind::StageSignedRelease {
                release_id,
                release_digest,
            } => self.stage(command, operation, release_id, release_digest),
            p::MaintenanceKind::UninstallPlan {
                preserve_owner_data: true,
            } => self.uninstall_scope(command, operation),
            _ => Err(invalid()),
        }
    }

    fn source(
        &self,
        id: &str,
        digest: &p::SchemaDigest,
    ) -> p::Result<(VerifiedRelease, Vec<u8>, PathBuf)> {
        p::validate_release_id(id)?;
        p::validate_product_digest(digest)?;
        let root = self
            .installation
            .paths
            .host_state
            .join("release-inbox")
            .join(id);
        reject_linked_ancestors(&root)?;
        let manifest: p::ReleaseManifest = json(&root.join("manifest.json"), 8 * 1024 * 1024)?;
        let signature = read(&root.join("signature.ed25519"), 64)?;
        if manifest.release_id != id || &manifest.digest()? != digest {
            return Err(p::Error(
                "release identity or digest differs from its maintenance plan".into(),
            ));
        }
        // Bound disk growth before any writes. Individual files retain the verifier's 2 GiB bound.
        let bytes = manifest.files.iter().try_fold(0_u64, |total, file| {
            total.checked_add(file.size).ok_or_else(invalid)
        })?;
        if bytes > 16 * 1024 * 1024 * 1024 {
            return Err(p::Error(
                "release exceeds the supported staging size".into(),
            ));
        }
        let content = root.join("content");
        let release = self
            .keys
            .verify(&manifest, &signature, &content, self.target())?;
        Ok((release, signature, content))
    }

    fn destination(&self, id: &str) -> p::Result<PathBuf> {
        p::validate_release_id(id)?;
        let destination = self.installation.paths.releases.join(id);
        reject_linked_ancestors(&destination)?;
        Ok(destination)
    }

    fn stage(
        &self,
        command: &p::MaintenanceCommand,
        operation: &p::MaintenanceOperation,
        id: &str,
        digest: &p::SchemaDigest,
    ) -> p::Result<p::SchemaDigest> {
        let (verified, signature, source) = self.source(id, digest)?;
        let releases = &self.installation.paths.releases;
        directory(releases)?;
        let name = p::canonical_digest(&command.operation_id)?.0;
        let staged = releases.join(format!(".stage-{}", &name[7..]));
        reject_linked_ancestors(&staged)?;
        // create_dir, not create_dir_all: a surviving intent must never be replayed.
        fs::create_dir(&staged).map_err(|_| invalid())?;
        sync_directory(releases)?;
        for entry in &verified.manifest().files {
            let target = staged.join(&entry.relative_path);
            directory(target.parent().ok_or_else(invalid)?)?;
            storage::copy(&source.join(&entry.relative_path), &target, entry)?;
        }
        verified.recheck(&staged)?;
        sync_directory(&staged)?;
        let destination = self.destination(id)?;
        if destination.try_exists().map_err(|_| invalid())? {
            return Err(invalid());
        }
        fs::rename(&staged, &destination).map_err(|_| invalid())?;
        sync_directory(releases)?;
        verified.recheck(&destination)?;
        let record = StagedReleaseRecord {
            schema_version: p::SchemaVersion(1),
            install_ref: self.install_ref().into(),
            operation_id: command.operation_id.clone(),
            command_digest: operation.command_digest.clone(),
            manifest: verified.manifest().clone(),
            signature,
        };
        self.record(&command.operation_id, &record)
    }

    fn uninstall_scope(
        &self,
        command: &p::MaintenanceCommand,
        operation: &p::MaintenanceOperation,
    ) -> p::Result<p::SchemaDigest> {
        let paths = &self.installation.paths;
        let scope = UninstallScope {
            schema_version: p::SchemaVersion(1),
            install_ref: self.install_ref().into(),
            installation_digest: self.installation.digest()?,
            operation_id: command.operation_id.clone(),
            command_digest: operation.command_digest.clone(),
            services: vec![
                p::BuiltinServiceId::Brain,
                p::BuiltinServiceId::Web,
                p::BuiltinServiceId::Host,
            ],
            program_releases: paths.releases.clone(),
            preserved_paths: vec![
                paths.config.clone(),
                paths.brain.clone(),
                paths.web_auth.clone(),
                paths.host_state.clone(),
                paths.keys.clone(),
                paths.logs.clone(),
                paths.backups.clone(),
                paths.workspaces.clone(),
                paths.ipc.clone(),
            ],
        };
        self.record(&command.operation_id, &scope)
    }

    fn record(&self, operation_id: &str, record: &impl Serialize) -> p::Result<p::SchemaDigest> {
        let root = self.installation.paths.host_state.join("release-records");
        directory(&root)?;
        let name = p::canonical_digest(&operation_id)?.0;
        let bytes = serde_json::to_vec(record).map_err(|_| invalid())?;
        immutable(&root.join(format!("{}.json", &name[7..])), &bytes)?;
        p::canonical_digest(record)
    }
}
