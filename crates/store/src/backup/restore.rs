//! New-path copies use the same historical execution fence as offline recovery.
//! The caller owns approval; this service never switches the active authority.
use super::*;
use crate::{offline_access::OfflineSource, store_access::normalized, StoreAccessGuard};

impl BackupService {
    /// Compatibility entry for local storage callers. A production owner
    /// approval must use `restore_approved_to` with its persisted plan digest.
    pub fn restore_to(&self, source: &Path, target: &Path) -> p::Result<RestoreReport> {
        let binding = p::canonical_digest(&(
            "forme-local-restore-copy-v1",
            source.to_string_lossy(),
            target.to_string_lossy(),
        ))?;
        self.restore_approved_to(source, target, &binding)
    }

    /// Source and new target stay exclusively fenced through verification,
    /// sealing and the final manifest. Failure preserves any partial target;
    /// the same approval must never be used to overwrite or repeat it.
    pub fn restore_approved_to(
        &self,
        source: &Path,
        target: &Path,
        approved_plan: &p::SchemaDigest,
    ) -> p::Result<RestoreReport> {
        p::sha256_digest_bytes(approved_plan)?;
        let source = normalized(source)?;
        reject_same_path(&source, target)?;
        let _source_access = OfflineSource::acquire(&source)?;
        let verified = self.verify_backup(&source)?;
        let identity = authority_identity(&read_only_connection(&source)?)?;
        let expected = self
            .store
            .brain_authority()?
            .map(|m| (m.owner_digest, m.store_identity));
        if identity != expected {
            return Err(p::Error(
                "restore source belongs to another brain authority".into(),
            ));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| p::Error("restore target directory unavailable".into()))?;
        }
        let target = normalized(target)?;
        let _target_access = StoreAccessGuard::new_offline_target(&target)?;
        prepare_new_target(&target)?;
        let connection = read_only_connection(&source)?;
        connection
            .backup(rusqlite::MAIN_DB, &target, None)
            .map_err(|_| p::Error("restore copy failed; inspect the original operation".into()))?;
        drop(connection);
        let before = self.manifest_for(&target)?;
        if before.owner_digest != verified.manifest.owner_digest
            || before.schema_digest != verified.manifest.schema_digest
            || before.event_count != verified.manifest.event_count
            || before.last_stream_seq != verified.manifest.last_stream_seq
            || authority_identity(&read_only_connection(&target)?)? != identity
            || file_digest(&source)? != verified.manifest.database_digest
        {
            return Err(p::Error("restored copy changed its source binding".into()));
        }
        let fence = identity
            .as_ref()
            .map(|(owner, store)| {
                crate::restoration_fence::seal_snapshot(
                    &target,
                    owner,
                    store,
                    &verified.manifest.database_digest,
                    approved_plan,
                )
            })
            .transpose()?;
        // Sealing changes the target bytes and may upgrade the local schema.
        // The final checksum must be taken after that transaction is durable.
        let manifest = self.manifest_for(&target)?;
        write_manifest(&target, &manifest)?;
        let verification = self.verify_backup(&target)?;
        if file_digest(&source)? != verified.manifest.database_digest {
            return Err(p::Error(
                "restore source changed during verification".into(),
            ));
        }
        Ok(RestoreReport {
            schema_version: BACKUP_SCHEMA_VERSION,
            manifest,
            verification,
            store_identity: identity.map(|(_, store)| store),
            execution_fence_digest: fence,
        })
    }
}

fn authority_identity(c: &Connection) -> p::Result<Option<(p::SchemaDigest, p::SchemaDigest)>> {
    c.query_row(
        "SELECT owner_digest,store_identity FROM brain_authority",
        [],
        |row| Ok((p::SchemaDigest(row.get(0)?), p::SchemaDigest(row.get(1)?))),
    )
    .optional()
    .map_err(|_| p::Error("restore authority metadata is invalid".into()))
}
