//! SCM status and control signals; OS remains the sole service supervisor.
use super::run_host;
use forme_protocol as p;
use std::time::Duration;

windows_service::define_windows_service!(ffi_service_main, windows_service_main);

pub(super) fn dispatch() -> Result<(), windows_service::Error> {
    windows_service::service_dispatcher::start(
        p::BuiltinServiceId::Host.service_name(),
        ffi_service_main,
    )
}

#[cfg(windows)]
fn windows_service_main(_arguments: Vec<std::ffi::OsString>) {
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    let service_name = p::BuiltinServiceId::Host.service_name();
    let handler =
        move |event| -> windows_service::service_control_handler::ServiceControlHandlerResult {
            use windows_service::service::ServiceControl;
            use windows_service::service_control_handler::ServiceControlHandlerResult;
            match event {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    let _ = shutdown_tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };
    let status = match windows_service::service_control_handler::register(service_name, handler) {
        Ok(status) => status,
        Err(_) => return,
    };
    use windows_service::service::{
        ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
    };
    let _ = status.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 1,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    });
    let result = run_host(Some(shutdown_rx), false, true, || {
        status
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })
            .map_err(|_| p::Error("host service could not report readiness".into()))
    });
    let _ = status.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(if result.is_ok() { 0 } else { 1 }),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    });
}
