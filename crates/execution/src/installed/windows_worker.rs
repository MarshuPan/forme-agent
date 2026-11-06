#![cfg(windows)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use forme_protocol as p;
use win_custody::{Job, JobLimits, Launch, Process, WaitOutcome};

use super::process::{
    set_private_directory, validate_scope_key, write_private_json, WorkerLaunchRegistry,
    WorkerLaunchSpec,
};
use super::{reject_linked_ancestors, WorkerIsolation};

const CANCEL_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct RunningWorker {
    process: Process,
    job: Job,
    started_at: p::Timestamp,
}

/// Windows worker isolation using a kill-on-close Job Object.
///
/// The job is created before launch and `Launch::spawn_in_job` assigns the
/// suspended process before its first instruction. This closes the process
/// tree lifetime gap left by a post-spawn Job Object assignment. It does not
/// replace the required service-account, ACL, network and credential-slot
/// controls; those remain separate product gates.
pub struct WindowsJobWorkerIsolation {
    runtime_root: PathBuf,
    launches: WorkerLaunchRegistry,
    workspace_roots: BTreeMap<String, PathBuf>,
    children: Mutex<BTreeMap<String, RunningWorker>>,
}

impl WindowsJobWorkerIsolation {
    pub fn open(
        runtime_root: PathBuf,
        launches: WorkerLaunchRegistry,
        workspace_roots: BTreeMap<String, PathBuf>,
    ) -> p::Result<Self> {
        if !runtime_root.is_absolute() || workspace_roots.is_empty() {
            return Err(p::Error(
                "Windows worker isolation requires absolute runtime and workspace paths".into(),
            ));
        }
        reject_linked_ancestors(&runtime_root)?;
        fs::create_dir_all(&runtime_root)
            .map_err(|_| p::Error("worker runtime directory cannot be created".into()))?;
        reject_linked_ancestors(&runtime_root)?;
        set_private_directory(&runtime_root)?;
        let runtime_identity = fs::canonicalize(&runtime_root)
            .map_err(|_| p::Error("worker runtime directory cannot be resolved".into()))?;

        for (scope, path) in &workspace_roots {
            validate_scope_key(scope)?;
            reject_linked_ancestors(path)?;
            let metadata = fs::symlink_metadata(path)
                .map_err(|_| p::Error("worker workspace is unavailable".into()))?;
            if !metadata.is_dir() {
                return Err(p::Error("worker workspace is not a directory".into()));
            }
            let workspace = fs::canonicalize(path)
                .map_err(|_| p::Error("worker workspace cannot be resolved".into()))?;
            if workspace == runtime_identity
                || workspace.starts_with(&runtime_identity)
                || runtime_identity.starts_with(&workspace)
            {
                return Err(p::Error(
                    "worker runtime and workspace paths must be disjoint".into(),
                ));
            }
        }

        Ok(Self {
            runtime_root,
            launches,
            workspace_roots,
            children: Mutex::new(BTreeMap::new()),
        })
    }

    fn workspace(&self, scope: &p::Scope) -> p::Result<&Path> {
        self.workspace_roots
            .get(&scope.0)
            .map(PathBuf::as_path)
            .ok_or_else(|| p::Error("worker workspace scope has no installed path".into()))
    }

    fn run_dir(&self, job: &p::WorkerJob) -> p::Result<PathBuf> {
        let digest = p::canonical_digest(&("worker-run", &job.job_id, &job.plan_digest))?.0;
        let safe_digest = digest.strip_prefix("sha256:").unwrap_or(&digest);
        let path = self.runtime_root.join(format!("job-{safe_digest}"));
        reject_linked_ancestors(&path)?;
        Ok(path)
    }

    fn spawn_worker(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
    ) -> p::Result<(Process, Job)> {
        let spec: WorkerLaunchSpec = self.launches.get(&job.sandbox_profile_ref)?.clone();
        spec.validate()?;
        if spec.sandbox_profile_ref != permit.sandbox_profile_ref {
            return Err(p::Error("worker launch profile is not permit-bound".into()));
        }
        let workspace = self.workspace(&job.workspace_scope)?.to_path_buf();
        reject_linked_ancestors(&workspace)?;
        if !fs::symlink_metadata(&workspace)
            .map_err(|_| p::Error("worker workspace is unavailable".into()))?
            .is_dir()
        {
            return Err(p::Error("worker workspace is not a directory".into()));
        }
        let run_dir = self.run_dir(job)?;
        fs::create_dir(&run_dir)
            .map_err(|_| p::Error("worker run directory cannot be created".into()))?;
        set_private_directory(&run_dir)?;

        let job_path = run_dir.join("job.json");
        let permit_path = run_dir.join("permit.json");
        write_private_json(&job_path, job)?;
        write_private_json(&permit_path, permit)?;
        let execution_path = run_dir.join("execution.json");
        if let Some(execution) = execution {
            write_private_json(&execution_path, execution)?;
        }

        let job_object = Job::create(&JobLimits::new().kill_on_close(true))
            .map_err(|_| p::Error("worker Job Object could not be created".into()))?;
        let mut launch = Launch::new(spec.program);
        launch
            .args(spec.args)
            .current_dir(workspace.clone())
            .env_clear()
            .env("FORME_WORKER_PROTOCOL_VERSION", "1")
            .env("FORME_WORKER_JOB_PATH", job_path)
            .env("FORME_WORKER_PERMIT_PATH", permit_path)
            .env("FORME_WORKER_WORKSPACE_PATH", workspace.clone())
            .env("FORME_WORKER_WORKSPACE_SCOPE", &job.workspace_scope.0)
            .env("FORME_WORKER_DEADLINE", job.deadline.to_string());
        if execution.is_some() {
            launch.env("FORME_WORKER_EXECUTION_PATH", execution_path);
        }
        let process = launch
            .spawn_in_job(&job_object)
            .map_err(|_| p::Error("worker process could not be admitted to a Job Object".into()))?;
        Ok((process, job_object))
    }

