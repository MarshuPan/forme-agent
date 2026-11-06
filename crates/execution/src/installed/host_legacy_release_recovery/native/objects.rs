//! Private trust and independently audited source records, not web input.
use ed25519_dalek::{Signature, VerifyingKey};
use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{invalid, SourceContext};

pub(super) const SCHEMA: p::SchemaVersion = p::SchemaVersion(1);
pub(super) const IMAGES: [&str; 6] = [
    "forme-hostd.exe",
    "forme-webd.exe",
    "forme-gatewayd.exe",
    "forme-cli.exe",
    "forme-hostctl.exe",
    "forme-executord.exe",
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TrustConfig {
    pub schema_version: p::SchemaVersion,
    pub install_ref: String,
    pub original_operation_id: String,
    pub report_digest: p::SchemaDigest,
    pub review_public_key: [u8; 32],
    pub source_review_digest: p::SchemaDigest,
    pub source_public_key: [u8; 32],
    pub archive_digest: p::SchemaDigest,
    pub archive_origin_digest: p::SchemaDigest,
    pub archive_origin_public_key: [u8; 32],
}

impl TrustConfig {
    pub fn validate(&self, installation: &p::InstallationManifest) -> p::Result<()> {
        if self.schema_version != SCHEMA || self.install_ref != installation.install_id {
            return Err(invalid());
        }
        p::validate_product_id(&self.original_operation_id)?;
        for digest in [
            &self.report_digest,
            &self.source_review_digest,
            &self.archive_digest,
            &self.archive_origin_digest,
        ] {
            p::validate_product_digest(digest)?;
        }
        // Provision each role independently; no key is inferred from a package
        // signature, the report itself or another role's successful check.
        for bytes in [
            self.review_public_key,
            self.source_public_key,
            self.archive_origin_public_key,
        ] {
            VerifyingKey::from_bytes(&bytes).map_err(|_| invalid())?;
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceImage {
    pub binary: String,
    pub size: u64,
    pub digest: p::SchemaDigest,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AuditedOrder {
    DurableIntentBeforeSideEffect,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdaterAudit {
    pub image_digest: p::SchemaDigest,
    pub source_commit: String,
    pub source_tree_digest: p::SchemaDigest,
    pub cargo_lock_digest: p::SchemaDigest,
    pub toolchain_digest: p::SchemaDigest,
    pub build_record_digest: p::SchemaDigest,
    pub code_audit_digest: p::SchemaDigest,
    pub backup_order: AuditedOrder,
    pub switching_order: AuditedOrder,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceReview {
    pub schema_version: p::SchemaVersion,
    pub source_ref: String,
    pub install_ref: String,
    pub installation_digest: p::SchemaDigest,
    pub owner: p::OwnerPrincipalRef,
    pub original_operation_id: String,
    pub original_command_digest: p::SchemaDigest,
    pub original_release_ref: String,
    pub original_release_digest: p::SchemaDigest,
    pub archive_origin_digest: p::SchemaDigest,
    pub source_recorded_at: p::Timestamp,
    pub reviewed_at: p::Timestamp,
    pub images: Vec<SourceImage>,
    pub updater: UpdaterAudit,
    pub config_files_digest: p::SchemaDigest,
    pub service_configuration_digest: p::SchemaDigest,
    pub worker_state_digest: p::SchemaDigest,
    pub configuration_history_digest: p::SchemaDigest,
    pub worker_history_digest: p::SchemaDigest,
    pub service_path_repair_digest: p::SchemaDigest,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SignedSourceReview {
    pub schema_version: p::SchemaVersion,
    pub review: SourceReview,
    pub signature: Vec<u8>,
}

impl SignedSourceReview {
    pub fn verify(&self, key: [u8; 32]) -> p::Result<()> {
        if self.schema_version != SCHEMA {
            return Err(invalid());
        }
        let key = VerifyingKey::from_bytes(&key).map_err(|_| invalid())?;
        let signature = Signature::from_slice(&self.signature).map_err(|_| invalid())?;
        key.verify_strict(self.review.digest()?.0.as_bytes(), &signature)
            .map_err(|_| invalid())
    }
}

impl SourceReview {
    pub fn digest(&self) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&("forme-legacy-source-review-v1", self))
    }

    pub fn proofs(&self) -> [(&'static str, &p::SchemaDigest); 5] {
        [
            ("build-record", &self.updater.build_record_digest),
            ("code-audit", &self.updater.code_audit_digest),
            ("configuration-history", &self.configuration_history_digest),
            ("worker-history", &self.worker_history_digest),
            ("service-path-repair", &self.service_path_repair_digest),
        ]
    }

    pub fn validate_for(
        &self,
        installation: &p::InstallationManifest,
        config: &TrustConfig,
        source: &SourceContext,
        binding: &p::LegacyReleaseReviewBinding,
        now: p::Timestamp,
    ) -> p::Result<()> {
        binding.validate()?;
        source.takeover.validate()?;
        p::validate_product_id(&self.source_ref)?;
        let takeover = &source.takeover;
        if self.schema_version != SCHEMA
            || self.install_ref != installation.install_id
            || self.installation_digest != installation.digest()?
            || self.installation_digest != takeover.installation_digest
            || self.owner != takeover.owner
            || self.owner != binding.owner
            || self.install_ref != binding.install_ref
            || self.original_operation_id != takeover.release.operation_id
            || self.original_operation_id != config.original_operation_id
            || self.original_operation_id != binding.original_operation_id
            || self.original_command_digest != takeover.release.command_digest
            || self.original_command_digest != binding.original_command_digest
            || self.original_release_ref != installation.release_id
            || self.original_release_ref != binding.original_release_ref
            || self.original_release_ref != takeover.release.original_release_ref
            || self.original_release_digest != installation.release_digest
            || self.original_release_digest != binding.original_release_digest
            || self.original_release_digest != takeover.release.original_release_digest
            || self.archive_origin_digest != config.archive_origin_digest
            || binding.config_revision != takeover.expected_config_revision
            || binding.config_digest != takeover.expected_config_digest
            || self.source_recorded_at < 0
            || self.source_recorded_at >= source.planned_at
            || self.reviewed_at < source.planned_at
            || self.reviewed_at > now
            || self.images.len() != IMAGES.len()
            || self.updater.source_commit.len() != 40
            || !self
                .updater
                .source_commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        for (image, name) in self.images.iter().zip(IMAGES) {
            if image.binary != name || image.size == 0 || image.size > 512 * 1024 * 1024 {
                return Err(invalid());
            }
            p::validate_product_digest(&image.digest)?;
        }
        if self.updater.image_digest != self.images[0].digest {
            return Err(invalid());
        }
        for digest in [
            &self.config_files_digest,
            &self.service_configuration_digest,
            &self.worker_state_digest,
            &self.updater.source_tree_digest,
            &self.updater.cargo_lock_digest,
            &self.updater.toolchain_digest,
        ] {
            p::validate_product_digest(digest)?;
        }
        for (_, digest) in self.proofs() {
            p::validate_product_digest(digest)?;
        }
        Ok(())
    }

    pub fn image_bindings(&self) -> Vec<(String, u64, p::SchemaDigest)> {
        self.images
            .iter()
            .map(|image| (image.binary.clone(), image.size, image.digest.clone()))
            .collect()
    }
}
