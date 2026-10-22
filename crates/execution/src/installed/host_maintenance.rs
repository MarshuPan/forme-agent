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
        self.verify_web_configuration_auth(auth)?;
        request.validate()?;
        let config = self
            .product_config
            .as_ref()
            .ok_or_else(|| p::Error("managed product configuration is unavailable".into()))?;
        let mut config = config
            .lock()
            .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
        let view = config.snapshot()?;
        if view.owner.as_ref() != Some(&request.owner) {
            return Err(p::Error(
                "maintenance owner does not match the installed owner".into(),
            ));
        }
        let existing = self
            .journal
            .operations(0)?
            .into_iter()
            .find(|operation| operation.operation_id == request.operation_id);
        let original = existing
            .as_ref()
            .map_or(self.active_release.as_str(), |op| &op.original_release_ref);
        let target = match &request.action {
            p::ProductMaintenanceAction::StageSignedRelease { release_id, .. } => release_id,
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
                data_candidate_ref: None,
                backup_ref: None,
            },
        };
        if existing.is_some() {
            // Only validate/query the exact original binding. Never resume an
            // interrupted or lost-response command by executing it a second time.
            return self.plan_authorized(auth, command, now);
        }
        if now >= request.requested_deadline
            || request.requested_deadline.saturating_sub(now) > 300_000
        {
            return Err(p::Error("owner maintenance authorization expired".into()));
        }
        if matches!(
            request.action,
            p::ProductMaintenanceAction::StartCore {} | p::ProductMaintenanceAction::RestartCore {}
        ) {
            config.validate_revision(view.active_revision, &request.owner)?;
        }
        self.plan_authorized(auth, command, now)?;
        // Keep configuration locked until native IO finishes: configuration
        // cannot be replaced between validation and start/restart.
        self.execute_authorized(auth, &request.operation_id, now)
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
                | p::MaintenanceKind::StageSignedRelease { .. }
                | p::MaintenanceKind::UninstallPlan {
                    preserve_owner_data: true
                }
        ) {
            return Err(p::Error(
                "operation is outside the owner maintenance domain".into(),
            ));
        }
        self.journal.operation(id)
    }
}
