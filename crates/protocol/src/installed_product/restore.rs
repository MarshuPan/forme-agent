//! Private, bounded Host-to-local-maintenance worker messages; never brain events.
use crate::*;
use serde::{Deserialize, Serialize};

pub const RESTORE_WORKER_MAX_FRAME: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductRestoreRequest {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub expected_config_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub candidate_ref: String,
    pub candidate_digest: SchemaDigest,
}

impl ProductRestoreRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.expected_config_revision == 0 {
            return Err(Error("restore worker request is invalid".into()));
        }
        validate_product_id(&self.install_ref)?;
        validate_product_id(&self.operation_id)?;
        // A single basename only; product ids containing ':' cannot name files.
        validate_release_id(&self.candidate_ref)?;
        self.owner.validate()?;
        validate_product_digest(&self.command_digest)?;
        validate_product_digest(&self.candidate_digest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductRestoreStep {
    Backup,
    FenceSessions,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductRestoreStepRequest {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub step: ProductRestoreStep,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductRestoreWorkerResult {
    Validated {
        binding: ProductDataBinding,
        current_store_identity: SchemaDigest,
        current_database_digest: SchemaDigest,
    },
    BackedUp {
        backup_ref: String,
        manifest_digest: SchemaDigest,
    },
    SessionsFenced {
        revision: AuthRevision,
    },
    Released {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductRestoreWorkerResponse {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub result: ProductRestoreWorkerResult,
}

impl ProductRestoreWorkerResponse {
    pub fn validate_for(&self, request: &ProductRestoreRequest) -> Result<()> {
        request.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.operation_id != request.operation_id
            || self.command_digest != request.command_digest
        {
            return Err(Error(
                "restore worker response belongs to another operation".into(),
            ));
        }
        match &self.result {
            ProductRestoreWorkerResult::Validated {
                binding,
                current_store_identity,
                current_database_digest,
            } => {
                binding.validate()?;
                if binding.candidate_ref != request.candidate_ref
                    || binding.manifest_digest != request.candidate_digest
                    || binding.owner_digest != sha256_content_digest(request.owner.0.as_bytes())
                    || binding.store_identity != *current_store_identity
                    || binding.execution_fence_digest.is_none()
                {
                    return Err(Error(
                        "restored brain identity is not bound to the request".into(),
                    ));
                }
                validate_product_digest(current_store_identity)?;
                validate_product_digest(current_database_digest)
            }
            ProductRestoreWorkerResult::BackedUp {
                backup_ref,
                manifest_digest,
            } => {
                validate_release_id(backup_ref)?;
                validate_product_digest(manifest_digest)
            }
            ProductRestoreWorkerResult::SessionsFenced { revision } => {
                if revision.schema_version != SchemaVersion(1)
                    || revision.auth_epoch == 0
                    || revision.credential_revision == 0
                {
                    return Err(Error(
                        "restore worker authentication revision is invalid".into(),
                    ));
                }
                Ok(())
            }
            ProductRestoreWorkerResult::Released {} => Ok(()),
        }
    }
}
