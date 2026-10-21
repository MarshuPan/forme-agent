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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebLoginChallengeView {
    pub schema_version: SchemaVersion,
    pub attempt_ref: SchemaDigest,
    pub csrf: String,
    pub expires_at: Timestamp,
}

/// A short-lived, request-bound proof issued by the authenticated Web
/// process for the core gateway. The browser never receives this object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreSessionAssertion {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub owner: OwnerPrincipalRef,
    pub session_digest: SchemaDigest,
    pub auth_epoch: u64,
    pub audience: String,
    pub request_digest: SchemaDigest,
    pub expires_at: Timestamp,
    pub nonce: String,
    pub signature: Vec<u8>,
}

/// A product-control update consumed by the core's Web-session fence journal.
/// It is transported only over the authenticated Web-to-core bridge and is
/// not a brain event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebSessionFenceUpdate {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub owner: OwnerPrincipalRef,
    pub session_digest: Option<SchemaDigest>,
    pub minimum_auth_epoch: u64,
}

impl WebSessionFenceUpdate {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.minimum_auth_epoch == 0 {
            return Err(Error("invalid Web session fence update".into()));
        }
        crate::validate_product_id(&self.install_ref)?;
        self.owner.validate()?;
        if let Some(session) = &self.session_digest {
            crate::validate_product_digest(session)?;
        }
        Ok(())
    }
}

impl CoreSessionAssertion {
    pub fn validate(&self) -> Result<()> {
        self.validate_unsigned()?;
        if self.signature.len() != 64 {
            return Err(Error("invalid core session assertion signature".into()));
        }
        Ok(())
    }

    pub fn signing_digest(&self) -> Result<SchemaDigest> {
        self.validate_unsigned()?;
        crate::canonical_digest(&(
            "forme-core-session-assertion-v1",
            self.schema_version,
            &self.install_ref,
            &self.owner,
            &self.session_digest,
            self.auth_epoch,
            &self.audience,
            &self.request_digest,
            self.expires_at,
            &self.nonce,
        ))
    }

    fn validate_unsigned(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.auth_epoch == 0
            || self.expires_at <= 0
            || self.nonce.is_empty()
            || self.nonce.len() > 128
            || self.nonce.chars().any(char::is_control)
            || self.audience != "forme-core"
        {
            return Err(Error("invalid core session assertion".into()));
        }
        crate::validate_product_id(&self.install_ref)?;
        self.owner.validate()?;
        crate::validate_product_digest(&self.session_digest)?;
        crate::validate_product_digest(&self.request_digest)
    }
}
