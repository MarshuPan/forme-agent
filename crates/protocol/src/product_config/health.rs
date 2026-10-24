//! Signed, minimal proof that the managed Web listener is serving this install.
use std::net::SocketAddr;

use super::*;
use crate::{canonical_digest, validate_product_digest};

pub const PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductNetworkHealthObservation {
    Verified(ProductNetworkHealthProof),
    Unreachable,
    Untrusted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductNetworkHealthProof {
    pub schema_version: SchemaVersion,
    pub installation_digest: SchemaDigest,
    pub listener_digest: SchemaDigest,
    pub nonce: String,
    pub signature: Vec<u8>,
}

impl ProductNetworkHealthProof {
    pub fn validate(&self) -> Result<()> {
        self.validate_unsigned()?;
        if self.signature.len() != 64 {
            return Err(Error("Web health proof signature is invalid".into()));
        }
        Ok(())
    }

    pub fn signing_digest(&self) -> Result<SchemaDigest> {
        self.validate_unsigned()?;
        canonical_digest(&(
            "forme-product-web-health-v1",
            self.schema_version,
            &self.installation_digest,
            &self.listener_digest,
            &self.nonce,
        ))
    }

    fn validate_unsigned(&self) -> Result<()> {
        if self.schema_version != PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION
            || self.nonce.len() != 64
            || !self
                .nonce
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(Error("Web health proof is invalid".into()));
        }
        validate_product_digest(&self.installation_digest)?;
        validate_product_digest(&self.listener_digest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductNetworkProgressPhase {
    TargetIntent,
    TargetVerified,
    FallbackIntent,
    FallbackVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductNetworkProgress {
    pub schema_version: SchemaVersion,
    pub phase: ProductNetworkProgressPhase,
    pub requested_listener_digest: SchemaDigest,
    pub active_bind: Option<SocketAddr>,
    pub active_listener_digest: Option<SchemaDigest>,
    pub health_receipt_digest: Option<SchemaDigest>,
    pub failure_ref: Option<SchemaDigest>,
}

impl ProductNetworkProgress {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION {
            return Err(Error("Web network progress is invalid".into()));
        }
        validate_product_digest(&self.requested_listener_digest)?;
        for digest in [
            &self.active_listener_digest,
            &self.health_receipt_digest,
            &self.failure_ref,
        ]
        .into_iter()
        .flatten()
        {
            validate_product_digest(digest)?;
        }
        match self.phase {
            ProductNetworkProgressPhase::TargetIntent => {
                if self.active_bind.is_some()
                    || self.active_listener_digest.is_some()
                    || self.health_receipt_digest.is_some()
                    || self.failure_ref.is_some()
                {
                    return Err(Error("Web target intent is not empty".into()));
                }
            }
            ProductNetworkProgressPhase::FallbackIntent => {
                if self.active_bind.is_some()
                    || self.active_listener_digest.is_some()
                    || self.health_receipt_digest.is_some()
                    || self.failure_ref.is_none()
                {
                    return Err(Error("Web fallback intent is not bound".into()));
                }
            }
            ProductNetworkProgressPhase::TargetVerified
            | ProductNetworkProgressPhase::FallbackVerified => {
                if self.active_bind.is_none()
                    || self.active_listener_digest.is_none()
                    || self.health_receipt_digest.is_none()
                    || self.failure_ref.is_some()
                {
                    return Err(Error("Web health verification is incomplete".into()));
                }
            }
        }
        Ok(())
    }

    pub fn result(&self) -> Option<ProductNetworkResult> {
        let outcome = match self.phase {
            ProductNetworkProgressPhase::TargetVerified => ProductNetworkOutcome::Activated,
            ProductNetworkProgressPhase::FallbackVerified => ProductNetworkOutcome::RolledBack,
            ProductNetworkProgressPhase::TargetIntent
            | ProductNetworkProgressPhase::FallbackIntent => return None,
        };
        Some(ProductNetworkResult {
            schema_version: PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION,
            outcome,
            active_bind: self.active_bind?,
            requested_listener_digest: self.requested_listener_digest.clone(),
            active_listener_digest: self.active_listener_digest.clone()?,
            health_receipt_digest: self.health_receipt_digest.clone()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductNetworkOutcome {
    Activated,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductNetworkResult {
    pub schema_version: SchemaVersion,
    pub outcome: ProductNetworkOutcome,
    pub active_bind: SocketAddr,
    pub requested_listener_digest: SchemaDigest,
    pub active_listener_digest: SchemaDigest,
    pub health_receipt_digest: SchemaDigest,
}

impl ProductNetworkResult {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_NETWORK_HEALTH_SCHEMA_VERSION {
            return Err(Error("Web network result is invalid".into()));
        }
        validate_product_digest(&self.requested_listener_digest)?;
        validate_product_digest(&self.active_listener_digest)?;
        validate_product_digest(&self.health_receipt_digest)
    }
}

pub fn product_network_installation_digest(install_ref: &str) -> Result<SchemaDigest> {
    validate_product_id(install_ref)?;
    canonical_digest(&("forme-product-web-health-installation-v1", install_ref))
}

pub fn product_network_listener_digest(
    bind: SocketAddr,
    authorities: &[String],
    secure: bool,
) -> Result<SchemaDigest> {
    validate_product_network_hosts(authorities, bind.port())?;
    canonical_digest(&(
        "forme-product-web-health-listener-v1",
        bind,
        authorities,
        secure,
    ))
}
