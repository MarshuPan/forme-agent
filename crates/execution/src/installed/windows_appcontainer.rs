#![cfg(windows)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use forme_protocol as p;

use super::process::{
    set_private_directory, validate_scope_key, write_private_json, WorkerLaunchRegistry,
};
use super::sandbox_lease::{SandboxLeaseBinding, SandboxLeaseJournal, SandboxTargetAccess};
use super::windows_appcontainer_acl as acl;
use super::windows_appcontainer_native as native;
#[cfg(test)]
use super::windows_appcontainer_support::{environment_block, WorkerEnvironment};
use super::windows_appcontainer_support::{job_object_name, profile_name_for, recovery_timestamp};
use super::windows_workspace_overlay::PreparedOverlay;
use super::{reject_linked_ancestors, WorkerIsolation};

#[path = "windows_appcontainer_recovery.rs"]
mod recovery;
#[path = "windows_appcontainer_results.rs"]
mod results;

/// Windows AppContainer worker isolation with typed lifecycle and recovery.
/// Win32 calls remain behind the audited native adapters.
///
/// Explicit component composition only: production dispatch stays unavailable
/// until the installed-platform evidence closes.
/// This adapter never lowers an existing workspace's integrity label.
pub struct WindowsAppContainerWorkerIsolation {
    runtime_root: PathBuf,
    launches: WorkerLaunchRegistry,
    workspace_roots: BTreeMap<String, PathBuf>,
    journal: SandboxLeaseJournal,
    children: Mutex<BTreeMap<String, RunningWorker>>,
    recovery_required: AtomicBool,
}

struct RunningWorker {
    native: native::NativeWorker,
    lease_id: String,
    run_dir: PathBuf,
    started_at: p::Timestamp,
    overlay: PreparedOverlay,
    terminal_receipt: Option<p::WorkerReceipt>,
}

impl WindowsAppContainerWorkerIsolation {
    /// Managed-service composition adds the broker identity proof before any
    /// runtime directory, lease journal, or AppContainer state is opened.
    pub fn open_managed(
        runtime_root: PathBuf,
        launches: WorkerLaunchRegistry,
        workspace_roots: BTreeMap<String, PathBuf>,
    ) -> p::Result<Self> {
        acl::verify_current_process_is_local_system()?;
        Self::open(runtime_root, launches, workspace_roots)
    }

    pub fn open(
        runtime_root: PathBuf,
        launches: WorkerLaunchRegistry,
        workspace_roots: BTreeMap<String, PathBuf>,
    ) -> p::Result<Self> {
        if !runtime_root.is_absolute() || workspace_roots.is_empty() {
            return Err(p::Error(
                "Windows AppContainer isolation requires absolute paths".into(),
            ));
        }
        reject_linked_ancestors(&runtime_root)?;
        fs::create_dir_all(&runtime_root)
            .map_err(|_| p::Error("worker runtime directory cannot be created".into()))?;
        reject_linked_ancestors(&runtime_root)?;
        set_private_directory(&runtime_root)?;
        let runtime_root = fs::canonicalize(&runtime_root)
            .map_err(|_| p::Error("worker runtime directory cannot be resolved".into()))?;
        let journal = SandboxLeaseJournal::open(&runtime_root.join("sandbox-leases.jsonl"))?;
        // Incomplete cleanup blocks worker admission while preserving the
        // authenticated Host maintenance/query control plane. Configuration,
        // identities, paths and the journal itself still fail closed above.
        let recovery_required = recovery::recover_pending(&journal).is_err();

        let mut roots = BTreeMap::new();
        for (scope, path) in workspace_roots {
            validate_scope_key(&scope)?;
            reject_linked_ancestors(&path)?;
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| p::Error("worker workspace is unavailable".into()))?;
            if !metadata.is_dir() {
                return Err(p::Error("worker workspace is not a directory".into()));
            }
            let canonical = fs::canonicalize(&path)
                .map_err(|_| p::Error("worker workspace cannot be resolved".into()))?;
            if canonical == runtime_root
                || canonical.starts_with(&runtime_root)
                || runtime_root.starts_with(&canonical)
            {
                return Err(p::Error(
                    "worker runtime and workspace paths must be disjoint".into(),
                ));
            }
            roots.insert(scope, canonical);
        }

