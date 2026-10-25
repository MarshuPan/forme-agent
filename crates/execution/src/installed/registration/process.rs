//! Read-only binding between this Host process and its fixed SCM registration.
#![forbid(unsafe_code)]

use windows_service::service::ServiceStatus;

use super::*;

/// Holds the original SCM handle and exact registration expectation. This is
/// not an Agent permission, token inspection, or core-peer authentication.
pub struct WindowsHostServiceBinding {
    service: Service,
    expected: ServiceInfo,
    process_id: u32,
}

/// Checks the fixed Host registration before loading runtime credentials.
///
/// Does not start, stop, repair, or modify a service. The caller must also use
/// `verify_running` after reporting Running and before admitting requests:
/// Windows does not guarantee a valid service PID while StartPending.
pub fn bind_windows_host_service() -> p::Result<WindowsHostServiceBinding> {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    if arguments.len() != 3 || arguments[1] != "--config" {
        return Err(invalid());
    }
    let config = PathBuf::from(&arguments[2]);
    let image = current_executable(p::BuiltinServiceId::Host)?;
    let expected = desired(p::BuiltinServiceId::Host, &config, image.clone())?;
    if !config.is_file() || !image.is_file() {
        return Err(invalid());
    }
    let service = manager(ServiceManagerAccess::CONNECT)?
        .open_service(
            p::BuiltinServiceId::Host.service_name(),
            ServiceAccess::QUERY_CONFIG | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|_| invalid())?;
    verify(&service, &expected)?;
    Ok(WindowsHostServiceBinding {
        service,
        expected,
        process_id: std::process::id(),
    })
}

impl WindowsHostServiceBinding {
    /// Rechecks the exact registration between two Running/PID observations.
    ///
    /// Startup status propagation has a fixed two-second read-only bound.
    /// A failure leaves request admission closed; it never repairs SCM or
    /// treats a missing, zero, stale, or transitional PID as authentication.
    pub fn verify_running(&self) -> p::Result<()> {
        verify_running_registration(
            || self.service.query_status().map_err(|_| invalid()),
            || verify(&self.service, &self.expected),
            self.process_id,
        )
    }
}

fn verify_running_registration(
    mut observe: impl FnMut() -> p::Result<ServiceStatus>,
    recheck: impl FnOnce() -> p::Result<()>,
    process_id: u32,
) -> p::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let before = loop {
        let status = observe()?;
        if status.current_state == ServiceState::StartPending && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        break status;
    };
    if !matches_running(&before, process_id) {
        return Err(invalid());
    }
    recheck()?;
    if !matches_running(&observe()?, process_id) {
        return Err(invalid());
    }
    Ok(())
}

fn matches_running(status: &ServiceStatus, process_id: u32) -> bool {
    process_id != 0
        && status.service_type == ServiceType::OWN_PROCESS
        && status.current_state == ServiceState::Running
        && status.process_id == Some(process_id)
}

fn invalid() -> p::Error {
    p::Error("current Host process is not bound to its running SCM service".into())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::VecDeque;

    use windows_service::service::{ServiceControlAccept, ServiceExitCode};

    use super::*;

    fn running() -> ServiceStatus {
        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::ZERO,
            process_id: Some(42),
        }
    }

    #[test]
    fn host_process_proof_requires_running_own_process_with_the_exact_nonzero_pid() {
        assert!(matches_running(&running(), 42));
        assert!(!matches_running(&running(), 0));
        for pid in [None, Some(0), Some(41)] {
            let mut status = running();
            status.process_id = pid;
            assert!(!matches_running(&status, 42));
        }
        for state in [
            ServiceState::StartPending,
            ServiceState::StopPending,
            ServiceState::Stopped,
            ServiceState::Paused,
            ServiceState::PausePending,
            ServiceState::ContinuePending,
        ] {
            let mut status = running();
            status.current_state = state;
            assert!(!matches_running(&status, 42));
        }
        let mut status = running();
        status.service_type = ServiceType::SHARE_PROCESS;
        assert!(!matches_running(&status, 42));
    }

    #[test]
    fn registration_recheck_cannot_hide_a_changed_or_unqueryable_running_process() {
        for drift in [None, Some(0), Some(43)] {
            let mut changed = running();
            changed.process_id = drift;
            let mut observations = VecDeque::from([running(), changed]);
            let rechecks = Cell::new(0);
            assert!(verify_running_registration(
                || Ok(observations.pop_front().unwrap()),
                || {
                    rechecks.set(rechecks.get() + 1);
                    Ok(())
                },
                42,
            )
            .is_err());
            assert_eq!(rechecks.get(), 1);
            assert!(observations.is_empty());
        }
        let observed = Cell::new(0);
        assert!(verify_running_registration(
            || {
                observed.set(observed.get() + 1);
                Ok(running())
            },
            || Err(p::Error("registration changed".into())),
            42,
        )
        .is_err());
        assert_eq!(observed.get(), 1);
        let mut observations = VecDeque::from([Ok(running()), Err(invalid())]);
        assert!(
            verify_running_registration(|| observations.pop_front().unwrap(), || Ok(()), 42,)
                .is_err()
        );
        let mut observations = VecDeque::from([running(), running()]);
        verify_running_registration(|| Ok(observations.pop_front().unwrap()), || Ok(()), 42)
            .unwrap();
        assert!(observations.is_empty());
    }
}
