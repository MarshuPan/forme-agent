//! Explicit managed metadata discovery. Hashes bind data; they never confer trust.
use crate::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io};

const MAX_CATALOG_BYTES: u64 = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpDiscoverySpec {
    pub schema_version: SchemaVersion,
    pub server: McpServerRef,
    pub program_ref: String,
    pub registration_digest: SchemaDigest,
    pub settings_digest: SchemaDigest,
    pub timeout: DurationMs,
    pub output_budget_bytes: u64,
}

impl ManagedMcpDiscoverySpec {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        validate_product_id(&self.server.0)?;
        validate_product_id(&self.program_ref)?;
        sha256_digest_bytes(&self.registration_digest)?;
        sha256_digest_bytes(&self.settings_digest)?;
        if !(1..=60_000).contains(&self.timeout.0)
            || !(1..=MAX_CATALOG_BYTES).contains(&self.output_budget_bytes)
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// Static semantics only. The authenticated owner ingress, exact approval,
    /// live registration and isolated Host admission must also be verified.
    pub fn validate_intent(&self, intent: &ActionIntent) -> Result<()> {
        self.validate()?;
        if intent.schema_version != SchemaVersion(1)
            || intent.backend_hint != BackendKind::Mcp
            || intent.action_type != ActionType::Execute
            || intent.risk_hint != Risk::High
            || intent.expected_effect != ExpectedEffect::Outward
            || intent.source != Source::OwnerControl
            || intent.capability_ref.0 != format!("mcp-discovery:{}", self.server.0)
            || !matches!(&intent.parameters, ActionParameters::McpDiscovery(spec) if spec == self)
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// Compare the complete current settings, never just the server name.
    pub fn validate_settings(
        &self,
        settings: &ProductMcpServerConfig,
        scope: &Scope,
    ) -> Result<()> {
        self.validate()?;
        settings.validate()?;
        if !settings.enabled
            || settings.trust != ProductSkillTrust::OwnerInput
            || settings.scope != *scope
            || settings.server_ref != self.server.0
            || settings.program_ref != self.program_ref
            || self.settings_digest != managed_mcp_settings_digest(settings)?
            || self.timeout.0 > settings.timeout.0
            || self.output_budget_bytes > settings.output_budget
        {
            return Err(invalid());
        }
        Ok(())
    }
}

pub fn managed_mcp_settings_digest(settings: &ProductMcpServerConfig) -> Result<SchemaDigest> {
    settings.validate()?;
    canonical_digest(&("forme.managed-mcp.settings.v1", settings))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpDiscoveryToolV1 {
    pub schema_version: SchemaVersion,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub schema_digest: SchemaDigest,
}

impl ManagedMcpDiscoveryToolV1 {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        text(&self.name, 2048, false, false)?;
        text(&self.description, 16_384, true, true)?;
        if !self.input_schema.is_object() {
            return Err(invalid());
        }
        bounded_json(&self.input_schema, 65_536)?;
        if self.schema_digest != canonical_digest(&self.input_schema)? {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpDiscoveryResourceV1 {
    pub schema_version: SchemaVersion,
    pub uri: String,
    pub name: String,
    pub description: String,
    pub mime_type: Option<String>,
}

impl ManagedMcpDiscoveryResourceV1 {
    pub fn validate(&self) -> Result<()> {
        version(self.schema_version)?;
        text(&self.uri, 2048, false, false)?;
        text(&self.name, 2048, false, false)?;
        text(&self.description, 16_384, true, true)?;
        if let Some(mime) = &self.mime_type {
            text(mime, 256, false, false)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedMcpDiscoveryCatalogV1 {
    pub schema_version: SchemaVersion,
    pub spec: ManagedMcpDiscoverySpec,
    pub tools: Vec<ManagedMcpDiscoveryToolV1>,
    pub resources: Vec<ManagedMcpDiscoveryResourceV1>,
    pub catalog_digest: SchemaDigest,
}

impl ManagedMcpDiscoveryCatalogV1 {
    /// Decode only a complete bounded body. This does not verify the outer
    /// WorkerResultManifest or give the metadata permission to execute.
    pub fn from_slice(bytes: &[u8], expected: &ManagedMcpDiscoverySpec) -> Result<Self> {
        expected.validate()?;
        if bytes.len() as u64 > expected.output_budget_bytes {
            return Err(invalid());
        }
        let catalog: Self = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        catalog.validate_for(expected)?;
        Ok(catalog)
    }

    pub fn validate_for(&self, expected: &ManagedMcpDiscoverySpec) -> Result<()> {
        expected.validate()?;
        version(self.schema_version)?;
        if &self.spec != expected || self.tools.len() > 128 || self.resources.len() > 128 {
            return Err(invalid());
        }
        bounded_json(self, expected.output_budget_bytes)?;
        let mut names = BTreeSet::new();
        for tool in &self.tools {
            tool.validate()?;
            if !names.insert(&tool.name) {
                return Err(invalid());
            }
        }
        let mut uris = BTreeSet::new();
        for resource in &self.resources {
            resource.validate()?;
            if !uris.insert(&resource.uri) {
                return Err(invalid());
            }
        }
        if self.catalog_digest != self.computed_digest()? {
            return Err(invalid());
        }
        Ok(())
    }

    /// A digest is integrity evidence only. Discovery is always UntrustedData.
    pub fn trust(&self) -> TrustTier {
        TrustTier::Untrusted
    }

    pub fn computed_digest(&self) -> Result<SchemaDigest> {
        self.spec.validate()?;
        let material = (
            "forme.managed-mcp.discovery.catalog.v1",
            &self.spec,
            &self.tools,
            &self.resources,
        );
        bounded_json(&material, MAX_CATALOG_BYTES)?;
        canonical_digest(&material)
    }
}

fn invalid() -> Error {
    Error("managed MCP discovery data is invalid, stale or outside its bounds".into())
}
fn version(value: SchemaVersion) -> Result<()> {
    if value != SchemaVersion(1) {
        return Err(invalid());
    }
    Ok(())
}
fn text(value: &str, limit: usize, empty: bool, multiline: bool) -> Result<()> {
    if (!empty && value.trim().is_empty())
        || value.len() > limit
        || value
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
    {
        return Err(invalid());
    }
    Ok(())
}

// Count before allocating a canonical serialization of untrusted JSON.
fn bounded_json(value: &impl Serialize, maximum: u64) -> Result<()> {
    struct Counter(u64);
    impl io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self.0.checked_sub(bytes.len() as u64).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "discovery output limit exceeded",
                )
            })?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(maximum), value).map_err(|_| invalid())
}