        Ok(Self {
            runtime_root,
            launches,
            workspace_roots: roots,
            journal,
            children: Mutex::new(BTreeMap::new()),
            recovery_required: AtomicBool::new(recovery_required),
        })
    }

    fn workspace(&self, scope: &p::Scope) -> p::Result<&Path> {
        self.workspace_roots
            .get(&scope.0)
            .map(PathBuf::as_path)
            .ok_or_else(|| p::Error("worker workspace scope has no installed path".into()))
    }

    fn profile_name(&self, job: &p::WorkerJob) -> p::Result<String> {
        let root = self
            .runtime_root
            .to_str()
            .ok_or_else(|| p::Error("worker runtime path is not valid Unicode".into()))?;
        profile_name_for(root, &job.job_id, &job.plan_digest)
    }

    fn run_dir(&self, job: &p::WorkerJob) -> p::Result<PathBuf> {
        let digest =
            p::canonical_digest(&("forme-worker-run-v1", &job.job_id, &job.plan_digest))?.0;
        let path = self
            .runtime_root
            .join(format!("job-{}", digest.trim_start_matches("sha256:")));
        reject_linked_ancestors(&path)?;
        fs::create_dir(&path)
            .map_err(|_| p::Error("worker job directory cannot be created".into()))?;
        set_private_directory(&path)?;
        Ok(path)
    }

    fn spawn_worker(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<RunningWorker> {
        let spec = self.launches.get(&job.sandbox_profile_ref)?.clone();
        spec.validate()?;
        if spec.sandbox_profile_ref != permit.sandbox_profile_ref {
            return Err(p::Error("worker launch profile is not permit-bound".into()));
        }
        let original = self.workspace(&job.workspace_scope)?.to_path_buf();
        reject_linked_ancestors(&original)?;
        let run_dir = self.run_dir(job)?;
        let profile_name = self.profile_name(job)?;
        let run_dir_identity = acl::file_identity(&run_dir)?;
        let binding = SandboxLeaseBinding::new(
            job.job_id.clone(),
            p::SchemaDigest(job.plan_digest.0.clone()),
            profile_name.clone(),
            &run_dir,
            run_dir_identity,
        )?;
        let lease_id = self.journal.begin_lease(binding)?;
        let mut resources = native::SetupResources::new();

        let result = (|| {
            resources.create_profile(&profile_name)?;
            let profile = resources
                .profile()
                .ok_or_else(|| p::Error("AppContainer profile is not available".into()))?;

            let overlay =
                PreparedOverlay::prepare(&run_dir, &original, job, permit, execution, now)?;
            overlay.prepare_low_integrity()?;
            let workspace = overlay.workspace();
            let results = run_dir.join("results");
            fs::create_dir(&results)
                .map_err(|_| p::Error("worker result directory cannot be created".into()))?;
            acl::set_low_integrity(&results)?;

            let job_path = run_dir.join("job.json");
            let permit_path = run_dir.join("permit.json");
            write_private_json(&job_path, job)?;
            write_private_json(&permit_path, permit)?;
            let execution_path = run_dir.join("execution.json");
            if let Some(execution) = execution {
                write_private_json(&execution_path, execution)?;
            }
            let file_target_path = run_dir.join("file-target.json");
            let file_target = overlay.file_target()?;
            if let Some(target) = &file_target {
                write_private_json(&file_target_path, target)?;
            }

            self.grant(
                &lease_id,
                &workspace,
                SandboxTargetAccess::WorkspaceReadWrite,
                profile,
            )?;
            self.grant(
                &lease_id,
                &results,
                SandboxTargetAccess::WorkspaceReadWrite,
                profile,
            )?;
            self.grant(
                &lease_id,
                &run_dir,
                SandboxTargetAccess::JobInputReadOnly,
                profile,
            )?;
            let mut read_only_inputs = vec![job_path.as_path(), permit_path.as_path()];
            if execution.is_some() {
                read_only_inputs.push(execution_path.as_path());
            }
            if file_target.is_some() {
                read_only_inputs.push(file_target_path.as_path());
            }
            for input in read_only_inputs {
                self.grant(
                    &lease_id,
                    input,
                    SandboxTargetAccess::JobInputReadOnly,
                    profile,
                )?;
            }
            self.grant(
                &lease_id,
                &spec.program,
                SandboxTargetAccess::RegisteredProgramReadExecute,
                profile,
            )?;

            resources.create_job(&job_object_name(&profile_name)?)?;
            resources.launch(native::LaunchRequest {
                spec: &spec,
                workspace: &workspace,
                job_path: &job_path,
                permit_path: &permit_path,
                execution_path: execution.as_ref().map(|_| execution_path.as_path()),
                file_target_path: file_target.as_ref().map(|_| file_target_path.as_path()),
                workspace_scope: &job.workspace_scope.0,
                deadline: job.deadline,
            })?;
            results::begin_handoff(&run_dir, job)?;
            resources.verify_resume()?;
            let native = resources.finish()?;
            Ok(RunningWorker {
                native,
                lease_id: lease_id.clone(),
                run_dir: run_dir.clone(),
                started_at: now,
                overlay,
                terminal_receipt: None,
            })
        })();

        match result {
            Ok(worker) => Ok(worker),
            Err(error) => {
                let termination = resources.terminate();
                let cleanup = termination
                    .and_then(|_| resources.terminal_state())
                    .and_then(|terminal| {
                        resources.profile().map_or_else(
                            || Err(p::Error("AppContainer profile ownership is unknown".into())),
                            |profile| {
                                recovery::cleanup_lease(
                                    &self.journal,
                                    &lease_id,
                                    &run_dir,
                                    profile,
                                    terminal,
                                    now,
                                )
                            },
                        )
                    });
                if cleanup.is_err() {
                    self.recovery_required.store(true, Ordering::SeqCst);
                    return Err(p::Error(
                        "worker setup failed and sandbox cleanup is unverifiable".into(),
                    ));
                }
                Err(error)
            }
        }
    }

    fn grant(
        &self,
        lease_id: &str,
        target: &Path,
        access: SandboxTargetAccess,
        profile: &acl::Profile,
    ) -> p::Result<()> {
        let mutation =
            self.journal
                .begin_acl_grant(lease_id, target, acl::file_identity(target)?, access)?;
        let readback = acl::apply_acl(&mutation, profile)?;
        self.journal.record_acl_readback(&mutation, readback)
    }
}

