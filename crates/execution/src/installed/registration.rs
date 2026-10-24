//! Closed native registration. No brain state, arbitrary service, shell or env.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use forme_protocol as p;
use windows_service::service::{
    Service, ServiceAccess, ServiceAction, ServiceActionType, ServiceConfig, ServiceDependency,
    ServiceErrorControl, ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo,
    ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

pub(super) mod release;
pub(super) mod release_lifecycle;
pub(super) mod updater;

/// Register only this daemon, without starting it. Existing registrations must
/// match exactly; partial/foreign registrations need explicit administrator repair.
pub fn register_windows_service(id: p::BuiltinServiceId, config: &Path) -> p::Result<()> {
    let info = desired(id, config, current_executable(id)?)?;
    let manager = manager(ServiceManagerAccess::CREATE_SERVICE | ServiceManagerAccess::CONNECT)?;
    let access = ServiceAccess::QUERY_CONFIG | ServiceAccess::CHANGE_CONFIG | ServiceAccess::START;
    match manager.create_service(&info, access) {
        Ok(service) => {
            service.update_failure_actions(recovery()).map_err(|_| {
                p::Error("service registered but recovery policy requires repair".into())
            })?;
            service
                .set_failure_actions_on_non_crash_failures(true)
                .map_err(|_| {
                    p::Error("service registered but failure reporting requires repair".into())
                })?;
            verify(&service, &info)
        }
        Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1073) => {
            let service = manager
                .open_service(&info.name, ServiceAccess::QUERY_CONFIG)
                .map_err(|_| p::Error("existing service cannot be inspected".into()))?;
            verify(&service, &info)
        }
        Err(_) => Err(p::Error("native service registration failed".into())),
    }
}

