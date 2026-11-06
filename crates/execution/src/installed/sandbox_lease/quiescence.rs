//! Original lease validation while recovery holds the actual resource fence.
use super::*;

impl SandboxLeaseJournal {
    pub(in crate::installed) fn with_quiescent_leases(
        &self,
        action: &mut dyn FnMut() -> p::Result<()>,
    ) -> p::Result<()> {
        let mut writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        let checked = read_locked_projection(&mut writer.file).and_then(|actual| {
            if actual.sequence != writer.projection.sequence
                || actual.head()? != writer.projection.head()?
            {
                return Err(invalid_journal());
            }
            Ok(())
        });
        if let Err(error) = checked {
            writer.poisoned = true;
            return Err(error);
        }
        if writer
            .projection
            .leases
            .values()
            .any(|r| !r.cleanup_complete)
        {
            return Err(p::Error(
                "sandbox lease recovery is required; worker admission is blocked".into(),
            ));
        }
        let result = action();
        drop(writer);
        result
    }
}

fn read_locked_projection(file: &mut File) -> p::Result<Projection> {
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
        let mut projection = Projection::default();
        for line in bytes.split_inclusive(|b| *b == b'\n') {
            if line.len() > MAX_RECORD_BYTES || line.last() != Some(&b'\n') {
                return Err(invalid_journal());
            }
            let envelope: LeaseEnvelope =
                serde_json::from_slice(line).map_err(|_| invalid_journal())?;
            projection.fold(&envelope)?;
        }
        Ok(projection)
    })();
    file.seek(SeekFrom::Start(position))
        .map_err(|_| storage_error())?;
    result
}

#[cfg(test)]
#[path = "quiescence_tests.rs"]
mod tests;
