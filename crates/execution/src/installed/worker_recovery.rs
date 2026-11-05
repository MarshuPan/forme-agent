//! Read-only worker watermark. An unknown job is never quiescence evidence.
use super::*;

#[cfg(test)]
#[path = "worker_recovery_tests.rs"]
mod tests;

impl WorkerJournal {
    /// Agent input only confirms an already persisted Host observation. It
    /// cannot create an outcome, artifact, cleanup or journal fact.
    pub(in crate::installed) fn confirm_receipt(
        &self,
        receipt: &p::WorkerReceipt,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerObservation> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        if let Err(error) = recheck_writer(&mut writer) {
            writer.poisoned = true;
            return Err(error);
        }
        let record = writer
            .projection
            .jobs
            .get(&receipt.job_id)
            .ok_or_else(|| p::Error("worker job does not exist".into()))?;
        receipt.validate_for(&record.job)?;
        if record.receipt.as_ref() != Some(receipt) {
            return Err(p::Error(
                "worker receipt is not an existing Host observation".into(),
            ));
        }
        Ok(observation(record, now))
    }

    pub(in crate::installed) fn quiescent_digest(&self) -> p::Result<p::SchemaDigest> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        if let Err(error) = recheck_writer(&mut writer) {
            writer.poisoned = true;
            return Err(error);
        }
        if writer.projection.jobs.values().any(|job| {
            !matches!(
                job.state,
                p::WorkerState::Completed | p::WorkerState::Failed | p::WorkerState::Cancelled
            )
        }) {
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

fn recheck_writer(writer: &mut WorkerWriter) -> p::Result<()> {
    let actual = read_locked_projection(&mut writer.file)?;
    if actual.sequence != writer.projection.sequence
        || actual.head()? != writer.projection.head()?
    {
        return Err(invalid_journal());
    }
    Ok(())
}

/// Retain the original file/OS lock and restore its position even on failure.
/// An empty new journal is valid current state, never proof of past inactivity.
fn read_locked_projection(file: &mut File) -> p::Result<WorkerProjection> {
    let position = file.stream_position().map_err(|_| storage_error())?;
    let result = (|| {
        let length = file.metadata().map_err(|_| storage_error())?.len();
        if length > MAX_JOURNAL_BYTES {
            return Err(invalid_journal());
        }
        file.seek(SeekFrom::Start(0)).map_err(|_| storage_error())?;
        let mut bytes = Vec::new();
        (&mut *file)
            .take(length + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| storage_error())?;
        if bytes.len() as u64 != length
            || file.metadata().map_err(|_| storage_error())?.len() != length
        {
            return Err(invalid_journal());
        }
        let mut projection = WorkerProjection::default();
        for line in bytes.split_inclusive(|byte| *byte == b'\n') {
            if line.len() > MAX_RECORD_BYTES || line.last() != Some(&b'\n') {
                return Err(invalid_journal());
            }
            let envelope: WorkerEnvelope =
                serde_json::from_slice(line).map_err(|_| invalid_journal())?;
            projection.fold(&envelope)?;
        }
        Ok(projection)
    })();
    file.seek(SeekFrom::Start(position))
        .map_err(|_| storage_error())?;
    result
}
