//! Fixed on-demand SCM maintenance service, with no automatic retry policy.
use super::*;
use crate::installed::RELEASE_UPDATER_SERVICE;

fn wanted(installation: &p::InstallationManifest, config: &Path) -> p::Result<ServiceInfo> {
    super::super::validate_installation_paths(installation)?;
    let image = installation.paths.releases.join("bin/forme-hostd.exe");
    if installation.target != p::ProductTarget::WindowsX86_64
        || config != installation.paths.config.join("host.json")
    {
        return Err(invalid());
    }
    validate_path(&image)?;
    validate_path(config)?;
    Ok(ServiceInfo {
        name: RELEASE_UPDATER_SERVICE.into(),
        display_name: RELEASE_UPDATER_SERVICE.into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::OnDemand,
        error_control: ServiceErrorControl::Normal,
        executable_path: image,
        launch_arguments: vec![
            "--release-updater".into(),
            "--config".into(),
            config.as_os_str().to_owned(),
        ],
        dependencies: Vec::new(),
        account_name: None,
        account_password: None,
    })
}
fn policy() -> ServiceFailureActions {
    ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::Never,
        reboot_msg: Some(OsString::new()),
        command: Some(OsString::new()),
        actions: Some(vec![ServiceAction {
            action_type: ServiceActionType::None,
            delay: Duration::ZERO,
        }]),
    }
}
fn verify(service: &Service, info: &ServiceInfo) -> p::Result<()> {
    verify_service_sid(service, &info.name).map_err(|_| invalid())?;
    let actual = service.query_config().map_err(|_| invalid())?;
    let recovery = service.get_failure_actions().map_err(|_| invalid())?;
    let non_crash = service
        .get_failure_actions_on_non_crash_failures()
        .map_err(|_| invalid())?;
    if !matches(&actual, info)
        || recovery.reset_period != policy().reset_period
        || recovery.actions != policy().actions
        || recovery.command.as_ref().is_some_and(|s| !s.is_empty())
        || recovery.reboot_msg.as_ref().is_some_and(|s| !s.is_empty())
        || non_crash
    {
        return Err(invalid());
    }
    Ok(())
}
fn matches(actual: &ServiceConfig, info: &ServiceInfo) -> bool {
    let Some(image) = info.executable_path.to_str() else {
        return false;
    };
    let Some(config) = info.launch_arguments[2].to_str() else {
        return false;
    };
    let Some(actual_command) = actual.executable_path.to_str() else {
        return false;
    };
    let wanted = command_variants(image, config)
        .into_iter()
        .map(|s| s.replacen(" --config ", " --release-updater --config ", 1));
    actual.service_type == info.service_type
        && actual.start_type == info.start_type
        && actual.error_control == info.error_control
        && actual.dependencies.is_empty()
        && actual
            .account_name
            .as_ref()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .eq_ignore_ascii_case("LocalSystem")
        && wanted
            .into_iter()
            .any(|s| service_commands_match(&s, actual_command))
}
pub fn register(installation: &p::InstallationManifest, config: &Path) -> p::Result<()> {
    let info = wanted(installation, config)?;
    if std::env::current_exe().map_err(|_| invalid())? != info.executable_path {
        return Err(invalid());
    }
    let manager = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    let access = ServiceAccess::QUERY_CONFIG | ServiceAccess::CHANGE_CONFIG;
    match manager.create_service(&info, access) {
        Ok(service) => {
            ensure_service_sid(&service, std::ffi::OsStr::new(RELEASE_UPDATER_SERVICE))?;
            service
                .update_failure_actions(policy())
                .map_err(|_| invalid())?;
            service
                .set_failure_actions_on_non_crash_failures(false)
                .map_err(|_| invalid())?;
            verify(&service, &info)
        }
        Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1073) => {
            let service = manager
                .open_service(
                    RELEASE_UPDATER_SERVICE,
                    ServiceAccess::QUERY_CONFIG | ServiceAccess::CHANGE_CONFIG,
                )
                .map_err(|_| invalid())?;
            ensure_service_sid(&service, std::ffi::OsStr::new(RELEASE_UPDATER_SERVICE))?;
            verify(&service, &info)
        }
        Err(_) => Err(invalid()),
    }
}
pub fn unregister(installation: &p::InstallationManifest, config: &Path) -> p::Result<()> {
    let info = wanted(installation, config)?;
    let manager = manager(ServiceManagerAccess::CONNECT)?;
    let service = match manager.open_service(
        RELEASE_UPDATER_SERVICE,
        ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS | ServiceAccess::DELETE,
    ) {
        Ok(service) => service,
        Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1060) => return Ok(()),
        Err(_) => return Err(invalid()),
    };
    verify(&service, &info)?;
    // An executing self-update cannot be uninstalled out from under itself.
    if service.query_status().map_err(|_| invalid())?.current_state != ServiceState::Stopped {
        return Err(invalid());
    }
    service.delete().map_err(|_| invalid())
}
pub(in crate::installed) fn verify_installed(
    installation: &p::InstallationManifest,
    config: &Path,
) -> p::Result<()> {
    let info = wanted(installation, config)?;
    let manager = manager(ServiceManagerAccess::CONNECT)?;
    let service = manager
        .open_service(RELEASE_UPDATER_SERVICE, ServiceAccess::QUERY_CONFIG)
        .map_err(|_| invalid())?;
    verify(&service, &info)
}
pub(in crate::installed) fn observe_stopped(
    installation: &p::InstallationManifest,
    config: &Path,
) -> p::Result<p::SchemaDigest> {
    let info = wanted(installation, config)?;
    let manager = manager(ServiceManagerAccess::CONNECT)?;
    let service = manager
        .open_service(
            RELEASE_UPDATER_SERVICE,
            ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|_| invalid())?;
    verify(&service, &info)?;
    let status = service.query_status().map_err(|_| invalid())?;
    if status.current_state != ServiceState::Stopped || status.process_id.is_some() {
        return Err(invalid());
    }
    p::canonical_digest(&(
        "forme-release-recovery-updater-v1",
        format!("{:?}", service.query_config().map_err(|_| invalid())?),
        format!(
            "{:?}",
            service.get_failure_actions().map_err(|_| invalid())?
        ),
    ))
}
pub(in crate::installed) fn launch(
    installation: &p::InstallationManifest,
    config: &Path,
) -> p::Result<()> {
    let info = wanted(installation, config)?;
    let manager = manager(ServiceManagerAccess::CONNECT)?;
    let service = manager
        .open_service(
            RELEASE_UPDATER_SERVICE,
            ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS | ServiceAccess::START,
        )
        .map_err(|_| invalid())?;
    verify(&service, &info)?;
    if service.query_status().map_err(|_| invalid())?.current_state != ServiceState::Stopped {
        return Err(invalid());
    }
    service
        .start::<&std::ffi::OsStr>(&[])
        .map_err(|_| invalid())
}
fn invalid() -> p::Error {
    p::Error("fixed independent release updater registration is unavailable or differs".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maintenance_registration_never_matches_business_daemon_or_automatic_retries() {
        let info = ServiceInfo {
            name: RELEASE_UPDATER_SERVICE.into(),
            display_name: "test".into(),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::OnDemand,
            error_control: ServiceErrorControl::Normal,
            executable_path: "C:\\ForMe\\bin\\forme-hostd.exe".into(),
            launch_arguments: vec![
                "--release-updater".into(),
                "--config".into(),
                "C:\\ForMe\\host.json".into(),
            ],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };
        let mut actual = ServiceConfig {
            service_type: info.service_type,
            start_type: info.start_type,
            error_control: info.error_control,
            executable_path:
                "C:\\ForMe\\bin\\forme-hostd.exe --release-updater --config C:\\ForMe\\host.json"
                    .into(),
            load_order_group: None,
            tag_id: 0,
            dependencies: vec![],
            account_name: Some("LocalSystem".into()),
            display_name: "test".into(),
        };
        assert!(matches(&actual, &info));
        actual.executable_path =
            "C:\\ForMe\\bin\\forme-hostd.exe --config C:\\ForMe\\host.json".into();
        assert!(!matches(&actual, &info));
        actual.start_type = ServiceStartType::AutoStart;
        assert!(!matches(&actual, &info));
        assert_eq!(
            policy().actions.unwrap()[0].action_type,
            ServiceActionType::None
        );
    }
}
