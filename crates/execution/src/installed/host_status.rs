#![forbid(unsafe_code)]

use forme_protocol as p;

use super::{
    NativeServiceManager, ProductHostBroker, ISSUE_CORE_CONFIGURATION_PENDING,
    ISSUE_CORE_RECONCILIATION_UNAVAILABLE, ISSUE_CORE_SERVICE_STATE_UNKNOWN,
    ISSUE_CORE_SERVICE_STATUS_UNAVAILABLE, ISSUE_PRODUCT_LIFECYCLE_RECOVERY_REQUIRED,
    ISSUE_PRODUCT_SERVICE_STATUS_UNAVAILABLE,
};

impl<M: NativeServiceManager> ProductHostBroker<M> {
    pub(super) fn status_for(&self) -> p::Result<p::HostStatus> {
        let operations = self.journal.operations(0)?;
        let recovery = operations.iter().any(|operation| {
            operation.stage == p::MaintenanceStage::RecoveryRequired
                || operation.pending_stage.is_some()
        });
        let pending = operations
            .iter()
            .any(|operation| !operation.stage.is_terminal() || operation.pending_stage.is_some());
        let (native, service_status_unavailable) =
            match self.services.inspect(p::BuiltinServiceId::Brain) {
                Ok(status) => (status, false),
                Err(_) => (
                    p::NativeServiceStatus {
                        schema_version: p::SchemaVersion(1),
                        service: p::BuiltinServiceId::Brain,
                        state: p::NativeServiceState::Unknown,
                    },
                    true,
                ),
            };
        let configuration = self
            .product_config
            .as_ref()
            .map(|store| {
                let mut store = store
                    .lock()
                    .map_err(|_| p::Error("managed product configuration is unavailable".into()))?;
                store.snapshot()
            })
            .transpose()?;
        let startup_reconciliation_failed = *self
            .startup_reconciliation_failed
            .lock()
            .map_err(|_| p::Error("host startup status is unavailable".into()))?;
        let configuration_ready = configuration.as_ref().is_some_and(|view| {
            view.configured
                && view.active_revision > 0
                && view.acknowledged_revision == Some(view.active_revision)
        });
        let core_health = match native.state {
            p::NativeServiceState::Running if startup_reconciliation_failed => {
                p::CoreHealth::Degraded
            }
            p::NativeServiceState::Running if configuration_ready => p::CoreHealth::Ready,
            p::NativeServiceState::Running => p::CoreHealth::Starting,
            p::NativeServiceState::Stopped => p::CoreHealth::Stopped,
            p::NativeServiceState::Starting => p::CoreHealth::Starting,
            p::NativeServiceState::Stopping => p::CoreHealth::Degraded,
            p::NativeServiceState::NotInstalled => p::CoreHealth::Unconfigured,
            p::NativeServiceState::Failed | p::NativeServiceState::Unknown => {
                p::CoreHealth::Unreachable
            }
        };
        let product_state = if recovery {
            p::ProductState::RecoveryRequired
        } else if pending {
            p::ProductState::Maintenance
        } else {
            match core_health {
                p::CoreHealth::Ready => p::ProductState::Ready,
                p::CoreHealth::Starting => p::ProductState::Starting,
                p::CoreHealth::Stopped => p::ProductState::Stopped,
                p::CoreHealth::Unconfigured => p::ProductState::ConfigurationRequired,
                _ => p::ProductState::Degraded,
            }
        };
        let mut allowed_operations = vec![
            "inspect".into(),
            "start_core".into(),
            "stop_core".into(),
            "restart_core".into(),
            "plan".into(),
            "execute".into(),
        ];
        let (network_change_available, network_status_unavailable) =
            match self.network_change_available() {
                Ok(available) => (available, false),
                Err(_) => (false, true),
            };
        if self.product_config.is_some() && network_change_available {
            allowed_operations.push("configure_lan".into());
        }
        if self.release_stager.is_some() {
            allowed_operations.extend(["stage_signed_release".into(), "uninstall_plan".into()]);
        }
        if self.release_stager.is_some()
            && (self.release_worker.is_some()
                || self.release_updater.as_ref().is_some_and(|u| u.available()))
        {
            allowed_operations.extend(["upgrade".into(), "rollback_compatible_release".into()]);
        }
        if self
            .restore_worker
            .as_ref()
            .is_some_and(|worker| worker.preflight().is_ok())
        {
            allowed_operations.push("commit_restore".into());
        }
        let mut issue_refs = Vec::new();
        if service_status_unavailable {
            issue_refs.push(ISSUE_CORE_SERVICE_STATUS_UNAVAILABLE.into());
        } else if native.state == p::NativeServiceState::Unknown {
            issue_refs.push(ISSUE_CORE_SERVICE_STATE_UNKNOWN.into());
        }
        if network_status_unavailable {
            issue_refs.push(ISSUE_PRODUCT_SERVICE_STATUS_UNAVAILABLE.into());
        }
        if recovery {
            issue_refs.push(ISSUE_PRODUCT_LIFECYCLE_RECOVERY_REQUIRED.into());
        }
        if configuration.as_ref().is_some_and(|view| {
            view.configured
                && view.active_revision > 0
                && view.acknowledged_revision != Some(view.active_revision)
        }) {
            issue_refs.push(ISSUE_CORE_CONFIGURATION_PENDING.into());
        }
        if startup_reconciliation_failed {
            issue_refs.push(ISSUE_CORE_RECONCILIATION_UNAVAILABLE.into());
        }

        Ok(p::HostStatus {
            schema_version: p::SchemaVersion(1),
            install_id_safe_ref: self.install_id.clone(),
            product_state,
            core_health,
            active_release: self.active_release_id()?,
            active_config_revision: configuration
                .as_ref()
                .map_or(0, |view| view.active_revision),
            acknowledged_config_revision: configuration
                .as_ref()
                .and_then(|view| view.acknowledged_revision),
            issue_refs,
            allowed_operations,
        })
    }

    pub(super) fn mark_startup_reconciliation_failed(&self, failed: bool) {
        if let Ok(mut state) = self.startup_reconciliation_failed.lock() {
            *state = failed;
        }
    }
}
