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
