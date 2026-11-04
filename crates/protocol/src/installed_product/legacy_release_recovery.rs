//! Legacy decisions reference independent evidence, never a fabricated baseline.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseRecoveryRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub idempotency_key: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub review_id: String,
    pub review_digest: SchemaDigest,
    pub requested_deadline: Timestamp,
}

impl LegacyReleaseRecoveryRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.expected_revision == 0
            || self.requested_deadline <= 0
        {
            return Err(invalid());
        }
        self.owner.validate()?;
        for id in [&self.operation_id, &self.idempotency_key, &self.review_id] {
            validate_product_id(id)?;
        }
        validate_product_digest(&self.review_digest)?;
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(&("forme-legacy-release-recovery-request-v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseRecoveryVerification {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub request_digest: SchemaDigest,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub requested_deadline: Timestamp,
}

impl LegacyReleaseRecoveryVerification {
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
        canonical_digest(&("forme-legacy-release-recovery-verification-v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseRecoveryView {
    pub schema_version: SchemaVersion,
    pub request: LegacyReleaseRecoveryRequest,
    pub request_digest: SchemaDigest,
    pub revision: u64,
    pub stage: LegacyReleaseRecoveryStage,
    pub review_digest: SchemaDigest,
    pub receipt_digest: Option<SchemaDigest>,
}

impl LegacyReleaseRecoveryView {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.request.digest()? != self.request_digest
            || self.revision <= self.request.expected_revision
            || self.review_digest != self.request.review_digest
            || (self.stage == LegacyReleaseRecoveryStage::Committed)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyReleaseRecoveryList {
    pub schema_version: SchemaVersion,
    pub product_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub operations: Vec<LegacyReleaseRecoveryView>,
}

impl LegacyReleaseRecoveryList {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyReleaseRecoveryStage {
    Intent,
    Committed,
}

fn invalid() -> Error {
    Error("legacy release recovery binding is invalid or unsupported".into())
}
