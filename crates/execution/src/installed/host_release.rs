//! Host-side transaction for activating an already verified release.
use super::ProductHostBroker;
use crate::installed::{NativeServiceManager, ReleaseWorkerSession};
use forme_protocol as p;
use std::time::{Duration, Instant};

struct ReleaseExecution<'a> {
    auth: &'a super::HostAuth,
    id: &'a str,
    request: &'a p::ProductReleaseRequest,
    now: p::Timestamp,
    deadline: Instant,
}

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub(super) fn preflight_release(
        &self,
        command: &p::MaintenanceCommand,
        references: &p::MaintenanceReferences,
    ) -> p::Result<()> {
        let active = self.active_release_state()?;
        let original_digest = active.digest.ok_or_else(unavailable)?;
        if references.original_release_ref != active.id {
            return Err(p::Error(
                "release command is bound to a stale active release".into(),
            ));
        }
        let request = self.release_request(
            command,
            &references.original_release_ref,
            &original_digest,
            &references.target_release_ref,
            references.backup_ref.as_deref().ok_or_else(unavailable)?,
            command.bound_digest(references)?,
        )?;
        let stager = self.release_stager.as_ref().ok_or_else(unavailable)?;
        let verified = stager.verified_staged(
            &self.journal,
            &request.target_release_ref,
            &request.target_release_digest,
        )?;
        verified.recheck()?;
        if let Some(updater) = &self.release_updater {
            updater.preflight(&self.journal, &request)
        } else {
            self.verified_release_worker()?.preflight(&request)
        }
    }

    pub(super) fn execute_release_authorized(
        &self,
        auth: &super::HostAuth,
        id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation> {
        use p::MaintenanceStage as Stage;

        if self.release_updater.is_some() {
            return Err(unavailable());
        }

        let operation = self.operation_state(id)?;
        let command = self.journal.command(id)?;
        if operation.stage != Stage::Planned || operation.pending_stage.is_some() {
            return Err(p::Error(
                "interrupted release activation requires review of the original operation".into(),
            ));
        }
        if now >= command.requested_deadline
            || operation.original_release_ref != self.active_release_id()?
        {
            return Err(unavailable());
        }
        let active = self.active_release_state()?;
        let original_digest = active.digest.ok_or_else(unavailable)?;
        let backup_ref = operation.backup_ref.as_deref().ok_or_else(unavailable)?;
        let request = self.release_request(
            &command,
            &operation.original_release_ref,
            &original_digest,
            &operation.target_release_ref,
            backup_ref,
            operation.command_digest.clone(),
        )?;
        let stager = self.release_stager.as_ref().ok_or_else(unavailable)?;
        let verified = stager.verified_staged(
            &self.journal,
            &request.target_release_ref,
            &request.target_release_digest,
        )?;
        verified.recheck()?;
        let worker = self.verified_release_worker()?;
        worker.preflight(&request)?;
        let remaining = u64::try_from(command.requested_deadline.saturating_sub(now))
            .map_err(|_| unavailable())?;
        if remaining == 0 || remaining > 300_000 {
            return Err(unavailable());
        }
        let deadline = Instant::now() + Duration::from_millis(remaining);

        self.record_stage(
            id,
            Stage::Validated,
            auth.actor,
            current_time(now),
            p::canonical_digest(&("release-validated-v1", &request, verified.manifest()))?,
        )?;

        let outcome = (|| {
            let mut session = worker.start(request.clone(), deadline)?;
            let execution = ReleaseExecution {
                auth,
                id,
                request: &request,
                now,
                deadline,
            };
            let quiesced = self.release_step(
                &execution,
                Stage::Quiescing,
                p::ProductReleaseStep::Quiesce,
                &mut *session,
            )?;
            let backed_up = self.release_step(
                &execution,
                Stage::BackedUp,
                p::ProductReleaseStep::Backup,
                &mut *session,
            )?;

            let switched = self.release_switch(auth, id, &request, &mut *session, now, deadline)?;

            let checked = self.release_check(auth, id, &request, &mut *session, now, deadline)?;
            Ok::<_, p::Error>((
                session,
                p::canonical_digest(&(quiesced, backed_up, switched, checked))?,
            ))
        })();

        match outcome {
            Ok((mut session, receipt)) => {
                let committed = self.record_stage(
                    id,
                    Stage::Committed,
                    auth.actor,
                    current_time(now),
                    receipt,
                )?;
                // Never make a new run admissible before the durable commit.
                // A lost release receipt does not undo the committed version or
                // authorize repeating any lifecycle side effect.
                if Instant::now() >= deadline {
                    return Err(unavailable());
                }
                let released = session.step(&request, p::ProductReleaseStep::Release)?;
                released.validate_for(&request, p::ProductReleaseStep::Release)?;
                Ok(committed)
            }
            Err(error) => {
                self.journal.require_recovery(
                    id,
                    self.journal.revision()?,
                    p::canonical_digest(&error.to_string())?,
                    current_time(now),
                )?;
                self.operation_state(id)
            }
        }
    }

    pub(super) fn release_request(
        &self,
        command: &p::MaintenanceCommand,
        original_release_ref: &str,
        original_release_digest: &p::SchemaDigest,
        target_release_ref: &str,
        backup_ref: &str,
        command_digest: p::SchemaDigest,
    ) -> p::Result<p::ProductReleaseRequest> {
        let (change, target_id, target_digest) = match &command.kind {
            p::MaintenanceKind::Upgrade {
                release_id,
                release_digest,
                ..
            } => (p::ProductReleaseChange::Upgrade, release_id, release_digest),
            p::MaintenanceKind::RollbackCompatibleRelease {
                release_id,
                release_digest,
                ..
            } => (
                p::ProductReleaseChange::RollbackCompatibleRelease,
                release_id,
                release_digest,
            ),
            _ => return Err(unavailable()),
        };
        if target_release_ref != target_id {
            return Err(unavailable());
        }
        let request = p::ProductReleaseRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: self.install_id.clone(),
            operation_id: command.operation_id.clone(),
            command_digest,
            change,
            original_release_ref: original_release_ref.into(),
            original_release_digest: original_release_digest.clone(),
            target_release_ref: target_release_ref.into(),
            target_release_digest: target_digest.clone(),
            backup_ref: backup_ref.into(),
        };
        request.validate()?;
        Ok(request)
    }

    fn verified_release_worker(
        &self,
    ) -> p::Result<&std::sync::Arc<dyn crate::installed::ReleaseWorkerProvider>> {
        let worker = self.release_worker.as_ref().ok_or_else(unavailable)?;
        if self.journal.installation_verification()?
            != Some((
                worker.installation().digest()?,
                worker.installation().release_digest.clone(),
            ))
        {
            return Err(unavailable());
        }
        Ok(worker)
    }

    fn release_step(
        &self,
        execution: &ReleaseExecution<'_>,
        stage: p::MaintenanceStage,
        step: p::ProductReleaseStep,
        session: &mut dyn ReleaseWorkerSession,
    ) -> p::Result<p::SchemaDigest> {
        let ReleaseExecution {
            auth,
            id,
            request,
            now,
            deadline,
        } = *execution;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        let operation = self.operation_state(id)?;
        self.journal
            .begin_stage(id, operation.revision, stage, auth.actor, current_time(now))?;
        let response = session.step(request, step)?;
        response.validate_for(request, step)?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        let receipt = p::canonical_digest(&response)?;
        let current = self.operation_state(id)?;
        self.journal.finish_stage(
            id,
            current.revision,
            stage,
            vec![receipt.clone()],
            current_time(now),
        )?;
        Ok(receipt)
    }

    fn release_switch(
        &self,
        auth: &super::HostAuth,
        id: &str,
        request: &p::ProductReleaseRequest,
        session: &mut dyn ReleaseWorkerSession,
        now: p::Timestamp,
        deadline: Instant,
    ) -> p::Result<p::SchemaDigest> {
        use p::MaintenanceStage::Switching;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        self.release_stager
            .as_ref()
            .ok_or_else(unavailable)?
            .verified_staged(
                &self.journal,
                &request.target_release_ref,
                &request.target_release_digest,
            )?
            .recheck()?;
        let operation = self.operation_state(id)?;
        self.journal.begin_stage(
            id,
            operation.revision,
            Switching,
            auth.actor,
            current_time(now),
        )?;
        let response = session.step(request, p::ProductReleaseStep::Switch)?;
        response.validate_for(request, p::ProductReleaseStep::Switch)?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        self.set_active_release(
            id,
            &request.original_release_ref,
            &request.original_release_digest,
            &request.target_release_ref,
            &request.target_release_digest,
        )?;
        let receipt = p::canonical_digest(&response)?;
        let current = self.operation_state(id)?;
        self.journal.finish_stage(
            id,
            current.revision,
            Switching,
            vec![receipt.clone()],
            current_time(now),
        )?;
        Ok(receipt)
    }

    fn release_check(
        &self,
        auth: &super::HostAuth,
        id: &str,
        request: &p::ProductReleaseRequest,
        session: &mut dyn ReleaseWorkerSession,
        now: p::Timestamp,
        deadline: Instant,
    ) -> p::Result<p::SchemaDigest> {
        use p::MaintenanceStage::Checking;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        let operation = self.operation_state(id)?;
        self.journal.begin_stage(
            id,
            operation.revision,
            Checking,
            auth.actor,
            current_time(now),
        )?;
        let checked = session.step(request, p::ProductReleaseStep::Check)?;
        checked.validate_for(request, p::ProductReleaseStep::Check)?;
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        let receipt = p::canonical_digest(&checked)?;
        let current = self.operation_state(id)?;
        self.journal.finish_stage(
            id,
            current.revision,
            Checking,
            vec![receipt.clone()],
            current_time(now),
        )?;
        Ok(receipt)
    }
}

fn unavailable() -> p::Error {
    p::Error(
        "signed release activation requires a bound updater/backup worker; inspect the original operation".into(),
    )
}

fn current_time(floor: p::Timestamp) -> p::Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| {
            i64::try_from(duration.as_millis())
                .unwrap_or(i64::MAX)
                .max(floor)
        })
        .unwrap_or(floor)
}