/// Remove a verified registration after observed stop. Owner data is untouched;
/// failure or unknown state never triggers deletion or a second operation.
pub fn unregister_windows_service(id: p::BuiltinServiceId, config: &Path) -> p::Result<()> {
    let info = desired(id, config, current_executable(id)?)?;
    let manager = manager(ServiceManagerAccess::CONNECT)?;
    let access = ServiceAccess::DELETE
        | ServiceAccess::STOP
        | ServiceAccess::QUERY_STATUS
        | ServiceAccess::QUERY_CONFIG;
    let service = match manager.open_service(&info.name, access) {
        Ok(service) => service,
        Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1060) => {
            return Ok(())
        }
        Err(_) => return Err(p::Error("native service cannot be inspected".into())),
    };
    verify(&service, &info)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let state = service
            .query_status()
            .map_err(|_| p::Error("service state is unknown; registration retained".into()))?
            .current_state;
        match state {
            ServiceState::Stopped => break,
            ServiceState::Running | ServiceState::Paused => {
                service
                    .stop()
                    .map_err(|_| p::Error("service stop failed; registration retained".into()))?;
                wait_stopped(&service, deadline)?;
                break;
            }
            ServiceState::StopPending | ServiceState::StartPending => {
                if Instant::now() >= deadline {
                    return Err(p::Error(
                        "service did not settle; registration retained".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => return Err(p::Error("service state cannot be safely stopped".into())),
        }
    }
    // Recheck configuration immediately before the destructive native call.
    verify(&service, &info)?;
    service
        .delete()
        .map_err(|_| p::Error("service registration could not be removed".into()))
}

fn wait_stopped(service: &Service, deadline: Instant) -> p::Result<()> {
    loop {
        let state = service
            .query_status()
            .map_err(|_| p::Error("service state is unknown; registration retained".into()))?
            .current_state;
        if state == ServiceState::Stopped {
            return Ok(());
        }
        if !matches!(
            state,
            ServiceState::StopPending | ServiceState::Running | ServiceState::Paused
        ) || Instant::now() >= deadline
        {
            return Err(p::Error(
                "service did not stop; registration retained".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn manager(access: ServiceManagerAccess) -> p::Result<ServiceManager> {
    ServiceManager::local_computer(None::<&str>, access)
        .map_err(|_| p::Error("Windows service manager is unavailable".into()))
}

fn current_executable(id: p::BuiltinServiceId) -> p::Result<PathBuf> {
    let executable = std::env::current_exe()
        .map_err(|_| p::Error("service executable is unavailable".into()))?;
    if executable.file_name() != Some(std::ffi::OsStr::new(&format!("{}.exe", id.binary_name()))) {
        return Err(p::Error(
            "service registration requires its own daemon executable".into(),
        ));
    }
    Ok(executable)
}

/// Read-only proof for the fixed privileged network adapter. SCM registrations
/// and recovery policy must match the running installation, never test paths.
pub(super) fn verify_network_installation() -> p::Result<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    let arguments: Vec<_> = std::env::args_os().collect();
    if arguments.len() != 3 || arguments[1] != "--config" {
        return Err(p::Error(
            "native network adapter requires installed Host arguments".into(),
        ));
    }
    let host_program = current_executable(p::BuiltinServiceId::Host)?;
    let host_config = PathBuf::from(&arguments[2]);
    if host_config.file_name() != Some(std::ffi::OsStr::new("host.json")) {
        return Err(p::Error(
            "native network adapter requires its fixed Host configuration".into(),
        ));
    }
    let web_program = host_program.with_file_name("forme-webd.exe");
    let web_config = host_config.with_file_name("web.json");
    let manager = manager(ServiceManagerAccess::CONNECT)?;
    for (id, config, program) in [
        (p::BuiltinServiceId::Host, &host_config, &host_program),
        (p::BuiltinServiceId::Web, &web_config, &web_program),
    ] {
        let expected = desired(id, config, program.clone())?;
        for path in [config, program] {
            super::reject_linked_ancestors(path)?;
            if !path.is_file() {
                return Err(p::Error(
                    "native network installation files are unavailable".into(),
                ));
            }
        }
        let service = manager
            .open_service(id.service_name(), ServiceAccess::QUERY_CONFIG)
            .map_err(|_| p::Error("native network service registration is unavailable".into()))?;
        verify(&service, &expected)?;
    }
    Ok((host_program, web_program, host_config, web_config))
}

fn desired(id: p::BuiltinServiceId, config: &Path, executable: PathBuf) -> p::Result<ServiceInfo> {
    for path in [config, executable.as_path()] {
        validate_path(path)?;
    }
    let account = match id {
        p::BuiltinServiceId::Host => None,
        _ => Some(OsString::from(
            id.required_account(p::ProductTarget::WindowsX86_64),
        )),
    };
    Ok(ServiceInfo {
        name: id.service_name().into(),
        display_name: id.service_name().into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: if id == p::BuiltinServiceId::Brain {
            ServiceStartType::OnDemand
        } else {
            ServiceStartType::AutoStart
        },
        error_control: ServiceErrorControl::Normal,
        executable_path: executable,
        launch_arguments: vec!["--config".into(), config.as_os_str().to_owned()],
        dependencies: if id == p::BuiltinServiceId::Host {
            vec![]
        } else {
            vec![ServiceDependency::Service(
                p::BuiltinServiceId::Host.service_name().into(),
            )]
        },
        account_name: account,
        account_password: None,
    })
}

fn validate_path(path: &Path) -> p::Result<()> {
    super::reject_linked_ancestors(path)?;
    let value = path
        .to_str()
        .ok_or_else(|| p::Error("service path is not valid text".into()))?;
    if value.chars().any(|ch| ch.is_control() || ch == '"') || value.ends_with(['/', '\\']) {
        return Err(p::Error("service path cannot be safely bound".into()));
    }
    Ok(())
}

fn verify(service: &Service, expected: &ServiceInfo) -> p::Result<()> {
    let actual = service
        .query_config()
        .map_err(|_| p::Error("existing service configuration is unavailable".into()))?;
    if !matches_config(&actual, expected) {
        return Err(p::Error(
            "existing service differs; refusing to replace or remove it".into(),
        ));
    }
    let policy = service
        .get_failure_actions()
        .map_err(|_| p::Error("service recovery policy cannot be inspected".into()))?;
    let non_crash = service
        .get_failure_actions_on_non_crash_failures()
        .map_err(|_| p::Error("service failure policy cannot be inspected".into()))?;
    let wanted = recovery();
    if policy.reset_period != wanted.reset_period
        || policy.actions != wanted.actions
        || policy.command.as_ref().is_some_and(|s| !s.is_empty())
        || policy.reboot_msg.as_ref().is_some_and(|s| !s.is_empty())
        || !non_crash
    {
        return Err(p::Error("existing service recovery policy differs".into()));
    }
    Ok(())
}

fn matches_config(actual: &ServiceConfig, expected: &ServiceInfo) -> bool {
    let Some(command) = actual.executable_path.to_str() else {
        return false;
    };
    let Some(executable) = expected.executable_path.to_str() else {
        return false;
    };
    let Some(config) = expected.launch_arguments[1].to_str() else {
        return false;
    };
    let account = actual
        .account_name
        .as_ref()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let expected_account = expected
        .account_name
        .as_ref()
        .and_then(|s| s.to_str())
        .unwrap_or("LocalSystem");
    actual.service_type == expected.service_type
        && actual.start_type == expected.start_type
        && actual.error_control == expected.error_control
        && actual.dependencies == expected.dependencies
        && account.eq_ignore_ascii_case(expected_account)
        && command_variants(executable, config)
            .iter()
            .any(|s| s.eq_ignore_ascii_case(command))
}

fn command_variants(executable: &str, config: &str) -> Vec<String> {
    let quoted = |value: &str| {
        let mut forms = vec![format!("\"{value}\"")];
        if !value.chars().any(char::is_whitespace) {
            forms.push(value.to_owned());
        }
        forms
    };
    let mut result = Vec::new();
    for image in quoted(executable) {
        for path in quoted(config) {
            result.push(format!("{image} --config {path}"));
        }
    }
    result
}

fn recovery() -> ServiceFailureActions {
    let policy = p::ServiceRecoveryPolicy::product_default();
    debug_assert!(policy.validate().is_ok());
    ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_millis(policy.reset_after.0)),
        reboot_msg: Some(OsString::new()),
        command: Some(OsString::new()),
        actions: Some(
            policy
                .restart_delays
                .into_iter()
                .map(|delay| ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: Duration::from_millis(delay.0),
                })
                .chain([ServiceAction {
                    action_type: ServiceActionType::None,
                    delay: Duration::ZERO,
                }])
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests;
