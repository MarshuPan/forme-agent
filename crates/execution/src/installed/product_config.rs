//! Host-owned, recoverable configuration transactions. Never a brain writer.
#[cfg(test)]
mod activation_tests;
mod journal;
#[cfg(test)]
mod legacy_journal_tests;
#[cfg(test)]
mod maintenance_tests;
mod materialize;
mod network;
#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod preference_tests;
mod restore;
mod service_files;
#[cfg(test)]
mod skill_tests;
mod storage;
#[cfg(test)]
pub(crate) mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use journal::{ConfigJournal, Intent, SnapshotRef};
use storage::invalid;

const MAX_CONFIG_BYTES: u64 = 256 * 1024;
const MAX_STATE_BYTES: u64 = 128 * 1024;

pub struct ProductConfigStore {
    template: p::ProductConfigTemplate,
    state: ConfigState,
    journal: ConfigJournal,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigState {
    schema_version: p::SchemaVersion,
    install_ref: String,
    owner: Option<p::OwnerPrincipalRef>,
    revision: u64,
    models: Vec<p::ProductModelProfileView>,
    credentials: BTreeMap<String, PathBuf>,
    #[serde(default)]
    active_data: Option<p::ProductDataBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brain: Option<p::ProductBrainPreferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capabilities: Option<p::ProductCapabilitySettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    network: Option<p::ProductNetworkConfig>,
}

impl ConfigState {
    fn empty(install: &str) -> Self {
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
        }
    }

