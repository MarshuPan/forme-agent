//! The installed file image runs the finite channel entry in a fresh Low Job.
use super::super::management_repair::storage::ReadCustody;
use super::super::process::set_private_directory;
use super::super::sandbox_lease::{SandboxLeaseBinding, SandboxLeaseJournal, SandboxTargetAccess};
use super::super::windows_appcontainer::{recovery, results};
use super::super::windows_appcontainer_acl as acl;
use super::super::windows_appcontainer_native as native;
use super::super::windows_appcontainer_support::{
    job_object_name, profile_name_for, recovery_timestamp,
};
use super::super::WorkerIsolation;
use super::{Admission, InstalledChannelDelivery};
use forme_protocol as p;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

mod lifecycle;
use super::super::managed_mcp_stdio::controller::Controller;
use super::super::worker_stdio as wire;

pub(crate) struct ChannelIsolation {
    root: PathBuf,
    pub(crate) channels: Arc<InstalledChannelDelivery>,
    journal: SandboxLeaseJournal,
    children: Mutex<BTreeMap<String, ChannelWorker>>,
    blocked: bool,
    image: PathBuf,
    image_digest: p::SchemaDigest,
    _image_custody: ReadCustody,
}
struct ChannelWorker {
    admission: Admission,
    setup: native::SetupResources,
    native: Option<native::NativeWorker>,
    controller: Controller,
    dir: PathBuf,
    lease_id: String,
    started_at: p::Timestamp,
    receipt: Option<p::WorkerReceipt>,
    result_written: bool,
}
// Failed shutdown cannot drop the original custody/threads and admit another
// copy. These originals remain retained until process exit/startup recovery.
static RETAINED: OnceLock<Mutex<Vec<ChannelWorker>>> = OnceLock::new();
fn retained() -> &'static Mutex<Vec<ChannelWorker>> {
    RETAINED.get_or_init(|| Mutex::new(vec![]))
}

