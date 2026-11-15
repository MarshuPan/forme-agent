#![cfg(windows)]
#![forbid(unsafe_code)]

use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::path::Path;
use std::time::{Duration, Instant};

use forme_protocol as p;
use windows_sys::Win32::Foundation::HANDLE;

use super::super::process::WorkerLaunchSpec;
use super::super::windows_appcontainer_acl::{self as acl, Profile};
use super::super::windows_appcontainer_support::{
    environment_block, managed_mcp_environment, quote_windows_arg, wide, WorkerEnvironment,
};
use super::{
    active_processes, create_job, exit_code, launch_suspended, process_exited,
    terminate_job_if_active, terminate_process_if_active, verify_worker_token, wait_for_process,
};

pub(super) struct LaunchBuffers {
    pub(super) application: Vec<u16>,
    pub(super) command: Vec<u16>,
    pub(super) directory: Vec<u16>,
    pub(super) environment: Vec<u16>,
}

impl LaunchBuffers {
    pub(super) fn prepare(request: &LaunchRequest<'_>) -> p::Result<Self> {
        let program = request
            .spec
            .program
            .to_str()
            .ok_or_else(|| p::Error("worker program path is not valid Unicode".into()))?;
        let current_dir = request
            .workspace
            .to_str()
            .ok_or_else(|| p::Error("worker workspace path is not valid Unicode".into()))?;
        let mut command = quote_windows_arg(program);
        for arg in &request.spec.args {
            command.push(' ');
            command.push_str(&quote_windows_arg(arg));
        }
        let stdio = request.stdio().is_some();
        let profile_allows_stdio = matches!(
            request.spec.sandbox_profile_ref.as_str(),
            p::MANAGED_MCP_PROGRAM_PROFILE | p::INSTALLED_PTY_PROFILE
        );
        if stdio != profile_allows_stdio {
            return Err(p::Error("worker stdio mode is not profile-bound".into()));
        }
        #[cfg(test)]
        if stdio && request.pty_mechanism_probe {
            return Err(p::Error("managed MCP cannot enable child processes".into()));
        }
        let environment = match request.input {
            LaunchInput::Worker(input) => environment_block(
                input,
                &super::system_windows_directory()?,
                &acl::local_app_data()?,
            )?,
            LaunchInput::ManagedMcp(_) => {
                managed_mcp_environment(request.workspace, &super::system_windows_directory()?)?
            }
        };
        Ok(Self {
            application: wide(program),
            command: wide(&command),
            directory: wide(current_dir),
            environment,
        })
    }
}

pub(crate) struct SetupResources {
    profile: Option<Profile>,
    job: Option<OwnedHandle>,
    process: Option<OwnedHandle>,
    thread: Option<OwnedHandle>,
}

pub(crate) struct LaunchRequest<'a> {
    pub(crate) spec: &'a WorkerLaunchSpec,
    pub(crate) workspace: &'a Path,
    pub(crate) input: LaunchInput<'a>,
    pub(crate) stdio: Option<&'a NativeStdio>,
    #[cfg(test)]
    pub(crate) pty_mechanism_probe: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum LaunchInput<'a> {
    Worker(WorkerEnvironment<'a>),
    ManagedMcp(&'a NativeStdio),
}
impl LaunchRequest<'_> {
    pub(super) fn stdio(&self) -> Option<&NativeStdio> {
        if self.stdio.is_some() {
            return self.stdio;
        }
        match self.input {
            LaunchInput::Worker(_) => None,
            LaunchInput::ManagedMcp(pipes) => Some(pipes),
        }
    }
}
pub(crate) struct NativeStdio {
    pub(super) input: OwnedHandle,
    pub(super) output: OwnedHandle,
    pub(super) error: OwnedHandle,
}
/// The parent-side pipe endpoints remain noninheritable.
pub(crate) fn stdio_handles(
    input: &std::io::PipeReader,
    output: &std::io::PipeWriter,
    error: &std::io::PipeWriter,
) -> p::Result<NativeStdio> {
    Ok(NativeStdio {
        input: super::duplicate_inheritable(input.as_raw_handle())?,
        output: super::duplicate_inheritable(output.as_raw_handle())?,
        error: super::duplicate_inheritable(error.as_raw_handle())?,
    })
}
pub(super) fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle()
}
pub(super) fn native_error(message: &str) -> p::Error {
    p::Error(message.into())
}

