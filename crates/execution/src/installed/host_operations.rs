//! Shared journal transaction, entered only after domain-specific authorization.
use forme_protocol as p;

use super::{HostAuth, NativeServiceManager, ProductHostBroker};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub(super) fn plan_authorized(
        &self,
        auth: &HostAuth,
        request: p::HostManagementRequest,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation> {
        request.validate()?;
        if request.command.actor_proof.proof_ref != auth.proof_ref.0 {
            return Err(p::Error(
                "maintenance proof is not bound to this request".into(),
            ));
        }
        // Historical queries do not depend on the original release inbox.
        if self
            .journal
            .operation(&request.command.operation_id)
            .is_ok()
        {
            return self
                .journal
                .plan(request.command, auth.actor, request.references, now);
        }
        if request.references.original_release_ref != self.active_release {
            return Err(p::Error(
                "maintenance command is bound to a stale active release".into(),
            ));
        }
        if !matches!(
            &request.command.kind,
            p::MaintenanceKind::Inspect
                | p::MaintenanceKind::StartCore
                | p::MaintenanceKind::StopCore
                | p::MaintenanceKind::RestartCore
        ) {
            self.release_stager
                .as_ref()
                .ok_or_else(|| {
                    p::Error("maintenance operation has no installed lifecycle worker".into())
                })?
                .preflight(&request.command.kind)?;
        }
        self.journal
            .plan(request.command, auth.actor, request.references, now)
    }

    pub(super) fn execute_authorized(
        &self,
        auth: &HostAuth,
        operation_id: &str,
        now: p::Timestamp,
    ) -> p::Result<p::MaintenanceOperation> {
        p::validate_product_id(operation_id)?;
        let operation = self.operation_state(operation_id)?;
        if operation.stage == p::MaintenanceStage::Committed {
            return Ok(operation);
        }
        if operation.stage == p::MaintenanceStage::RecoveryRequired {
            return Err(p::Error(
                "operation requires explicit recovery review".into(),
            ));
        }
        if operation.pending_stage.is_some() {
            return Err(p::Error("operation has an unresolved stage intent".into()));
        }
        let command = self.journal.command(operation_id)?;
        if now >= command.requested_deadline
            || operation.original_release_ref != self.active_release
        {
            return Err(p::Error(
                "maintenance plan expired or its active release changed".into(),
            ));
        }
        if operation.stage != p::MaintenanceStage::Planned {
            return Err(p::Error(
                "interrupted maintenance requires read-only recovery review".into(),
            ));
        }
        let operation = self.record_stage(
            operation_id,
            p::MaintenanceStage::Validated,
            auth.actor,
            now,
            p::canonical_digest(&"host-validation-passed")?,
        )?;
        if operation.command_digest.0.is_empty() {
            return Err(p::Error("maintenance command digest is unavailable".into()));
        }
        // Intent is durable before IO; a lost receipt never permits blind replay.
        self.journal.begin_stage(
            operation_id,
            operation.revision,
            p::MaintenanceStage::Checking,
            auth.actor,
            now,
        )?;
        let receipt = match self.apply_maintenance(&command, &operation) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.journal.require_recovery(
                    operation_id,
                    self.journal.revision()?,
                    p::canonical_digest(&error.to_string())?,
                    now,
                )?;
                return self.operation_state(operation_id);
            }
        };
        self.journal.finish_stage(
            operation_id,
            self.journal.revision()?,
            p::MaintenanceStage::Checking,
            vec![receipt],
            now,
        )?;
        self.record_stage(
            operation_id,
            p::MaintenanceStage::Committed,
            auth.actor,
            now,
            p::canonical_digest(&"host-commit-observed")?,
        )
    }
}
