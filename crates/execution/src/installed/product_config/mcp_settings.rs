//! MCP list edits retain all other state in the original config transaction.
use super::*;

impl ProductConfigStore {
    pub(super) fn mcp_consumer(
        &self,
    ) -> p::Result<(
        std::sync::Arc<super::super::InstalledManagedMcpPrograms>,
        String,
    )> {
        let binding = self.mcp_placement()?;
        let credential = super::super::worker_client::read_credential(&binding.credential_path)?;
        let authority =
            super::super::worker_results::installation_ref(&p::canonical_digest(&*credential)?)?;
        let programs = self.installed_mcp.upgrade().ok_or_else(rejected_patch)?;
        programs.require_live_consumer(&authority)?;
        Ok((programs, authority))
    }
    pub(super) fn validate_mcp_servers(
        &self,
        servers: &[p::ProductMcpServerConfig],
    ) -> p::Result<()> {
        for settings in servers.iter().filter(|v| v.enabled) {
            let (programs, authority) = self.mcp_consumer()?;
            programs.validate_enabled_settings(
                &self.template.install_ref,
                self.state.owner.as_ref().ok_or_else(rejected_patch)?,
                &authority,
                settings,
            )?;
        }
        Ok(())
    }
    pub fn mcp_settings(&mut self) -> p::Result<p::ProductMcpSettingsView> {
        let current = self.snapshot()?;
        Ok(p::ProductMcpSettingsView {
            schema_version: p::SchemaVersion(1),
            owner: current.owner,
            active_revision: current.active_revision,
            pending_restart: current.pending_restart,
            configured: current.configured,
            mcp_servers: self
                .state
                .managed_capabilities
                .as_ref()
                .map(|v| v.mcp_servers.clone())
                .unwrap_or_default(),
            available: self.mcp_consumer().is_ok(),
        })
    }
    pub fn apply_mcp_settings(
        &mut self,
        patch: p::ProductMcpSettingsPatch,
    ) -> p::Result<p::ProductMcpSettingsResponse> {
        patch.validate().map_err(|_| rejected_patch())?;
        self.recover()?;
        self.verify_published()?;
        let digest = patch.digest()?;
        if self.journal.existing(&patch.operation_id, &digest)? {
            return self.mcp_response(patch.operation_id, false);
        }
        self.check_revision(patch.expected_revision, &patch.owner, true)?;
        if self.state.models.is_empty() || self.state.revision == 0 {
            return Err(rejected_patch());
        }
        self.validate_mcp_servers(&patch.mcp_servers)?;
        let mut state = self.state.clone();
        state.revision = state
            .revision
            .checked_add(1)
            .filter(|v| *v <= 1_000_000_000)
            .ok_or_else(rejected_patch)?;
        if patch.mcp_servers.iter().any(|s| s.enabled) {
            state.mcp_worker = Some(self.mcp_placement()?);
        }
        state.schema_version =
            p::SchemaVersion(state.schema_version.0.max(if state.mcp_worker.is_some() {
                12
            } else {
                10
            }));
        state
            .managed_capabilities
            .get_or_insert_with(Default::default)
            .mcp_servers = patch.mcp_servers;
        state
            .validate(&self.template)
            .map_err(|_| rejected_patch())?;
        materialize::validate_size(&self.template, &state).map_err(|_| rejected_patch())?;
        self.commit_state(patch.operation_id.clone(), digest, state)?;
        self.mcp_response(patch.operation_id, true)
    }
    pub fn mcp_settings_operation(
        &mut self,
        operation: &str,
    ) -> p::Result<p::ProductMcpSettingsOperation> {
        p::validate_product_id(operation)?;
        self.verify_published()?;
        Ok(p::ProductMcpSettingsOperation {
            schema_version: p::SchemaVersion(1),
            owner: self.state.owner.clone().ok_or_else(invalid)?,
            operation_id: operation.into(),
            request_digest: self.journal.committed_digest(operation)?,
            committed: true,
        })
    }
    fn mcp_response(
        &mut self,
        operation_id: String,
        applied: bool,
    ) -> p::Result<p::ProductMcpSettingsResponse> {
        Ok(p::ProductMcpSettingsResponse {
            schema_version: p::SchemaVersion(1),
            operation_id,
            applied,
            view: self.mcp_settings()?,
        })
    }
}