impl SetupResources {
    pub(crate) fn new() -> Self {
        Self {
            profile: None,
            job: None,
            process: None,
            thread: None,
        }
    }

    pub(crate) fn create_profile(&mut self, name: &str) -> p::Result<()> {
        self.profile = Some(acl::create_profile(name)?);
        Ok(())
    }

    pub(crate) fn profile(&self) -> Option<&Profile> {
        self.profile.as_ref()
    }

    pub(crate) fn create_job(&mut self, name: &str) -> p::Result<()> {
        self.job = Some(create_job(name)?);
        Ok(())
    }

    pub(crate) fn launch(&mut self, request: LaunchRequest<'_>) -> p::Result<()> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| p::Error("AppContainer profile is not available".into()))?;
        let job = self
            .job
            .as_ref()
            .ok_or_else(|| p::Error("worker Job Object handle is not available".into()))?;
        let (process, thread) = launch_suspended(&request, profile, raw(job))?;
        self.process = Some(process);
        self.thread = Some(thread);
        Ok(())
    }

    pub(crate) fn verify_resume(&mut self) -> p::Result<()> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| p::Error("AppContainer profile is not available".into()))?;
        let process = self
            .process
            .as_ref()
            .ok_or_else(|| p::Error("worker process handle is not available".into()))?;
        let job = self
            .job
            .as_ref()
            .ok_or_else(|| p::Error("worker Job Object handle is not available".into()))?;
        if active_processes(raw(job))? != 1 {
            return Err(p::Error(
                "worker was not created as the only process in its Job Object".into(),
            ));
        }
        verify_worker_token(raw(process), profile.raw_sid())?;
        let thread = self
            .thread
            .take()
            .ok_or_else(|| p::Error("worker thread handle is not available".into()))?;
        super::resume_worker_thread(raw(&thread))
    }

    pub(crate) fn terminal_state(&self) -> p::Result<(bool, u32)> {
        let root_exit_observed = self
            .process
            .as_ref()
            .map_or(Ok(true), |process| process_exited(raw(process)))?;
        let active_processes = self
            .job
            .as_ref()
            .map_or(Ok(0), |job| active_processes(raw(job)))?;
        Ok((root_exit_observed, active_processes))
    }

    pub(crate) fn terminate(&mut self) -> p::Result<()> {
        if let Some(job) = self.job.as_ref() {
            terminate_job_if_active(raw(job))?;
        } else if let Some(process) = self.process.as_ref() {
            terminate_process_if_active(raw(process))?;
        }
        if let Some(process) = self.process.as_ref() {
            wait_for_process(raw(process))?;
        }
        self.thread.take();
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> p::Result<NativeWorker> {
        if self.thread.is_some() {
            return Err(p::Error("worker thread was not resumed".into()));
        }
        if self.profile.is_none() || self.process.is_none() || self.job.is_none() {
            return Err(p::Error(
                "worker setup resources are incomplete after resume".into(),
            ));
        }
        let profile = self
            .profile
            .take()
            .ok_or_else(|| p::Error("AppContainer profile is not available".into()))?;
        let process = self
            .process
            .take()
            .ok_or_else(|| p::Error("worker process handle is not available".into()))?;
        let job = self
            .job
            .take()
            .ok_or_else(|| p::Error("worker Job Object handle is not available".into()))?;
        Ok(NativeWorker {
            profile,
            process,
            job,
        })
    }
}

pub(crate) struct NativeWorker {
    profile: Profile,
    process: OwnedHandle,
    job: OwnedHandle,
}

impl NativeWorker {
    #[cfg(test)]
    pub(crate) fn verify_observed_process(&self, pid: u32) -> p::Result<()> {
        let process = super::observe_process(pid, raw(&self.job), self.profile.raw_sid())?;
        let (_, exited, _) = super::process_identity(&process)?;
        if exited {
            return Err(native_error("observed MCP process is already terminal"));
        }
        Ok(())
    }

    pub(crate) fn profile(&self) -> &Profile {
        &self.profile
    }

    pub(crate) fn terminal_state(&self) -> p::Result<(bool, u32)> {
        Ok((
            process_exited(raw(&self.process))?,
            active_processes(raw(&self.job))?,
        ))
    }

