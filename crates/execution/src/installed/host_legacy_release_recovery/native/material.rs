//! Fixed protected material layout. Reads never create trust or origin files.
use std::{path::PathBuf, time::Instant};

use ed25519_dalek::{Signature, VerifyingKey};
use forme_protocol as p;

use super::{ensure_time, invalid, objects::*, SourceContext};
use crate::installed::{
    management_repair::storage::{closed_directory, ReadCustody},
    ProductJournal,
};

pub(super) struct Materials {
    pub installation: p::InstallationManifest,
    pub trust: TrustConfig,
    pub context: SourceContext,
    pub signed: p::SignedLegacyReleaseReview,
    pub sources: SourceReview,
    pub sources_digest: p::SchemaDigest,
    root: PathBuf,
    custody: ReadCustody,
}

impl Materials {
    pub fn open(
        installation: p::InstallationManifest,
        journal: &ProductJournal,
    ) -> p::Result<Option<Self>> {
        crate::installed::validate_installation_paths(&installation)?;
        let root = installation.paths.host_state.join("legacy-release-review");
        match std::fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(invalid()),
            Ok(_) => (),
        }
        if installation.target != p::ProductTarget::WindowsX86_64 || !cfg!(windows) {
            return Err(invalid());
        }
        check_layout(&root)?;
        let mut custody = ReadCustody::default();
        let trust: TrustConfig = custody.json(&root.join("config.json"), 32 * 1024)?;
        trust.validate(&installation)?;
        let context = journal.legacy_source_context(&trust.original_operation_id)?;
        let report = custody.read(&root.join("report.json"), 128 * 1024)?;
        if p::sha256_content_digest(&report) != trust.report_digest {
            return Err(invalid());
        }
        let signed: p::SignedLegacyReleaseReview =
            serde_json::from_slice(&report).map_err(|_| invalid())?;
        signed.validate()?;
        let key = VerifyingKey::from_bytes(&trust.review_public_key).map_err(|_| invalid())?;
        let signature = Signature::from_slice(&signed.signature).map_err(|_| invalid())?;
        key.verify_strict(signed.review_digest()?.0.as_bytes(), &signature)
            .map_err(|_| invalid())?;
        let bytes = custody.read(&root.join("sources.json"), 64 * 1024)?;
        if p::sha256_content_digest(&bytes) != trust.source_review_digest {
            return Err(invalid());
        }
        let sources: SignedSourceReview = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        sources.verify(trust.source_public_key)?;
        let now = super::super::timestamp()?;
        sources.review.validate_for(
            &installation,
            &trust,
            &context,
            &signed.review.binding,
            now,
        )?;
        for (name, digest) in sources.review.proofs() {
            let bytes = custody.read(&root.join("proofs").join(name), 4 * 1024 * 1024)?;
            if bytes.is_empty() || p::sha256_content_digest(&bytes) != *digest {
                return Err(invalid());
            }
        }
        let materials = Self {
            installation,
            trust,
            context,
            signed,
            sources_digest: sources.review.digest()?,
            sources: sources.review,
            root,
            custody,
        };
        materials.recheck(Instant::now() + std::time::Duration::from_secs(5))?;
        Ok(Some(materials))
    }

    pub fn recheck(&self, deadline: Instant) -> p::Result<()> {
        ensure_time(deadline)?;
        check_layout(&self.root)?;
        self.custody.recheck_records()?;
        ensure_time(deadline)
    }
}

fn check_layout(root: &std::path::Path) -> p::Result<()> {
    closed_directory(
        root,
        &["config.json", "report.json", "sources.json", "proofs"],
    )?;
    closed_directory(
        &root.join("proofs"),
        &[
            "build-record",
            "code-audit",
            "configuration-history",
            "worker-history",
            "service-path-repair",
        ],
    )
}
