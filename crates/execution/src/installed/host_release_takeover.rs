//! Writer handoff is authorized before the HTTP response, dispatched after EOF.
use super::{HostAuth, NativeServiceManager, ProductHostBroker};
use forme_protocol as p;
use std::sync::Arc;

pub struct PreparedOwnerRelease {
    auth: HostAuth,
    takeover: p::ProductReleaseTakeover,
}

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn with_release_updater(
        mut self,
        updater: Arc<crate::installed::InstalledReleaseUpdater>,
    ) -> p::Result<Self> {
        if updater.installation().install_id != self.install_id
            || updater.installation().target != self.target
            || self.release_worker.is_some()
        {
            return Err(invalid());
        }
        self.release_updater = Some(updater);
        Ok(self)
    }

    pub fn prepare_owner_release(
        &self,
        auth: &HostAuth,
        request: p::ProductMaintenanceRequest,
        now: p::Timestamp,
    ) -> p::Result<(p::MaintenanceOperation, Option<PreparedOwnerRelease>)> {
        self.verify_web_configuration_auth(auth)?;
        request.validate()?;
        if !matches!(
            request.action,
            p::ProductMaintenanceAction::Upgrade { .. }
                | p::ProductMaintenanceAction::RollbackCompatibleRelease { .. }
        ) {
            return Err(invalid());
        }
        let updater = self.release_updater.as_ref().ok_or_else(invalid)?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        if config.maintenance_owner()?.as_ref() != Some(&request.owner) {
            return Err(invalid());
        }
        let existing = self
            .journal
            .operations(0)?
            .into_iter()
            .find(|op| op.operation_id == request.operation_id);
        let active = self.active_release_state()?;
        let original = existing
            .as_ref()
            .map_or(active.id.as_str(), |op| &op.original_release_ref);
        let (target, backup_ref) = match &request.action {
            p::ProductMaintenanceAction::Upgrade {
                release_id,
                backup_ref,
                ..
            }
            | p::ProductMaintenanceAction::RollbackCompatibleRelease {
                release_id,
                backup_ref,
                ..
            } => (release_id, backup_ref),
            _ => return Err(invalid()),
        };
        let command = p::HostManagementRequest {
            schema_version: p::SchemaVersion(1),
            command: p::MaintenanceCommand {
                schema_version: p::SchemaVersion(1),
                operation_id: request.operation_id.clone(),
                idempotency_key: request.digest()?.0,
                expected_revision: request.expected_revision,
                actor_proof: p::MaintenanceProofRef {
                    schema_version: p::SchemaVersion(1),
                    proof_ref: auth.proof_ref.0.clone(),
                },
                kind: request.action.maintenance_kind(),
                requested_deadline: request.requested_deadline,
            },
            references: p::MaintenanceReferences {
                schema_version: p::SchemaVersion(1),
                original_release_ref: original.into(),
                target_release_ref: target.clone(),
                backup_ref: Some(backup_ref.clone()),
                data_candidate_ref: None,
            },
        };
        if existing.is_some() {
            return self
                .plan_without_restore_lock(auth, command, now)
                .map(|op| (op, None));
        }
        if now >= request.requested_deadline
            || request.requested_deadline.saturating_sub(now) > 300_000
        {
            return Err(invalid());
        }
        let (owner, revision, config_digest) = config.release_configuration_identity()?;
        config.validate_revision(revision, &owner)?;
        if owner != request.owner {
            return Err(invalid());
        }
        let planned = self.plan_authorized(auth, command.clone(), now)?;
        let release = self.release_request(
            &command.command,
            original,
            active.digest.as_ref().ok_or_else(invalid)?,
            target,
            backup_ref,
            planned.command_digest.clone(),
        )?;
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(|_| invalid())?;
        let takeover = p::ProductReleaseTakeover {
            schema_version: p::SchemaVersion(1),
            release,
            installation_digest: updater.installation().digest()?,
            owner,
            expected_config_revision: revision,
            expected_config_digest: config_digest,
            deadline: request.requested_deadline,
            readiness_nonce: random.iter().map(|b| format!("{b:02x}")).collect(),
        };
        takeover.validate()?;
        self.record_stage(
            &request.operation_id,
            p::MaintenanceStage::Validated,
            auth.actor,
            now,
            p::canonical_digest(&takeover)?,
        )?;
        self.journal
            .prepare_release_takeover(self.journal.revision()?, takeover.clone(), now)?;
        self.journal.begin_stage(
            &request.operation_id,
            self.journal.revision()?,
            p::MaintenanceStage::Quiescing,
            auth.actor,
            now,
        )?;
        if let Err(error) = updater.prepare_handoff(&takeover) {
            self.journal.require_recovery(
                &request.operation_id,
                self.journal.revision()?,
                p::canonical_digest(&error.to_string())?,
                now,
            )?;
            return Err(error);
        }
        Ok((
            self.operation_state(&request.operation_id)?,
            Some(PreparedOwnerRelease {
                auth: auth.clone(),
                takeover,
            }),
        ))
    }
    pub fn execute_owner_release(&self, ticket: PreparedOwnerRelease) -> p::Result<()> {
        self.verify_web_configuration_auth(&ticket.auth)?;
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        if config.release_configuration_identity()?
            != (
                ticket.takeover.owner.clone(),
                ticket.takeover.expected_config_revision,
                ticket.takeover.expected_config_digest.clone(),
            )
        {
            return Err(invalid());
        }
        self.release_updater
            .as_ref()
            .ok_or_else(invalid)?
            .launch(&self.journal, &ticket.takeover)
    }
    pub fn reconcile_release_checking(&self) -> p::Result<()> {
        let Some(updater) = &self.release_updater else {
            return Ok(());
        };
        let mut config = self
            .product_config
            .as_ref()
            .ok_or_else(invalid)?
            .lock()
            .map_err(|_| invalid())?;
        updater.reconcile(&self.journal, &mut config)
    }
    pub fn ensure_release_settled(&self) -> p::Result<()> {
        if self.journal.release_is_unresolved()? {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn with_installed_work_admission<T>(
        &self,
        action: impl FnOnce() -> p::Result<T>,
    ) -> p::Result<T> {
        let _configuration = self
            .product_config
            .as_ref()
            .map(|c| c.lock().map_err(|_| invalid()))
            .transpose()?;
        self.ensure_release_settled()?;
        action()
    }
}
fn invalid() -> p::Error {
    p::Error("installed release dispatch is unavailable, stale or unresolved".into())
}