    pub(crate) fn exit_code(&self) -> p::Result<u32> {
        exit_code(raw(&self.process))
    }

    pub(crate) fn cancel(&self) -> p::Result<CancelState> {
        let root_exited = process_exited(raw(&self.process))?;
        terminate_job_if_active(raw(&self.job))?;
        if active_processes(raw(&self.job))? != 0 {
            return Err(p::Error(
                "worker Job Object still contains active processes".into(),
            ));
        }
        if root_exited {
            Ok(CancelState::AlreadyExited)
        } else {
            wait_for_process(raw(&self.process))?;
            Ok(CancelState::Terminated)
        }
    }

    /// Independent original-object handles let a deadline supervisor terminate
    /// blocked I/O without acquiring configuration, worker or registry locks.
    pub(crate) fn cancellation(&self) -> p::Result<NativeCancellation> {
        Ok(NativeCancellation {
            process: self
                .process
                .try_clone()
                .map_err(|_| native_error("worker process could not be pinned for cancellation"))?,
            job: self
                .job
                .try_clone()
                .map_err(|_| native_error("worker Job could not be pinned for cancellation"))?,
        })
    }

    pub(crate) fn terminate_for_drop(&self) {
        let _ = self.cancel();
    }
}

pub(crate) struct NativeCancellation {
    process: OwnedHandle,
    job: OwnedHandle,
}
impl NativeCancellation {
    pub(crate) fn terminal_state(&self) -> p::Result<(bool, u32)> {
        Ok((
            process_exited(raw(&self.process))?,
            active_processes(raw(&self.job))?,
        ))
    }
    pub(crate) fn exit_code(&self) -> p::Result<u32> {
        exit_code(raw(&self.process))
    }
    pub(crate) fn cancel(&self) -> p::Result<()> {
        terminate_job_if_active(raw(&self.job))?;
        wait_for_process(raw(&self.process))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelState {
    Terminated,
    AlreadyExited,
}

pub(super) fn wait_for_job_empty(job: HANDLE) -> p::Result<()> {
    // Job accounting, rather than a signaled handle, proves quiescence.
    let deadline = Instant::now() + super::CANCEL_WAIT;
    loop {
        if active_processes(job)? == 0 {
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(p::Error(
                "worker Job Object did not become empty within the bound".into(),
            ));
        }
        std::thread::sleep(remaining.min(Duration::from_millis(20)));
    }
}

#[cfg(test)]
pub(super) struct ObservedProcess {
    pub(super) pid: u32,
    pub(super) created: u64,
    pub(super) image: String,
    process: OwnedHandle,
}

#[cfg(test)]
impl ObservedProcess {
    pub(super) fn exited(&self) -> p::Result<bool> {
        let (created, exited, image) = super::process_identity(&self.process)?;
        // The owned handle pins the original kernel object. Its exit time
        // proves termination even when Windows no longer exposes its image.
        if created != self.created || (!exited && image != self.image) {
            return Err(p::Error("probe process identity changed".into()));
        }
        Ok(exited)
    }
}

#[cfg(test)]
impl SetupResources {
    pub(super) fn observe_process(&self, pid: u32) -> p::Result<ObservedProcess> {
        let profile = self
            .profile
            .as_ref()
            .ok_or_else(|| p::Error("probe profile missing".into()))?;
        let job = self
            .job
            .as_ref()
            .ok_or_else(|| p::Error("probe Job missing".into()))?;
        let process = super::observe_process(pid, raw(job), profile.raw_sid())?;
        let (created, exited, image) = super::process_identity(&process)?;
        if exited {
            return Err(p::Error(
                "probe process exited before independent inspection".into(),
            ));
        }
        Ok(ObservedProcess {
            pid,
            created,
            image,
            process,
        })
    }

    pub(super) fn root_identity(&self) -> p::Result<(u64, bool, String)> {
        let process = self
            .process
            .as_ref()
            .ok_or_else(|| p::Error("probe root missing".into()))?;
        super::process_identity(process)
    }

    pub(super) fn exit_code(&self) -> p::Result<u32> {
        let process = self
            .process
            .as_ref()
            .ok_or_else(|| p::Error("probe root missing".into()))?;
        super::exit_code(raw(process))
    }
}
