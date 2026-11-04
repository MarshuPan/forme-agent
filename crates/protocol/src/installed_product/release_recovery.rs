//! Owner adjudication is a separate product transaction, never an upgrade retry.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseRecoveryCapabilities {
    pub schema_version: SchemaVersion,
    pub journal_read_max: u32,
    pub updater_version: u32,
}
impl ProductReleaseRecoveryCapabilities {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.journal_read_max != 2
            || self.updater_version != 2
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseRecoveryBaseline {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub takeover_digest: SchemaDigest,
    pub source_images_digest: SchemaDigest,
    pub service_configuration_digest: SchemaDigest,
    pub config_files_digest: SchemaDigest,
    pub worker_state_digest: SchemaDigest,
    pub data_identity: ProductReleaseDataIdentity,
    pub captured_at: Timestamp,
}

impl ProductReleaseRecoveryBaseline {
    pub fn validate_for(&self, takeover: &ProductReleaseTakeover) -> Result<()> {
        takeover.validate()?;
        self.data_identity.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.operation_id != takeover.release.operation_id
            || self.command_digest != takeover.release.command_digest
            || self.takeover_digest != takeover.digest()?
            || self.captured_at < 0
            || self.captured_at >= takeover.deadline
            || self.data_identity.owner_digest != sha256_content_digest(takeover.owner.0.as_bytes())
        {
            return Err(invalid());
        }
        for value in [
            &self.source_images_digest,
            &self.service_configuration_digest,
            &self.config_files_digest,
            &self.worker_state_digest,
        ] {
            validate_product_digest(value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseRecoveryCandidate {
    pub schema_version: SchemaVersion,
    pub product_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub original_operation_id: String,
    pub original_command_digest: SchemaDigest,
    pub original_release_ref: String,
    pub original_release_digest: SchemaDigest,
    pub config_revision: u64,
    pub config_digest: SchemaDigest,
    pub evidence_digest: SchemaDigest,
}

impl ProductReleaseRecoveryCandidate {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.product_revision == 0
            || self.config_revision == 0
        {
            return Err(invalid());
        }
        self.owner.validate()?;
        validate_product_id(&self.original_operation_id)?;
        validate_release_id(&self.original_release_ref)?;
        for digest in [
            &self.original_command_digest,
            &self.original_release_digest,
            &self.config_digest,
            &self.evidence_digest,
        ] {
            validate_product_digest(digest)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductReleaseRecoveryDecision {
    AbortBeforeSwitch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseRecoveryRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub idempotency_key: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub decision: ProductReleaseRecoveryDecision,
    pub candidate: ProductReleaseRecoveryCandidate,
    pub requested_deadline: Timestamp,
}

impl ProductReleaseRecoveryRequest {
    pub fn validate(&self) -> Result<()> {
        self.candidate.validate()?;
        validate_product_id(&self.operation_id)?;
        validate_product_id(&self.idempotency_key)?;
        self.owner.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.expected_revision != self.candidate.product_revision
            || self.owner != self.candidate.owner
            || self.requested_deadline <= 0
            || self.operation_id == self.candidate.original_operation_id
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-release-recovery-request-v1", self))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductReleaseRecoveryStage {
    Intent,
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseRecoveryView {
    pub schema_version: SchemaVersion,
    pub request: ProductReleaseRecoveryRequest,
    pub request_digest: SchemaDigest,
    pub revision: u64,
    pub stage: ProductReleaseRecoveryStage,
    pub receipt_digest: Option<SchemaDigest>,
}

impl ProductReleaseRecoveryView {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.request.digest()? != self.request_digest
            || self.revision <= self.request.expected_revision
            || (self.stage == ProductReleaseRecoveryStage::Committed)
                != self.receipt_digest.is_some()
        {
            return Err(invalid());
        }
        if let Some(digest) = &self.receipt_digest {
            validate_product_digest(digest)?;
        }
        Ok(())
    }
}

/// Read-only recovery history, including any durable unfinished decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseRecoveryList {
    pub schema_version: SchemaVersion,
    pub product_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub operations: Vec<ProductReleaseRecoveryView>,
}

impl ProductReleaseRecoveryList {
    pub fn validate(&self) -> Result<()> {
        self.owner.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.product_revision == 0
            || self.operations.len() > 25
        {
            return Err(invalid());
        }
        let mut previous = None;
        let mut ids = std::collections::BTreeSet::new();
        for view in &self.operations {
            view.validate()?;
            if view.request.owner != self.owner
                || view.revision > self.product_revision
                || previous.is_some_and(|seq| seq <= view.revision)
                || !ids.insert(&view.request.operation_id)
            {
                return Err(invalid());
            }
            previous = Some(view.revision);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseRecoveryVerification {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub request_digest: SchemaDigest,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub requested_deadline: Timestamp,
}

impl ProductReleaseRecoveryVerification {
    pub fn validate(&self) -> Result<()> {
        self.owner.validate()?;
        validate_product_id(&self.operation_id)?;
        validate_product_digest(&self.request_digest)?;
        if self.schema_version != SchemaVersion(1)
            || self.expected_revision == 0
            || self.requested_deadline <= 0
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-release-recovery-verification-v1", self))
    }
}

/// Result of reviewing an installation whose upgrade predates the recovery
/// baseline protocol.  This is deliberately separate from normal recovery:
/// the current state can never be used to manufacture a historical baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReleaseRecoveryEvidenceStatus {
    Verified,
    Rejected,
    Unverifiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseRecoveryEvidence {
    pub schema_version: SchemaVersion,
    pub status: LegacyReleaseRecoveryEvidenceStatus,
    pub installation_id: String,
    pub owner: OwnerPrincipalRef,
    pub original_operation_id: String,
    pub original_command_digest: SchemaDigest,
    pub original_release_ref: String,
    pub original_release_digest: SchemaDigest,
    pub historical_journal_head: SchemaDigest,
    pub historical_journal_sequence: u64,
    pub historical_journal_history_digest: SchemaDigest,
    pub historical_archive_digest: SchemaDigest,
    pub current_journal_head: SchemaDigest,
    pub current_journal_sequence: u64,
    pub current_journal_history_digest: SchemaDigest,
    pub current_observation_digest: SchemaDigest,
    pub historical_data_identity: ProductReleaseDataIdentity,
    pub current_data_identity: ProductReleaseDataIdentity,
    pub historical_worker_digest: SchemaDigest,
    pub current_worker_digest: SchemaDigest,
    pub historical_service_digest: SchemaDigest,
    pub current_service_digest: SchemaDigest,
    pub historical_config_digest: SchemaDigest,
    pub current_config_digest: SchemaDigest,
    pub historical_program_digest: SchemaDigest,
    pub current_program_digest: SchemaDigest,
    pub repair_package_digest: SchemaDigest,
    pub reader_updater_compatibility_digest: SchemaDigest,
    pub reviewed_at: Timestamp,
    pub reason_digest: Option<SchemaDigest>,
}

impl LegacyReleaseRecoveryEvidence {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.historical_journal_sequence == 0
            || self.current_journal_sequence == 0
            || self.historical_journal_sequence > self.current_journal_sequence
            || self.reviewed_at < 0
        {
            return Err(invalid());
        }
        validate_product_id(&self.installation_id)?;
        self.owner.validate()?;
        validate_product_id(&self.original_operation_id)?;
        validate_release_id(&self.original_release_ref)?;
        self.historical_data_identity.validate()?;
        self.current_data_identity.validate()?;
        let owner_digest = sha256_content_digest(self.owner.0.as_bytes());
        if self.historical_data_identity.owner_digest != owner_digest
            || self.current_data_identity.owner_digest != owner_digest
        {
            return Err(invalid());
        }
        for digest in [
            &self.original_command_digest,
            &self.original_release_digest,
            &self.historical_journal_head,
            &self.historical_journal_history_digest,
            &self.historical_archive_digest,
            &self.current_journal_head,
            &self.current_journal_history_digest,
            &self.current_observation_digest,
            &self.historical_worker_digest,
            &self.current_worker_digest,
            &self.historical_service_digest,
            &self.current_service_digest,
            &self.historical_config_digest,
            &self.current_config_digest,
            &self.historical_program_digest,
            &self.current_program_digest,
            &self.repair_package_digest,
            &self.reader_updater_compatibility_digest,
        ] {
            validate_product_digest(digest)?;
        }
        if let Some(reason) = &self.reason_digest {
            validate_product_digest(reason)?;
        }
        if self.historical_journal_sequence == self.current_journal_sequence
            && (self.historical_journal_head != self.current_journal_head
                || self.historical_journal_history_digest != self.current_journal_history_digest)
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-legacy-release-recovery-evidence-v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseRecoveryReviewRequest {
    pub schema_version: SchemaVersion,
    pub review_id: String,
    pub idempotency_key: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub evidence: LegacyReleaseRecoveryEvidence,
    pub evidence_digest: SchemaDigest,
    pub requested_deadline: Timestamp,
}

impl LegacyReleaseRecoveryReviewRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.expected_revision == 0
            || self.requested_deadline <= 0
            || self.evidence.status != LegacyReleaseRecoveryEvidenceStatus::Verified
            || self.review_id == self.evidence.original_operation_id
            || self.owner != self.evidence.owner
        {
            return Err(invalid());
        }
        validate_product_id(&self.review_id)?;
        validate_product_id(&self.idempotency_key)?;
        self.owner.validate()?;
        self.evidence.validate()?;
        if self.expected_revision != self.evidence.current_journal_sequence
            || self.evidence.digest()? != self.evidence_digest
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-legacy-release-recovery-review-request-v1", self))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReleaseRecoveryReviewStage {
    Intent,
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseRecoveryReviewView {
    pub schema_version: SchemaVersion,
    pub request: LegacyReleaseRecoveryReviewRequest,
    pub request_digest: SchemaDigest,
    pub revision: u64,
    pub stage: LegacyReleaseRecoveryReviewStage,
    pub receipt_digest: Option<SchemaDigest>,
}

impl LegacyReleaseRecoveryReviewView {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.revision <= self.request.expected_revision
            || self.request.digest()? != self.request_digest
            || (self.stage == LegacyReleaseRecoveryReviewStage::Committed)
                != self.receipt_digest.is_some()
        {
            return Err(invalid());
        }
        if let Some(receipt) = &self.receipt_digest {
            validate_product_digest(receipt)?;
        }
        Ok(())
    }
}

/// Explicit re-verification of an unfinished legacy review.  A read of the
/// review never upgrades an intent; this object is the separate owner action
/// required to commit it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseRecoveryReviewVerification {
    pub schema_version: SchemaVersion,
    pub review_id: String,
    pub request_digest: SchemaDigest,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub requested_deadline: Timestamp,
}

impl LegacyReleaseRecoveryReviewVerification {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.expected_revision == 0
            || self.requested_deadline <= 0
        {
            return Err(invalid());
        }
        validate_product_id(&self.review_id)?;
        validate_product_digest(&self.request_digest)?;
        self.owner.validate()
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-legacy-release-recovery-review-verification-v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseRecoveryReviewList {
    pub schema_version: SchemaVersion,
    pub product_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub reviews: Vec<LegacyReleaseRecoveryReviewView>,
}

impl LegacyReleaseRecoveryReviewList {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.product_revision == 0
            || self.reviews.len() > 25
        {
            return Err(invalid());
        }
        self.owner.validate()?;
        let mut previous = None;
        let mut ids = std::collections::BTreeSet::new();
        for review in &self.reviews {
            review.validate()?;
            if review.request.owner != self.owner
                || review.revision > self.product_revision
                || previous.is_some_and(|revision| revision <= review.revision)
                || !ids.insert(&review.request.review_id)
            {
                return Err(invalid());
            }
            previous = Some(review.revision);
        }
        Ok(())
    }
}

fn invalid() -> Error {
    Error("release recovery binding is invalid or unverifiable".into())
}
