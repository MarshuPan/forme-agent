#![forbid(unsafe_code)]

use forme_protocol as p;

use super::{
    host_configuration::{configuration_unavailable, current_timestamp},
    HostAuth, NativeServiceManager, ProductHostBroker,
};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    /// Reconcile Brain after Host has restored product state.
    pub fn reconcile_core_startup(&self) -> p::Result<()> {
        if !self.automatic_core_activation {
            return Ok(());
        }
        let result = (|| {
            let store = self
                .product_config
                .as_ref()
                .ok_or_else(configuration_unavailable)?;
            let view = store
                .lock()
                .map_err(|_| configuration_unavailable())?
                .snapshot()?;
            let Some(owner) = view.owner.as_ref() else {
                return Ok(());
            };
            if !view.configured || view.active_revision == 0 {
                return Ok(());
            }
            let Some(bearer) = self.web_configuration_bearer.as_deref() else {
                return Ok(());
            };
            let auth = self.authenticate_web_configuration(bearer)?;
            self.reconcile_core(
                &auth,
                owner,
                view.active_revision,
                view.pending_restart,
                current_timestamp()?,
                true,
            )
            .map(|_| ())
        })();
        self.mark_startup_reconciliation_failed(result.is_err());
        result
    }

    pub(super) fn reconcile_core(
        &self,
        auth: &HostAuth,
        owner: &p::OwnerPrincipalRef,
        revision: u64,
        pending_restart: bool,
        now: p::Timestamp,
        startup: bool,
    ) -> p::Result<Option<p::ProductCoreActivationResponse>> {
        self.verify_web_configuration_auth(auth)?;
        let mut store = self
            .product_config
            .as_ref()
            .ok_or_else(configuration_unavailable)?
            .lock()
            .map_err(|_| configuration_unavailable())?;
        let current = store.snapshot()?;
        if current.owner.as_ref() != Some(owner) || current.active_revision != revision {
            return Err(p::Error("core reconciliation configuration changed".into()));
        }
        let operations = self.journal.operations(0)?;
        let unfinished: Vec<_> = operations
            .iter()
            .filter(|op| !op.stage.is_terminal() || op.pending_stage.is_some())
            .collect();
        if !unfinished.is_empty() {
            if startup {
                let mut release_checking = true;
                for operation in unfinished {
                    release_checking &= operation.stage == p::MaintenanceStage::Switching
                        && operation.pending_stage == Some(p::MaintenanceStage::Checking)
                        && self
                            .journal
                            .release_checking(&operation.operation_id)?
                            .is_some();
                }
                if release_checking {
                    // The updater owns service startup; the target Host only
                    // verifies its original Checking transaction. Yielding is
                    // not an activation failure or proof of readiness.
                    return Ok(None);
                }
            }
            return Err(p::Error(
                "core reconciliation requires original operation review".into(),
            ));
        }
        if self.core_was_explicitly_stopped(&operations)? {
            return Ok(None);
        }
        let status = self.services.inspect(p::BuiltinServiceId::Brain)?;
        if status.schema_version != p::SchemaVersion(1)
            || status.service != p::BuiltinServiceId::Brain
        {
            return Err(p::Error("core service observation is invalid".into()));
        }
        let action = match status.state {
            p::NativeServiceState::Stopped => p::ProductCoreAction::Start,
            p::NativeServiceState::Running if pending_restart && current.pending_restart => {
                if self.configuration_was_activated(&operations, revision)? {
                    return Ok(None);
                }
                p::ProductCoreAction::Restart
            }
            _ => return Ok(None),
        };
        let operation_id = self.automatic_core_operation_id(revision, action)?;
        if operations.iter().any(|op| op.operation_id == operation_id) {
            // One attempt per host generation/revision/action. A subsequent
            // stopped service belongs to OS recovery, not a retry loop here.
            return Ok(None);
        }
        self.activate_configuration_locked(
            auth,
            p::ProductCoreActivation {
                schema_version: p::PRODUCT_CONFIG_SCHEMA_VERSION,
                operation_id,
                expected_revision: revision,
                owner: owner.clone(),
                action,
                requested_deadline: now.saturating_add(120_000),
            },
            now,
            &mut store,
        )
        .map(Some)
    }

    fn core_was_explicitly_stopped(
        &self,
        operations: &[p::MaintenanceOperation],
    ) -> p::Result<bool> {
        for operation in operations.iter().rev() {
            match self.journal.command_kind(&operation.operation_id)? {
                p::MaintenanceKind::StopCore => return Ok(true),
                p::MaintenanceKind::StartCore | p::MaintenanceKind::RestartCore => {
                    return Ok(false)
                }
                _ => {}
            }
        }
        Ok(false)
    }

    fn configuration_was_activated(
        &self,
        operations: &[p::MaintenanceOperation],
        revision: u64,
    ) -> p::Result<bool> {
        for operation in operations {
            if matches!(
                self.journal.command_kind(&operation.operation_id)?,
                p::MaintenanceKind::ApplyValidatedConfig { config_revision, .. }
                    if config_revision == revision
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn automatic_core_operation_id(
        &self,
        revision: u64,
        action: p::ProductCoreAction,
    ) -> p::Result<String> {
        let digest = p::canonical_digest(&(
            "forme-automatic-core-activation-v2",
            &self.install_id,
            self.startup_revision,
            revision,
            action,
        ))?;
        let id = format!("core-auto-{}", digest.0.trim_start_matches("sha256:"));
        p::validate_product_id(&id)?;
        Ok(id)
    }
}
