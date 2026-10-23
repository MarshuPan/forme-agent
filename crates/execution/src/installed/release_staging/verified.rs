//! Reauthenticate installed release bytes against their committed staging fact.
use super::{invalid, json, ReleaseStager, StagedReleaseRecord, VerifiedRelease};
use crate::installed::ProductJournal;
use forme_protocol as p;
use std::path::{Path, PathBuf};

/// Constructed only after journal, immutable receipt, signature and bytes agree.
/// This is a local verified handle, never a request-deserializable capability.
pub struct VerifiedStagedRelease {
    release: VerifiedRelease,
    root: PathBuf,
    record_path: PathBuf,
    record_digest: p::SchemaDigest,
    staging_operation: String,
}

impl VerifiedStagedRelease {
    pub fn manifest(&self) -> &p::ReleaseManifest {
        self.release.manifest()
    }

    pub fn digest(&self) -> &p::SchemaDigest {
        self.release.digest()
    }

    pub fn content_root(&self) -> &Path {
        &self.root
    }

    pub fn staging_operation(&self) -> &str {
        &self.staging_operation
    }

    /// Must run under the installation operation lock immediately before a
    /// switch. A receipt file is evidence, not a substitute for current bytes.
    pub fn recheck(&self) -> p::Result<()> {
        let record: StagedReleaseRecord = json(&self.record_path, 8 * 1024 * 1024)?;
        if p::canonical_digest(&record)? != self.record_digest {
            return Err(invalid());
        }
        self.release.recheck(&self.root)
    }

    /// The caller supplies schemas observed from the protected configuration
    /// and offline database, not from the candidate or a BackupManifest DTO.
    /// No schema-changing product migrator is installed by this operation.
    pub fn check_runtime_compatibility(
        &self,
        config_schema: u32,
        data_schema: u32,
        upgrader_version: u32,
    ) -> p::Result<()> {
        let manifest = self.manifest();
        if config_schema == 0
            || data_schema == 0
            || upgrader_version < manifest.minimum_upgrader
            || !manifest.config_range.can_read(config_schema)
            || !manifest.data_range.can_read(data_schema)
            || manifest.config_range.write_version != config_schema
            || manifest.data_range.write_version != data_schema
        {
            return Err(p::Error(
                "installed release is incompatible with the observed configuration or database"
                    .into(),
            ));
        }
        Ok(())
    }
}

impl ReleaseStager {
    /// Read-only, no service/active-pointer/data writes. The source inbox is not
    /// needed after successful publication. Ambiguous/incomplete staging fails.
    pub fn verified_staged(
        &self,
        journal: &ProductJournal,
        release_id: &str,
        release_digest: &p::SchemaDigest,
    ) -> p::Result<VerifiedStagedRelease> {
        p::validate_release_id(release_id)?;
        p::validate_product_digest(release_digest)?;
        if journal.installation_verification()?
            != Some((
                self.installation.digest()?,
                self.installation.release_digest.clone(),
            ))
        {
            return Err(invalid());
        }
        let revision = journal.revision()?;
        let mut selected = None;
        for operation in journal.operations(0)? {
            let command = journal.command(&operation.operation_id)?;
            let p::MaintenanceKind::StageSignedRelease {
                release_id: id,
                release_digest: digest,
            } = command.kind
            else {
                continue;
            };
            if id != release_id {
                continue;
            }
            if selected.is_some()
                || digest != *release_digest
                || operation.target_release_ref != release_id
                || operation.stage != p::MaintenanceStage::Committed
                || operation.pending_stage.is_some()
            {
                return Err(invalid());
            }
            selected = Some(operation);
        }
        let operation = selected.ok_or_else(invalid)?;
        let name = p::canonical_digest(&operation.operation_id)?;
        let record_path = self
            .installation
            .paths
            .host_state
            .join("release-records")
            .join(format!("{}.json", &name.0[7..]));
        let record: StagedReleaseRecord = json(&record_path, 8 * 1024 * 1024)?;
        let record_digest = p::canonical_digest(&record)?;
        if record.schema_version != p::SchemaVersion(1)
            || record.install_ref != self.installation.install_id
            || record.operation_id != operation.operation_id
            || record.command_digest != operation.command_digest
            || record.manifest.release_id != release_id
            || record.manifest.digest()? != *release_digest
            || !operation.receipts.contains(&record_digest.0)
        {
            return Err(invalid());
        }
        let root = self.destination(release_id)?;
        let release =
            self.keys
                .verify(&record.manifest, &record.signature, &root, self.target())?;
        if journal.revision()? != revision {
            return Err(p::Error("release staging snapshot changed".into()));
        }
        Ok(VerifiedStagedRelease {
            release,
            root,
            record_path,
            record_digest,
            staging_operation: operation.operation_id,
        })
    }
}
