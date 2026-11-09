//! Independent settings v3; legacy validators and serialized shapes stay unchanged.
use super::*;

pub const PRODUCT_MANAGED_SETTINGS_SCHEMA_VERSION: SchemaVersion = SchemaVersion(3);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigViewV3 {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub owner: Option<OwnerPrincipalRef>,
    pub active_revision: u64,
    pub acknowledged_revision: Option<u64>,
    pub pending_restart: bool,
    pub configured: bool,
    pub models: Vec<ProductModelProfileView>,
    pub brain: ProductBrainPreferences,
    pub brain_source: ProductPreferenceSource,
    pub capabilities: ProductCapabilitySettings,
    pub capabilities_source: ProductPreferenceSource,
    pub network: ProductNetworkSettingsView,
    pub network_active: bool,
    pub network_source: ProductPreferenceSource,
    pub proxy: ProductProxySettingsView,
    pub proxy_source: ProductPreferenceSource,
    pub availability: ProductConfigAvailability,
    pub issue_refs: Vec<String>,
    pub managed_capabilities: ProductManagedCapabilitySettings,
    pub channels: ProductChannelSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCoreActivationResponseV3 {
    pub schema_version: SchemaVersion,
    pub operation: crate::MaintenanceOperation,
    pub configuration: ProductConfigViewV3,
    pub service: crate::NativeServiceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigPatchV3 {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub models: Vec<ProductModelProfilePatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain: Option<ProductBrainPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ProductCapabilitySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<ProductNetworkSettingsPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProductProxyConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_capabilities: Option<ProductManagedCapabilityPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<ProductChannelSettings>,
}

impl ProductConfigPatchV3 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_MANAGED_SETTINGS_SCHEMA_VERSION {
            return Err(Error("product settings patch version is invalid".into()));
        }
        ProductConfigPatch {
            schema_version: PRODUCT_SETTINGS_SCHEMA_VERSION,
            operation_id: self.operation_id.clone(),
            expected_revision: self.expected_revision,
            owner: self.owner.clone(),
            models: self.models.clone(),
            brain: self.brain.clone(),
            capabilities: self.capabilities.clone(),
            network: None,
        }
        .validate()?;
        if let Some(network) = &self.network {
            network.validate()?;
        }
        if let Some(proxy) = &self.proxy {
            proxy.validate()?;
        }
        if let Some(managed) = &self.managed_capabilities {
            managed.validate()?;
        }
        if let Some(channels) = &self.channels {
            channels.validate()?;
        }
        if serde_json::to_vec(self)
            .map_err(|_| Error("settings cannot be encoded".into()))?
            .len()
            > 256 * 1024
        {
            return Err(Error("managed settings exceed request bound".into()));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme.product.settings.v3", self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigResponseV3 {
    pub schema_version: SchemaVersion,
    pub view: ProductConfigViewV3,
    pub applied: bool,
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_activation: Option<ProductCoreActivationResponseV3>,
}

impl ProductConfigViewV2 {
    pub fn into_settings_v3(
        self,
        managed_capabilities: ProductManagedCapabilitySettings,
        channels: ProductChannelSettings,
    ) -> ProductConfigViewV3 {
        ProductConfigViewV3 {
            schema_version: PRODUCT_MANAGED_SETTINGS_SCHEMA_VERSION,
            install_ref: self.install_ref,
            owner: self.owner,
            active_revision: self.active_revision,
            acknowledged_revision: self.acknowledged_revision,
            pending_restart: self.pending_restart,
            configured: self.configured,
            models: self.models,
            brain: self.brain,
            brain_source: self.brain_source,
            capabilities: self.capabilities,
            capabilities_source: self.capabilities_source,
            network: self.network,
            network_active: self.network_active,
            network_source: self.network_source,
            proxy: self.proxy,
            proxy_source: self.proxy_source,
            availability: self.availability,
            issue_refs: self.issue_refs,
            managed_capabilities,
            channels,
        }
    }
}
