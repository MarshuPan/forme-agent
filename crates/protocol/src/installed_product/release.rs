//! Typed Host-to-updater messages for signed release activation.
use crate::*;
use serde::{Deserialize, Serialize};

pub const RELEASE_WORKER_MAX_FRAME: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductReleaseChange {
    Upgrade,
    RollbackCompatibleRelease,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseRequest {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub change: ProductReleaseChange,
    pub original_release_ref: String,
    pub original_release_digest: SchemaDigest,
    pub target_release_ref: String,
    pub target_release_digest: SchemaDigest,
    pub backup_ref: String,
}

impl ProductReleaseRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) {
            return Err(Error("release worker request is invalid".into()));
        }
        validate_product_id(&self.install_ref)?;
        validate_product_id(&self.operation_id)?;
        validate_product_digest(&self.command_digest)?;
        validate_release_id(&self.original_release_ref)?;
        validate_product_digest(&self.original_release_digest)?;
        validate_release_id(&self.target_release_ref)?;
        validate_product_digest(&self.target_release_digest)?;
        validate_product_id(&self.backup_ref)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductReleaseStep {
    Quiesce,
    Backup,
    Switch,
    Check,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ProductReleaseWorkerResult {
    Quiesced {
        fence_digest: SchemaDigest,
    },
    BackedUp {
        backup_ref: String,
        backup_digest: SchemaDigest,
    },
    Switched {
        active_release_ref: String,
        active_release_digest: SchemaDigest,
    },
    Checked {
        active_release_ref: String,
        active_release_digest: SchemaDigest,
        service_digest: SchemaDigest,
    },
    Released {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseWorkerResponse {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub step: ProductReleaseStep,
    pub result: ProductReleaseWorkerResult,
}

impl ProductReleaseWorkerResponse {
    pub fn validate_for(
        &self,
        request: &ProductReleaseRequest,
        expected_step: ProductReleaseStep,
    ) -> Result<()> {
        request.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.operation_id != request.operation_id
            || self.command_digest != request.command_digest
            || self.step != expected_step
        {
            return Err(Error(
                "release worker response belongs to another operation".into(),
            ));
        }
        match (&self.step, &self.result) {
            (
                ProductReleaseStep::Quiesce,
                ProductReleaseWorkerResult::Quiesced { fence_digest },
            ) => validate_product_digest(fence_digest),
            (
                ProductReleaseStep::Backup,
                ProductReleaseWorkerResult::BackedUp {
                    backup_ref,
                    backup_digest,
                },
            ) => {
                if backup_ref != &request.backup_ref {
                    return Err(Error("release backup is not bound to the request".into()));
                }
                validate_product_id(backup_ref)?;
                validate_product_digest(backup_digest)
            }
            (
                ProductReleaseStep::Switch,
                ProductReleaseWorkerResult::Switched {
                    active_release_ref,
                    active_release_digest,
                },
            ) => validate_target(active_release_ref, active_release_digest, request),
            (
                ProductReleaseStep::Check,
                ProductReleaseWorkerResult::Checked {
                    active_release_ref,
                    active_release_digest,
                    service_digest,
                },
            ) => {
                validate_target(active_release_ref, active_release_digest, request)?;
                validate_product_digest(service_digest)
            }
            (ProductReleaseStep::Release, ProductReleaseWorkerResult::Released {}) => Ok(()),
            _ => Err(Error(
                "release worker returned the wrong step result".into(),
            )),
        }
    }
}

fn validate_target(
    release_ref: &str,
    release_digest: &SchemaDigest,
    request: &ProductReleaseRequest,
) -> Result<()> {
    if release_ref != request.target_release_ref {
        return Err(Error(
            "release worker target is not bound to the request".into(),
        ));
    }
    validate_release_id(release_ref)?;
    if release_digest != &request.target_release_digest {
        return Err(Error(
            "release worker target digest is not bound to the request".into(),
        ));
    }
    validate_product_digest(release_digest)
}
