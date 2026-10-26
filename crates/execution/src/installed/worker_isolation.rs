use std::path::PathBuf;

use forme_protocol as p;

/// OS isolation lifecycle. A terminal observation retains its result source
/// until the Host durably seals the result and acknowledges that handoff.
pub trait WorkerIsolation: Send + Sync {
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
