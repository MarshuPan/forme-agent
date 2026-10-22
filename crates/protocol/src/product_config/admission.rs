//! Host-owned work admission is independent of model/brain facts and readiness.
use crate::{validate_product_digest, validate_product_id, Result, SchemaDigest, SchemaVersion};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductWorkAdmission {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub config_revision: u64,
    pub owner_digest: SchemaDigest,
    pub execution_fence_digest: SchemaDigest,
    pub released: bool,
}

impl ProductWorkAdmission {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.config_revision == 0 {
            return Err(crate::Error("invalid installed work admission".into()));
        }
        validate_product_id(&self.install_ref)?;
        validate_product_id(&self.operation_id)?;
        for digest in [
            &self.command_digest,
            &self.owner_digest,
            &self.execution_fence_digest,
        ] {
            validate_product_digest(digest)?;
        }
        Ok(())
    }
}
