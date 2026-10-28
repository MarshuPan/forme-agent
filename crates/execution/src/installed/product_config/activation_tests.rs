//! Service-manager contract fixtures. These do not install native services.
use super::{
    tests::{fixture, patch},
    *,
};
use crate::installed::{HostCredentials, HostManagement, NativeServiceManager, ProductHostBroker};
use std::sync::Arc;

const WEB: &str = "web-configuration-fixture-abcdefghijklmnopqrstuvwxyz";
const ADMIN: &str = "admin-fixture-abcdefghijklmnopqrstuvwxyz";
const AGENT: &str = "agent-fixture-abcdefghijklmnopqrstuvwxyz";
const NOW: i64 = 1_000_000;

#[derive(Clone, Default)]
struct Services {
    calls: Arc<Mutex<Vec<p::NativeServiceAction>>>,
    unknown: bool,
    failure: bool,
    inspect_failure: bool,
    state: Arc<Mutex<Option<p::NativeServiceState>>>,
}
impl NativeServiceManager for Services {
    fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        if self.inspect_failure {
            return Err(p::Error("fixture service status unavailable".into()));
        }
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service,
            state: self
                .state
                .lock()
                .unwrap()
                .unwrap_or(p::NativeServiceState::Running),
        })
    }
    fn apply(&self, change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        assert_eq!(change.service, p::BuiltinServiceId::Brain);
        self.calls.lock().unwrap().push(change.action);
        if self.failure {
            return Err(p::Error(
                "fixture lost receipt after possible effect".into(),
            ));
        }
        if !self.unknown {
            *self.state.lock().unwrap() = Some(match change.action {
                p::NativeServiceAction::Start => p::NativeServiceState::Running,
                p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
            });
        }
        Ok(p::NativeReceipt {
            schema_version: p::SchemaVersion(1),
            service: change.service,
            requested: change.action,
            observed: if self.unknown {
                p::NativeServiceState::Unknown
            } else {
                match change.action {
                    p::NativeServiceAction::Start => p::NativeServiceState::Running,
                    p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
                }
            },
        })
    }
}
fn broker(
    root: &Path,
    config: Arc<Mutex<ProductConfigStore>>,
    services: Services,
) -> ProductHostBroker<Services> {
    let credentials = HostCredentials::new(ADMIN.into(), Some(AGENT.into()))
        .unwrap()
        .with_web_configuration(Some(WEB.into()))
        .unwrap();
    ProductHostBroker::open_with_service_manager_and_config(
        &root.join("product.jsonl"),
        "install:config-test",
        credentials,
        p::ProductTarget::WindowsX86_64,
        "0.1.0".into(),
        Some(config),
        services,
    )
    .unwrap()
}
fn activation() -> p::ProductCoreActivation {
    p::ProductCoreActivation {
        schema_version: p::SchemaVersion(1),
        operation_id: "activate:first".into(),
        expected_revision: 1,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        action: p::ProductCoreAction::Start,
        requested_deadline: NOW + 60_000,
    }
}

fn patch_v2(operation: &str, revision: u64) -> p::ProductConfigPatchV2 {
    let base = patch(operation, revision);
    p::ProductConfigPatchV2 {
        schema_version: p::PRODUCT_SETTINGS_SCHEMA_VERSION,
        operation_id: base.operation_id,
        expected_revision: base.expected_revision,
        owner: base.owner,
        models: base.models,
        brain: base.brain,
        capabilities: base.capabilities,
        network: None,
        proxy: None,
    }
}

#[test]
fn concurrent_activation_and_replay_after_restart_change_service_exactly_once() {
    let (directory, _path, store) = fixture();
    store.lock().unwrap().apply(patch("save:one", 0)).unwrap();
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..6 {
            scope.spawn(|| {
                let result = host
                    .activate_configuration(&auth, activation(), NOW)
                    .unwrap();
                assert_eq!(result.operation.stage, p::MaintenanceStage::Committed);
                assert!(result.operation.pending_stage.is_none());
                assert_eq!(result.configuration.acknowledged_revision, None);
            });
        }
    });
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Start]
    );
    drop(host);
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    config.lock().unwrap().apply(patch("save:two", 1)).unwrap();
    // A receipt remains queryable after a later config revision and expiration.
    assert_eq!(
        host.activate_configuration(&auth, activation(), NOW + 600_000)
            .unwrap()
            .operation
            .stage,
        p::MaintenanceStage::Committed
    );
    let mut changed = activation();
    changed.action = p::ProductCoreAction::Restart;
    assert!(host.activate_configuration(&auth, changed, NOW).is_err());
    assert_eq!(services.calls.lock().unwrap().len(), 1);
}

