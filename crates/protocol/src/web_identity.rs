//! Versioned Web authentication projections; never brain events or credentials.
use serde::{Deserialize, Serialize};

use crate::{Error, OwnerPrincipalRef, Result, SchemaDigest, SchemaVersion, Timestamp};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebClaimCommit {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub operation_id: String,
    pub candidate_ref: String,
    pub owner: OwnerPrincipalRef,
    pub account_commitment: SchemaDigest,
    pub expires_at: Timestamp,
}

impl WebClaimCommit {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.expires_at <= 0 {
            return Err(Error("unsupported or invalid Web claim receipt".into()));
        }
        for value in [&self.install_ref, &self.operation_id, &self.candidate_ref] {
            crate::validate_product_id(value)?;
        }
        self.owner.validate()?;
        crate::validate_product_digest(&self.account_commitment)
    }
}

/// The signature is issued only after the broker's verified-data claim commits.
/// Its verification key is installed independently, never accepted from a browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedWebClaimCommit {
    pub schema_version: SchemaVersion,
    pub receipt: WebClaimCommit,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebSessionView {
    pub schema_version: SchemaVersion,
    pub account_ref: String,
    pub owner: OwnerPrincipalRef,
    pub login_name: String,
    pub auth_epoch: u64,
    pub idle_expires_at: Timestamp,
    pub absolute_expires_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthRevision {
    pub schema_version: SchemaVersion,
    pub credential_revision: u64,
    pub auth_epoch: u64,
}
