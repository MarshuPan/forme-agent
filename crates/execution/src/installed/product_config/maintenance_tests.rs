//! Product-journal and native-adapter evidence, without installing OS services.
use super::{
    tests::{fixture, patch},
    *,
};
use crate::installed::{HostCredentials, HostManagement, NativeServiceManager, ProductHostBroker};
use std::sync::Arc;

const WEB: &str = "fixture-maintenance-web-abcdefghijklmnopqrstuvwxyz";
const ADMIN: &str = "fixture-maintenance-admin-abcdefghijklmnopqrstuvwxyz";
const AGENT: &str = "fixture-maintenance-agent-abcdefghijklmnopqrstuvwxyz";
const NOW: i64 = 1_000_000;

#[derive(Clone, Default)]
struct Services {
    calls: Arc<Mutex<Vec<p::NativeServiceAction>>>,
    unknown: bool,
}
impl NativeServiceManager for Services {
    fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service,
            state: p::NativeServiceState::Stopped,
        })
    }
    fn apply(&self, change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        self.calls.lock().unwrap().push(change.action);
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
    ProductHostBroker::open_with_service_manager_and_config(
        &root.join("maintenance.jsonl"),
        "install:config-test",
        HostCredentials::new(ADMIN.into(), Some(AGENT.into()))
            .unwrap()
            .with_web_configuration(Some(WEB.into()))
            .unwrap(),
        p::ProductTarget::WindowsX86_64,
        "0.1.0".into(),
        Some(config),
        services,
    )
    .unwrap()
}
fn command() -> p::ProductMaintenanceRequest {
    p::ProductMaintenanceRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: "maintain:first".into(),
        expected_revision: 1,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        action: p::ProductMaintenanceAction::RestartCore {},
        requested_deadline: NOW + 60_000,
    }
}

#[test]
fn owner_maintenance_is_once_across_concurrency_restart_and_lost_response() {
    let (directory, _, store) = fixture();
    store.lock().unwrap().apply(patch("save:one", 0)).unwrap();
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                assert_eq!(
                    host.owner_maintenance(&auth, command(), NOW).unwrap().stage,
                    p::MaintenanceStage::Committed
                );
            });
        }
    });
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Stop, p::NativeServiceAction::Start]
    );
    let snapshot = host.maintenance_snapshot(&auth).unwrap();
    assert!(snapshot.product_revision > 1);
    assert_eq!(snapshot.operations.len(), 1);
    drop(host);
    config.lock().unwrap().apply(patch("save:two", 1)).unwrap();
    let host = broker(directory.path(), config, services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    assert_eq!(
        host.owner_maintenance(&auth, command(), NOW + 600_000)
            .unwrap()
            .stage,
        p::MaintenanceStage::Committed
    );
    assert_eq!(services.calls.lock().unwrap().len(), 2);
    drop(host);
    let journal = std::fs::read_to_string(directory.path().join("maintenance.jsonl")).unwrap();
    assert!(journal.contains("owner_reauthenticated"));
    assert!(!journal.contains("local_administrator"));
}

#[test]
fn owner_maintenance_rejects_wrong_domain_owner_revision_deadline_and_schema() {
    let (directory, _, store) = fixture();
    store.lock().unwrap().apply(patch("save:one", 0)).unwrap();
    let services = Services::default();
    let host = broker(directory.path(), Arc::new(store), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    for mutation in 0..6 {
        let mut request = command();
        match mutation {
            0 => request.owner = p::OwnerPrincipalRef("owner:other".into()),
            1 => request.expected_revision = 3,
            2 => request.requested_deadline = NOW,
            3 => request.requested_deadline = NOW + 600_000,
            4 => request.schema_version = p::SchemaVersion(99),
            _ => request.expected_revision = 0,
        }
        assert!(host.owner_maintenance(&auth, request, NOW).is_err());
    }
    for auth in [
        host.authenticate(ADMIN).unwrap(),
        host.authenticate_agent(AGENT).unwrap(),
    ] {
        assert!(host.owner_maintenance(&auth, command(), NOW).is_err());
        assert!(host.maintenance_snapshot(&auth).is_err());
    }
    assert!(services.calls.lock().unwrap().is_empty());
    assert_eq!(
        host.maintenance_snapshot(&auth).unwrap().product_revision,
        1
    );
}

#[test]
fn owner_maintenance_exact_binding_and_unknown_outcome_prevent_reexecution() {
    let (directory, _, store) = fixture();
    store.lock().unwrap().apply(patch("save:one", 0)).unwrap();
    let services = Services {
        unknown: true,
        ..Services::default()
    };
    let host = broker(directory.path(), Arc::new(store), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let result = host.owner_maintenance(&auth, command(), NOW).unwrap();
    assert_eq!(result.stage, p::MaintenanceStage::RecoveryRequired);
    assert_eq!(services.calls.lock().unwrap().len(), 1);
    assert_eq!(
        host.owner_maintenance(&auth, command(), NOW).unwrap(),
        result
    );
    assert_eq!(
        host.owner_maintenance_operation(&auth, &command().operation_id)
            .unwrap(),
        result
    );
    for mutation in 0..3 {
        let mut changed = command();
        match mutation {
            0 => changed.requested_deadline += 1,
            1 => changed.expected_revision += 1,
            _ => changed.action = p::ProductMaintenanceAction::StopCore {},
        }
        assert!(host.owner_maintenance(&auth, changed, NOW).is_err());
    }
    let mut next = command();
    next.operation_id = "maintain:second".into();
    next.expected_revision = host.maintenance_snapshot(&auth).unwrap().product_revision;
    assert!(host.owner_maintenance(&auth, next, NOW).is_err());
    assert_eq!(services.calls.lock().unwrap().len(), 1);
}

#[test]
fn start_needs_current_valid_configuration_but_stop_does_not() {
    let (directory, _, store) = fixture();
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    assert!(host.owner_maintenance(&auth, command(), NOW).is_err());
    assert!(services.calls.lock().unwrap().is_empty());
    let mut stop = command();
    stop.action = p::ProductMaintenanceAction::StopCore {};
    assert_eq!(
        host.owner_maintenance(&auth, stop, NOW).unwrap().stage,
        p::MaintenanceStage::Committed
    );
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Stop]
    );
    let before = host.maintenance_snapshot(&auth).unwrap().product_revision;
    config.lock().unwrap().apply(patch("save:one", 0)).unwrap();
    config.lock().unwrap().apply(patch("save:two", 1)).unwrap();
    assert_eq!(
        host.maintenance_snapshot(&auth).unwrap().product_revision,
        before
    );
}

#[test]
fn browser_cannot_widen_the_fixed_maintenance_actions() {
    for value in [
        serde_json::json!({"kind":"shell","command":"whoami"}),
        serde_json::json!({"kind":"uninstall_plan","preserve_owner_data":false}),
        serde_json::json!({"kind":"start_core","service":"host"}),
    ] {
        assert!(serde_json::from_value::<p::ProductMaintenanceAction>(value).is_err());
    }
}
