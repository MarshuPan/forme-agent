//! One on-demand operation under SCM; never a second permanent supervisor.
use super::config::{from_managed_config, launch_from_args, Launch};
use forme_protocol as p;
use windows_service::service::{
    ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};

windows_service::define_windows_service!(ffi_main, service_main);
pub(super) fn dispatch() -> Result<(), windows_service::Error> {
    windows_service::service_dispatcher::start(
        forme_execution::installed::RELEASE_UPDATER_SERVICE,
        ffi_main,
    )
}
fn service_main(_: Vec<std::ffi::OsString>) {
    let name = forme_execution::installed::RELEASE_UPDATER_SERVICE;
    let status = match windows_service::service_control_handler::register(name, |_| {
        windows_service::service_control_handler::ServiceControlHandlerResult::NotImplemented
    }) {
        Ok(status) => status,
        Err(_) => return,
    };
    let report = |state, failed| {
        status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(if failed { 1 } else { 0 }),
            checkpoint: 0,
            wait_hint: std::time::Duration::ZERO,
            process_id: None,
        })
    };
    if report(ServiceState::Running, false).is_err() {
        return;
    }
    let result = (|| {
        let Launch::ReleaseUpdater(path) = launch_from_args(std::env::args_os().skip(1))? else {
            return Err(p::Error("SCM release updater arguments are invalid".into()));
        };
        let runtime = from_managed_config(&path)?;
        runtime
            .release_updater
            .ok_or_else(|| {
                p::Error("installed release updater trust configuration is missing".into())
            })?
            .run()
    })();
    let _ = report(ServiceState::Stopped, result.is_err());
}
