//! Narrow desktop settings edits preserve write-only managed trust material.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductComputerSettingsPatch {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub computer: ProductComputerPlacementSettings,
}
impl ProductComputerSettingsPatch {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.expected_revision > 1_000_000_000 {
            return Err(Error(
                "desktop settings version or revision is invalid".into(),
            ));
        }
        validate_product_id(&self.operation_id)?;
        self.owner.validate()?;
        self.computer.validate()
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme.product.computer-settings.v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductComputerSettingsView {
    pub schema_version: SchemaVersion,
    pub owner: Option<OwnerPrincipalRef>,
    pub active_revision: u64,
    pub pending_restart: bool,
    pub configured: bool,
    pub computer: ProductComputerPlacementSettings,
    pub available: bool,
    pub available_scope: Option<crate::Scope>,
    pub available_surfaces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductComputerSettingsOperation {
    pub schema_version: SchemaVersion,
    pub owner: OwnerPrincipalRef,
    pub operation_id: String,
    pub request_digest: SchemaDigest,
    pub committed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductComputerSettingsResponse {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub applied: bool,
    pub view: ProductComputerSettingsView,
}
