//! Offline installed-release data lane, not a brain event or update permission.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseDataRequest {
    pub schema_version: SchemaVersion,
    pub release: ProductReleaseRequest,
    pub owner: OwnerPrincipalRef,
    pub expected_config_revision: u64,
}

impl ProductReleaseDataRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.expected_config_revision == 0 {
            return Err(invalid());
        }
        self.release.validate()?;
        self.owner.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseDataIdentity {
    pub schema_version: SchemaVersion,
    pub config_schema_version: u32,
    pub database_schema_version: u32,
    pub owner_digest: SchemaDigest,
    pub store_identity: SchemaDigest,
    pub schema_digest: SchemaDigest,
    pub database_digest: SchemaDigest,
    pub event_count: u64,
    pub last_stream_seq: u64,
}

impl ProductReleaseDataIdentity {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1)
            || self.config_schema_version == 0
            || self.database_schema_version == 0
        {
            return Err(invalid());
        }
        for digest in [
            &self.owner_digest,
            &self.store_identity,
            &self.schema_digest,
            &self.database_digest,
        ] {
            sha256_digest_bytes(digest)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductReleaseDataStep {
    Backup,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseDataStepRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub step: ProductReleaseDataStep,
}

impl ProductReleaseDataStepRequest {
    pub fn validate_for(&self, request: &ProductReleaseDataRequest) -> Result<()> {
        request.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.operation_id != request.release.operation_id
            || self.command_digest != request.release.command_digest
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ProductReleaseDataResult {
    Inspected {
        identity: ProductReleaseDataIdentity,
    },
    BackedUp {
        backup_ref: String,
        backup_digest: SchemaDigest,
        identity: ProductReleaseDataIdentity,
    },
    Released {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseDataResponse {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub result: ProductReleaseDataResult,
}

impl ProductReleaseDataResponse {
    pub fn validate_for(&self, request: &ProductReleaseDataRequest) -> Result<()> {
        request.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.operation_id != request.release.operation_id
            || self.command_digest != request.release.command_digest
        {
            return Err(invalid());
        }
        let identity = match &self.result {
            ProductReleaseDataResult::Inspected { identity } => identity,
            ProductReleaseDataResult::BackedUp {
                backup_ref,
                backup_digest,
                identity,
            } => {
                if backup_ref != &request.release.backup_ref {
                    return Err(invalid());
                }
                validate_product_digest(backup_digest)?;
                identity
            }
            ProductReleaseDataResult::Released {} => return Ok(()),
        };
        identity.validate()?;
        if identity.owner_digest != sha256_content_digest(request.owner.0.as_bytes()) {
            return Err(invalid());
        }
        Ok(())
    }
}

fn invalid() -> Error {
    Error("offline release data binding is invalid".into())
}
