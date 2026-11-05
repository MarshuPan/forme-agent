//! Re-read the sole writer's actual journal; never trust a report's own copy.
use super::*;

impl ProductJournal {
    pub(crate) fn legacy_journal_evidence(
        &self,
        binding: &p::LegacyReleaseReviewBinding,
        now: p::Timestamp,
    ) -> p::Result<p::LegacyReleaseEvidence> {
        binding.validate()?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let actual = read_locked(&mut writer.file)
            .and_then(|bytes| replay(&bytes, binding, now).map(|replayed| (bytes, replayed)));
        let (bytes, (projection, prefix)) = match actual {
            Ok(value) => value,
            Err(error) => {
                writer.poisoned = true;
                return Err(error);
            }
        };
        if projection.sequence != writer.projection.sequence
            || projection.head()? != writer.projection.head()?
        {
            writer.poisoned = true;
            return Err(invalid_journal());
        }
        validate_suffix(&projection, binding, now)?;
        let (length, takeover_digest) = prefix?;
        let binding_digest = binding.digest()?;
        Ok(p::LegacyReleaseEvidence {
            schema_version: p::SchemaVersion(1),
            kind: p::LegacyReleaseEvidenceKind::OriginalJournal,
            artifact_digest: p::sha256_content_digest(&bytes[..length]),
            observation_digest: p::canonical_digest(&(
                "forme-legacy-original-journal-v1",
                &binding_digest,
                length as u64,
                takeover_digest,
            ))?,
            binding_digest,
        })
    }
}

fn read_locked(file: &mut File) -> p::Result<Vec<u8>> {
    let position = file.stream_position().map_err(|_| storage_error())?;
    let result = (|| {
        let length = file.metadata().map_err(|_| storage_error())?.len();
        if length == 0 || length > MAX_JOURNAL_BYTES {
            return Err(invalid_journal());
        }
        file.seek(SeekFrom::Start(0)).map_err(|_| storage_error())?;
        let mut bytes = Vec::new();
        (&mut *file)
            .take(MAX_JOURNAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| storage_error())?;
        if bytes.len() as u64 != length
            || file.metadata().map_err(|_| storage_error())?.len() != length
        {
            return Err(invalid_journal());
        }
        Ok(bytes)
    })();
    let restored = file
        .seek(SeekFrom::Start(position))
        .map_err(|_| storage_error());
    restored?;
    result
}

type Prefix = p::Result<(usize, p::SchemaDigest)>;

fn replay(
    bytes: &[u8],
    binding: &p::LegacyReleaseReviewBinding,
    now: p::Timestamp,
) -> p::Result<(Projection, Prefix)> {
    let mut projection = Projection::default();
    let mut length = 0;
    let mut prefix = Err(invalid_journal());
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if line.len() > MAX_RECORD_BYTES || line.last() != Some(&b'\n') {
            return Err(invalid_journal());
        }
        let envelope: JournalEnvelope =
            serde_json::from_slice(line).map_err(|_| invalid_journal())?;
        let raw: PersistedFact<'_> = serde_json::from_slice(line).map_err(|_| invalid_journal())?;
        let digest = p::canonical_digest(&(
            envelope.schema_version,
            envelope.product_seq,
            &envelope.previous,
            raw.fact,
        ))?;
        projection.fold_with_digest_and_max(&envelope, digest, PRODUCT_JOURNAL_READER_VERSION)?;
        length += line.len();
        if projection.sequence == binding.journal_seq {
            prefix = (|| {
                super::legacy_recovery_review::validate_review_source(
                    &projection,
                    binding,
                    None,
                    now,
                    true,
                )?;
                let takeover = projection
                    .operations
                    .get(&binding.original_operation_id)
                    .ok_or_else(invalid_journal)?
                    .release_takeover
                    .as_ref()
                    .ok_or_else(invalid_journal)?;
                Ok((length, takeover.digest()?))
            })();
        }
    }
    Ok((projection, prefix))
}

fn validate_suffix(
    projection: &Projection,
    binding: &p::LegacyReleaseReviewBinding,
    now: p::Timestamp,
) -> p::Result<()> {
    if projection.sequence == binding.journal_seq {
        return super::legacy_recovery_review::validate_review_source(
            projection, binding, None, now, true,
        );
    }
    let pending = projection
        .legacy_reviews
        .values()
        .find(|state| {
            state.view.stage == p::LegacyReleaseRecoveryStage::Intent
                && state.review.review.binding == *binding
        })
        .ok_or_else(invalid_journal)?;
    if projection.sequence != pending.intent_revision
        || binding.journal_seq.checked_add(1) != Some(pending.intent_revision)
        || now < pending.intent_at
    {
        return Err(invalid_journal());
    }
    super::legacy_recovery_review::validate_review_source(
        projection,
        binding,
        Some(&pending.view.request.operation_id),
        now,
        false,
    )
}

#[cfg(test)]
impl ProductJournal {
    pub(crate) fn legacy_source_bytes_for_test(&self) -> p::Result<Vec<u8>> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        read_locked(&mut writer.file)
    }
}
