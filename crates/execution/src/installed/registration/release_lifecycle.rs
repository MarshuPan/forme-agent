//! Exact image/account/recovery checks and one shared absolute lifecycle deadline.
use super::*;

fn service(
    installation: &p::InstallationManifest,
    root: &Path,
    id: p::BuiltinServiceId,
    modify: bool,
) -> p::Result<(Service, ServiceInfo)> {
    super::super::validate_installation_paths(installation)?;
    let config = installation.paths.config.join(match id {
        p::BuiltinServiceId::Host => "host.json",
        p::BuiltinServiceId::Web => "web.json",
        p::BuiltinServiceId::Brain => "core/core.json",
    });
    let info = desired(
        id,
        &config,
        root.join(format!("bin/{}.exe", id.binary_name())),
    )?;
    let manager = manager(ServiceManagerAccess::CONNECT)?;
    let mut access = ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS;
    if modify {
        access |= ServiceAccess::START | ServiceAccess::STOP;
    }
    let service = manager
        .open_service(id.service_name(), access)
        .map_err(|_| invalid())?;
    verify(&service, &info)?;
    Ok((service, info))
}
pub(in crate::installed) fn observe(
    installation: &p::InstallationManifest,
    root: &Path,
    id: p::BuiltinServiceId,
    wanted: Option<p::NativeServiceState>,
) -> p::Result<p::SchemaDigest> {
    let (service, info) = service(installation, root, id, false)?;
    let status = service.query_status().map_err(|_| invalid())?;
    let state = match status.current_state {
        ServiceState::Running => p::NativeServiceState::Running,
        ServiceState::Stopped => p::NativeServiceState::Stopped,
        _ => return Err(invalid()),
    };
    if wanted.is_some_and(|wanted| state != wanted) {
        return Err(invalid());
    }
    verify(&service, &info)?;
    p::canonical_digest(&(
        "release-service-state-v1",
        id,
        state,
        status.process_id,
        format!("{:?}", service.query_config().map_err(|_| invalid())?),
    ))
}
pub(in crate::installed) fn change(
    installation: &p::InstallationManifest,
    root: &Path,
    id: p::BuiltinServiceId,
    action: p::NativeServiceAction,
    deadline: Instant,
) -> p::Result<p::SchemaDigest> {
    if Instant::now() >= deadline {
        return Err(invalid());
    }
    let (service, info) = service(installation, root, id, true)?;
    let wanted = match action {
        p::NativeServiceAction::Start => ServiceState::Running,
        p::NativeServiceAction::Stop => ServiceState::Stopped,
    };
    let status = service.query_status().map_err(|_| invalid())?.current_state;
    if status != wanted {
        if Instant::now() >= deadline {
            return Err(invalid());
        }
        match (action, status) {
            (p::NativeServiceAction::Start, ServiceState::Stopped) => service
                .start::<&std::ffi::OsStr>(&[])
                .map_err(|_| invalid())?,
            (p::NativeServiceAction::Stop, ServiceState::Running) => {
                service.stop().map_err(|_| invalid())?;
            }
            _ => return Err(invalid()),
        }
    }
    loop {
        if Instant::now() >= deadline {
            return Err(invalid());
        }
        verify(&service, &info)?;
        let state = service.query_status().map_err(|_| invalid())?.current_state;
        if state == wanted {
            break;
        }
        if !matches!(
            state,
            ServiceState::StartPending | ServiceState::StopPending
        ) {
            return Err(invalid());
        }
        std::thread::sleep(
            Duration::from_millis(25).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    observe(
        installation,
        root,
        id,
        Some(match action {
            p::NativeServiceAction::Start => p::NativeServiceState::Running,
            p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
        }),
    )
}
fn invalid() -> p::Error {
    p::Error("release service lifecycle is unverified or interrupted; do not retry".into())
}
