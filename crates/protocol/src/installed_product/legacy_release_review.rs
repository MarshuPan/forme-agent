//! Independent legacy evidence. Structural validity does not certify its source.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseReviewBinding {
    pub schema_version: SchemaVersion,
    pub review_id: String,
    pub install_ref: String,
    pub owner: OwnerPrincipalRef,
    pub original_operation_id: String,
    pub original_command_digest: SchemaDigest,
    pub original_release_ref: String,
    pub original_release_digest: SchemaDigest,
    pub config_revision: u64,
    pub config_digest: SchemaDigest,
    pub journal_seq: u64,
    pub journal_head: SchemaDigest,
    pub expires_at: Timestamp,
}

impl LegacyReleaseReviewBinding {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.config_revision == 0
            || self.journal_seq == 0
            || self.expires_at <= 0
        {
            return Err(invalid());
        }
        self.owner.validate()?;
        for id in [
            &self.review_id,
            &self.install_ref,
            &self.original_operation_id,
        ] {
            validate_product_id(id)?;
        }
        validate_release_id(&self.original_release_ref)?;
        for digest in [
            &self.original_command_digest,
            &self.original_release_digest,
            &self.config_digest,
            &self.journal_head,
        ] {
            validate_product_digest(digest)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-legacy-release-review-binding-v1", self))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReleaseEvidenceKind {
    OriginalJournal,
    ArchivedDataIdentity,
    UpdaterCodeAudit,
    SourceImagesAndServices,
    ConfigurationContinuity,
    WorkerContinuity,
    ManagementReaderRepair,
    CurrentExclusiveInspection,
}

impl LegacyReleaseEvidenceKind {
    pub const ALL: [Self; 8] = [
        Self::OriginalJournal,
        Self::ArchivedDataIdentity,
        Self::UpdaterCodeAudit,
        Self::SourceImagesAndServices,
        Self::ConfigurationContinuity,
        Self::WorkerContinuity,
        Self::ManagementReaderRepair,
        Self::CurrentExclusiveInspection,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseEvidence {
    pub schema_version: SchemaVersion,
    pub kind: LegacyReleaseEvidenceKind,
    pub artifact_digest: SchemaDigest,
    pub binding_digest: SchemaDigest,
    pub observation_digest: SchemaDigest,
}

impl LegacyReleaseEvidence {
    pub fn validate_for(&self, binding_digest: &SchemaDigest) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || &self.binding_digest != binding_digest {
            return Err(invalid());
        }
        for value in [
            &self.artifact_digest,
            &self.binding_digest,
            &self.observation_digest,
        ] {
            validate_product_digest(value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReleaseReviewStatus {
    Verified,
    Rejected,
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseReview {
    pub schema_version: SchemaVersion,
    pub binding: LegacyReleaseReviewBinding,
    pub evidence: Vec<LegacyReleaseEvidence>,
    pub status: LegacyReleaseReviewStatus,
    pub issues: Vec<String>,
}

impl LegacyReleaseReview {
    /// Checks closed structure and bindings only. A trusted collector must
    /// separately authenticate every artifact and its actual observations.
    pub fn validate(&self) -> Result<()> {
        self.binding.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.evidence.len() > LegacyReleaseEvidenceKind::ALL.len()
            || self.issues.len() > 8
        {
            return Err(invalid());
        }
        let binding = self.binding.digest()?;
        let mut previous = None;
        for item in &self.evidence {
            if previous.is_some_and(|kind| kind >= item.kind) {
                return Err(invalid());
            }
            item.validate_for(&binding)?;
            previous = Some(item.kind);
        }
        let mut issues = std::collections::BTreeSet::new();
        for issue in &self.issues {
            validate_product_id(issue)?;
            if !issues.insert(issue) {
                return Err(invalid());
            }
        }
        match self.status {
            LegacyReleaseReviewStatus::Verified => {
                if self.evidence.len() != 8 || !self.issues.is_empty() {
                    return Err(invalid());
                }
            }
            _ if self.issues.is_empty() => return Err(invalid()),
            _ => {}
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-legacy-release-review-v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedLegacyReleaseReview {
    pub schema_version: SchemaVersion,
    pub review: LegacyReleaseReview,
    pub signature: Vec<u8>,
}

impl SignedLegacyReleaseReview {
    /// Signature authenticity is enforced by the Host against its protected,
    /// independently provisioned public key. No key is accepted in this DTO.
    pub fn validate(&self) -> Result<()> {
        self.review.validate()?;
        if self.schema_version != SchemaVersion(1) || self.signature.len() != 64 {
            return Err(invalid());
        }
        Ok(())
    }

    /// Digest of the reviewed report, excluding its detached signature.
    /// Requests bind this value so a signature cannot be swapped for another
    /// report after the owner has approved the operation.
    pub fn review_digest(&self) -> Result<SchemaDigest> {
        self.review.digest()
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-signed-legacy-release-review-v1", self))
    }
}

fn invalid() -> Error {
    Error("legacy release review is incomplete, inconsistent or unsupported".into())
}
