//! Shared installed backend instances for execution and desktop admission.
use super::*;

pub(super) struct Bindings<'a> {
    pub worker: Option<&'a ManagedWorkerBinding>,
    pub desktop: Option<&'a ManagedWorkerBinding>,
}

pub(super) fn try_register_managed_worker_backend(
    managed_mode: bool,
    binding: Option<&ManagedWorkerBinding>,
    kind: p::BackendKind,
    config: &ProductionRuntimeConfig,
    budget: &OutputBudget,
    timeout: p::DurationMs,
    backends: &ExecutionBackendRegistry,
) -> p::Result<bool> {
    Ok(register(
        managed_mode,
        binding,
        kind,
        config,
        budget,
        timeout,
        backends,
    )?
    .is_some())
}

pub(super) fn register(
    managed_mode: bool,
    binding: Option<&ManagedWorkerBinding>,
    kind: p::BackendKind,
    config: &ProductionRuntimeConfig,
    budget: &OutputBudget,
    timeout: p::DurationMs,
    backends: &ExecutionBackendRegistry,
) -> p::Result<Option<Arc<WorkerActionBackend>>> {
    if !managed_mode {
        return Ok(None);
    }
    let Some(binding) = binding else {
        return Ok(None);
    };
    if !binding.supports(kind) {
        return Ok(None);
    }
    let owner = p::VerifiedPrincipal(config.owner.0.clone());
    let Ok(backend) =
        WorkerActionBackend::from_binding(kind, binding, owner, budget.clone(), timeout)
    else {
        return Ok(None);
    };
    let backend = Arc::new(backend);
    backends.register(backend.clone())?;
    Ok(Some(backend))
}

pub(super) fn register_computer(
    binding: Option<&ManagedWorkerBinding>,
    config: &ProductionRuntimeConfig,
    budget: &OutputBudget,
    timeout: p::DurationMs,
    backends: &ExecutionBackendRegistry,
    artifacts: Option<Arc<dyn forme_execution::ArtifactStore>>,
) -> p::Result<Option<Arc<WorkerActionBackend>>> {
    let Some(binding) = binding else {
        return Ok(None);
    };
    let Some(artifacts) = artifacts.filter(|value| value.supports_worker_output()) else {
        return Ok(None);
    };
    if !binding.supports(p::BackendKind::Computer) {
        return Ok(None);
    }
    let Ok(backend) = WorkerActionBackend::from_binding(
        p::BackendKind::Computer,
        binding,
        p::VerifiedPrincipal(config.owner.0.clone()),
        budget.clone(),
        timeout,
    ) else {
        return Ok(None);
    };
    let backend = backend.with_artifact_store(artifacts);
    let backend = Arc::new(backend);
    backends.register(backend.clone())?;
    Ok(Some(backend))
}

/// The managed discovery path requires the same protected Core lifecycle as
/// other retained tool material. No catalog is published by this consumer.
pub(super) fn register_mcp(
    binding: Option<&ManagedWorkerBinding>,
    config: &ProductionRuntimeConfig,
    budget: &OutputBudget,
    timeout: p::DurationMs,
    backends: &ExecutionBackendRegistry,
    protection: forme_store::StoreContentProtection,
    store: SqliteEventStore,
) -> p::Result<bool> {
    let Some(binding) = binding.filter(|binding| binding.supports(p::BackendKind::Mcp)) else {
        return Ok(false);
    };
    let owner = p::VerifiedPrincipal(config.owner.0.clone());
    let Ok(backend) = WorkerActionBackend::from_binding(
        p::BackendKind::Mcp,
        binding,
        owner.clone(),
        OutputBudget::strict(budget.max_bytes),
        timeout,
    ) else {
        return Ok(false);
    };
    let consumer = Arc::new(ProtectedRuntimeArtifacts {
        inner: FileArtifactStore::new(config.data.artifact_root.clone())?,
        protection,
        store,
        owner,
    });
    backends.register(Arc::new(backend.with_discovery_consumer(consumer)))?;
    Ok(true)
}

/// Loading metadata can only read protected originals through the Host verifier.
pub(super) fn mcp_registry(
    managed: bool,
    config: &ProductionRuntimeConfig,
    binding: Option<&ManagedWorkerBinding>,
    store: SqliteEventStore,
) -> p::Result<(Arc<StdioMcpRegistry>, BTreeSet<p::ProviderId>)> {
    let (resolved_mcp_servers, unresolved_mcp) = resolve_mcp_servers(&config.capabilities, managed);
    if !managed {
        return Ok((
            Arc::new(StdioMcpRegistry::with_servers(resolved_mcp_servers)?),
            unresolved_mcp,
        ));
    }
    let source = binding
        .and_then(|b| b.connect_mcp_source().ok())
        .map(|authority| {
            Arc::new(
                crate::production_artifact_store::ProtectedMcpCatalogSource {
                    store,
                    owner: p::VerifiedPrincipal(config.owner.0.clone()),
                    authority,
                },
            ) as Arc<dyn forme_capabilities::ManagedMcpCatalogSource>
        });
    Ok((
        Arc::new(StdioMcpRegistry::with_managed_servers(
            resolved_mcp_servers,
            source,
        )?),
        unresolved_mcp,
    ))
}

pub(super) fn resolve_mcp_servers(
    capability_config: &CapabilityRuntimeConfig,
    managed: bool,
) -> (
    Vec<forme_capabilities::StdioMcpServer>,
    BTreeSet<p::ProviderId>,
) {
    // Resolve declared environments before registering development servers.
    let mut unresolved_mcp = BTreeSet::new();
    let mut resolved_mcp_servers = Vec::new();
    for entry in &capability_config.mcp {
        let mut server = entry.server.clone();
        // Managed configuration names an opaque Host program. It cannot
        // inherit Brain environment values or provide process parameters.
        if managed
            && (!entry.inherit_env.is_empty() || !server.env.is_empty() || !server.args.is_empty())
        {
            unresolved_mcp.insert(server.provider_id.clone());
            continue;
        }
        let mut available = true;
        for name in &entry.inherit_env {
            match std::env::var(name) {
                Ok(value) if !value.is_empty() => server.env.push((name.clone(), value)),
                _ => available = false,
            }
        }
        if available {
            resolved_mcp_servers.push(server);
        } else {
            unresolved_mcp.insert(entry.server.provider_id.clone());
        }
    }
    (resolved_mcp_servers, unresolved_mcp)
}
