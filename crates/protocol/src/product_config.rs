//! Versioned installed-product configuration contracts.
//!
//! These DTOs cross the Web-to-host configuration boundary. They intentionally
//! contain only the fields required to assemble the managed core; secret values
//! are accepted only in a short-lived patch and are never returned in a view.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

use crate::{
    validate_product_id, AgentProfileRef, DurationMs, Error, OwnerPrincipalRef, Result,
    SchemaDigest, SchemaVersion, WorkspaceRef,
};

mod proxy;
pub use proxy::*;

pub const PRODUCT_CONFIG_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
pub const PRODUCT_DATA_BINDING_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);
pub const PRODUCT_PROXY_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

fn default_web_bind() -> SocketAddr {
    "127.0.0.1:8765".parse().expect("valid default Web bind")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigTemplate {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub core_config_path: PathBuf,
    pub configuration_ack_path: PathBuf,
    pub model_credentials_directory: PathBuf,
    pub brain_store_path: PathBuf,
    pub artifact_root: PathBuf,
    pub core_owner_credential_path: PathBuf,
    pub core_web_credential_path: PathBuf,
    pub core_web_assertion_verifying_key: [u8; 32],
    pub core_session_fence_path: PathBuf,
    pub core_bind: SocketAddr,
    #[serde(default = "default_web_bind")]
    pub web_bind: SocketAddr,
    #[serde(default)]
    pub web_network_config_path: Option<PathBuf>,
    #[serde(default)]
    pub web_material_directory: Option<PathBuf>,
    pub workspace: WorkspaceRef,
    pub agent_profile: AgentProfileRef,
    pub scheduler_tick: DurationMs,
    pub scheduler_lease: DurationMs,
    pub scheduler_max_claims: u32,
    pub cognition_tick: DurationMs,
    pub stable_evidence_max_age: DurationMs,
    pub authority_lease: DurationMs,
    pub drain_timeout: DurationMs,
}

/// Host-owned binding for a verified restored brain. It is deliberately not
/// accepted by the ordinary browser model patch; only the local restore
/// transaction may persist it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductDataBinding {
    pub schema_version: SchemaVersion,
    pub candidate_ref: String,
    pub store_path: PathBuf,
    pub content_key_directory: PathBuf,
    pub owner_digest: SchemaDigest,
    pub store_identity: SchemaDigest,
    pub manifest_digest: SchemaDigest,
    pub execution_fence_digest: Option<SchemaDigest>,
}

impl ProductDataBinding {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_DATA_BINDING_SCHEMA_VERSION
            || self.candidate_ref.trim().is_empty()
            || self.candidate_ref.len() > 256
            || !self.store_path.is_absolute()
            || !self.content_key_directory.is_absolute()
            || self.store_path.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
            || self.content_key_directory.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(Error("product data binding is invalid".into()));
        }
        for digest in [
            &self.owner_digest,
            &self.store_identity,
            &self.manifest_digest,
        ]
        .into_iter()
        .chain(self.execution_fence_digest.iter())
        {
            crate::sha256_digest_bytes(digest)?;
        }
        Ok(())
    }
}

