//! Product-only setup messages. No brain events, arbitrary paths or owner input.
use serde::{Deserialize, Serialize};

use crate::{Result, SchemaDigest, SchemaVersion, Timestamp};

/// Sensitive authenticated-service input; intentionally has no Debug.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapReserveRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub proof: String,
    pub setup_digest: SchemaDigest,
}

/// Only the trusted Web service may use this binding. It is not a browser grant.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapSessionBinding {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub setup_digest: SchemaDigest,
}

impl BootstrapSessionBinding {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) {
            return Err(crate::Error("unsupported product setup version".into()));
        }
        crate::validate_product_id(&self.operation_id)?;
        crate::validate_product_digest(&self.setup_digest)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapDataChoice {
    pub schema_version: SchemaVersion,
    pub session: BootstrapSessionBinding,
    pub candidate_ref: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapCommitRequest {
    pub schema_version: SchemaVersion,
    pub session: BootstrapSessionBinding,
    pub candidate_ref: String,
    pub account_commitment: SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapReservationView {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapDataCandidateView {
    pub schema_version: SchemaVersion,
    pub candidate_ref: String,
    pub selected: bool,
    pub available: bool,
    pub issue_code: Option<BootstrapProblemCode>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapProofRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
}

/// Delivered exclusively to an authenticated local management caller.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapProofView {
    pub schema_version: SchemaVersion,
    pub proof: String,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapProblemCode {
    InvalidInput,
    Unauthenticated,
    Conflict,
    Expired,
    DataUnavailable,
    Unavailable,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapProblem {
    pub schema_version: SchemaVersion,
    pub code: BootstrapProblemCode,
}
