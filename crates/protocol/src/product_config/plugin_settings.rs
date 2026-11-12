//! A bounded plugin list edit that preserves write-only publisher material.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPluginSettingsPatch {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub plugins: Vec<ProductPluginConfig>,
}
impl ProductPluginSettingsPatch {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SchemaVersion(1) || self.expected_revision > 1_000_000_000 {
            return Err(Error(
                "plugin settings version or revision is invalid".into(),
            ));
        }
        validate_product_id(&self.operation_id)?;
        self.owner.validate()?;
        ProductManagedCapabilitySettings {
            plugins: self.plugins.clone(),
            ..Default::default()
        }
        .validate()
    }
    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme.product.plugin-settings.v1", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPluginSettingsView {
    pub schema_version: SchemaVersion,
    pub owner: Option<OwnerPrincipalRef>,
    pub active_revision: u64,
    pub pending_restart: bool,
    pub configured: bool,
    pub plugins: Vec<ProductPluginConfig>,
    pub sources: Vec<ProductPluginConfig>,
    pub sources_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPluginSettingsOperation {
    pub schema_version: SchemaVersion,
    pub owner: OwnerPrincipalRef,
    pub operation_id: String,
    pub request_digest: SchemaDigest,
    pub committed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPluginSettingsResponse {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub applied: bool,
    pub view: ProductPluginSettingsView,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_settings_digest_matches_web_golden() {
        let patch: ProductPluginSettingsPatch = serde_json::from_str(r#"{"schema_version":1,"operation_id":"save:plugin-golden","expected_revision":1,"owner":"owner:test","plugins":[{"schema_version":1,"plugin_ref":"plugin:golden","source_ref":"source:golden","expected_source_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","scope":"workspace:default","enabled":false,"trust":"untrusted"}]}"#).unwrap();
        assert_eq!(
            patch.digest().unwrap().0,
            "sha256:772e8bf2256a51ef35884b18bfab7645750600e1a658a2e7a31513285e732454"
        );
    }
}
