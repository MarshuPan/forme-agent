//! Reuse the installed execution endpoint with the dedicated MCP isolation profile.
use super::*;
use crate::installed::{ManagedWorkerBinding, ManagedWorkerEndpoint};
use std::collections::BTreeSet;

pub(super) fn validate(
    binding: &ManagedWorkerBinding,
    template: &p::ProductConfigTemplate,
) -> p::Result<()> {
    binding.validate()?;
    let digest = p::canonical_digest(&("forme-agent-execution-pipe-v1", &template.install_ref))?;
    if binding.endpoint
        != (ManagedWorkerEndpoint::WindowsNamedPipe {
            name: format!(r"\\.\pipe\forme-agent-execution-v1-{}", &digest.0[7..]),
        })
        || binding.authority_epoch != p::AuthorityEpoch(1)
        || binding.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
        || binding.backend_kinds != BTreeSet::from([p::BackendKind::Mcp])
        || binding.credential_path
            != template
                .model_credentials_directory
                .parent()
                .ok_or_else(invalid)?
                .join("agent-execution.token")
    {
        return Err(invalid());
    }
    Ok(())
}
impl ProductConfigStore {
    pub(super) fn mcp_placement(&self) -> p::Result<ManagedWorkerBinding> {
        if let Some(binding) = &self.state.mcp_worker {
            validate(binding, &self.template)?;
            return Ok(binding.clone());
        }
        let file = self
            .state
            .worker
            .as_ref()
            .or(self.installed_file.as_ref())
            .ok_or_else(invalid)?;
        file.validate_template(&self.template)?;
        let mut projection = serde_json::json!({});
        file.materialize(&mut projection)?;
        let mut binding: ManagedWorkerBinding =
            serde_json::from_value(projection["worker_binding"].clone()).map_err(|_| invalid())?;
        binding.backend_kinds = BTreeSet::from([p::BackendKind::Mcp]);
        binding.sandbox_profile_ref = p::MANAGED_MCP_PROGRAM_PROFILE.into();
        validate(&binding, &self.template)?;
        Ok(binding)
    }
}
