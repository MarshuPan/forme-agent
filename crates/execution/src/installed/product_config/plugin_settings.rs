//! Plugin edits retain other capability, key and channel configuration.
use super::*;

impl ProductConfigStore {
    pub fn plugin_settings(&mut self) -> p::Result<p::ProductPluginSettingsView> {
        let current = self.snapshot()?;
        let sources = self.plugin_sources();
        let sources_available = sources.is_ok();
        Ok(p::ProductPluginSettingsView {
            schema_version: p::SchemaVersion(1),
            owner: current.owner,
            active_revision: current.active_revision,
            pending_restart: current.pending_restart,
            configured: current.configured,
            plugins: self
                .state
                .managed_capabilities
                .as_ref()
                .map(|m| m.plugins.clone())
                .unwrap_or_default(),
            sources: sources.unwrap_or_default(),
            sources_available,
        })
    }
    pub fn apply_plugin_settings(
        &mut self,
        patch: p::ProductPluginSettingsPatch,
    ) -> p::Result<p::ProductPluginSettingsResponse> {
        patch.validate().map_err(|_| rejected_patch())?;
        self.recover()?;
        self.verify_published()?;
        let digest = patch.digest()?;
        if self.journal.existing(&patch.operation_id, &digest)? {
            return self.plugin_response(patch.operation_id, false);
        }
        self.check_revision(patch.expected_revision, &patch.owner, true)?;
        if self.state.revision == 0 || self.state.models.is_empty() {
            return Err(rejected_patch());
        }
        let mut state = self.state.clone();
        state.revision = state
            .revision
            .checked_add(1)
            .filter(|v| *v <= 1_000_000_000)
            .ok_or_else(rejected_patch)?;
        state.schema_version = p::SchemaVersion(state.schema_version.0.max(10));
        state
            .managed_capabilities
            .get_or_insert_with(Default::default)
            .plugins = patch.plugins;
        plugins::prepare(&self.template, &mut state)?;
        state
            .validate(&self.template)
            .map_err(|_| rejected_patch())?;
        materialize::validate_size(&self.template, &state).map_err(|_| rejected_patch())?;
        self.commit_state(patch.operation_id.clone(), digest, state)?;
        self.plugin_response(patch.operation_id, true)
    }
    pub fn plugin_settings_operation(
        &mut self,
        operation: &str,
    ) -> p::Result<p::ProductPluginSettingsOperation> {
        p::validate_product_id(operation)?;
        self.verify_published()?;
        Ok(p::ProductPluginSettingsOperation {
            schema_version: p::SchemaVersion(1),
            owner: self.state.owner.clone().ok_or_else(invalid)?,
            operation_id: operation.into(),
            request_digest: self.journal.committed_digest(operation)?,
            committed: true,
        })
    }
    fn plugin_response(
        &mut self,
        operation_id: String,
        applied: bool,
    ) -> p::Result<p::ProductPluginSettingsResponse> {
        Ok(p::ProductPluginSettingsResponse {
            schema_version: p::SchemaVersion(1),
            operation_id,
            applied,
            view: self.plugin_settings()?,
        })
    }
}