impl WorkerIsolation for WindowsAppContainerWorkerIsolation {
    fn with_quiescent_resources(&self, action: &mut dyn FnMut() -> p::Result<()>) -> p::Result<()> {
        super::worker_isolation::with_empty_registry(&self.children, &mut || {
            if self.recovery_required.load(Ordering::SeqCst) {
                return Err(p::Error("sandbox resource recovery is required".into()));
            }
            self.journal.with_quiescent_leases(action)
        })
    }

    fn admit(&self, job: &p::WorkerJob, permit: &p::WorkerPermit) -> p::Result<()> {
        self.admit_at(job, permit, permit.issued_at)
    }

    fn admit_at(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        now: p::Timestamp,
    ) -> p::Result<()> {
        self.admit_with_execution(job, permit, None, now)
    }

    fn admit_with_execution(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<()> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| p::Error("worker process registry is unavailable".into()))?;
        if self.recovery_required.load(Ordering::SeqCst)
            || self.journal.pending_cleanup()?.iter().any(|lease| {
                !children
                    .values()
                    .any(|worker| worker.lease_id == lease.lease_id)
            })
        {
            self.recovery_required.store(true, Ordering::SeqCst);
            return Err(p::Error(
                "sandbox lease recovery is required; worker admission is blocked".into(),
            ));
        }
        if children.contains_key(&job.job_id) {
            return Err(p::Error("worker process is already admitted".into()));
        }
        let worker = self.spawn_worker(job, permit, execution, now)?;
        children.insert(job.job_id.clone(), worker);
        Ok(())
    }

    fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
        self.cancel_at(job, job.deadline)
    }

    fn cancel_at(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt> {
        self.fence_on_error(self.cancel_worker(job, now))
    }

    fn observe(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        self.fence_on_error(self.observe_worker(job, now))
    }

    fn result_path(&self, job: &p::WorkerJob) -> p::Result<Option<PathBuf>> {
        self.fence_on_error(self.terminal_result_path(job))
    }

    fn result_sealed(&self, job: &p::WorkerJob, now: p::Timestamp) -> p::Result<()> {
        self.fence_on_error(self.finish_result_handoff(job, now))
    }
}

