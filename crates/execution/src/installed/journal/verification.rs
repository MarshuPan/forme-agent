use forme_protocol as p;

use super::{storage_error, write_fact, JournalFact, JournalReleaseRef, ProductJournal};

impl ProductJournal {
    /// Bind the checked installation manifest to this journal exactly once.
    /// Repeating the same check is idempotent; a different manifest or release
    /// is a replacement attempt and fails closed.
    pub fn record_installation_verification(
        &self,
        manifest_digest: p::SchemaDigest,
        release_digest: p::SchemaDigest,
    ) -> p::Result<u64> {
        self.record_installation_verification_with_active_release(
            manifest_digest,
            release_digest,
            None,
        )
    }

    /// Bind the initial active release without adding a second source of truth.
    /// Existing journals without this projection are upgraded by one additive
    /// product fact; historical installation facts remain unchanged.
    pub fn record_installation_verification_with_active_release(
        &self,
        manifest_digest: p::SchemaDigest,
        release_digest: p::SchemaDigest,
        active_release: Option<(&str, &p::SchemaDigest)>,
    ) -> p::Result<u64> {
        p::validate_product_digest(&manifest_digest)?;
        p::validate_product_digest(&release_digest)?;
        let active_release = active_release
            .map(|(release_id, release_digest)| {
                p::validate_release_id(release_id)?;
                p::validate_product_digest(release_digest)?;
                Ok(JournalReleaseRef {
                    release_id: release_id.into(),
                    release_digest: release_digest.clone(),
                })
            })
            .transpose()?;
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        if let Some((existing_manifest, existing_release)) =
            &writer.projection.installation_verification
        {
            if existing_manifest == &manifest_digest && existing_release == &release_digest {
                if let Some(active) = active_release {
                    match writer.projection.active_release.as_ref() {
                        Some(existing) if existing == &active => {}
                        Some(_) => {
                            return Err(p::Error(
                                "product journal active release differs from its bound value"
                                    .into(),
                            ));
                        }
                        None => {
                            let expected = writer.projection.sequence;
                            write_fact(
                                &mut writer,
                                expected,
                                JournalFact::ActiveReleaseBound { release: active },
                            )?;
                        }
                    }
                }
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
                active_release,
            },
        )?;
        Ok(writer.projection.sequence)
    }

    /// Atomically move the product's active release projection while the
    /// operation is in its durable Switching stage.
    pub fn change_active_release(
        &self,
        operation_id: &str,
        expected: u64,
        from_release: &str,
        from_digest: &p::SchemaDigest,
        to_release: &str,
        to_digest: &p::SchemaDigest,
    ) -> p::Result<()> {
        p::validate_product_id(operation_id)?;
        p::validate_release_id(from_release)?;
        p::validate_product_digest(from_digest)?;
        p::validate_release_id(to_release)?;
        p::validate_product_digest(to_digest)?;
        let from = JournalReleaseRef {
            release_id: from_release.into(),
            release_digest: from_digest.clone(),
        };
        let to = JournalReleaseRef {
            release_id: to_release.into(),
            release_digest: to_digest.clone(),
        };
        if from == to {
            return Err(p::Error(
                "active release transition must change the release".into(),
            ));
        }
        self.append(
            expected,
            JournalFact::ActiveReleaseChanged {
                operation_id: operation_id.into(),
                from,
                to,
            },
        )
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

pub(super) fn validate_release_ref(release: &JournalReleaseRef) -> p::Result<()> {
    p::validate_release_id(&release.release_id)?;
    p::validate_product_digest(&release.release_digest)
}
