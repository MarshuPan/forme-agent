//! Windows SCM adapter for the brain daemon.
//!
//! The service callback only supplies an OS-owned stop signal. Runtime
//! construction and graceful drain remain in the same public gateway path used
//! by foreground development.
use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use forme_gateway::{serve_managed_until, CoreServiceConfig};
use forme_protocol as p;

windows_service::define_windows_service!(ffi_service_main, windows_service_main);

pub(super) fn dispatch() -> Result<(), windows_service::Error> {
    windows_service::service_dispatcher::start(
        p::BuiltinServiceId::Brain.service_name(),
        ffi_service_main,
    )
}

pub(super) fn administrative_command() -> p::Result<bool> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, flag, path] if command == "--install-service" && flag == "--config" => {
            install(Path::new(path))?;
            Ok(true)
        }
        [command, flag, path] if command == "--uninstall-service" && flag == "--config" => {
            uninstall(Path::new(path))?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn install(config_path: &Path) -> p::Result<()> {
    // Core is registered before claim; its managed config is checked on start.
    forme_harness::register_windows_service(p::BuiltinServiceId::Brain, config_path)
}

fn uninstall(config_path: &Path) -> p::Result<()> {
    forme_harness::unregister_windows_service(p::BuiltinServiceId::Brain, config_path)
}

#[cfg(windows)]
fn windows_service_main(_arguments: Vec<OsString>) {
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let stop_tx = Mutex::new(Some(stop_tx));
    let service_name = p::BuiltinServiceId::Brain.service_name();
    let handler = move |event| {
        use windows_service::service::ServiceControl;
        use windows_service::service_control_handler::ServiceControlHandlerResult;
        match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                if let Ok(mut sender) = stop_tx.lock() {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(());
                    }
                }
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
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 1,
        wait_hint: Duration::from_secs(15),
        process_id: None,
    });
    let result = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(async move {
            let path = super::configuration_path(true)?
                .ok_or_else(|| p::Error("managed core configuration is required".into()))?;
            let config = CoreServiceConfig::load(&path)?;
            let drain_timeout = Duration::from_millis(config.drain_timeout_ms);
            let status = status;
            let shutdown = async move {
                stop_rx
                    .await
                    .map_err(|_| p::Error("brain service control disconnected".into()))?;
                status
                    .set_service_status(ServiceStatus {
                        service_type: ServiceType::OWN_PROCESS,
                        current_state: ServiceState::StopPending,
                        controls_accepted: ServiceControlAccept::empty(),
                        exit_code: ServiceExitCode::Win32(0),
                        checkpoint: 1,
                        wait_hint: drain_timeout,
                        process_id: None,
                    })
                    .map_err(|_| p::Error("brain service drain report failed".into()))?;
                Ok(())
            };
            serve_managed_until(
                config,
                move |_| {
                    status
                        .set_service_status(ServiceStatus {
                            service_type: ServiceType::OWN_PROCESS,
                            current_state: ServiceState::Running,
                            controls_accepted: ServiceControlAccept::STOP
                                | ServiceControlAccept::SHUTDOWN,
                            exit_code: ServiceExitCode::Win32(0),
                            checkpoint: 0,
                            wait_hint: Duration::default(),
                            process_id: None,
                        })
                        .map_err(|_| p::Error("brain service readiness report failed".into()))
                },
                shutdown,
            )
            .await
        }),
        Err(error) => Err(p::Error(format!(
            "brain service runtime unavailable: {error}"
        ))),
    };
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
