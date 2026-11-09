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
