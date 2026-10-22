//! A live composition acknowledgement, separate from an OS Running observation.
use crate::{Error, Result, SchemaDigest, SchemaVersion};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductDataAcknowledgement {
    pub schema_version: SchemaVersion,
    pub owner_digest: SchemaDigest,
    pub store_identity: SchemaDigest,
    pub execution_fence_digest: Option<SchemaDigest>,
    pub composition_digest: SchemaDigest,
}

impl ProductDataAcknowledgement {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) {
            return Err(Error("product data acknowledgement is invalid".into()));
        }
        for digest in [
            &self.owner_digest,
            &self.store_identity,
            &self.composition_digest,
        ]
        .into_iter()
        .chain(self.execution_fence_digest.iter())
        {
            crate::sha256_digest_bytes(digest)?;
        }
        Ok(())
    }
}
