//! A bounded MCP list edit that preserves write-only publisher material.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMcpSettingsPatch {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub mcp_servers: Vec<ProductMcpServerConfig>,
}
impl ProductMcpSettingsPatch {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.expected_revision > 1_000_000_000 {
            return Err(Error("MCP settings version or revision is invalid".into()));
        }
        validate_product_id(&self.operation_id)?;
        self.owner.validate()?;
        ProductManagedCapabilitySettings {
            mcp_servers: self.mcp_servers.clone(),
            ..Default::default()
        }
        .validate()
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme.product.mcp-settings.v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMcpSettingsView {
    pub schema_version: SchemaVersion,
    pub owner: Option<OwnerPrincipalRef>,
    pub active_revision: u64,
    pub pending_restart: bool,
    pub configured: bool,
    pub mcp_servers: Vec<ProductMcpServerConfig>,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMcpSettingsOperation {
    pub schema_version: SchemaVersion,
    pub owner: OwnerPrincipalRef,
    pub operation_id: String,
    pub request_digest: SchemaDigest,
    pub committed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMcpSettingsResponse {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub applied: bool,
    pub view: ProductMcpSettingsView,
}
