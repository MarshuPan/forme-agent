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
    SchemaVersion, WorkspaceRef,
};

pub const PRODUCT_CONFIG_SCHEMA_VERSION: SchemaVersion = SchemaVersion(1);

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

impl ProductConfigTemplate {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION
            || self.core_bind.port() == 0
            || !self.core_bind.ip().is_loopback()
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
        Ok(())
    }
}

impl ProductConfigPatch {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != PRODUCT_CONFIG_SCHEMA_VERSION
            || self.expected_revision > 1_000_000_000
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
