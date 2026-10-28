//! Reopen real product journals while simulating the native service boundary.
use super::*;

fn owner_action(
    host: &ProductHostBroker<Services>,
    auth: &crate::installed::HostAuth,
    action: p::ProductMaintenanceAction,
    id: &str,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let result = host
        .owner_maintenance(
            auth,
            p::ProductMaintenanceRequest {
                schema_version: p::SchemaVersion(1),
                operation_id: id.into(),
                expected_revision: host.maintenance_snapshot(auth).unwrap().product_revision,
                owner: p::OwnerPrincipalRef("owner:test".into()),
                action,
                requested_deadline: now + 60_000,
            },
            now,
        )
        .unwrap();
    assert_eq!(result.stage, p::MaintenanceStage::Committed);
}

#[test]
fn new_host_generation_starts_a_stopped_brain_without_reexecuting_the_old_operation() {
    let (directory, _, store) = fixture();
    let config = Arc::new(store);
    let services = Services {
        state: Arc::new(Mutex::new(Some(p::NativeServiceState::Stopped))),
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let response = host
        .apply_configuration(&auth, patch("save:boot-one", 0))
        .unwrap();
    let original = response.core_activation.unwrap().operation;
    drop(host);

    // The OS has stopped the service between Host generations; configuration
    // revision and original successful operation remain unchanged on disk.
    *services.state.lock().unwrap() = Some(p::NativeServiceState::Stopped);
    let host = broker(directory.path(), config, services.clone());
    host.reconcile_core_startup().unwrap();
    host.reconcile_core_startup().unwrap();
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let operations = host.maintenance_snapshot(&auth).unwrap().operations;
    assert_eq!(operations.len(), 2);
    assert_ne!(operations[0].operation_id, original.operation_id);
    assert_eq!(
        host.configuration_operation(&auth, &original.operation_id)
            .unwrap(),
        original
    );
    assert_eq!(host.configuration(&auth).unwrap().active_revision, 1);
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Start; 2]
    );

    // A later stop in this same generation does not become an auto-retry loop.
    *services.state.lock().unwrap() = Some(p::NativeServiceState::Stopped);
    host.reconcile_core_startup().unwrap();
    assert_eq!(services.calls.lock().unwrap().len(), 2);
}

#[test]
fn host_reopen_waits_for_ack_without_restarting_an_already_activated_running_brain() {
    let (directory, _, store) = fixture();
    let config = Arc::new(store);
    let services = Services::default();
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    host.apply_configuration(&auth, patch("save:await-ack", 0))
        .unwrap();
    assert_eq!(services.calls.lock().unwrap().len(), 2);
    drop(host);
    let host = broker(directory.path(), config, services.clone());
    host.reconcile_core_startup().unwrap();
    let auth = host.authenticate(ADMIN).unwrap();
    assert_eq!(
        host.status(&auth).unwrap().core_health,
        p::CoreHealth::Starting
    );
    assert_eq!(services.calls.lock().unwrap().len(), 2);
}

#[test]
fn host_reopen_preserves_unknown_start_and_never_creates_a_replacement_attempt() {
    let (directory, _, store) = fixture();
    let config = Arc::new(store);
    let services = Services {
        failure: true,
        state: Arc::new(Mutex::new(Some(p::NativeServiceState::Stopped))),
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    let response = host
        .apply_configuration(&auth, patch("save:lost-start", 0))
        .unwrap();
    let original = response.core_activation.unwrap().operation;
    assert_eq!(original.stage, p::MaintenanceStage::RecoveryRequired);
    drop(host);
    let host = broker(directory.path(), config, services.clone());
    assert!(host.reconcile_core_startup().is_err());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    assert_eq!(
        host.maintenance_snapshot(&auth).unwrap().operations,
        vec![original]
    );
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Start]
    );
}

#[test]
fn explicit_stop_survives_config_save_and_host_reopen_until_explicit_start() {
    let (directory, _, store) = fixture();
    let config = Arc::new(store);
    let services = Services {
        state: Arc::new(Mutex::new(Some(p::NativeServiceState::Stopped))),
        ..Services::default()
    };
    let host = broker(directory.path(), config.clone(), services.clone());
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    host.apply_configuration(&auth, patch("save:before-stop", 0))
        .unwrap();
    owner_action(
        &host,
        &auth,
        p::ProductMaintenanceAction::StopCore {},
        "owner:stop",
    );
    host.reconcile_core_startup().unwrap();
    let response = host
        .apply_configuration(&auth, patch("save:while-stopped", 1))
        .unwrap();
    assert!(response.core_activation.is_none());
    drop(host);
    let host = broker(directory.path(), config.clone(), services.clone());
    host.reconcile_core_startup().unwrap();
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Start, p::NativeServiceAction::Stop]
    );

    let auth = host.authenticate_web_configuration(WEB).unwrap();
    owner_action(
        &host,
        &auth,
        p::ProductMaintenanceAction::StartCore {},
        "owner:start",
    );
    drop(host);
    *services.state.lock().unwrap() = Some(p::NativeServiceState::Stopped);
    let host = broker(directory.path(), config, services.clone());
    host.reconcile_core_startup().unwrap();
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![
            p::NativeServiceAction::Start,
            p::NativeServiceAction::Stop,
            p::NativeServiceAction::Start,
            p::NativeServiceAction::Start
        ]
    );
}

#[test]
fn concurrent_startup_checks_admit_one_automatic_service_effect() {
    let (directory, _, store) = fixture();
    store
        .lock()
        .unwrap()
        .apply(patch("save:before-host", 0))
        .unwrap();
    let services = Services {
        state: Arc::new(Mutex::new(Some(p::NativeServiceState::Stopped))),
        ..Services::default()
    };
    let host = broker(directory.path(), Arc::new(store), services.clone());
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| host.reconcile_core_startup().unwrap());
        }
    });
    assert_eq!(
        *services.calls.lock().unwrap(),
        vec![p::NativeServiceAction::Start]
    );
    let auth = host.authenticate_web_configuration(WEB).unwrap();
    assert_eq!(
        host.maintenance_snapshot(&auth).unwrap().operations.len(),
        1
    );
}