impl ChannelIsolation {
    pub(crate) fn open(
        root: PathBuf,
        channels: Arc<InstalledChannelDelivery>,
        image: PathBuf,
    ) -> p::Result<Self> {
        if !root.is_absolute() {
            return Err(invalid());
        }
        super::super::reject_linked_ancestors(&root)?;
        fs::create_dir_all(&root).map_err(|_| invalid())?;
        set_private_directory(&root)?;
        let root = fs::canonicalize(root).map_err(|_| invalid())?;
        let journal = SandboxLeaseJournal::open(&root.join("sandbox-leases.jsonl"))?;
        let blocked = recovery::recover_pending(&journal).is_err();
        if image.file_name().and_then(|v| v.to_str()) != Some("forme-file-worker.exe") {
            return Err(invalid());
        }
        let mut custody = ReadCustody::default();
        custody.pin(&image, 1024 * 1024 * 1024)?;
        let image_digest = super::super::file_digest(&image)?;
        Ok(Self {
            image,
            image_digest,
            _image_custody: custody,
            root,
            channels,
            journal,
            children: Mutex::new(BTreeMap::new()),
            blocked,
        })
    }
    /// Called only inside the configuration -> worker admission exclusion.
    pub(crate) fn admit_authorized(
        &self,
        admission: Admission,
        now: p::Timestamp,
    ) -> p::Result<()> {
        let mut children = self.children.lock().map_err(|_| invalid())?;
        if self.blocked
            || children.len() >= 8
            || !retained().lock().map_err(|_| invalid())?.is_empty()
            || self.journal.pending_cleanup()?.iter().any(|lease| {
                !children
                    .values()
                    .any(|worker| worker.lease_id == lease.lease_id)
            })
            || children.contains_key(&admission.input.execution.job.job_id)
            || Instant::now() >= admission.deadline
        {
            return Err(invalid());
        }
        let job = &admission.input.execution.job;
        let profile = profile_name_for(
            self.root.to_str().ok_or_else(invalid)?,
            &job.job_id,
            &job.plan_digest,
        )?;
        let dir = self.root.join(&profile);
        fs::create_dir(&dir).map_err(|_| invalid())?;
        set_private_directory(&dir)?;
        let binding = SandboxLeaseBinding::new(
            job.job_id.clone(),
            p::SchemaDigest(job.plan_digest.0.clone()),
            profile.clone(),
            &dir,
            acl::file_identity(&dir)?,
        )?;
        let lease_id = self.journal.begin_lease(binding)?;
        let id = job.job_id.clone();
        children.insert(
            id.clone(),
            ChannelWorker {
                admission,
                setup: native::SetupResources::new(),
                native: None,
                controller: Controller::prepare(),
                dir,
                lease_id,
                started_at: now,
                receipt: None,
                result_written: false,
            },
        );
        let worker = children.get_mut(&id).ok_or_else(invalid)?;
        if let Err(error) = self.launch(worker, &profile) {
            worker.controller.cancel();
            if let Some(native) = &worker.native {
                let _ = native.cancel();
            } else {
                let _ = worker.setup.terminate();
            }
            // The original entry and lease remain even when cancellation fails.
            return Err(error);
        }
        Ok(())
    }
    fn launch(&self, worker: &mut ChannelWorker, name: &str) -> p::Result<()> {
        if super::super::file_digest(&self.image)? != self.image_digest {
            return Err(invalid());
        }
        let spec = super::super::WorkerLaunchSpec::new(
            p::MANAGED_CHANNEL_PROFILE,
            self.image.clone(),
            vec!["--installed-channel-worker".into()],
        )?;
        worker.setup.create_profile(name)?;
        let profile = worker.setup.profile().ok_or_else(invalid)?;
        let workspace = worker.dir.join("workspace");
        fs::create_dir(&workspace).map_err(|_| invalid())?;
        acl::set_low_integrity(&workspace)?;
        self.grant(
            &worker.lease_id,
            &workspace,
            SandboxTargetAccess::WorkspaceReadWrite,
            profile,
        )?;
        self.grant(
            &worker.lease_id,
            &spec.program,
            SandboxTargetAccess::RegisteredProgramReadExecute,
            profile,
        )?;
        worker.setup.create_job(&job_object_name(name)?)?;
        let (wire, child) = wire::Wire::prepare(1024)?;
        let pipes = native::stdio_handles(&child.input, &child.output, &child.error)?;
        if Instant::now() >= worker.admission.deadline {
            return Err(invalid());
        }
        worker.setup.launch(native::LaunchRequest {
            spec: &spec,
            workspace: &workspace,
            input: native::LaunchInput::ManagedMcp(&pipes),
            stdio: None,
            #[cfg(test)]
            pty_mechanism_probe: false,
        })?;
        drop(pipes);
        drop(child);
        results::begin_handoff(&worker.dir, &worker.admission.input.execution.job)?;
        worker.setup.verify_resume()?;
        worker.native = Some(worker.setup.finish()?);
        let channels = self.channels.clone();
        let admission = worker.admission.clone();
        worker.controller.start_exchange(
            worker.native.as_ref().ok_or_else(invalid)?,
            wire,
            admission.deadline,
            move |wire, cancelled, handles, attempted| {
                super::exchange::run(wire, &channels, &admission, cancelled, handles, attempted)
            },
        )
    }
    fn grant(
        &self,
        lease: &str,
        target: &Path,
        access: SandboxTargetAccess,
        profile: &acl::Profile,
    ) -> p::Result<()> {
        let mutation =
            self.journal
                .begin_acl_grant(lease, target, acl::file_identity(target)?, access)?;
        self.journal
            .record_acl_readback(&mutation, acl::apply_acl(&mutation, profile)?)
    }
}
impl ChannelWorker {
    fn terminal(&self) -> p::Result<(bool, u32)> {
        self.native.as_ref().map_or_else(
            || self.setup.terminal_state(),
            native::NativeWorker::terminal_state,
        )
    }
    fn cleanup_confirmed(&mut self) -> p::Result<bool> {
        Ok(self.controller.poll()? && self.terminal()? == (true, 0))
    }
    fn matches(&self, job: &p::WorkerJob) -> p::Result<()> {
        if &self.admission.input.execution.job != job {
            return Err(invalid());
        }
        Ok(())
    }
}
impl Drop for ChannelIsolation {
    fn drop(&mut self) {
        let workers = self
            .children
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        let mut retained = retained().lock().unwrap_or_else(|error| error.into_inner());
        for (_, mut worker) in std::mem::take(workers) {
            worker.controller.cancel();
            if let Some(native) = &worker.native {
                let _ = native.cancel();
            } else {
                let _ = worker.setup.terminate();
            }
            // Unsealed data and ACL/file custody survive even a clean tree exit.
            retained.push(worker);
        }
    }
}
fn invalid() -> p::Error {
    p::Error("managed channel isolation or original cleanup is unconfirmed".into())
}