    fn receipt(
        job: &p::WorkerJob,
        started_at: p::Timestamp,
        finished_at: p::Timestamp,
        outcome: p::WorkerOutcome,
    ) -> p::Result<p::WorkerReceipt> {
        let digest = p::canonical_digest(&("worker-receipt", &job.job_id, &job.plan_digest))?.0;
        Ok(p::WorkerReceipt {
            schema_version: p::SchemaVersion(1),
            receipt_ref: format!("worker-receipt-{digest}"),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            observed_outcome: outcome,
            safe_result_ref: Some(format!("worker-result-{digest}")),
            ground_truth_refs: vec![format!("worker-exit-{digest}")],
            started_at,
            finished_at: Some(finished_at),
        })
    }

    fn cancel_receipt(
        job: &p::WorkerJob,
        now: p::Timestamp,
        outcome: p::WorkerOutcome,
        reason: Option<String>,
    ) -> p::Result<p::WorkerCancelReceipt> {
        let digest = p::canonical_digest(&("worker-cancel", &job.job_id, &job.plan_digest))?.0;
        Ok(p::WorkerCancelReceipt {
            schema_version: p::SchemaVersion(1),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            nonce: job.nonce.clone(),
            outcome,
            observed_at: now,
            receipt_ref: Some(format!("worker-cancel-{digest}")),
            reason_ref: reason,
        })
    }

    fn admit_at_with_execution(
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
        if children.contains_key(&job.job_id) {
            return Err(p::Error("worker process is already admitted".into()));
        }
        let (process, job_object) = self.spawn_worker(job, permit, execution)?;
        children.insert(
            job.job_id.clone(),
            RunningWorker {
                process,
                job: job_object,
                started_at: now,
            },
        );
        Ok(())
    }
}

impl WorkerIsolation for WindowsJobWorkerIsolation {
    fn with_quiescent_resources(&self, action: &mut dyn FnMut() -> p::Result<()>) -> p::Result<()> {
        super::worker_isolation::with_empty_registry(&self.children, action)
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
        self.admit_at_with_execution(job, permit, None, now)
    }

    fn admit_with_execution(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<()> {
        self.admit_at_with_execution(job, permit, execution, now)
    }

    fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
        self.cancel_at(job, job.deadline)
    }

    fn cancel_at(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<p::WorkerCancelReceipt> {
        let running = self
            .children
            .lock()
            .map_err(|_| p::Error("worker process registry is unavailable".into()))?
            .remove(&job.job_id)
            .ok_or_else(|| p::Error("worker process is not active".into()))?;

        if running
            .process
            .exit_code()
            .map_err(|_| p::Error("worker process outcome is unknown".into()))?
            .is_some()
        {
            return Self::cancel_receipt(
                job,
                now,
                p::WorkerOutcome::Unknown,
                Some("worker-process-finished-before-cancel".into()),
            );
        }

        if running.job.terminate(1).is_err()
            || !matches!(
                running.process.wait(Some(CANCEL_WAIT)),
                Ok(WaitOutcome::Signaled | WaitOutcome::Abandoned)
            )
        {
            return Self::cancel_receipt(
                job,
                now,
                p::WorkerOutcome::Unknown,
                Some("worker-process-tree-cancel-outcome-unknown".into()),
            );
        }
        Self::cancel_receipt(job, now, p::WorkerOutcome::Cancelled, None)
    }

    fn observe(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| p::Error("worker process registry is unavailable".into()))?;
        let Some(running) = children.get(&job.job_id) else {
            return Ok(None);
        };
        let Some(exit_code) = running
            .process
            .exit_code()
            .map_err(|_| p::Error("worker process outcome is unknown".into()))?
        else {
            return Ok(None);
        };
        let started_at = running.started_at;
        let outcome = if exit_code == 0 {
            p::WorkerOutcome::Completed
        } else {
            p::WorkerOutcome::Failed
        };
        children.remove(&job.job_id);
        Ok(Some(Self::receipt(job, started_at, now, outcome)?))
    }
}

impl Drop for WindowsJobWorkerIsolation {
    fn drop(&mut self) {
        if let Ok(mut children) = self.children.lock() {
            for (_, running) in std::mem::take(&mut *children) {
                let _ = running.job.terminate(1);
                let _ = running.process.wait(Some(CANCEL_WAIT));
            }
        }
    }
}
