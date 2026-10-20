#[cfg(not(windows))]
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use forme_protocol as p;

pub trait NativeServiceManager: Send + Sync {
    fn inspect(&self, id: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus>;
    fn apply(&self, change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt>;
}

/// Calls only native service-management APIs with closed arguments. It neither
/// installs arbitrary services nor accepts shell text, executable paths or env.
pub struct SystemServiceManager;

impl NativeServiceManager for SystemServiceManager {
    fn inspect(&self, id: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        #[cfg(windows)]
        let state = windows_scm::inspect(id)?;
        #[cfg(not(windows))]
        let state = {
            #[cfg(target_os = "macos")]
            if !macos_service_installed(id)? {
                return Ok(p::NativeServiceStatus {
                    schema_version: p::SchemaVersion(1),
                    service: id,
                    state: p::NativeServiceState::NotInstalled,
                });
            }
            let (program, args) = invocation(id, None)?;
            let output = bounded_command(program, args)?;
            parse_status(&output)
        };
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service: id,
            state,
        })
    }
    fn apply(&self, change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        if change.schema_version != p::SchemaVersion(1) {
            return Err(p::Error("unsupported native service schema".into()));
        }
        let before = self.inspect(change.service)?;
        if matches!(
            (change.action, before.state),
            (
                p::NativeServiceAction::Start,
                p::NativeServiceState::Running
            ) | (p::NativeServiceAction::Stop, p::NativeServiceState::Stopped)
        ) {
            return Ok(p::NativeReceipt {
                schema_version: p::SchemaVersion(1),
                service: change.service,
                requested: change.action,
                observed: before.state,
            });
        }
        if matches!(
            before.state,
            p::NativeServiceState::Unknown | p::NativeServiceState::NotInstalled
        ) {
            return Err(p::Error(
                "native service must be installed and inspectable before a change".into(),
            ));
        }
        #[cfg(windows)]
        windows_scm::apply(change.service, change.action)?;
        #[cfg(not(windows))]
        {
            let (program, args) = invocation(change.service, Some(change.action))?;
            let result = bounded_command(program, args)?;
            if !result.success {
                return Err(p::Error(
                    "native service rejected the bounded change; inspect its original state".into(),
                ));
            }
        }
        let expected = match change.action {
            p::NativeServiceAction::Start => p::NativeServiceState::Running,
            p::NativeServiceAction::Stop => p::NativeServiceState::Stopped,
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let observed = loop {
            let observed = self.inspect(change.service)?.state;
            if observed == expected {
                break observed;
            }
            if !matches!(
                observed,
                p::NativeServiceState::Starting | p::NativeServiceState::Stopping
            ) || std::time::Instant::now() >= deadline
            {
                return Err(p::Error(
                    "native service did not settle; probe the original operation before any retry"
                        .into(),
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        Ok(p::NativeReceipt {
            schema_version: p::SchemaVersion(1),
            service: change.service,
            requested: change.action,
            observed,
        })
    }
}

#[cfg(windows)]
mod windows_scm {
    use super::p;
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    fn open(
        id: p::BuiltinServiceId,
        access: ServiceAccess,
    ) -> std::result::Result<windows_service::service::Service, windows_service::Error> {
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?
            .open_service(id.service_name(), access)
    }

    pub(super) fn inspect(id: p::BuiltinServiceId) -> p::Result<p::NativeServiceState> {
        let service = match open(id, ServiceAccess::QUERY_STATUS) {
            Ok(service) => service,
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1060) => {
                return Ok(p::NativeServiceState::NotInstalled);
            }
            Err(_) => return Ok(p::NativeServiceState::Unknown),
        };
        Ok(service
            .query_status()
            .map_or(p::NativeServiceState::Unknown, |status| {
                state(status.current_state)
            }))
    }

    pub(super) fn apply(id: p::BuiltinServiceId, action: p::NativeServiceAction) -> p::Result<()> {
        let access = match action {
            p::NativeServiceAction::Start => ServiceAccess::START,
            p::NativeServiceAction::Stop => ServiceAccess::STOP,
        };
        let service = open(id, access)
            .map_err(|_| p::Error("native service control is unavailable".into()))?;
        match action {
            p::NativeServiceAction::Start => service.start::<&str>(&[]),
            p::NativeServiceAction::Stop => service.stop().map(|_| ()),
        }
        .map_err(|_| p::Error("native service outcome must be inspected before retrying".into()))
    }

    pub(super) fn state(state: ServiceState) -> p::NativeServiceState {
        match state {
            ServiceState::Stopped => p::NativeServiceState::Stopped,
            ServiceState::StartPending | ServiceState::ContinuePending => {
                p::NativeServiceState::Starting
            }
            ServiceState::StopPending => p::NativeServiceState::Stopping,
            ServiceState::Running => p::NativeServiceState::Running,
            _ => p::NativeServiceState::Unknown,
        }
    }
}

#[cfg(not(windows))]
fn invocation(
    id: p::BuiltinServiceId,
    action: Option<p::NativeServiceAction>,
) -> p::Result<(PathBuf, Vec<String>)> {
    #[cfg(target_os = "linux")]
    {
        let verb = match action {
            None => "show",
            Some(p::NativeServiceAction::Start) => "start",
            Some(p::NativeServiceAction::Stop) => "stop",
        };
        let mut args = vec![verb.into()];
        if action.is_none() {
            args.push("--property=LoadState,ActiveState".into());
        }
        args.extend(["--".into(), format!("{}.service", id.service_name())]);
        Ok((PathBuf::from("/usr/bin/systemctl"), args))
    }
    #[cfg(target_os = "macos")]
    {
        let label = macos_service_label(id);
        let args = match action {
            None => vec!["print".into(), format!("system/{label}")],
            Some(action) => vec![
                if action == p::NativeServiceAction::Start {
                    "bootstrap"
                } else {
                    "bootout"
                }
                .into(),
                "system".into(),
                format!("/Library/LaunchDaemons/{label}.plist"),
            ],
        };
        Ok((PathBuf::from("/bin/launchctl"), args))
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (id, action);
        Err(p::Error("native service platform is unsupported".into()))
    }
}

#[cfg(not(windows))]
struct CommandResult {
    success: bool,
    #[cfg(target_os = "macos")]
    exit_code: Option<i32>,
    stdout: String,
}

#[cfg(not(windows))]
fn bounded_command(program: PathBuf, args: Vec<String>) -> p::Result<CommandResult> {
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| p::Error("native service manager is unavailable".into()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| p::Error("native service observation is unavailable".into()))?;
    let reader = thread::spawn(move || {
        let mut data = Vec::new();
        let result = stdout.by_ref().take(65_537).read_to_end(&mut data);
        // Closing a full pipe bounds both memory and the emitting command.
        result.map(|_| data)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(p::Error(
                    "native service outcome is unknown; inspect without retrying".into(),
                ));
            }
        }
    };
    let data = reader
        .join()
        .map_err(|_| p::Error("native service observation failed".into()))?
        .map_err(|_| p::Error("native service observation failed".into()))?;
    let status = status?;
    if data.len() > 65_536 {
        return Err(p::Error(
            "native service observation exceeded its bound".into(),
        ));
    }
    Ok(CommandResult {
        success: status.success(),
        #[cfg(target_os = "macos")]
        exit_code: status.code(),
        stdout: String::from_utf8_lossy(&data).into_owned(),
    })
}

#[cfg(not(windows))]
fn parse_status(result: &CommandResult) -> p::NativeServiceState {
    use p::NativeServiceState::*;
    #[cfg(target_os = "linux")]
    {
        if result
            .stdout
            .lines()
            .any(|line| line == "LoadState=not-found")
        {
            return NotInstalled;
        }
        if !result.success {
            return Unknown;
        }
        match result
            .stdout
            .lines()
            .find_map(|line| line.strip_prefix("ActiveState="))
        {
            Some("active") => Running,
            Some("inactive") => Stopped,
            Some("activating") => Starting,
            Some("deactivating") => Stopping,
            Some("failed") => Failed,
            _ => Unknown,
        }
    }
    #[cfg(target_os = "macos")]
    {
        if !result.success {
            // launchd has no loaded job after bootout, but its protected plist
            // still defines an installed service. Other failures stay unknown.
            return if result.exit_code == Some(113) {
                Stopped
            } else {
                Unknown
            };
        }
        if result
            .stdout
            .lines()
            .any(|line| line.trim() == "state = running")
        {
            Running
        } else if result
            .stdout
            .lines()
            .any(|line| line.trim() == "state = not running")
        {
            Stopped
        } else {
            Unknown
        }
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = result;
        Unknown
    }
}

#[cfg(target_os = "macos")]
fn macos_service_label(id: p::BuiltinServiceId) -> String {
    format!(
        "org.forme.{}",
        match id {
            p::BuiltinServiceId::Host => "host",
            p::BuiltinServiceId::Web => "web",
            p::BuiltinServiceId::Brain => "brain",
        }
    )
}

#[cfg(target_os = "macos")]
fn macos_service_installed(id: p::BuiltinServiceId) -> p::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let plist =
        PathBuf::from("/Library/LaunchDaemons").join(format!("{}.plist", macos_service_label(id)));
    super::reject_linked_ancestors(&plist)?;
    match std::fs::symlink_metadata(plist) {
        Ok(metadata)
            if metadata.is_file() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0 =>
        {
            Ok(true)
        }
        Ok(_) => Err(p::Error(
            "native service definition is not protected".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(p::Error(
            "native service definition cannot be inspected".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_commands_have_only_builtin_targets_and_no_shell() {
        #[cfg(not(windows))]
        for id in [
            p::BuiltinServiceId::Host,
            p::BuiltinServiceId::Web,
            p::BuiltinServiceId::Brain,
        ] {
            for action in [
                None,
                Some(p::NativeServiceAction::Start),
                Some(p::NativeServiceAction::Stop),
            ] {
                let (program, arguments) = invocation(id, action).unwrap();
                assert!(program.is_absolute());
                assert!(arguments.iter().all(|a| !a.contains(['\n', ';', '|', '&'])));
                assert!(arguments.iter().any(|a| a.contains(id.service_name())
                    || a.contains(match id {
                        p::BuiltinServiceId::Host => "org.forme.host",
                        p::BuiltinServiceId::Web => "org.forme.web",
                        p::BuiltinServiceId::Brain => "org.forme.brain",
                    })));
            }
        }
        #[cfg(windows)]
        for (id, name) in [
            (p::BuiltinServiceId::Host, "forme-host"),
            (p::BuiltinServiceId::Web, "forme-web"),
            (p::BuiltinServiceId::Brain, "forme-brain"),
        ] {
            assert_eq!(id.service_name(), name);
            assert!(!id.service_name().contains(['\\', '/', ':', ' ', ';']));
        }
    }

    #[cfg(windows)]
    #[test]
    fn service_observation_never_turns_ambiguous_output_into_running() {
        use windows_service::service::ServiceState;
        assert_eq!(
            windows_scm::state(ServiceState::Running),
            p::NativeServiceState::Running
        );
        assert_eq!(
            windows_scm::state(ServiceState::StartPending),
            p::NativeServiceState::Starting
        );
        assert_eq!(
            windows_scm::state(ServiceState::Paused),
            p::NativeServiceState::Unknown
        );
        assert_eq!(
            windows_scm::state(ServiceState::PausePending),
            p::NativeServiceState::Unknown
        );
    }
}