#[test]
fn activation_rejects_stale_owner_expiry_and_credential_domain_before_service_io() {
    let (directory, _path, store) = fixture();
    store.lock().unwrap().apply(patch("save:one", 0)).unwrap();
    let services = Services::default();
    let host = broker(directory.path(), Arc::new(store), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    for mutate in [0, 1, 2, 3, 4] {
        let mut request = activation();
        match mutate {
            0 => request.expected_revision = 2,
            1 => request.owner = p::OwnerPrincipalRef("owner:foreign".into()),
            2 => request.requested_deadline = NOW,
            3 => request.requested_deadline = NOW + 600_000,
            _ => request.schema_version = p::SchemaVersion(99),
        }
        assert!(host.activate_configuration(&auth, request, NOW).is_err());
    }
    for auth in [
        host.authenticate(ADMIN).unwrap(),
        host.authenticate_agent(AGENT).unwrap(),
    ] {
        assert!(host
            .activate_configuration(&auth, activation(), NOW)
            .is_err());
    }
    assert!(host.authenticate_web_configuration(ADMIN).is_err());
    assert!(services.calls.lock().unwrap().is_empty());
}

#[test]
fn restart_requires_stop_receipt_and_unknown_or_failed_effect_is_never_repeated() {
    for failure in [false, true] {
        let (directory, _path, store) = fixture();
        store.lock().unwrap().apply(patch("save:one", 0)).unwrap();
        let config = Arc::new(store);
        let services = Services {
            unknown: !failure,
            failure,
            ..Services::default()
        };
        let host = broker(directory.path(), config.clone(), services.clone());
        let auth = host.authenticate_web_configuration(WEB).unwrap();
        let mut request = activation();
        request.action = p::ProductCoreAction::Restart;
        let result = host
            .activate_configuration(&auth, request.clone(), NOW)
            .unwrap();
        assert_eq!(
            result.operation.stage,
            p::MaintenanceStage::RecoveryRequired
        );
        assert_eq!(
            *services.calls.lock().unwrap(),
            vec![p::NativeServiceAction::Stop]
        );
        drop(host);
        let host = broker(directory.path(), config, services.clone());
        let auth = host.authenticate_web_configuration(WEB).unwrap();
        assert_eq!(
            host.activate_configuration(&auth, request.clone(), NOW)
                .unwrap()
                .operation
                .stage,
            p::MaintenanceStage::RecoveryRequired
        );
        request.operation_id = "activate:replacement".into();
        assert!(host.activate_configuration(&auth, request, NOW).is_err());
        assert_eq!(services.calls.lock().unwrap().len(), 1);
    }
}

#[test]
fn verified_restart_orders_stop_then_start_and_does_not_forge_core_ack() {
    let (directory, _path, store) = fixture();
    store.lock().unwrap().apply(patch("save:one", 0)).unwrap();
    let services = Services::default();
    let host = broker(directory.path(), Arc::new(store), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let mut request = activation();
    request.action = p::ProductCoreAction::Restart;
    let result = host.activate_configuration(&auth, request, NOW).unwrap();
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Stop, p::NativeServiceAction::Start]
    );
    assert_eq!(result.operation.stage, p::MaintenanceStage::Committed);
    assert_eq!(result.configuration.acknowledged_revision, None);
    assert!(result.configuration.pending_restart);
}

#[test]
fn configuration_write_automatically_starts_a_stopped_brain() {
    let (directory, _path, store) = fixture();
    let services = Services {
        state: Arc::new(Mutex::new(Some(p::NativeServiceState::Stopped))),
        ..Services::default()
    };
    let host = broker(directory.path(), Arc::new(store), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();

    let response = host
        .apply_configuration(&auth, patch("save:auto-start", 0))
        .unwrap();

    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Start]
    );
    assert_eq!(
        response.core_activation.as_ref().unwrap().operation.stage,
        p::MaintenanceStage::Committed
    );
}

