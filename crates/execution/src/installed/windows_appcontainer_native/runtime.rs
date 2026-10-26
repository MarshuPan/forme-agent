#![cfg(windows)]
#![forbid(unsafe_code)]

use std::os::windows::io::OwnedHandle;
use std::path::Path;

use forme_protocol as p;

use super::super::process::WorkerLaunchSpec;
use super::super::windows_appcontainer_acl::{self as acl, Profile};
use super::{
    active_processes, create_job, exit_code, launch_suspended, process_exited, raw,
    terminate_job_if_active, terminate_process_if_active, verify_worker_token, wait_for_process,
};

pub(crate) struct SetupResources {
    profile: Option<Profile>,
    job: Option<OwnedHandle>,
    process: Option<OwnedHandle>,
    thread: Option<OwnedHandle>,
}

pub(crate) struct LaunchRequest<'a> {
    pub(crate) spec: &'a WorkerLaunchSpec,
    pub(crate) workspace: &'a Path,
    pub(crate) job_path: &'a Path,
    pub(crate) permit_path: &'a Path,
    pub(crate) execution_path: Option<&'a Path>,
    pub(crate) file_target_path: Option<&'a Path>,
    pub(crate) workspace_scope: &'a str,
    pub(crate) deadline: p::Timestamp,
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

    pub(crate) fn terminate_for_drop(&self) {
        let _ = self.cancel();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelState {
    Terminated,
    AlreadyExited,
}
