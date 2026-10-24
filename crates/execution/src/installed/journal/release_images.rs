//! One-shot configuration effects within the original Switching intent.
use super::*;

impl ProductJournal {
    pub(crate) fn release_images_claimed(&self, id: &str) -> p::Result<bool> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        Ok(writer
            .projection
            .operations
            .get(id)
            .ok_or_else(invalid_journal)?
            .release_images_claimed)
    }

    pub(crate) fn claim_release_images(
        &self,
        expected: u64,
        id: &str,
        command_digest: p::SchemaDigest,
        prepared_digest: p::SchemaDigest,
        at: p::Timestamp,
    ) -> p::Result<()> {
        p::validate_product_id(id)?;
        p::validate_product_digest(&command_digest)?;
        p::validate_product_digest(&prepared_digest)?;
        self.append(
            expected,
            JournalFact::ReleaseImagesClaimed {
                operation_id: id.into(),
                command_digest,
                prepared_digest,
                at,
            },
        )
    }
}
