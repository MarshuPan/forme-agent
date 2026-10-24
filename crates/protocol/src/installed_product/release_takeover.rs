//! Durable updater bindings, independent from brain events and restore admission.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseTakeover {
    pub schema_version: SchemaVersion,
    pub release: ProductReleaseRequest,
    pub installation_digest: SchemaDigest,
    pub owner: OwnerPrincipalRef,
    pub expected_config_revision: u64,
    pub expected_config_digest: SchemaDigest,
    pub deadline: Timestamp,
    pub readiness_nonce: String,
}

impl ProductReleaseTakeover {
    pub fn validate(&self) -> Result<()> {
        self.release.validate()?;
        self.owner.validate()?;
        validate_product_digest(&self.installation_digest)?;
        validate_product_digest(&self.expected_config_digest)?;
        validate_nonce(&self.readiness_nonce)?;
        if self.schema_version != SchemaVersion(1)
            || self.expected_config_revision == 0
            || self.expected_config_revision >= 1_000_000_000
            || self.deadline <= 0
            || self.release.original_release_ref == self.release.target_release_ref
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(self)
    }
}

/// Only the product configuration writer publishes this object. Browser model
/// patches cannot supply it or remove it from a later configuration revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseBinding {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub target_release_ref: String,
    pub target_release_digest: SchemaDigest,
    pub core_image_digest: SchemaDigest,
    pub owner_digest: SchemaDigest,
    pub store_identity: SchemaDigest,
    pub config_revision: u64,
    pub readiness_nonce: String,
}

impl ProductReleaseBinding {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.config_revision == 0
            || self.config_revision > 1_000_000_000
        {
            return Err(invalid());
        }
        validate_product_id(&self.install_ref)?;
        validate_product_id(&self.operation_id)?;
        validate_release_id(&self.target_release_ref)?;
        for digest in [
            &self.command_digest,
            &self.target_release_digest,
            &self.core_image_digest,
            &self.owner_digest,
            &self.store_identity,
        ] {
            validate_product_digest(digest)?;
        }
        validate_nonce(&self.readiness_nonce)
    }

    pub fn validate_for(&self, takeover: &ProductReleaseTakeover) -> Result<()> {
        self.validate()?;
        takeover.validate()?;
        let release = &takeover.release;
        if self.install_ref != release.install_ref
            || self.operation_id != release.operation_id
            || self.command_digest != release.command_digest
            || self.target_release_ref != release.target_release_ref
            || self.target_release_digest != release.target_release_digest
            || self.owner_digest != sha256_content_digest(takeover.owner.0.as_bytes())
            || self.config_revision != takeover.expected_config_revision + 1
            || self.readiness_nonce != takeover.readiness_nonce
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        canonical_digest(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseAdmission {
    pub schema_version: SchemaVersion,
    pub binding_digest: SchemaDigest,
    pub released: bool,
    pub at: Timestamp,
}

impl ProductReleaseAdmission {
    pub fn validate_for(&self, binding: &ProductReleaseBinding) -> Result<()> {
        binding.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.at < 0
            || self.binding_digest != binding.digest()?
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseReadiness {
    pub schema_version: SchemaVersion,
    pub binding: ProductReleaseBinding,
    pub config_revision: u64,
    pub actual_image_digest: SchemaDigest,
    pub data_identity: ProductDataAcknowledgement,
}

/// Offline evidence handed from the independent updater to the new Host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseChecking {
    pub schema_version: SchemaVersion,
    pub takeover: ProductReleaseTakeover,
    pub binding: ProductReleaseBinding,
    pub data_identity: ProductReleaseDataIdentity,
    pub service_receipt: ProductReleaseServiceReceipt,
}

impl ProductReleaseChecking {
    pub fn validate(&self) -> Result<()> {
        self.binding.validate_for(&self.takeover)?;
        self.data_identity.validate()?;
        self.service_receipt.validate_for(&self.takeover.release)?;
        if self.schema_version != SchemaVersion(1)
            || self.binding.owner_digest != self.data_identity.owner_digest
            || self.binding.store_identity != self.data_identity.store_identity
            || self
                .service_receipt
                .images
                .iter()
                .find(|image| image.service == BuiltinServiceId::Brain)
                .is_none_or(|image| image.target_image_digest != self.binding.core_image_digest)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

impl ProductReleaseReadiness {
    pub fn validate(&self) -> Result<()> {
        self.binding.validate()?;
        self.data_identity.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.config_revision < self.binding.config_revision
            || self.config_revision > 1_000_000_000
            || self.actual_image_digest != self.binding.core_image_digest
            || self.data_identity.owner_digest != self.binding.owner_digest
            || self.data_identity.store_identity != self.binding.store_identity
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn validate_nonce(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> Error {
    Error("installed release takeover binding is invalid".into())
}
