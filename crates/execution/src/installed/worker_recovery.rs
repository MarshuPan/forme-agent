//! Read-only worker watermark. An unknown job is never quiescence evidence.
use super::*;

impl WorkerJournal {
    pub(in crate::installed) fn quiescent_digest(&self) -> p::Result<p::SchemaDigest> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned
            || writer.projection.jobs.values().any(|job| {
                !matches!(
                    job.state,
                    p::WorkerState::Completed | p::WorkerState::Failed | p::WorkerState::Cancelled
                )
            })
        {
            return Err(p::Error(
                "release recovery requires settled worker outcomes".into(),
            ));
        }
        p::canonical_digest(&(
            "forme-worker-quiescent-v1",
            writer.projection.sequence,
            writer.projection.head()?,
        ))
    }
}
