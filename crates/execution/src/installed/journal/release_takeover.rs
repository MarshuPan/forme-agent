//! Original-operation evidence for writer transfer. Replays never dispatch work.
use super::*;
#[cfg(test)]
mod tests;

impl ProductJournal {
    pub fn release_is_unresolved(&self) -> p::Result<bool> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        Ok(writer
            .projection
            .operations
            .values()
            .any(|state| state.release_takeover.is_some() && !state.view.stage.is_terminal()))
    }
    pub fn release_takeover(&self, id: &str) -> p::Result<Option<p::ProductReleaseTakeover>> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        Ok(writer
            .projection
            .operations
            .get(id)
            .ok_or_else(invalid_journal)?
            .release_takeover
            .clone())
    }

    pub fn release_checking(&self, id: &str) -> p::Result<Option<p::ProductReleaseChecking>> {
        let writer = self.writer.lock().map_err(|_| storage_error())?;
        if writer.poisoned {
            return Err(storage_error());
        }
        Ok(writer
            .projection
            .operations
            .get(id)
            .ok_or_else(invalid_journal)?
            .release_checking
            .clone())
    }

    pub(crate) fn prepare_release_takeover(
        &self,
        expected: u64,
        takeover: p::ProductReleaseTakeover,
        at: p::Timestamp,
    ) -> p::Result<()> {
        self.append(
            expected,
            JournalFact::ReleaseTakeoverPrepared {
                takeover: Box::new(takeover),
                at,
            },
        )
    }

    pub(crate) fn prepare_release_checking(
        &self,
        expected: u64,
        checking: p::ProductReleaseChecking,
        at: p::Timestamp,
    ) -> p::Result<()> {
        self.append(
            expected,
            JournalFact::ReleaseCheckingPrepared {
                checking: Box::new(checking),
                at,
            },
        )
    }
}

pub(super) fn fold_takeover(
    projection: &mut Projection,
    takeover: &p::ProductReleaseTakeover,
    at: p::Timestamp,
    seq: u64,
) -> p::Result<()> {
    takeover.validate()?;
    let request = &takeover.release;
    let state = projection
        .operations
        .get_mut(&request.operation_id)
        .ok_or_else(invalid_journal)?;
    let expected_kind = match request.change {
        p::ProductReleaseChange::Upgrade => p::MaintenanceKind::Upgrade {
            release_id: request.target_release_ref.clone(),
            release_digest: request.target_release_digest.clone(),
            backup_ref: request.backup_ref.clone(),
        },
        p::ProductReleaseChange::RollbackCompatibleRelease => {
            p::MaintenanceKind::RollbackCompatibleRelease {
                release_id: request.target_release_ref.clone(),
                release_digest: request.target_release_digest.clone(),
                backup_ref: request.backup_ref.clone(),
            }
        }
    };
    if at < 0
        || at >= takeover.deadline
        || takeover.deadline != state.command.requested_deadline
        || state.command.kind != expected_kind
        || state.view.stage != p::MaintenanceStage::Validated
        || state.view.pending_stage.is_some()
        || state.release_takeover.is_some()
        || state.view.command_digest != request.command_digest
        || state.view.original_release_ref != request.original_release_ref
        || state.view.target_release_ref != request.target_release_ref
        || state.view.backup_ref.as_ref() != Some(&request.backup_ref)
        || projection.install_id.as_ref() != Some(&request.install_ref)
        || projection
            .installation_verification
            .as_ref()
            .map(|pair| &pair.0)
            != Some(&takeover.installation_digest)
        || projection.active_release.as_ref().is_none_or(|active| {
            active.release_id != request.original_release_ref
                || active.release_digest != request.original_release_digest
        })
    {
        return Err(invalid_journal());
    }
    state.release_takeover = Some(takeover.clone());
    state.view.revision = seq;
    Ok(())
}

pub(super) fn fold_checking(
    projection: &mut Projection,
    checking: &p::ProductReleaseChecking,
    at: p::Timestamp,
    seq: u64,
) -> p::Result<()> {
    checking.validate()?;
    let release = &checking.takeover.release;
    let state = projection
        .operations
        .get_mut(&release.operation_id)
        .ok_or_else(invalid_journal)?;
    if at < 0
        || at >= checking.takeover.deadline
        || state.release_takeover.as_ref() != Some(&checking.takeover)
        || state.release_checking.is_some()
        || !state.release_images_claimed
        || state.view.stage != p::MaintenanceStage::Switching
        || state.view.pending_stage != Some(p::MaintenanceStage::Checking)
        || state.view.command_digest != release.command_digest
        || projection.active_release.as_ref().is_none_or(|active| {
            active.release_id != release.target_release_ref
                || active.release_digest != release.target_release_digest
        })
    {
        return Err(invalid_journal());
    }
    state.release_checking = Some(checking.clone());
    state.view.revision = seq;
    Ok(())
}
