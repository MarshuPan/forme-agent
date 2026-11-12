//! Channel edits retain other capability, key and channel configuration.
use super::*;

impl ProductConfigStore {
    pub fn channel_settings(&mut self) -> p::Result<p::ProductChannelSettingsView> {
        let current = self.snapshot()?;
        let sources = self.channel_sources();
        let sources_available = sources.is_ok();
        Ok(p::ProductChannelSettingsView {
            schema_version: p::SchemaVersion(1),
            owner: current.owner,
            active_revision: current.active_revision,
            pending_restart: current.pending_restart,
            configured: current.configured,
            channels: self
                .state
                .channels
                .as_ref()
                .map(|m| m.channels.clone())
                .unwrap_or_default(),
            sources: sources.unwrap_or_default(),
            sources_available,
        })
    }
    pub fn apply_channel_settings(
        &mut self,
        patch: p::ProductChannelSettingsPatch,
    ) -> p::Result<p::ProductChannelSettingsResponse> {
        patch.validate().map_err(|_| rejected_patch())?;
        self.recover()?;
        self.verify_published()?;
        let digest = patch.digest()?;
        if self.journal.existing(&patch.operation_id, &digest)? {
            return self.channel_response(patch.operation_id, false);
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
        state.channels = Some(p::ProductChannelSettings {
            schema_version: p::SchemaVersion(1),
            channels: patch.channels,
        });
        channel_sources::prepare(
            &self.template,
            &mut state,
            self.installed_channel.upgrade().as_deref(),
        )?;
        state
            .validate(&self.template)
            .map_err(|_| rejected_patch())?;
        materialize::validate_size(&self.template, &state).map_err(|_| rejected_patch())?;
        self.commit_state(patch.operation_id.clone(), digest, state)?;
        self.channel_response(patch.operation_id, true)
    }
    pub fn channel_settings_operation(
        &mut self,
        operation: &str,
    ) -> p::Result<p::ProductChannelSettingsOperation> {
        p::validate_product_id(operation)?;
        self.verify_published()?;
        Ok(p::ProductChannelSettingsOperation {
            schema_version: p::SchemaVersion(1),
            owner: self.state.owner.clone().ok_or_else(invalid)?,
            operation_id: operation.into(),
            request_digest: self.journal.committed_digest(operation)?,
            committed: true,
        })
    }
    fn channel_response(
        &mut self,
        operation_id: String,
        applied: bool,
    ) -> p::Result<p::ProductChannelSettingsResponse> {
        Ok(p::ProductChannelSettingsResponse {
            schema_version: p::SchemaVersion(1),
            operation_id,
            applied,
            view: self.channel_settings()?,
        })
    }
}