#[test]
fn configuration_write_automatically_restarts_a_running_brain_for_new_revision() {
    let (directory, _path, store) = fixture();
    let services = Services::default();
    let host = broker(directory.path(), Arc::new(store), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();

    let response = host
        .apply_configuration(&auth, patch("save:auto-restart", 0))
        .unwrap();

    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Stop, p::NativeServiceAction::Start]
    );
    assert_eq!(
        response.core_activation.as_ref().unwrap().operation.stage,
        p::MaintenanceStage::Committed
    );
}

#[test]
fn automatic_reconciliation_does_not_touch_unknown_service_or_repeat_a_committed_operation() {
    let (directory, _path, store) = fixture();
    let unknown_services = Services {
        state: Arc::new(Mutex::new(Some(p::NativeServiceState::Unknown))),
        ..Services::default()
    };
    let unknown_host = broker(directory.path(), Arc::new(store), unknown_services.clone());
    let auth = unknown_host.authenticate_web_configuration(WEB).unwrap();
    let response = unknown_host
        .apply_configuration(&auth, patch("save:auto-unknown", 0))
        .unwrap();
    assert!(response.core_activation.is_none());
    assert!(unknown_services.calls.lock().unwrap().is_empty());

    let (directory, _path, store) = fixture();
    let services = Services {
        state: Arc::new(Mutex::new(Some(p::NativeServiceState::Stopped))),
        ..Services::default()
    };
    let config = Arc::new(store);
    let host = broker(directory.path(), config, services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    host.apply_configuration(&auth, patch("save:auto-replay", 0))
        .unwrap();
    host.reconcile_core_startup().unwrap();
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Start]
    );
}

#[test]
fn automatic_reconciliation_failure_is_visible_without_losing_saved_configuration() {
    let (directory, _path, store) = fixture();
    let services = Services {
        inspect_failure: true,
        ..Services::default()
    };
    let config = Arc::new(store);
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();

    let response = host
        .apply_configuration(&auth, patch("save:status-unavailable", 0))
        .unwrap();

    assert!(response.applied);
    assert!(response.core_activation.is_none());
    assert_eq!(response.view.active_revision, 1);
    assert!(response
        .view
        .issue_refs
        .iter()
        .any(|issue| issue == super::super::ISSUE_CORE_RECONCILIATION_UNAVAILABLE));
    assert!(services.calls.lock().unwrap().is_empty());

    let status_auth = host.authenticate(ADMIN).unwrap();
    let status = host.status(&status_auth).unwrap();
    assert_eq!(status.core_health, p::CoreHealth::Unreachable);
    assert!(status
        .issue_refs
        .iter()
        .any(|issue| issue == super::super::ISSUE_CORE_SERVICE_STATUS_UNAVAILABLE));
    assert!(status
        .issue_refs
        .iter()
        .any(|issue| issue == super::super::ISSUE_CORE_CONFIGURATION_PENDING));

    // Startup recovery remains available to the Host; an unavailable service
    // manager is observed and reported, never converted into a blind action.
    host.reconcile_core_startup().unwrap();
    assert!(services.calls.lock().unwrap().is_empty());
}

#[test]
fn v2_configuration_reconciliation_failure_is_visible_without_losing_saved_configuration() {
    let (directory, _path, store) = fixture();
    let services = Services {
        inspect_failure: true,
        ..Services::default()
    };
    let config = Arc::new(store);
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();

    let response = host
        .apply_configuration_v2(&auth, patch_v2("save:v2-status-unavailable", 0))
        .unwrap();

    assert!(response.applied);
    assert!(response.core_activation.is_none());
    assert_eq!(response.view.active_revision, 1);
    assert!(response
        .view
        .issue_refs
        .iter()
        .any(|issue| issue == super::super::ISSUE_CORE_RECONCILIATION_UNAVAILABLE));
    assert!(services.calls.lock().unwrap().is_empty());
}
