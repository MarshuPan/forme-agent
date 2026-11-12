//! Translate Host-owned MCP settings without accepting process parameters.
use super::*;
use crate::McpRuntimeConfig;
use forme_capabilities::{McpAllowlist, StdioMcpServer};

impl ManagedBrainConfig {
    pub(super) fn configure_mcp(
        &self,
        capabilities: &mut CapabilityRuntimeConfig,
    ) -> p::Result<()> {
        if let Some(binding) = &self.mcp_worker_binding {
            binding.validate()?;
            if binding.sandbox_profile_ref != p::MANAGED_MCP_PROGRAM_PROFILE
                || binding.backend_kinds != std::collections::BTreeSet::from([p::BackendKind::Mcp])
                || self.worker_binding.as_ref().is_some_and(|file| {
                    file.endpoint != binding.endpoint
                        || file.authority_epoch != binding.authority_epoch
                        || file.credential_path != binding.credential_path
                })
            {
                return Err(invalid());
            }
        }
        let Some(managed) = &self.managed_capabilities else {
            return Ok(());
        };
        managed.validate()?;
        for settings in &managed.mcp_servers {
            let declared = p::CapabilitySet {
                schema_version: p::SchemaVersion(1),
                capabilities: settings
                    .allowlist
                    .tools
                    .iter()
                    .map(|name| p::CapabilityRef(format!("mcp:{}:{name}", settings.server_ref)))
                    .collect(),
                permissions: vec![p::PermissionRef("execute".into())],
            };
            let eligible = settings.enabled && settings.trust == p::ProductSkillTrust::OwnerInput;
            if eligible {
                capabilities.output_budget_bytes =
                    capabilities.output_budget_bytes.max(settings.output_budget);
                capabilities.timeout.0 = capabilities.timeout.0.max(settings.timeout.0);
            }
            capabilities.mcp.push(McpRuntimeConfig {
                server: StdioMcpServer {
                    schema_version: p::SchemaVersion(1),
                    provider_id: p::ProviderId(settings.server_ref.clone()),
                    command: settings.program_ref.clone(),
                    args: Vec::new(),
                    env: Vec::new(),
                    timeout: settings.timeout,
                    declared_capabilities: declared.clone(),
                    allowlist: McpAllowlist {
                        schema_version: p::SchemaVersion(1),
                        tools: settings.allowlist.tools.clone(),
                        resources: settings.allowlist.resources.clone(),
                    },
                },
                inherit_env: Vec::new(),
                enabled: settings.enabled,
                discover_on_start: true,
                trust: Some(if eligible {
                    p::TrustTier::OwnerInput
                } else {
                    p::TrustTier::Untrusted
                }),
                grant: eligible.then(|| p::AutonomyEnvelope {
                    schema_version: p::SchemaVersion(1),
                    scope: settings.scope.clone(),
                    capability: declared,
                    action_type: vec![p::ActionType::Execute],
                    risk_limit: p::Risk::High,
                    approval_rule: p::ApprovalRule::Ask,
                    budget: capabilities.envelope_budget.clone(),
                    timebox: p::Timebox {
                        schema_version: p::SchemaVersion(1),
                        starts_at: 0,
                        expires_at: i64::MAX,
                        max_turns: 1,
                    },
                    rollback: p::RollbackReq {
                        schema_version: p::SchemaVersion(1),
                        required: false,
                        boundary: None,
                    },
                }),
            });
        }
        Ok(())
    }
}
