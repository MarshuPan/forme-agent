//! Challenge-response shapes: a saved acknowledgement is not live readiness.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseReadinessProbe {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub nonce: String,
    pub signature: Vec<u8>,
}

impl ProductReleaseReadinessProbe {
    pub fn signing_digest(&self) -> Result<SchemaDigest> {
        validate_product_id(&self.install_ref)?;
        validate_nonce(&self.nonce)?;
        if self.schema_version != SchemaVersion(1) {
            return Err(invalid());
        }
        canonical_digest(&(
            "forme-release-probe-request-v1",
            self.schema_version,
            &self.install_ref,
            &self.nonce,
        ))
    }
    pub fn validate(&self) -> Result<()> {
        self.signing_digest()?;
        if self.signature.len() != 64 {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseReadinessProof {
    pub schema_version: SchemaVersion,
    pub nonce: String,
    pub readiness: ProductReleaseReadiness,
    pub signature: Vec<u8>,
}

impl ProductReleaseReadinessProof {
    pub fn signing_digest(&self) -> Result<SchemaDigest> {
        self.readiness.validate()?;
        validate_nonce(&self.nonce)?;
        if self.schema_version != SchemaVersion(1) {
            return Err(invalid());
        }
        canonical_digest(&(
            "forme-release-probe-response-v1",
            self.schema_version,
            &self.nonce,
            &self.readiness,
        ))
    }
    pub fn validate_for(&self, probe: &ProductReleaseReadinessProbe) -> Result<()> {
        probe.validate()?;
        self.signing_digest()?;
        if self.signature.len() != 64
            || self.nonce != probe.nonce
            || self.readiness.binding.install_ref != probe.install_ref
        {
            return Err(invalid());
        }
        Ok(())
    }
}

/// Domain-separated derivation; the seed and credential never enter a DTO.
pub fn product_release_probe_seed(credential: &[u8]) -> Result<[u8; 32]> {
    if credential.len() < 32
        || credential.len() > 16384
        || credential.iter().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(invalid());
    }
    sha256_digest_bytes(&canonical_digest(&(
        "forme-core-release-probe-key-v1",
        credential,
    ))?)
}

fn validate_nonce(nonce: &str) -> Result<()> {
    if nonce.len() != 64
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    Ok(())
}
fn invalid() -> Error {
    Error("installed release readiness challenge is invalid".into())
}
