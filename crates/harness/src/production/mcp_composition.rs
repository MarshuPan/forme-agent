//! Catalog availability is separate from the ability to plan first discovery.
use super::*;

pub(super) struct PreparedMcp {
    pub active: BTreeSet<p::ProviderId>,
    pub tools: Vec<forme_capabilities::McpTool>,
    pub needs_worker: bool,
}

pub(super) fn prepare(
    capability_config: &CapabilityRuntimeConfig,
    mcp: &StdioMcpRegistry,
    unresolved_mcp: &BTreeSet<p::ProviderId>,
    managed_mode: bool,
    service_states: &mut Vec<RuntimeServiceState>,
) -> p::Result<PreparedMcp> {
    let mut active_mcp = BTreeSet::new();
    let mut needs_worker = false;
    let mut discovered_mcp = Vec::new();
    for entry in &capability_config.mcp {
        let id = entry.server.provider_id.clone();
        if unresolved_mcp.contains(&id) {
            service_states.push(RuntimeServiceState::unavailable(
                format!("mcp:{}", id.0),
                RuntimeServiceStatus::Unsupported,
                "MCP server environment is unavailable",
            ));
            continue;
        }
        mcp.configure(id.clone())?;
        if !entry.enabled {
            service_states.push(RuntimeServiceState::unavailable(
                format!("mcp:{}", id.0),
                RuntimeServiceStatus::Disabled,
                "MCP server disabled by explicit configuration",
            ));
            continue;
        }
        let (Some(trust), Some(grant)) = (entry.trust, entry.grant.clone()) else {
            service_states.push(RuntimeServiceState::unavailable(
                format!("mcp:{}", id.0),
                RuntimeServiceStatus::Disabled,
                "MCP server has no explicit trust and grant binding",
            ));
            continue;
        };
        if (managed_mode && trust != p::TrustTier::OwnerInput)
            || grant.budget != capability_config.envelope_budget
        {
            service_states.push(RuntimeServiceState::unavailable(
                format!("mcp:{}", id.0),
                RuntimeServiceStatus::Unsupported,
                "MCP trust or grant budget does not match the production capability envelope",
            ));
            continue;
        }
        if mcp.enable(id.clone()).is_err()
            || mcp.bind_trust(id.clone(), trust, p::Actor::Owner).is_err()
            || mcp.grant(id.clone(), grant).is_err()
        {
            let _ = mcp.disable(id.clone());
            service_states.push(RuntimeServiceState::unavailable(
                format!("mcp:{}", id.0),
                RuntimeServiceStatus::Unsupported,
                "MCP server lifecycle binding is invalid",
            ));
            continue;
        }
        needs_worker = true;
        if managed_mode {
            service_states.push(RuntimeServiceState::unavailable(
                format!("mcp:{}", id.0),
                RuntimeServiceStatus::Disabled,
                "managed MCP tools are selected from verified originals for each new run",
            ));
            continue;
        }
        if entry.discover_on_start {
            match mcp.discover_detailed(id.clone()) {
                Ok(discovery) => {
                    active_mcp.insert(id.clone());
                    discovered_mcp.extend(discovery.tools);
                    if !managed_mode {
                        service_states.push(RuntimeServiceState::ready(format!("mcp:{}", id.0)));
                    }
                }
                Err(_) => {
                    if !managed_mode {
                        let _ = mcp.disable(id.clone());
                    }
                    service_states.push(RuntimeServiceState::unavailable(
                        format!("mcp:{}", id.0),
                        RuntimeServiceStatus::Unsupported,
                        if managed_mode {
                            "managed MCP catalog requires an explicitly approved discovery"
                        } else {
                            "MCP discovery is unavailable"
                        },
                    ));
                }
            }
        } else {
            active_mcp.insert(id.clone());
            if !managed_mode {
                service_states.push(RuntimeServiceState::ready(format!("mcp:{}", id.0)));
            }
        }
    }
    Ok(PreparedMcp {
        active: active_mcp,
        tools: discovered_mcp,
        needs_worker,
    })
}

#[cfg(test)]
mod tests;
