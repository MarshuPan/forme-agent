use forme_protocol as p;

use super::{storage_error, write_fact, JournalFact, ProductJournal};

impl ProductJournal {
    /// Bind the checked installation manifest to this journal exactly once.
    /// Repeating the same check is idempotent; a different manifest or release
    /// is a replacement attempt and fails closed.
    pub fn record_installation_verification(
        &self,
        manifest_digest: p::SchemaDigest,
        release_digest: p::SchemaDigest,
    ) -> p::Result<u64> {
        p::validate_product_digest(&manifest_digest)?;
        p::validate_product_digest(&release_digest)?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        if let Some((existing_manifest, existing_release)) =
            &writer.projection.installation_verification
        {
            if existing_manifest == &manifest_digest && existing_release == &release_digest {
                return Ok(writer.projection.sequence);
            }
            return Err(p::Error(
                "product journal is bound to a different installation manifest".into(),
            ));
        }
        let expected = writer.projection.sequence;
        write_fact(
            &mut writer,
            expected,
            JournalFact::InstallationVerified {
                manifest_digest,
                release_digest,
            },
        )?;
        Ok(writer.projection.sequence)
    }

    pub fn installation_verification(
        &self,
    ) -> p::Result<Option<(p::SchemaDigest, p::SchemaDigest)>> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        Ok(writer.projection.installation_verification.clone())
    }
}
