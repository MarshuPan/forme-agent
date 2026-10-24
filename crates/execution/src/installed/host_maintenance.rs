//! Fixed owner maintenance commands, separate from AgentExecution and admin IPC.
use forme_protocol as p;

use super::{HostAuth, NativeServiceManager, ProductHostBroker};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub fn maintenance_snapshot(
        &self,
        auth: &HostAuth,
    ) -> p::Result<p::ProductMaintenanceSnapshot> {
        self.verify_web_configuration_auth(auth)?;
        let revision = self.journal.revision()?;
        let owner = self
            .configuration(auth)?
            .owner
            .ok_or_else(|| p::Error("installation is not yet claimed".into()))?;
        let host = self.status_for()?;
        let mut operations = self.journal.operations(0)?;
        operations.reverse();
        operations.truncate(25);
        if self.journal.revision()? != revision {
            return Err(p::Error("maintenance snapshot revision conflict".into()));
        }
        Ok(p::ProductMaintenanceSnapshot {
            schema_version: p::SchemaVersion(1),
            product_revision: revision,
            owner,
            host,
            operations,
        })
    }

    /// Web has already consumed an operation-bound password proof. Keep the
    /// actual actor in the journal; never manufacture a local-admin identity.
    pub fn owner_maintenance(
        &self,
        auth: &HostAuth,
        request: p::ProductMaintenanceRequest,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation> {
        if self.release_updater.is_some()
            && matches!(
                request.action,
                p::ProductMaintenanceAction::Upgrade { .. }
                    | p::ProductMaintenanceAction::RollbackCompatibleRelease { .. }
            )
        {
            return Err(p::Error(
                "installed release requires deferred updater dispatch".into(),
            ));
        }
        self.owner_maintenance_inner(auth, request, now, false)
            .map(|(op, _)| op)
    }

    pub(super) fn owner_maintenance_inner(
        &self,
        auth: &HostAuth,
        request: p::ProductMaintenanceRequest,
        now: p::Timestamp,
        defer_network: bool,
    ) -> p::Result<(p::MaintenanceOperation, Option<super::PreparedOwnerNetwork>)> {
        self.verify_web_configuration_auth(auth)?;
        request.validate()?;
        let config = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        let mut config = config
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
        if config.maintenance_owner()?.as_ref() != Some(&request.owner) {
            return Err(p::Error(
                "maintenance owner does not match the installed owner".into(),
            ));
        }
        let existing = self
            .journal
            .operations(0)?
            .into_iter()
            .find(|operation| operation.operation_id == request.operation_id);
        let active_release = self.active_release_id()?;
        let original = existing
            .as_ref()
            .map_or(active_release.as_str(), |op| &op.original_release_ref);
        let target = match &request.action {
            p::ProductMaintenanceAction::StageSignedRelease { release_id, .. }
            | p::ProductMaintenanceAction::Upgrade { release_id, .. }
            | p::ProductMaintenanceAction::RollbackCompatibleRelease { release_id, .. } => {
                release_id
            }
            _ => original,
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
                target_release_ref: target.into(),
                data_candidate_ref: match &request.action {
                    p::ProductMaintenanceAction::CommitRestore { candidate_ref, .. } => {
                        Some(candidate_ref.clone())
                    }
                    _ => None,
                },
                backup_ref: match &request.action {
                    p::ProductMaintenanceAction::Upgrade { backup_ref, .. }
                    | p::ProductMaintenanceAction::RollbackCompatibleRelease {
                        backup_ref, ..
                    } => Some(backup_ref.clone()),
                    _ => None,
                },
            },
        };
        if existing.is_some() {
            // Only validate/query the exact original binding. Never resume an
            // interrupted or lost-response command by executing it a second time.
            let op = self.plan_without_restore_lock(auth, command, now)?;
            if op.stage == p::MaintenanceStage::Committed
                && matches!(
                    request.action,
                    p::ProductMaintenanceAction::CommitRestore { .. }
                )
            {
                self.release_committed_restore(&request.operation_id, &mut config)?;
            }
            return Ok((op, None));
        }
        if now >= request.requested_deadline
            || request.requested_deadline.saturating_sub(now) > 300_000
        {
            return Err(p::Error("owner maintenance authorization expired".into()));
        }
        let view = config.snapshot()?;
        if matches!(
            request.action,
            p::ProductMaintenanceAction::StartCore {} | p::ProductMaintenanceAction::RestartCore {}
        ) {
            config.validate_revision(view.active_revision, &request.owner)?;
        }
        if let p::ProductMaintenanceAction::ConfigureLan {
            config_revision,
            validation_digest,
        } = &request.action
        {
            let actual = config.network_validation_digest(*config_revision, &request.owner)?;
            if actual != *validation_digest {
                return Err(p::Error(
                    "LAN configuration validation is stale; refresh before applying".into(),
                ));
            }
            if !self.network_change_preflight(&mut config)? {
                return Err(p::Error(
                    "LAN firewall reconciliation is unavailable before dispatch".into(),
                ));
            }
        }
        if matches!(
            request.action,
            p::ProductMaintenanceAction::CommitRestore { .. }
        ) {
            self.plan_restore_locked(auth, command, now, &mut config)?;
            return self
                .execute_restore_locked(auth, &request.operation_id, now, &mut config)
                .map(|op| (op, None));
        }
        // Plan while the configuration lock still serializes admissions. Only
        // the caller that first creates this operation receives a dispatch ticket.
        let planned = self.plan_authorized(auth, command, now)?;
        if matches!(
            request.action,
            p::ProductMaintenanceAction::ConfigureLan { .. }
        ) {
            drop(config);
            if defer_network {
                let ticket = super::PreparedOwnerNetwork {
                    auth: auth.clone(),
                    operation_id: request.operation_id,
                    command_digest: planned.command_digest.clone(),
                    owner: request.owner,
                };
                return Ok((planned, Some(ticket)));
            }
        }
        // Keep configuration locked until native IO finishes: configuration
        // cannot be replaced between validation and start/restart.
        self.execute_authorized(auth, &request.operation_id, now)
            .map(|op| (op, None))
    }

    pub fn owner_maintenance_operation(
        &self,
        auth: &HostAuth,
        id: &str,
    ) -> p::Result<p::MaintenanceOperation> {
        self.verify_web_configuration_auth(auth)?;
        p::validate_product_id(id)?;
        if !matches!(
            self.journal.command(id)?.kind,
            p::MaintenanceKind::StartCore
                | p::MaintenanceKind::StopCore
                | p::MaintenanceKind::RestartCore
                | p::MaintenanceKind::ConfigureLan { .. }
                | p::MaintenanceKind::StageSignedRelease { .. }
                | p::MaintenanceKind::Upgrade { .. }
                | p::MaintenanceKind::RollbackCompatibleRelease { .. }
                | p::MaintenanceKind::UninstallPlan {
                    preserve_owner_data: true
                }
                | p::MaintenanceKind::CommitRestore { .. }
        ) {
            return Err(p::Error(
                "operation is outside the owner maintenance domain".into(),
            ));
        }
        let op = self.journal.operation(id)?;
        if op.stage == p::MaintenanceStage::Committed
            && matches!(
                self.journal.command_kind(id)?,
                p::MaintenanceKind::CommitRestore { .. }
            )
        {
            let mut config = self
                .product_config
                .as_ref()
                .ok_or_else(|| p::Error("managed configuration unavailable".into()))?
                .lock()
                .map_err(|_| p::Error("managed configuration unavailable".into()))?;
            self.release_committed_restore(id, &mut config)?;
        }
        Ok(op)
    }
}
