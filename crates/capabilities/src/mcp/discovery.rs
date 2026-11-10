//! Catalog admission and publication are separate from either transport.
use super::*;

impl StdioMcpRegistry {
    pub fn discover_detailed(&self, id: p::ProviderId) -> Result<McpDiscovery, McpFailure> {
        let map_error =
            |error: p::Error| failure(id.clone(), McpErrorClass::ServerError, error.to_string());
        let _access = self.catalog_access().map_err(map_error)?;
        let record = self.providers.record(&id).map_err(map_error)?;
        if !matches!(record.state, p::ProviderLifecycle::Trusted(tier) if tier != p::TrustTier::Untrusted)
            || record.grant.is_none()
        {
            return Err(failure(
                id,
                McpErrorClass::ServerError,
                "MCP server is not active and granted",
            ));
        }
        let result = match &self.catalog_mode {
            CatalogMode::Development => {
                discover_over_stdio(&record.provider).map(|catalog| (catalog, None))
            }
            CatalogMode::Managed(_) => self
                .managed_catalog(&record.provider)
                .map(|(catalog, digest)| (catalog, Some(digest)))
                .map_err(map_error),
        };
        let mut state = self.lock_state().map_err(map_error)?;
        remove_catalog(&mut state, &id);
        match result {
            Ok((catalog, source_digest)) => {
                let discovery = catalog.discovery;
                for tool in &discovery.tools {
                    state.tools.insert(tool.tool_ref.clone(), tool.clone());
                }
                state.deferred_schemas.extend(catalog.schemas);
                if let Some(digest) = source_digest {
                    // Rehydrating a completed original creates no new brain
                    // discovery fact. Its owner action publishes that fact.
                    state.managed_digests.insert(id, digest);
                } else {
                    state
                        .events
                        .push(p::EventPayload::McpDiscovered(p::McpDiscoveredPayload {
                            server: p::McpServerRef(discovery.server.0.clone()),
                            tools: discovery
                                .tools
                                .iter()
                                .map(|tool| p::ToolRef(tool.name.clone()))
                                .collect(),
                            resources: discovery
                                .resources
                                .iter()
                                .map(|resource| p::ResourceRef(resource.uri.clone()))
                                .collect(),
                        }));
                }
                state
                    .discoveries
                    .insert(discovery.server.clone(), discovery.clone());
                Ok(discovery)
            }
            Err(failure) => {
                state.failures.push(failure.clone());
                Err(failure)
            }
        }
    }
}
