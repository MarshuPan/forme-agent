//! Settings protocol negotiation does not change inner object or disk schemas.
use super::*;

pub const PRODUCT_SETTINGS_SCHEMA_VERSION: SchemaVersion = SchemaVersion(2);

pub fn validate_product_settings_schema(version: SchemaVersion) -> Result<()> {
    if matches!(version, SchemaVersion(1 | 2)) {
        Ok(())
    } else {
        Err(Error("unsupported product settings schema".into()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductSettingFeature {
    Models,
    Skills,
    Mcp,
    Plugins,
    Packages,
    Channels,
    Proxy,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductSettingState {
    Configurable,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductSettingUnavailableReason {
    ManagedConsumerMissing,
    NetworkPathsMissing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductSettingAvailability {
    pub schema_version: SchemaVersion,
    pub feature: ProductSettingFeature,
    pub state: ProductSettingState,
    pub reason: Option<ProductSettingUnavailableReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigAvailability {
    pub schema_version: SchemaVersion,
    pub features: Vec<ProductSettingAvailability>,
}

/// Settings v2 is a public Web shape. It deliberately replaces the
/// path-bearing internal network object with host-managed material status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigViewV2 {
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigPatchV2 {
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
}

impl ProductConfigPatchV2 {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_SETTINGS_SCHEMA_VERSION {
            return Err(Error("product settings patch version is invalid".into()));
        }
        ProductConfigPatch {
            schema_version: self.schema_version,
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
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigResponseV2 {
    pub schema_version: SchemaVersion,
    pub view: ProductConfigViewV2,
    pub applied: bool,
    pub operation_id: Option<String>,
}

impl ProductConfigAvailability {
    /// Describe real managed consumers, never executable authority or health.
    pub fn managed(network_paths: bool) -> Self {
        use ProductSettingFeature as Feature;
        use ProductSettingUnavailableReason as Reason;
        Self {
            schema_version: SchemaVersion(1),
            features: [
                Feature::Models,
                Feature::Skills,
                Feature::Mcp,
                Feature::Plugins,
                Feature::Packages,
                Feature::Channels,
                Feature::Proxy,
                Feature::Network,
            ]
            .into_iter()
            .map(|feature| {
                let reason = match feature {
                    Feature::Models | Feature::Skills | Feature::Proxy => None,
                    Feature::Network if network_paths => None,
                    Feature::Network => Some(Reason::NetworkPathsMissing),
                    _ => Some(Reason::ManagedConsumerMissing),
                };
                ProductSettingAvailability {
                    schema_version: SchemaVersion(1),
                    feature,
                    state: if reason.is_none() {
                        ProductSettingState::Configurable
                    } else {
                        ProductSettingState::Unavailable
                    },
                    reason,
                }
            })
            .collect(),
        }
    }
}

impl ProductConfigView {
    pub fn with_settings_schema(
        mut self,
        version: SchemaVersion,
        network_paths: bool,
    ) -> Result<Self> {
        validate_product_settings_schema(version)?;
        self.schema_version = version;
        self.availability = (version == PRODUCT_SETTINGS_SCHEMA_VERSION)
            .then(|| ProductConfigAvailability::managed(network_paths));
        Ok(self)
    }

    pub fn into_settings_v2(
        self,
        network: ProductNetworkSettingsView,
        proxy: ProductProxySettingsView,
        proxy_source: ProductPreferenceSource,
        availability: ProductConfigAvailability,
    ) -> ProductConfigViewV2 {
        ProductConfigViewV2 {
            schema_version: PRODUCT_SETTINGS_SCHEMA_VERSION,
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
            network,
            network_active: self.network_active,
            network_source: self.network_source,
            proxy,
            proxy_source,
            availability,
            issue_refs: self.issue_refs,
        }
    }
}

impl ProductConfigResponse {
    pub fn with_settings_schema(
        mut self,
        version: SchemaVersion,
        network_paths: bool,
    ) -> Result<Self> {
        self.view = self.view.with_settings_schema(version, network_paths)?;
        self.schema_version = version;
        Ok(self)
    }
}
