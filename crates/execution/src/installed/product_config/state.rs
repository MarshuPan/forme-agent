//! Immutable private configuration state and compatibility validation.
use std::collections::BTreeMap;
use std::path::PathBuf;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use super::{materialize, release, storage};
use storage::invalid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfigState {
    pub(super) schema_version: p::SchemaVersion,
    pub(super) install_ref: String,
    pub(super) owner: Option<p::OwnerPrincipalRef>,
    pub(super) revision: u64,
    pub(super) models: Vec<p::ProductModelProfileView>,
    pub(super) credentials: BTreeMap<String, PathBuf>,
    #[serde(default)]
    pub(super) active_data: Option<p::ProductDataBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) brain: Option<p::ProductBrainPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) capabilities: Option<p::ProductCapabilitySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) network: Option<p::ProductNetworkConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) proxy: Option<p::ProductProxyConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) release: Option<p::ProductReleaseBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) worker: Option<super::super::InstalledFileProvisioning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) desktop: Option<super::super::InstalledDesktopProvisioning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) mcp_worker: Option<super::super::ManagedWorkerBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) managed_capabilities: Option<p::ProductManagedCapabilitySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) channels: Option<p::ProductChannelSettings>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) publisher_key_files: BTreeMap<String, PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) plugin_sources: Vec<p::ProductPluginSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) channel_sources: Vec<p::ProductChannelSource>,
}

impl ConfigState {
    pub(super) fn empty(install: &str) -> Self {
        Self {
            schema_version: p::SchemaVersion(3),
            install_ref: install.to_owned(),
            owner: None,
            revision: 0,
            models: Vec::new(),
            credentials: BTreeMap::new(),
            active_data: None,
            brain: None,
            capabilities: None,
            network: None,
            proxy: None,
            release: None,
            worker: None,
            desktop: None,
            mcp_worker: None,
            managed_capabilities: None,
            channels: None,
            publisher_key_files: BTreeMap::new(),
            plugin_sources: Vec::new(),
            channel_sources: Vec::new(),
        }
    }

    pub(super) fn validate(&self, template: &p::ProductConfigTemplate) -> p::Result<()> {
        if !matches!(
            self.schema_version,
            p::SchemaVersion(2)
                | p::SchemaVersion(3)
                | p::SchemaVersion(4)
                | p::SchemaVersion(5)
                | p::SchemaVersion(6)
                | p::SchemaVersion(7)
                | p::SchemaVersion(8)
                | p::SchemaVersion(9)
                | p::SchemaVersion(10)
                | p::SchemaVersion(11)
                | p::SchemaVersion(12)
                | p::SchemaVersion(13)
                | p::SchemaVersion(14)
        ) || (self.schema_version.0 < 9 && self.worker.is_some())
            || (self.schema_version == p::SchemaVersion(9) && self.worker.is_none())
            || (self.schema_version.0 < 11 && self.desktop.is_some())
            || (self.schema_version.0 == 11 && self.desktop.is_none())
            || (self.schema_version.0 < 12 && self.mcp_worker.is_some())
            || (self.schema_version.0 == 12 && self.mcp_worker.is_none())
            || self.install_ref != template.install_ref
            || (self.schema_version == p::SchemaVersion(2) && self.active_data.is_some())
            || (self.schema_version.0 < 4 && self.brain.is_some())
            || (self.schema_version.0 < 5 && self.capabilities.is_some())
            || (self.schema_version.0 == 5 && self.capabilities.is_none())
            || (self.schema_version.0 < 6 && self.network.is_some())
            || (self.schema_version == p::SchemaVersion(6) && self.network.is_none())
            || (self.schema_version.0 < 8 && self.proxy.is_some())
            || (self.schema_version == p::SchemaVersion(8) && self.proxy.is_none())
            || self.revision > 1_000_000_000
            || self.models.len() != self.credentials.len()
            || (self.revision == 0) != self.models.is_empty()
            || (self.revision > 0 && self.owner.is_none())
        {
            return Err(invalid());
        }
        super::managed::validate_state(self, template)?;
        super::plugins::validate_state(self)?;
        super::channel_sources::validate_state(self, template)?;
        release::validate_state(self, template)?;
        if let Some(worker) = &self.worker {
            worker.validate_template(template)?;
        }
        if let Some(binding) = &self.mcp_worker {
            super::mcp_placement::validate(binding, template)?;
        }
        if let Some(desktop) = &self.desktop {
            desktop.validate_template(template)?;
            if let Some(managed) = &self.managed_capabilities {
                desktop.validate_settings(&managed.computer)?;
            }
        }
        if self
            .managed_capabilities
            .as_ref()
            .is_some_and(|m| m.computer.enabled)
            && self.desktop.is_none()
        {
            return Err(invalid());
        }
        if let Some(owner) = &self.owner {
            owner.validate()?;
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
            network.validate_material_root(template.web_material_directory.as_deref())?;
        }
        if let Some(proxy) = &self.proxy {
            materialize::validate_proxy(proxy)?;
        }
        if let Some(binding) = &self.active_data {
            binding.validate()?;
            let owner = self.owner.as_ref().ok_or_else(invalid)?;
            if binding.owner_digest != p::sha256_content_digest(owner.0.as_bytes())
                || binding.store_path == template.brain_store_path
                || binding.store_path.starts_with(&template.artifact_root)
                || binding
                    .content_key_directory
                    .starts_with(&template.artifact_root)
                || binding
                    .store_path
                    .starts_with(&template.model_credentials_directory)
                || binding
                    .content_key_directory
                    .starts_with(&template.model_credentials_directory)
                || binding
                    .store_path
                    .starts_with(&binding.content_key_directory)
                || binding
                    .content_key_directory
                    .starts_with(&binding.store_path)
            {
                return Err(invalid());
            }
            let parent = template.brain_store_path.parent().ok_or_else(invalid)?;
            if !binding.store_path.starts_with(parent) {
                return Err(invalid());
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for model in &self.models {
            materialize::validate_model(&materialize::patch_from_view(model))?;
            if !model.credential_configured || !ids.insert(&model.id) {
                return Err(invalid());
            }
            let path = self.credentials.get(&model.id).ok_or_else(invalid)?;
            storage::check(path)?;
            if !path.starts_with(&template.model_credentials_directory) {
                return Err(invalid());
            }
        }
        if !self.models.is_empty() && self.models.iter().filter(|model| model.default).count() != 1
        {
            return Err(invalid());
        }
        Ok(())
    }
}
