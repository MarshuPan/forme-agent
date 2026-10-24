//! Native image observations, distinct from service and application readiness.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseServiceImageReceipt {
    pub schema_version: SchemaVersion,
    pub service: BuiltinServiceId,
    pub source_image_digest: SchemaDigest,
    pub target_image_digest: SchemaDigest,
    pub configuration_digest: SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReleaseServiceReceipt {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub command_digest: SchemaDigest,
    pub images: Vec<ProductReleaseServiceImageReceipt>,
}

impl ProductReleaseServiceReceipt {
    pub fn validate_for(&self, request: &ProductReleaseRequest) -> Result<()> {
        request.validate()?;
        if self.schema_version != SchemaVersion(1)
            || self.operation_id != request.operation_id
            || self.command_digest != request.command_digest
            || self.images.len() != 3
        {
            return Err(Error(
                "release service receipt is unbound or incomplete".into(),
            ));
        }
        let mut services = std::collections::BTreeSet::new();
        for image in &self.images {
            if image.schema_version != SchemaVersion(1) || !services.insert(image.service) {
                return Err(Error("release service observations are invalid".into()));
            }
            for digest in [
                &image.source_image_digest,
                &image.target_image_digest,
                &image.configuration_digest,
            ] {
                validate_product_digest(digest)?;
            }
        }
        Ok(())
    }
}
