//! Single-field owner edit through the existing immutable config transaction.
use super::*;

impl ProductConfigStore {
    pub fn computer_settings(&mut self) -> p::Result<p::ProductComputerSettingsView> {
        let current = self.snapshot()?;
        let placement = self
            .state
            .desktop
            .as_ref()
            .or(self.installed_desktop.as_ref());
        if let Some(placement) = placement {
            placement.validate_template(&self.template)?;
        }
        Ok(p::ProductComputerSettingsView {
            schema_version: p::SchemaVersion(1),
            owner: current.owner,
            active_revision: current.active_revision,
            pending_restart: current.pending_restart,
            configured: current.configured,
            computer: self
                .state
                .managed_capabilities
                .as_ref()
                .map(|value| value.computer.clone())
                .unwrap_or_default(),
            available: placement.is_some(),
            available_scope: placement.map(|value| value.scope().clone()),
            available_surfaces: placement
                .map(|value| value.surface_refs().to_vec())
                .unwrap_or_default(),
        })
    }

    pub fn apply_computer_settings(
        &mut self,
        patch: p::ProductComputerSettingsPatch,
    ) -> p::Result<p::ProductComputerSettingsResponse> {
        patch.validate().map_err(|_| rejected_patch())?;
        self.recover()?;
        self.verify_published()?;
        let digest = patch.digest()?;
        if self.journal.existing(&patch.operation_id, &digest)? {
            return self.computer_response(patch.operation_id, false);
        }
        self.check_revision(patch.expected_revision, &patch.owner, true)?;
        if self.state.models.is_empty() || self.state.revision == 0 {
            return Err(rejected_patch());
        }
        let placement = self
            .state
            .desktop
            .clone()
            .or_else(|| self.installed_desktop.clone());
        if patch.computer.enabled {
            placement
                .as_ref()
                .ok_or_else(rejected_patch)?
                .validate_settings(&patch.computer)
                .map_err(|_| rejected_patch())?;
        }
        let mut state = self.state.clone();
        state.revision = state
            .revision
            .checked_add(1)
            .filter(|value| *value <= 1_000_000_000)
            .ok_or_else(rejected_patch)?;
        state.desktop = placement;
        state.schema_version =
            p::SchemaVersion(state.schema_version.0.max(if state.desktop.is_some() {
                11
            } else {
                10
            }));
        state
            .managed_capabilities
            .get_or_insert_with(Default::default)
            .computer = patch.computer;
        state
            .validate(&self.template)
            .map_err(|_| rejected_patch())?;
        materialize::validate_size(&self.template, &state).map_err(|_| rejected_patch())?;
        self.commit_state(patch.operation_id.clone(), digest, state)?;
        self.computer_response(patch.operation_id, true)
    }
    pub fn computer_settings_operation(
        &mut self,
        operation: &str,
    ) -> p::Result<p::ProductComputerSettingsOperation> {
        self.verify_published()?;
        let request_digest = self.journal.committed_digest(operation)?;
        Ok(p::ProductComputerSettingsOperation {
            schema_version: p::SchemaVersion(1),
            owner: self.state.owner.clone().ok_or_else(invalid)?,
            operation_id: operation.into(),
            request_digest,
            committed: true,
        })
    }
    fn computer_response(
        &mut self,
        operation_id: String,
        applied: bool,
    ) -> p::Result<p::ProductComputerSettingsResponse> {
        Ok(p::ProductComputerSettingsResponse {
            schema_version: p::SchemaVersion(1),
            operation_id,
            applied,
            view: self.computer_settings()?,
        })
    }
}