impl ProductConfigTemplate {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION
            || self.core_bind.port() == 0
            || !self.core_bind.ip().is_loopback()
            || self.web_bind.port() == 0
            || !self.web_bind.ip().is_loopback()
            || self.scheduler_tick.0 == 0
            || self.scheduler_lease.0 == 0
            || self.scheduler_max_claims == 0
            || self.cognition_tick.0 == 0
            || self.stable_evidence_max_age.0 == 0
            || self.authority_lease.0 == 0
            || self.drain_timeout.0 == 0
        {
            return Err(Error("product configuration template is invalid".into()));
        }
        validate_product_id(&self.install_ref)?;
        if self.workspace.0.trim().is_empty() || self.agent_profile.0.trim().is_empty() {
            return Err(Error(
                "product configuration template references are incomplete".into(),
            ));
        }
        for path in [
            &self.core_config_path,
            &self.configuration_ack_path,
            &self.model_credentials_directory,
            &self.brain_store_path,
            &self.artifact_root,
            &self.core_owner_credential_path,
            &self.core_web_credential_path,
            &self.core_session_fence_path,
        ] {
            if !path.is_absolute() || path.as_os_str().is_empty() {
                return Err(Error(
                    "product configuration template path is invalid".into(),
                ));
            }
        }
        for path in self
            .web_network_config_path
            .iter()
            .chain(self.web_material_directory.iter())
        {
            if !path.is_absolute() || path.as_os_str().is_empty() {
                return Err(Error("product Web network path is invalid".into()));
            }
        }
        if self.web_network_config_path.is_some() != self.web_material_directory.is_some() {
            return Err(Error(
                "product Web network paths must be configured together".into(),
            ));
        }
        if matches!(
            (&self.web_network_config_path, &self.web_material_directory),
            (Some(network), Some(material)) if network == material
        ) {
            return Err(Error("product Web network paths overlap".into()));
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductModelProfilePatch {
    pub schema_version: SchemaVersion,
    pub id: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub credential: Option<String>,
    pub context_window: u32,
    pub tool_use: bool,
    pub strength: String,
    pub input_microunits_per_million: u64,
    pub output_microunits_per_million: u64,
    pub requests_per_minute: u32,
    pub tokens_per_minute: u64,
    pub timeout_ms: u64,
    pub fallback_group: Option<String>,
    pub default: bool,
    pub tool_schema_versions: Vec<u32>,
}

impl ProductModelProfilePatch {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION
            || self.context_window == 0
            || self.requests_per_minute == 0
            || self.tokens_per_minute == 0
            || self.timeout_ms == 0
            || !matches!(self.strength.as_str(), "basic" | "standard" | "strong")
            || self.tool_schema_versions.contains(&0)
        {
            return Err(Error("product model profile limits are invalid".into()));
        }
        for (value, name) in [
            (&self.id, "model id"),
            (&self.provider, "model provider"),
            (&self.model, "model name"),
            (&self.base_url, "model base URL"),
        ] {
            if value.trim().is_empty() || value.len() > 2048 {
                return Err(Error(format!("product {name} is invalid")));
            }
        }
        validate_product_id(&self.id)?;
        if let Some(credential) = &self.credential {
            if credential.trim().is_empty()
                || credential.contains(['\r', '\n'])
                || credential.len() > 16 * 1024
            {
                return Err(Error("product model credential is invalid".into()));
            }
        }
        let mut versions = self.tool_schema_versions.clone();
        versions.sort_unstable();
        versions.dedup();
        if versions.len() != self.tool_schema_versions.len() {
            return Err(Error(
                "product model tool schema versions are duplicated".into(),
            ));
        }
        Ok(())
    }
}

impl std::fmt::Debug for ProductModelProfilePatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductModelProfilePatch")
            .field("schema_version", &self.schema_version)
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("credential", &"[redacted]")
            .field("context_window", &self.context_window)
            .field("tool_use", &self.tool_use)
            .field("strength", &self.strength)
            .field(
                "input_microunits_per_million",
                &self.input_microunits_per_million,
            )
            .field(
                "output_microunits_per_million",
                &self.output_microunits_per_million,
            )
            .field("requests_per_minute", &self.requests_per_minute)
            .field("tokens_per_minute", &self.tokens_per_minute)
            .field("timeout_ms", &self.timeout_ms)
            .field("fallback_group", &self.fallback_group)
            .field("default", &self.default)
            .field("tool_schema_versions", &self.tool_schema_versions)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductModelProfileView {
    pub schema_version: SchemaVersion,
    pub id: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub context_window: u32,
    pub tool_use: bool,
    pub strength: String,
    pub input_microunits_per_million: u64,
    pub output_microunits_per_million: u64,
    pub requests_per_minute: u32,
    pub tokens_per_minute: u64,
    pub timeout_ms: u64,
    pub fallback_group: Option<String>,
    pub default: bool,
    pub tool_schema_versions: Vec<u32>,
    pub credential_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigView {
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
    #[serde(default)]
    pub capabilities: ProductCapabilitySettings,
    #[serde(default)]
    pub capabilities_source: ProductPreferenceSource,
    pub network: ProductNetworkConfig,
    pub network_active: bool,
    pub network_source: ProductPreferenceSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<ProductConfigAvailability>,
    pub issue_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigPatch {
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
    pub network: Option<ProductNetworkConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigOwnerBinding {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
}

impl ProductConfigOwnerBinding {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION
            || self.expected_revision > 1_000_000_000
        {
            return Err(Error(
                "product configuration owner binding is invalid".into(),
            ));
        }
        validate_product_id(&self.operation_id)?;
        self.owner.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigAcknowledgement {
    pub schema_version: SchemaVersion,
    pub install_ref: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_identity: Option<ProductDataAcknowledgement>,
}

impl ProductConfigAcknowledgement {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION
            || self.revision == 0
            || self.install_ref.trim().is_empty()
        {
            return Err(Error(
                "product configuration acknowledgement is invalid".into(),
            ));
        }
        validate_product_id(&self.install_ref)?;
        if let Some(identity) = &self.data_identity {
            identity.validate()?;
        }
        Ok(())
    }
}

impl ProductConfigPatch {
    pub fn validate(&self) -> Result<()> {
        validate_product_settings_schema(self.schema_version)?;
        if self.expected_revision > 1_000_000_000
            || self.models.is_empty()
            || self.models.len() > 32
        {
            return Err(Error("product configuration patch is invalid".into()));
        }
        validate_product_id(&self.operation_id)?;
        self.owner.validate()?;
        let mut ids = std::collections::BTreeSet::new();
        let defaults = self.models.iter().filter(|model| model.default).count();
        if defaults != 1 {
            return Err(Error(
                "product configuration needs exactly one default model".into(),
            ));
        }
        for model in &self.models {
            model.validate()?;
            if !ids.insert(model.id.clone()) {
                return Err(Error(
                    "product configuration contains duplicate model ids".into(),
                ));
            }
        }
        if let Some(brain) = &self.brain {
            brain.validate(
                &self
                    .models
                    .iter()
                    .map(|model| model.id.clone())
                    .collect::<Vec<_>>(),
            )?;
        }
        if let Some(capabilities) = &self.capabilities {
            capabilities.validate()?;
        }
        if let Some(network) = &self.network {
            network.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigResponse {
    pub schema_version: SchemaVersion,
    pub view: ProductConfigView,
    pub applied: bool,
    pub operation_id: Option<String>,
}

/// Static validation deliberately does not assert provider availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductModelTestStatus {
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigValidationReport {
    pub schema_version: SchemaVersion,
    pub revision: u64,
    pub valid: bool,
    pub restart_required: bool,
    pub network_validation_digest: SchemaDigest,
    pub model_test_status: ProductModelTestStatus,
    pub issue_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductConfigValidationRequest {
    pub schema_version: SchemaVersion,
    pub revision: u64,
    pub owner: OwnerPrincipalRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductCoreAction {
    Start,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCoreActivation {
    pub schema_version: SchemaVersion,
    pub operation_id: String,
    pub expected_revision: u64,
    pub owner: OwnerPrincipalRef,
    pub action: ProductCoreAction,
    pub requested_deadline: crate::Timestamp,
}

impl ProductCoreActivation {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION || self.expected_revision == 0 {
            return Err(Error("product core activation is invalid".into()));
        }
        validate_product_id(&self.operation_id)?;
        self.owner.validate()
    }

    pub fn digest(&self) -> Result<crate::SchemaDigest> {
        self.validate()?;
        crate::canonical_digest(&("forme-product-core-activation-v1", self))
    }
}

/// OS operation completion and core configuration acknowledgement are separate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCoreActivationResponse {
    pub schema_version: SchemaVersion,
    pub operation: crate::MaintenanceOperation,
    pub configuration: ProductConfigView,
    pub service: crate::NativeServiceStatus,
}
mod maintenance;
pub use maintenance::*;
mod readiness;
pub use readiness::*;
mod admission;
pub use admission::*;
mod preferences;
pub use preferences::*;
mod capabilities;
pub use capabilities::*;
mod network;
pub use network::*;
mod health;
pub use health::*;
mod settings_schema;
pub use settings_schema::*;
