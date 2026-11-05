//! Independently sourced historical data identity, one of eight review inputs.
//! This reader does not certify a full legacy review or acquire a data fence.
mod objects;
#[cfg(test)]
mod tests;

use std::{path::PathBuf, time::Instant};

use ed25519_dalek::{Signature, VerifyingKey};
use forme_protocol as p;

use super::{management_repair::storage::ReadCustody, validate_installation_paths};
use objects::{ArchiveOrigin, DataArchive, SignedArchiveOrigin, SCHEMA};

const ARCHIVE_LIMIT: u64 = 256 * 1024;
const ORIGIN_LIMIT: u64 = 16 * 1024;

/// Provision only from independently verified, protected Host configuration.
/// Neither a review, a browser nor a newly hashed local file supplies this trust.
/// No Deserialize: verifying a self-contained signed archive is insufficient.
pub(super) struct LegacyDataArchiveTrust {
    archive_digest: p::SchemaDigest,
    origin_digest: p::SchemaDigest,
    key: VerifyingKey,
}

impl LegacyDataArchiveTrust {
    pub(super) fn from_pinned_origin(
        archive_digest: p::SchemaDigest,
        origin_digest: p::SchemaDigest,
        public_key: [u8; 32],
    ) -> p::Result<Self> {
        p::validate_product_digest(&archive_digest)?;
        p::validate_product_digest(&origin_digest)?;
        let key = VerifyingKey::from_bytes(&public_key).map_err(|_| invalid())?;
        Ok(Self {
            archive_digest,
            origin_digest,
            key,
        })
    }
}

/// Holds original file custody. Loading proves provenance/binding only; a
/// comparison requires an identity from the collector's actual held data worker.
pub(super) struct LegacyDataArchiveEvidence {
    archive: DataArchive,
    origin: ArchiveOrigin,
    origin_digest: p::SchemaDigest,
    binding_digest: p::SchemaDigest,
    root: PathBuf,
    custody: ReadCustody,
}

impl LegacyDataArchiveEvidence {
    /// Read-only, fixed layout. `original_started_at` must come from the original
    /// journal, never a report or caller-provided replacement timestamp.
    pub(super) fn load(
        installation: &p::InstallationManifest,
        trust: &LegacyDataArchiveTrust,
        binding: &p::LegacyReleaseReviewBinding,
        original: &p::ProductReleaseRequest,
        original_started_at: p::Timestamp,
        deadline: Instant,
    ) -> p::Result<Self> {
        ensure_time(deadline)?;
        validate_installation_paths(installation)?;
        binding.validate()?;
        original.validate()?;
        let root = installation
            .paths
            .host_state
            .join("legacy-data-archives")
            .join(&trust.archive_digest.0[7..]);
        check_layout(&root)?;
        let mut custody = ReadCustody::default();
        let origin_bytes = custody.read(&root.join("origin.json"), ORIGIN_LIMIT)?;
        if p::sha256_content_digest(&origin_bytes) != trust.origin_digest {
            return Err(invalid());
        }
        let signed: SignedArchiveOrigin =
            serde_json::from_slice(&origin_bytes).map_err(|_| invalid())?;
        signed
            .origin
            .validate_for(installation, binding, original, original_started_at)?;
        let signature = Signature::from_slice(&signed.signature).map_err(|_| invalid())?;
        if signed.schema_version != SCHEMA || signed.origin.archive_digest != trust.archive_digest {
            return Err(invalid());
        }
        trust
            .key
            .verify_strict(signed.origin.digest()?.0.as_bytes(), &signature)
            .map_err(|_| invalid())?;
        let archive_bytes = custody.read(&root.join("archive.json"), ARCHIVE_LIMIT)?;
        if p::sha256_content_digest(&archive_bytes) != trust.archive_digest {
            return Err(invalid());
        }
        let archive: DataArchive = serde_json::from_slice(&archive_bytes).map_err(|_| invalid())?;
        archive.validate_for(&signed.origin)?;
        let evidence = Self {
            archive,
            origin: signed.origin,
            origin_digest: trust.origin_digest.clone(),
            binding_digest: binding.digest()?,
            root,
            custody,
        };
        evidence.recheck(deadline)?;
        Ok(evidence)
    }

    /// Does not synthesize a missing historical database version from a current
    /// observation. The collector must separately keep its actual data fence.
    pub(super) fn compare_current(
        &self,
        current: &p::ProductReleaseDataIdentity,
        deadline: Instant,
    ) -> p::Result<p::LegacyReleaseEvidence> {
        self.recheck(deadline)?;
        current.validate()?;
        let old = &self.archive.offline_identity;
        if current.owner_digest != old.owner_digest
            || current.store_identity != old.store_identity
            || current.schema_digest != old.schema_digest
            || current.database_digest != old.database_digest
            || current.event_count != old.event_count
            || current.last_stream_seq != old.last_stream_seq
            || current.config_schema_version != self.archive.config_schema_version
            || self
                .archive
                .database_schema_version
                .is_some_and(|version| version != current.database_schema_version)
        {
            return Err(invalid());
        }
        let observation_digest = p::canonical_digest(&(
            "forme-legacy-data-archive-observation-v1",
            &self.origin_digest,
            self.origin.digest()?,
            &self.binding_digest,
            old,
            self.archive.database_schema_version,
            current,
        ))?;
        self.recheck(deadline)?;
        Ok(p::LegacyReleaseEvidence {
            schema_version: SCHEMA,
            kind: p::LegacyReleaseEvidenceKind::ArchivedDataIdentity,
            artifact_digest: self.origin.archive_digest.clone(),
            binding_digest: self.binding_digest.clone(),
            observation_digest,
        })
    }

    pub(super) fn recheck(&self, deadline: Instant) -> p::Result<()> {
        ensure_time(deadline)?;
        check_layout(&self.root)?;
        self.custody.recheck_records()?;
        ensure_time(deadline)
    }
}

fn check_layout(root: &std::path::Path) -> p::Result<()> {
    // This is shared with the signed management evidence reader, without a new
    // native/unsafe boundary. Parent directories may contain other archives.
    super::management_repair::storage::closed_directory(root, &["archive.json", "origin.json"])
}

fn ensure_time(deadline: Instant) -> p::Result<()> {
    if Instant::now() >= deadline {
        Err(invalid())
    } else {
        Ok(())
    }
}

fn invalid() -> p::Error {
    p::Error("legacy data archive evidence is unavailable".into())
}