impl WindowsAppContainerWorkerIsolation {
    fn fence_on_error<T>(&self, result: p::Result<T>) -> p::Result<T> {
        if result.is_err() {
            self.recovery_required.store(true, Ordering::SeqCst);
        }
        result
    }

    fn cancel_worker(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt> {
        let worker = self
            .children
            .lock()
            .map_err(|_| p::Error("worker process registry is unavailable".into()))?
            .remove(&job.job_id)
            .ok_or_else(|| p::Error("worker process is not active".into()))?;
        let cancel_state = match worker.native.cancel() {
            Ok(state) => state,
            Err(error) => {
                self.restore_worker(job, worker)?;
                return Err(error);
            }
        };
        let terminal = match worker.native.terminal_state() {
            Ok(terminal) => terminal,
            Err(error) => {
                self.restore_worker(job, worker)?;
                return Err(error);
            }
        };
        if terminal != (true, 0) {
            self.restore_worker(job, worker)?;
            return Err(p::Error("worker cancellation is unverifiable".into()));
        }
        let outcome = match cancel_state {
            native::CancelState::Terminated => p::WorkerOutcome::Cancelled,
            native::CancelState::AlreadyExited => p::WorkerOutcome::Unknown,
        };
        self.restore_worker(job, worker)?;
        self.cancel_receipt(job, now, outcome)
    }
}

impl WindowsAppContainerWorkerIsolation {
    fn restore_worker(&self, job: &p::WorkerJob, worker: RunningWorker) -> p::Result<()> {
        self.children
            .lock()
            .map_err(|_| p::Error("worker process registry is unavailable".into()))?
            .insert(job.job_id.clone(), worker);
        Ok(())
    }

    fn cancel_receipt(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
        outcome: p::WorkerOutcome,
    ) -> p::Result<p::WorkerCancelReceipt> {
        let digest =
            p::canonical_digest(&("worker-appcontainer-cancel", &job.job_id, &job.plan_digest))?.0;
        Ok(p::WorkerCancelReceipt {
            schema_version: p::SchemaVersion(1),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            nonce: job.nonce.clone(),
            outcome,
            observed_at: now,
            receipt_ref: Some(format!("worker-cancel-{digest}")),
            reason_ref: (outcome == p::WorkerOutcome::Unknown)
                .then(|| "worker-cancel-outcome-unknown".into()),
        })
    }
}

impl Drop for WindowsAppContainerWorkerIsolation {
    fn drop(&mut self) {
        if let Ok(mut children) = self.children.lock() {
            for (_, worker) in std::mem::take(&mut *children) {
                worker.native.terminate_for_drop();
                let _ = recovery::cleanup_lease(
                    &self.journal,
                    &worker.lease_id,
                    &worker.run_dir,
                    worker.native.profile(),
                    worker.native.terminal_state().unwrap_or((false, u32::MAX)),
                    recovery_timestamp(),
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "windows_appcontainer_tests.rs"]
mod tests;
