use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

use forme_protocol as p;

use super::worker_input::WorkerInputMaterial;

/// OS isolation lifecycle. A terminal observation retains its result source
/// until the Host durably seals the result and acknowledges that handoff.
pub trait WorkerIsolation: Send + Sync {
    /// Whether this adapter has independently verified the fixed production
    /// PTY profile. Mechanism tests and ordinary process workers return false.
    fn supports_managed_pty(&self) -> bool {
        false
    }

    /// Keep the adapter's actual resource exclusion through one callback.
    /// Missing observation capability never proves quiescence, even if idle.
    /// The callback must not reenter worker or lease lifecycle methods.
    fn with_quiescent_resources(
        &self,
        _action: &mut dyn FnMut() -> p::Result<()>,
    ) -> p::Result<()> {
        Err(p::Error(
            "worker resource observation is unavailable".into(),
        ))
    }

    fn admit(&self, job: &p::WorkerJob, permit: &p::WorkerPermit) -> p::Result<()>;

    fn admit_with_execution(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        _execution: Option<&p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<()> {
        self.admit_at(job, permit, now)
    }

    /// Admit only after the Host has resolved all plan-bound input grants. A
    /// platform adapter must opt in to delivery; the default rejects grants
    /// so old or incomplete adapters cannot fall back to empty/Literal input.
    fn admit_with_input(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
        inputs: &[WorkerInputMaterial],
        now: p::Timestamp,
    ) -> p::Result<()> {
        if !inputs.is_empty() {
            return Err(p::Error(
                "worker input delivery is unavailable for this isolation adapter".into(),
            ));
        }
        self.admit_with_execution(job, permit, execution, now)
    }

    fn admit_at(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        _now: p::Timestamp,
    ) -> p::Result<()> {
        self.admit(job, permit)
    }

    fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt>;

    fn observe(
        &self,
        _job: &p::WorkerJob,
        _now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        Ok(None)
    }

    /// Adapter-owned source, never a caller-provided path.
    fn result_path(&self, _job: &p::WorkerJob) -> p::Result<Option<PathBuf>> {
        Ok(None)
    }

    /// Called only after encrypted artifact and manifest journal persistence.
    /// Cleanup failure prevents publication of a successful worker receipt.
    fn result_sealed(&self, _job: &p::WorkerJob, _now: p::Timestamp) -> p::Result<()> {
        Ok(())
    }

    fn cancel_at(
        &self,
        job: &p::WorkerJob,
        _now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt> {
        self.cancel(job)
    }
}

/// The registry owns the adapter's live child handles. Do not release the
/// guard between checking emptiness and returning from the recovery action.
pub(super) fn with_empty_registry<T>(
    registry: &Mutex<BTreeMap<String, T>>,
    action: &mut dyn FnMut() -> p::Result<()>,
) -> p::Result<()> {
    let children = registry
        .lock()
        .map_err(|_| p::Error("worker process registry is unavailable".into()))?;
    if !children.is_empty() {
        return Err(p::Error("worker resources are not quiescent".into()));
    }
    let result = action();
    drop(children);
    result
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn action_uses_network(execution: &p::WorkerExecutionSpec) -> bool {
    matches!(
        execution.intent.parameters,
        p::ActionParameters::Shell { network: true, .. }
    )
}

#[cfg(test)]
#[path = "worker_resource_guard_tests.rs"]
mod tests;
