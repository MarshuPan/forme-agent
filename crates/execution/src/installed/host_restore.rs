//! Complete Host restore lifecycle; every effect follows its durable stage intent.
use super::{HostAuth, NativeServiceManager, ProductHostBroker};
use crate::installed::ProductConfigStore;
use forme_protocol as p;
use std::time::{Duration, Instant};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub(super) fn plan_restore_locked(
        &self,
        auth: &HostAuth,
        request: p::HostManagementRequest,
        now: p::Timestamp,
        config: &mut ProductConfigStore,
    ) -> p::Result<p::MaintenanceOperation> {
        if !self
            .journal
            .operations(0)?
            .iter()
            .any(|op| op.operation_id == request.command.operation_id)
        {
            let worker = self.restore_worker.as_ref().ok_or_else(unavailable)?;
            worker.preflight()?;
            config.prepare_restore_request(
                &request.command,
                &request.command.bound_digest(&request.references)?,
            )?;
        }
        self.plan_without_restore_lock(auth, request, now)
    }

    pub(super) fn execute_restore_locked(
        &self,
        auth: &HostAuth,
        id: &str,
        now: p::Timestamp,
        config: &mut ProductConfigStore,
    ) -> p::Result<p::MaintenanceOperation> {
        let op = self.operation_state(id)?;
        if op.stage == p::MaintenanceStage::Committed {
            self.release_committed_restore(id, config)?;
            return Ok(op);
        }
        if op.stage != p::MaintenanceStage::Planned || op.pending_stage.is_some() {
            return Err(p::Error(
                "interrupted restore requires review of the original operation".into(),
            ));
        }
        let command = self.journal.command(id)?;
        if now >= command.requested_deadline
            || op.original_release_ref != self.active_release_id()?
        {
            return Err(unavailable());
        }
        let remaining = u64::try_from(command.requested_deadline.saturating_sub(now))
            .map_err(|_| unavailable())?;
        if remaining == 0 || remaining > 300_000 {
            return Err(unavailable());
        }
        let deadline = Instant::now() + Duration::from_millis(remaining);
        let request = config.prepare_restore_request(&command, &op.command_digest)?;
        let worker = self.restore_worker.as_ref().ok_or_else(unavailable)?;
        worker.preflight()?;
        let outcome = self.restore_transaction(auth, id, now, config, request, deadline);
        if let Err(error) = outcome {
            if self.operation_state(id)?.stage == p::MaintenanceStage::Committed {
                return Err(error);
            }
            self.journal.require_recovery(
                id,
                self.journal.revision()?,
                p::canonical_digest(&error.to_string())?,
                current_time(now),
            )?;
        }
        self.operation_state(id)
    }

    fn restore_transaction(
        &self,
        auth: &HostAuth,
        id: &str,
        now: p::Timestamp,
        config: &mut ProductConfigStore,
        request: p::ProductRestoreRequest,
        deadline: Instant,
    ) -> p::Result<()> {
        use p::MaintenanceStage as Stage;
        self.record_stage(
            id,
            Stage::Validated,
            auth.actor,
            current_time(now),
            p::canonical_digest(&request)?,
        )?;
        self.restore_stage(auth, id, Stage::Quiescing, now, deadline, || {
            p::canonical_digest(&self.change_core(p::NativeServiceAction::Stop)?)
        })?;
        // Acquiring either offline fence or validating the candidate may fail;
        // it is inside BackedUp intent and never authorizes a blind restart.
        let mut worker_session = None;
        let mut binding = None;
        self.restore_stage(auth, id, Stage::BackedUp, now, deadline, || {
            let mut session = self
                .restore_worker
                .as_ref()
                .ok_or_else(unavailable)?
                .start(request.clone(), deadline)?;
            let validated = session.validated()?;
            validated.validate_for(&request)?;
            if let p::ProductRestoreWorkerResult::Validated { binding: value, .. } =
                &validated.result
            {
                binding = Some(value.clone());
            } else {
                return Err(unavailable());
            }
            let backed_up = session.step(p::ProductRestoreStep::Backup)?;
            backed_up.validate_for(&request)?;
            if !matches!(
                backed_up.result,
                p::ProductRestoreWorkerResult::BackedUp { .. }
            ) {
                return Err(unavailable());
            }
            worker_session = Some(session);
            p::canonical_digest(&(validated, backed_up))
        })?;
        let binding = binding.ok_or_else(unavailable)?;
        let mut session = worker_session.ok_or_else(unavailable)?;
        let mut new_revision = None;
        self.restore_stage(auth, id, Stage::Switching, now, deadline, || {
            config.restore_work_admission(&request, &binding, false)?;
            let fenced = session.step(p::ProductRestoreStep::FenceSessions)?;
            fenced.validate_for(&request)?;
            if !matches!(
                fenced.result,
                p::ProductRestoreWorkerResult::SessionsFenced { .. }
            ) {
                return Err(unavailable());
            }
            if Instant::now() >= deadline {
                return Err(unavailable());
            }
            let publication = config.publish_restored_data(&request, binding.clone())?;
            new_revision = Some(publication.active_revision);
            let released = session.step(p::ProductRestoreStep::Release)?;
            released.validate_for(&request)?;
            if !matches!(
                released.result,
                p::ProductRestoreWorkerResult::Released { .. }
            ) {
                return Err(unavailable());
            }
            p::canonical_digest(&(fenced, publication, released))
        })?;
        let revision = new_revision.ok_or_else(unavailable)?;
        self.restore_stage(auth, id, Stage::Checking, now, deadline, || {
            let started = self.change_core(p::NativeServiceAction::Start)?;
            loop {
                if Instant::now() >= deadline {
                    return Err(unavailable());
                }
                let snapshot = config.snapshot()?;
                if snapshot.active_revision != revision {
                    return Err(unavailable());
                }
                if snapshot.acknowledged_revision == Some(revision) {
                    let status = self.services.inspect(p::BuiltinServiceId::Brain)?;
                    if status.schema_version != p::SchemaVersion(1)
                        || status.service != p::BuiltinServiceId::Brain
                        || status.state != p::NativeServiceState::Running
                    {
                        return Err(unavailable());
                    }
                    return p::canonical_digest(&(started, snapshot, status));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        })?;
        self.record_stage(
            id,
            Stage::Committed,
            auth.actor,
            current_time(now),
            p::canonical_digest(&("restored-composition-acknowledged-v1", revision, &binding))?,
        )?;
        config.restore_work_admission(&request, &binding, true)?;
        Ok(())
    }

    pub(super) fn release_committed_restore(
        &self,
        id: &str,
        config: &mut ProductConfigStore,
    ) -> p::Result<()> {
        let op = self.operation_state(id)?;
        if !config.restore_admission_pending(id, &op.command_digest)? {
            return Ok(());
        }
        if self.services.inspect(p::BuiltinServiceId::Brain)?.state
            != p::NativeServiceState::Running
        {
            return Err(unavailable());
        }
        if op.stage != p::MaintenanceStage::Committed {
            return Err(unavailable());
        }
        config.release_committed_restore(&self.journal.command(id)?, &op.command_digest)
    }

    fn restore_stage(
        &self,
        auth: &HostAuth,
        id: &str,
        stage: p::MaintenanceStage,
        now: p::Timestamp,
        deadline: Instant,
        effect: impl FnOnce() -> p::Result<p::SchemaDigest>,
    ) -> p::Result<()> {
        if Instant::now() >= deadline {
            return Err(unavailable());
        }
        let op = self.operation_state(id)?;
        self.journal
            .begin_stage(id, op.revision, stage, auth.actor, current_time(now))?;
        let receipt = effect()?;
        let op = self.operation_state(id)?;
        self.journal
            .finish_stage(id, op.revision, stage, vec![receipt], current_time(now))?;
        Ok(())
    }
}
fn unavailable() -> p::Error {
    p::Error("restore lifecycle inputs or readiness are unavailable".into())
}
fn current_time(floor: p::Timestamp) -> p::Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX).max(floor))
        .unwrap_or(floor)
}
