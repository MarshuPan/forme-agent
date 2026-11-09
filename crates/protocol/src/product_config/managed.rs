//! Host-managed capability choices are configuration, never execution authority.
use super::*;
use std::collections::BTreeSet;

const MAX_ITEMS: usize = 64;

fn invalid() -> Error {
    Error("managed capability configuration is invalid or unbounded".into())
}
fn version(version: SchemaVersion) -> Result<()> {
    if version == SchemaVersion(1) {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn bounded<T: Serialize>(value: &T, maximum: usize) -> Result<()> {
    if serde_json::to_vec(value).map_err(|_| invalid())?.len() > maximum {
        return Err(invalid());
    }
    Ok(())
}
fn refs<'a>(values: impl Iterator<Item = &'a str>, maximum: usize) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_product_id(value)?;
        if !seen.insert(value) || seen.len() > maximum {
            return Err(invalid());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMcpAllowlist {
    pub schema_version: SchemaVersion,
    pub tools: Vec<String>,
    pub resources: Vec<String>,
}
impl ProductMcpAllowlist {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        for list in [&self.tools, &self.resources] {
            if list.len() > 128 {
                return Err(invalid());
            }
            let mut seen = BTreeSet::new();
            for value in list {
                if value.trim().is_empty()
                    || value.len() > 2048
                    || value.contains('*')
                    || value.chars().any(char::is_control)
                    || !seen.insert(value)
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMcpServerConfig {
    pub schema_version: SchemaVersion,
    pub server_ref: String,
    pub program_ref: String,
    pub scope: crate::Scope,
    pub allowlist: ProductMcpAllowlist,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub trust: ProductSkillTrust,
    pub timeout: DurationMs,
    pub output_budget: u64,
}
impl ProductMcpServerConfig {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.server_ref)?;
        validate_product_id(&self.program_ref)?;
        validate_product_id(&self.scope.0)?;
        self.allowlist.validate()?;
        if !(1..=60_000).contains(&self.timeout.0) || !(1..=1_048_576).contains(&self.output_budget)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPluginConfig {
    pub schema_version: SchemaVersion,
    pub plugin_ref: String,
    pub source_ref: String,
    pub expected_source_digest: SchemaDigest,
    pub scope: crate::Scope,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub trust: ProductSkillTrust,
}
impl ProductPluginConfig {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.plugin_ref)?;
        validate_product_id(&self.source_ref)?;
        validate_product_id(&self.scope.0)?;
        crate::sha256_digest_bytes(&self.expected_source_digest)?;
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPublisherKeyPatch {
    pub schema_version: SchemaVersion,
    pub publisher_ref: String,
    pub public_key_digest: SchemaDigest,
    pub public_key: [u8; 32],
}
impl std::fmt::Debug for ProductPublisherKeyPatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductPublisherKeyPatch")
            .field("publisher_ref", &self.publisher_ref)
            .field("public_key_digest", &self.public_key_digest)
            .finish_non_exhaustive()
    }
}
impl ProductPublisherKeyPatch {
    /// Cryptographic point/weak-key validation is performed by the Host before writing.
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.publisher_ref)?;
        if crate::sha256_content_digest(&self.public_key) != self.public_key_digest {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn view(&self) -> ProductPublisherKeyView {
        ProductPublisherKeyView {
            schema_version: self.schema_version,
            publisher_ref: self.publisher_ref.clone(),
            public_key_digest: self.public_key_digest.clone(),
            configured: true,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductPublisherKeyView {
    pub schema_version: SchemaVersion,
    pub publisher_ref: String,
    pub public_key_digest: SchemaDigest,
    pub configured: bool,
}
impl ProductPublisherKeyView {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.publisher_ref)?;
        crate::sha256_digest_bytes(&self.public_key_digest)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductComputerPlacementSettings {
    pub schema_version: SchemaVersion,
    #[serde(default)]
    pub enabled: bool,
    pub scope: crate::Scope,
    pub surface_refs: Vec<String>,
}
impl Default for ProductComputerPlacementSettings {
    fn default() -> Self {
        Self {
            schema_version: SchemaVersion(1),
            enabled: false,
            scope: crate::Scope("workspace:default".into()),
            surface_refs: Vec::new(),
        }
    }
}
impl ProductComputerPlacementSettings {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.scope.0)?;
        refs(self.surface_refs.iter().map(String::as_str), MAX_ITEMS)?;
        if self.enabled && self.surface_refs.is_empty() {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductManagedCapabilitySettings {
    pub schema_version: SchemaVersion,
    pub mcp_servers: Vec<ProductMcpServerConfig>,
    pub plugins: Vec<ProductPluginConfig>,
    pub publisher_keys: Vec<ProductPublisherKeyView>,
    pub computer: ProductComputerPlacementSettings,
}
impl Default for ProductManagedCapabilitySettings {
    fn default() -> Self {
        Self {
            schema_version: SchemaVersion(1),
            mcp_servers: Vec::new(),
            plugins: Vec::new(),
            publisher_keys: Vec::new(),
            computer: Default::default(),
        }
    }
}
impl ProductManagedCapabilitySettings {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        refs(
            self.mcp_servers.iter().map(|v| v.server_ref.as_str()),
            MAX_ITEMS,
        )?;
        refs(
            self.plugins.iter().map(|v| v.plugin_ref.as_str()),
            MAX_ITEMS,
        )?;
        refs(
            self.publisher_keys.iter().map(|v| v.publisher_ref.as_str()),
            MAX_ITEMS,
        )?;
        for server in &self.mcp_servers {
            server.validate()?;
        }
        for plugin in &self.plugins {
            plugin.validate()?;
        }
        for key in &self.publisher_keys {
            key.validate()?;
        }
        self.computer.validate()?;
        bounded(self, 65_536)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductManagedCapabilityPatch {
    pub schema_version: SchemaVersion,
    pub mcp_servers: Vec<ProductMcpServerConfig>,
    pub plugins: Vec<ProductPluginConfig>,
    pub publisher_keys: Vec<ProductPublisherKeyPatch>,
    pub computer: ProductComputerPlacementSettings,
}
impl ProductManagedCapabilityPatch {
    pub fn validate(&self) -> Result<()> {
        self.view().validate()?;
        for key in &self.publisher_keys {
            key.validate()?;
        }
        bounded(self, 65_536)
    }
    pub fn view(&self) -> ProductManagedCapabilitySettings {
        ProductManagedCapabilitySettings {
            schema_version: self.schema_version,
            mcp_servers: self.mcp_servers.clone(),
            plugins: self.plugins.clone(),
            publisher_keys: self
                .publisher_keys
                .iter()
                .map(ProductPublisherKeyPatch::view)
                .collect(),
            computer: self.computer.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductChannelAdapter {
    Text,
    Loopback,
    LocalDevice,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelConfig {
    pub schema_version: SchemaVersion,
    pub channel_ref: String,
    pub adapter_ref: ProductChannelAdapter,
    pub connector_ref: String,
    pub scope: crate::Scope,
    #[serde(default)]
    pub enabled: bool,
    pub participant_binding: String,
    pub credential_ref: Option<String>,
}
impl ProductChannelConfig {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        for value in [
            &self.channel_ref,
            &self.connector_ref,
            &self.scope.0,
            &self.participant_binding,
        ]
        .into_iter()
        .chain(self.credential_ref.iter())
        {
            validate_product_id(value)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductChannelSettings {
    pub schema_version: SchemaVersion,
    pub channels: Vec<ProductChannelConfig>,
}
impl Default for ProductChannelSettings {
    fn default() -> Self {
        Self {
            schema_version: SchemaVersion(1),
            channels: Vec::new(),
        }
    }
}
impl ProductChannelSettings {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        refs(
            self.channels.iter().map(|v| v.channel_ref.as_str()),
            MAX_ITEMS,
        )?;
        for channel in &self.channels {
            channel.validate()?;
        }
        bounded(self, 16_384)
    }
}