    fn validate(&self, template: &p::ProductConfigTemplate) -> p::Result<()> {
        if !matches!(
            self.schema_version,
            p::SchemaVersion(2)
                | p::SchemaVersion(3)
                | p::SchemaVersion(4)
                | p::SchemaVersion(5)
                | p::SchemaVersion(6)
        ) || self.install_ref != template.install_ref
            || (self.schema_version == p::SchemaVersion(2) && self.active_data.is_some())
            || (self.schema_version.0 < 4 && self.brain.is_some())
            || (self.schema_version.0 < 5 && self.capabilities.is_some())
            || (self.schema_version.0 == 5 && self.capabilities.is_none())
            || (self.schema_version.0 < 6 && self.network.is_some())
            || (self.schema_version == p::SchemaVersion(6) && self.network.is_none())
            || self.revision > 1_000_000_000
            || self.models.len() != self.credentials.len()
            || (self.revision == 0) != self.models.is_empty()
            || (self.revision > 0 && self.owner.is_none())
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

impl ProductConfigStore {
    /// Own the OS writer lock until drop. Corrupt or unbound state never becomes
    /// an empty configuration; pending publication resumes only its original intent.
    pub fn open(template_path: &Path) -> p::Result<Mutex<Self>> {
        let template = storage::json::<p::ProductConfigTemplate>(template_path, MAX_CONFIG_BYTES)?;
        materialize::validate_template(&template, template_path)?;
        let journal_path = template.core_config_path.with_extension("journal.jsonl");
        if !journal_path.try_exists().map_err(|_| invalid())?
            && template
                .core_config_path
                .with_extension("revisions")
                .try_exists()
                .map_err(|_| invalid())?
        {
            // A missing journal cannot erase a previously committed owner,
            // including the bound-but-not-yet-configured revision zero.
            return Err(invalid());
        }
        let journal = ConfigJournal::open(&journal_path, &template.install_ref)?;
        let mut store = Self {
            state: ConfigState::empty(&template.install_ref),
            template,
            journal,
        };
        if let Some(reference) = store.journal.current() {
            store.state = store.load_state(reference)?;
        } else if store.journal.pending().is_none() {
            store.import_legacy()?;
        }
        store.recover()?;
        store.verify_published()?;
        Ok(Mutex::new(store))
    }

    pub fn snapshot(&mut self) -> p::Result<p::ProductConfigView> {
        self.recover()?;
        self.verify_published()?;
        self.sync_acknowledgement()?;
        let acknowledged = self.journal.acknowledged();
        let network = self
            .state
            .network
            .clone()
            .unwrap_or_else(|| p::ProductNetworkConfig::loopback(self.template.web_bind));
        let network_active = network == self.active_network()?;
        Ok(p::ProductConfigView {
            schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
            install_ref: self.template.install_ref.clone(),
            owner: self.state.owner.clone(),
            active_revision: self.state.revision,
            acknowledged_revision: acknowledged,
            pending_restart: (self.state.revision > 0 && acknowledged != Some(self.state.revision))
                || !network_active,
            configured: self.state.revision > 0,
            models: self.state.models.clone(),
            brain: self
                .state
                .brain
                .clone()
                .unwrap_or_else(|| p::ProductBrainPreferences {
                    schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
                    default_workspace: self.template.workspace.clone(),
                    model_routing: Default::default(),
                }),
            brain_source: if self.state.brain.is_some() {
                p::ProductPreferenceSource::ManagedConfiguration
            } else {
                p::ProductPreferenceSource::InstallationTemplate
            },
            issue_refs: Vec::new(),
            capabilities: self.state.capabilities.clone().unwrap_or_default(),
            capabilities_source: if self.state.capabilities.is_some() {
                p::ProductPreferenceSource::ManagedConfiguration
            } else {
                p::ProductPreferenceSource::InstallationTemplate
            },
            network,
            network_active,
            network_source: if self.state.network.is_some() {
                p::ProductPreferenceSource::ManagedConfiguration
            } else {
                p::ProductPreferenceSource::InstallationTemplate
            },
        })
    }

    /// Operation queries remain available when a core acknowledgement is bad.
    pub(crate) fn maintenance_owner(&mut self) -> p::Result<Option<p::OwnerPrincipalRef>> {
        self.recover()?;
        self.verify_published()?;
        Ok(self.state.owner.clone())
    }

    pub fn bind_owner(
        &mut self,
        binding: p::ProductConfigOwnerBinding,
    ) -> p::Result<p::ProductConfigResponse> {
        binding.validate()?;
        self.recover()?;
        let digest = p::canonical_digest(&(
            &binding.schema_version,
            &binding.operation_id,
            &binding.expected_revision,
            &binding.owner,
        ))?;
        if self.journal.existing(&binding.operation_id, &digest)? {
            return self.response(binding.operation_id, false);
        }
        self.check_revision(binding.expected_revision, &binding.owner, false)?;
        let mut state = self.state.clone();
        state.owner = Some(binding.owner);
        self.commit_state(binding.operation_id.clone(), digest, state)?;
        self.response(binding.operation_id, true)
    }

    /// Validate every profile and credential before writing staging files. Keys
    /// have immutable revision paths, including when a saved key is retained.
    pub fn apply(&mut self, patch: p::ProductConfigPatch) -> p::Result<p::ProductConfigResponse> {
        patch.validate().map_err(|_| rejected_patch())?;
        self.recover()?;
        let digest = patch.digest()?;
        if self.journal.existing(&patch.operation_id, &digest)? {
            return self.response(patch.operation_id, false);
        }
        self.check_revision(patch.expected_revision, &patch.owner, true)?;
        if !self.network_is_managed()
            && patch.network.as_ref().is_some_and(|network| {
                *network != p::ProductNetworkConfig::loopback(self.template.web_bind)
            })
        {
            return Err(rejected_patch());
        }
        let brain = patch.brain.or_else(|| self.state.brain.clone());
        let capabilities = patch
            .capabilities
            .or_else(|| self.state.capabilities.clone());
        let network = patch.network.or_else(|| self.state.network.clone());
        if let Some(brain) = &brain {
            brain
                .validate(
                    &patch
                        .models
                        .iter()
                        .map(|model| model.id.clone())
                        .collect::<Vec<_>>(),
                )
                .map_err(|_| rejected_patch())?;
        }
        let prepared = materialize::prepare_models(&patch.models, &self.state.credentials)
            .map_err(|_| rejected_patch())?;
        let revision = self
            .state
            .revision
            .checked_add(1)
            .filter(|value| *value <= 1_000_000_000)
            .ok_or_else(invalid)?;
        let id = materialize::new_ref()?;
        let credentials = materialize::credential_paths(&self.template, &id, &prepared)?;
        let active_network = self.active_network()?;
        let state = ConfigState {
            schema_version: p::SchemaVersion(if network.is_some() {
                6
            } else if capabilities.is_some() {
                5
            } else if brain.is_some() {
                4
            } else {
                3
            }),
            install_ref: self.template.install_ref.clone(),
            owner: Some(patch.owner),
            revision,
            models: prepared.iter().map(|(view, _)| view.clone()).collect(),
            credentials,
            active_data: self.state.active_data.clone(),
            brain,
            capabilities,
            network,
        };
        state
            .validate(&self.template)
            .map_err(|_| rejected_patch())?;
        materialize::validate_size(&self.template, &state).map_err(|_| rejected_patch())?;
        materialize::write_credentials(&self.template, &id, &prepared)?;
        self.commit_state_with_network(patch.operation_id.clone(), digest, state, active_network)?;
        self.response(patch.operation_id, true)
    }

    pub fn validate_revision(
        &mut self,
        revision: u64,
        owner: &p::OwnerPrincipalRef,
    ) -> p::Result<p::ProductConfigValidationReport> {
        self.recover()?;
        self.check_revision(revision, owner, true)?;
        if revision == 0 {
            return Err(p::Error("model configuration is not saved".into()));
        }
        self.verify_published()?;
        for path in self.state.credentials.values() {
            storage::secret(path)?;
        }
        let network = self
            .state
            .network
            .clone()
            .unwrap_or_else(|| p::ProductNetworkConfig::loopback(self.template.web_bind));
        Ok(p::ProductConfigValidationReport {
            schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
            revision,
            valid: true,
            restart_required: self.journal.acknowledged() != Some(revision)
                || network != self.active_network()?,
            network_validation_digest: network::validation_digest(&network)?,
            model_test_status: p::ProductModelTestStatus::NotRun,
            issue_refs: Vec::new(),
        })
    }

    pub fn acknowledge(
        &mut self,
        acknowledgement: p::ProductConfigAcknowledgement,
    ) -> p::Result<()> {
        acknowledgement.validate()?;
        if acknowledgement.install_ref != self.template.install_ref
            || acknowledgement.revision != self.state.revision
        {
            return Err(p::Error(
                "core acknowledged an unknown configuration revision".into(),
            ));
        }
        if let Some(binding) = &self.state.active_data {
            let identity = acknowledgement.data_identity.as_ref().ok_or_else(invalid)?;
            if identity.owner_digest != binding.owner_digest
                || identity.store_identity != binding.store_identity
                || identity.execution_fence_digest != binding.execution_fence_digest
            {
                return Err(invalid());
            }
        }
        if self.journal.acknowledged() != Some(acknowledgement.revision) {
            self.journal.acknowledge(acknowledgement.revision)?;
        }
        Ok(())
    }

    fn check_revision(
        &self,
        revision: u64,
        owner: &p::OwnerPrincipalRef,
        require_owner: bool,
    ) -> p::Result<()> {
        if revision != self.state.revision {
            return Err(p::Error(
                "configuration revision is stale; refresh before saving".into(),
            ));
        }
        if self
            .state
            .owner
            .as_ref()
            .is_some_and(|bound| bound != owner)
            || (require_owner && self.state.owner.is_none())
        {
            return Err(p::Error(
                "configuration owner does not match the installed owner".into(),
            ));
        }
        Ok(())
    }

    fn commit_state(
        &mut self,
        operation: String,
        digest: p::SchemaDigest,
        state: ConfigState,
    ) -> p::Result<()> {
        let network = self.active_network()?;
        self.commit_state_with_network(operation, digest, state, network)
    }

    fn commit_state_with_network(
        &mut self,
        operation: String,
        digest: p::SchemaDigest,
        state: ConfigState,
        network: p::ProductNetworkConfig,
    ) -> p::Result<()> {
        state.validate(&self.template)?;
        let reference = materialize::stage_with_network(&self.template, &state, &network)?;
        self.journal.prepare(Intent {
            operation,
            digest,
            snapshot: reference,
        })?;
        self.recover()
    }

    fn recover(&mut self) -> p::Result<()> {
        let Some(intent) = self.journal.pending().cloned() else {
            return Ok(());
        };
        let state = self.load_state(&intent.snapshot)?;
        if self.state.owner.is_some() && state.owner != self.state.owner {
            return Err(invalid());
        }
        if intent.snapshot.core_digest.is_some() {
            let bytes = materialize::core_bytes(&self.template, &intent.snapshot)?;
            let actual: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if actual != materialize::core_config(&self.template, &state)? {
                return Err(invalid());
            }
            for path in state.credentials.values() {
                storage::secret(path)?;
            }
            storage::publish(&self.template.core_config_path, &bytes)?;
        }
        if intent.snapshot.web_network_digest.is_some() {
            let bytes = materialize::web_network_bytes(&self.template, &intent.snapshot)?;
            let path = self
                .template
                .web_network_config_path
                .as_ref()
                .ok_or_else(invalid)?;
            storage::publish(path, &bytes)?;
        }
        self.journal.commit(intent.operation)?;
        self.state = state;
        Ok(())
    }

    fn load_state(&self, reference: &SnapshotRef) -> p::Result<ConfigState> {
        let bytes = storage::read(
            &materialize::revision_directory(&self.template, &reference.id).join("state.json"),
            MAX_STATE_BYTES,
        )?;
        if materialize::bytes_digest(&bytes) != reference.state_digest {
            return Err(invalid());
        }
        let state: ConfigState = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        state.validate(&self.template)?;
        if state.revision != reference.revision {
            return Err(invalid());
        }
        Ok(state)
    }

    fn verify_published(&self) -> p::Result<()> {
        self.state.validate(&self.template)?;
        if let Some(reference) = self
            .journal
            .current()
            .filter(|value| value.core_digest.is_some())
        {
            let bytes = storage::read(&self.template.core_config_path, MAX_CONFIG_BYTES)?;
            if Some(materialize::bytes_digest(&bytes)) != reference.core_digest {
                return Err(invalid());
            }
            let expected = materialize::core_config(&self.template, &self.state)?;
            let actual: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if actual != expected {
                return Err(invalid());
            }
        } else if self.template.core_config_path.exists() {
            return Err(invalid());
        }
        if let Some(reference) = self
            .journal
            .current()
            .filter(|value| value.web_network_digest.is_some())
        {
            let path = self
                .template
                .web_network_config_path
                .as_ref()
                .ok_or_else(invalid)?;
            let bytes = storage::read(path, MAX_CONFIG_BYTES)?;
            if Some(materialize::bytes_digest(&bytes)) != reference.web_network_digest
                || bytes != materialize::web_network_bytes(&self.template, reference)?
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    fn sync_acknowledgement(&mut self) -> p::Result<()> {
        if !self
            .template
            .configuration_ack_path
            .try_exists()
            .map_err(|_| invalid())?
        {
            return Ok(());
        }
        let ack = storage::json::<p::ProductConfigAcknowledgement>(
            &self.template.configuration_ack_path,
            4096,
        )?;
        ack.validate()?;
        if ack.install_ref != self.template.install_ref || ack.revision > self.state.revision {
            return Err(invalid());
        }
        if ack.revision == self.state.revision {
            self.acknowledge(ack)?;
        }
        Ok(())
    }

    fn response(
        &mut self,
        operation: String,
        applied: bool,
    ) -> p::Result<p::ProductConfigResponse> {
        Ok(p::ProductConfigResponse {
            schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
            view: self.snapshot()?,
            applied,
            operation_id: Some(operation),
        })
    }

    fn import_legacy(&mut self) -> p::Result<()> {
        materialize::import_legacy(self)
    }
}

fn rejected_patch() -> p::Error {
    // Only failures before credential staging or this command's durable intent
    // may be exposed as definite input rejections by the product transport.
    p::Error("configuration patch rejected before publication".into())
}
