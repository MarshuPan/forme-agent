//! Managed catalogs are read from verified Core originals, never local stdio.
use super::*;
use std::sync::Arc;

/// Implemented by Core. Each read must verify the original completed action,
/// protected material, current settings/registration and deletion fences.
/// Reading must not launch, rediscover, retry, or publish brain facts.
pub trait ManagedMcpCatalogSource: Send + Sync {
    fn read_catalog(
        &self,
        server: &p::McpServerRef,
    ) -> p::Result<(p::ProductMcpServerConfig, p::ManagedMcpDiscoveryCatalogV1)>;
}

pub(super) enum CatalogMode {
    Development,
    Managed(Option<Arc<dyn ManagedMcpCatalogSource>>),
}

impl StdioMcpRegistry {
    /// A missing source keeps managed metadata unavailable. This constructor
    /// cannot fall back to the development stdio transport, even after enable.
    pub fn with_managed_servers(
        servers: Vec<StdioMcpServer>,
        source: Option<Arc<dyn ManagedMcpCatalogSource>>,
    ) -> p::Result<Self> {
        let mut registry = Self::with_servers(servers)?;
        registry.catalog_mode = CatalogMode::Managed(source);
        Ok(registry)
    }

    pub(super) fn managed_catalog(
        &self,
        server: &StdioMcpServer,
    ) -> p::Result<(McpCatalog, p::SchemaDigest)> {
        let CatalogMode::Managed(Some(source)) = &self.catalog_mode else {
            return Err(unavailable());
        };
        let (settings, catalog) =
            source.read_catalog(&p::McpServerRef(server.provider_id.0.clone()))?;
        let record = self.providers.record(&server.provider_id)?;
        let grant = record.grant.as_ref().ok_or_else(unavailable)?;
        if !matches!(record.state, p::ProviderLifecycle::Trusted(tier) if tier != p::TrustTier::Untrusted)
        {
            return Err(unavailable());
        }
        catalog.spec.validate_settings(&settings, &grant.scope)?;
        catalog.validate_for(&catalog.spec)?;
        if server.schema_version != p::SchemaVersion(1)
            || server.provider_id.0 != settings.server_ref
            || server.command != settings.program_ref
            || !server.args.is_empty()
            || !server.env.is_empty()
            || server.timeout != settings.timeout
            || server.allowlist.schema_version != p::SchemaVersion(1)
            || server.allowlist.tools != settings.allowlist.tools
            || server.allowlist.resources != settings.allowlist.resources
            || catalog.spec.program_ref
                != format!("mcp-program:{}", &catalog.spec.registration_digest.0[7..])
        {
            return Err(unavailable());
        }
        let digest = catalog.catalog_digest.clone();
        let mut schemas = BTreeMap::new();
        let tools = catalog
            .tools
            .into_iter()
            .filter(|tool| settings.allowlist.tools.contains(&tool.name))
            .map(|tool| {
                let reference = p::McpToolRef(format!("{}/{}", settings.server_ref, tool.name));
                schemas.insert(reference.clone(), tool.input_schema);
                McpTool {
                    schema_version: p::SchemaVersion(1),
                    capability_ref: p::CapabilityRef(format!(
                        "mcp:{}:{}",
                        settings.server_ref, tool.name
                    )),
                    tool_ref: reference,
                    server: server.provider_id.clone(),
                    name: tool.name,
                    summary: tool.description,
                    input_schema: None,
                    schema_digest: None,
                }
            })
            .collect();
        let resources = catalog
            .resources
            .into_iter()
            .filter(|resource| settings.allowlist.resources.contains(&resource.uri))
            .map(|resource| McpResource {
                schema_version: p::SchemaVersion(1),
                server: server.provider_id.clone(),
                uri: resource.uri,
                name: Some(resource.name),
            })
            .collect();
        Ok((
            McpCatalog {
                discovery: McpDiscovery {
                    schema_version: p::SchemaVersion(1),
                    server: server.provider_id.clone(),
                    tools,
                    resources,
                },
                schemas,
            },
            digest,
        ))
    }

    pub(super) fn require_current_catalog(&self, server: &p::ProviderId) -> p::Result<()> {
        if matches!(self.catalog_mode, CatalogMode::Development) {
            return Ok(());
        }
        let verified = self
            .providers
            .record(server)
            .and_then(|record| self.managed_catalog(&record.provider));
        let mut state = self.lock_state()?;
        if let Ok((_, digest)) = verified {
            if state.managed_digests.get(server) == Some(&digest) {
                return Ok(());
            }
        }
        remove_catalog(&mut state, server);
        Err(unavailable())
    }

    pub(super) fn require_current_tool(&self, reference: &p::McpToolRef) -> p::Result<()> {
        if matches!(self.catalog_mode, CatalogMode::Development) {
            return Ok(());
        }
        let server = self
            .lock_state()?
            .tools
            .get(reference)
            .map(|tool| tool.server.clone())
            .ok_or_else(unavailable)?;
        self.require_current_catalog(&server)
    }

    pub(super) fn require_managed_call(
        &self,
        intent: &p::ActionIntent,
        provider: &StdioMcpServer,
        scope: &p::Scope,
    ) -> p::Result<()> {
        if matches!(self.catalog_mode, CatalogMode::Development) {
            return Ok(());
        }
        let p::ActionParameters::Mcp {
            transport,
            stdio,
            timeout,
            ..
        } = &intent.parameters
        else {
            return Err(unavailable());
        };
        if *transport != p::McpTransport::Stdio
            || stdio.schema_version != p::SchemaVersion(1)
            || stdio.command != provider.command
            || !stdio.args.is_empty()
            || !stdio.env.is_empty()
            || timeout.0 == 0
            || timeout.0 > provider.timeout.0
            || intent.scope != *scope
            || intent.risk_hint != p::Risk::High
            || intent.expected_effect != p::ExpectedEffect::Outward
        {
            return Err(unavailable());
        }
        Ok(())
    }

    pub(super) fn catalog_access(&self) -> p::Result<Option<MutexGuard<'_, ()>>> {
        if matches!(self.catalog_mode, CatalogMode::Development) {
            return Ok(None);
        }
        self.catalog_access
            .lock()
            .map(Some)
            .map_err(|_| unavailable())
    }
}

pub(super) fn remove_catalog(state: &mut McpState, server: &p::ProviderId) {
    state.discoveries.remove(server);
    state.managed_digests.remove(server);
    state.tools.retain(|_, tool| tool.server != *server);
    let retained = state.tools.keys().cloned().collect::<BTreeSet<_>>();
    state
        .deferred_schemas
        .retain(|tool, _| retained.contains(tool));
}
fn unavailable() -> p::Error {
    p::Error("managed MCP catalog is absent, changed or unavailable".into())
}
