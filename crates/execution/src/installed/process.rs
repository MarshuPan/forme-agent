use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use forme_protocol as p;
use serde::Serialize;

use super::{reject_linked_ancestors, WorkerIsolation};

const MAX_LAUNCH_ARGS: usize = 64;
const MAX_LAUNCH_ARG_BYTES: usize = 4096;

/// A launch entry is installed configuration, not model-provided input. The
/// job selects this entry by profile reference and cannot replace its program
/// or arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerLaunchSpec {
    pub schema_version: p::SchemaVersion,
    pub sandbox_profile_ref: String,
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl WorkerLaunchSpec {
    pub fn new(
        sandbox_profile_ref: impl Into<String>,
        program: PathBuf,
        args: Vec<String>,
    ) -> p::Result<Self> {
        let spec = Self {
            schema_version: p::SchemaVersion(1),
            sandbox_profile_ref: sandbox_profile_ref.into(),
            program,
            args,
        };
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version != p::SchemaVersion(1) {
            return Err(p::Error("unsupported worker launch schema".into()));
        }
        p::validate_product_id(&self.sandbox_profile_ref)
            .map_err(|_| p::Error("worker sandbox profile reference is invalid".into()))?;
        if self.args.len() > MAX_LAUNCH_ARGS {
            return Err(p::Error("worker launch arguments are unbounded".into()));
        }
        if self.args.iter().any(|arg| {
            arg.len() > MAX_LAUNCH_ARG_BYTES
                || arg.contains('\0')
                || arg.chars().any(char::is_control)
        }) {
            return Err(p::Error("worker launch argument is invalid".into()));
        }
        validate_program(&self.program)
    }
}

/// Closed launch registry loaded from the signed/managed installation
/// configuration. It deliberately has no API for arbitrary command text.
#[derive(Debug, Clone, Default)]
pub struct WorkerLaunchRegistry {
    entries: BTreeMap<String, WorkerLaunchSpec>,
}

impl WorkerLaunchRegistry {
    pub fn from_specs(specs: impl IntoIterator<Item = WorkerLaunchSpec>) -> p::Result<Self> {
        let mut entries = BTreeMap::new();
        for spec in specs {
            spec.validate()?;
            if entries
                .insert(spec.sandbox_profile_ref.clone(), spec)
                .is_some()
            {
                return Err(p::Error("worker sandbox profile is duplicated".into()));
            }
        }
        if entries.is_empty() {
            return Err(p::Error("worker launch registry is empty".into()));
        }
        Ok(Self { entries })
    }

    pub(crate) fn get(&self, profile_ref: &str) -> p::Result<&WorkerLaunchSpec> {
        self.entries
            .get(profile_ref)
            .ok_or_else(|| p::Error("worker sandbox profile is not installed".into()))
    }
}

#[derive(Debug)]
struct RunningWorker {
    child: Child,
    started_at: p::Timestamp,
}

/// Process-backed worker execution with a closed launch registry and exact
/// workspace mapping. This is a real child-process boundary, but it is not a
/// claim that Windows Job Objects, Linux cgroups/systemd sandboxing, or macOS
/// sandbox profiles have been installed. Production strict profiles must use a
/// platform adapter that supplies those controls; absent that adapter, the
/// default host remains fail-closed.
pub struct ProcessWorkerIsolation {
    runtime_root: PathBuf,
    launches: WorkerLaunchRegistry,
    workspace_roots: BTreeMap<String, PathBuf>,
    children: Mutex<BTreeMap<String, RunningWorker>>,
}

impl ProcessWorkerIsolation {
    pub fn open(
        runtime_root: PathBuf,
        launches: WorkerLaunchRegistry,
        workspace_roots: BTreeMap<String, PathBuf>,
    ) -> p::Result<Self> {
        if !runtime_root.is_absolute() || workspace_roots.is_empty() {
            return Err(p::Error(
                "worker process isolation requires absolute runtime and workspace paths".into(),
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
    ) -> p::Result<Child> {
        let spec = self.launches.get(&job.sandbox_profile_ref)?;
        spec.validate()?;
        if spec.sandbox_profile_ref != permit.sandbox_profile_ref {
            return Err(p::Error("worker launch profile is not permit-bound".into()));
        }
        let workspace = self.workspace(&job.workspace_scope)?;
        reject_linked_ancestors(workspace)?;
        if !fs::symlink_metadata(workspace)
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

        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(workspace)
            .env_clear()
            .env("FORME_WORKER_PROTOCOL_VERSION", "1")
            .env("FORME_WORKER_JOB_PATH", &job_path)
            .env("FORME_WORKER_PERMIT_PATH", &permit_path)
            .env("FORME_WORKER_WORKSPACE_PATH", workspace)
            .env("FORME_WORKER_WORKSPACE_SCOPE", &job.workspace_scope.0)
            .env("FORME_WORKER_DEADLINE", job.deadline.to_string())
            .stdin(Stdio::null())
            // Worker output is not a result channel. Discard it here so a
            // noisy child cannot grow an unbounded product log or fill the
            // installation volume; bounded result artifacts are a separate
            // execution-backend concern.
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if execution.is_some() {
            command.env("FORME_WORKER_EXECUTION_PATH", &execution_path);
        }
        command
            .spawn()
            .map_err(|_| p::Error("worker process could not be started".into()))
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
        let child = self.spawn_worker(job, permit, execution)?;
        children.insert(
            job.job_id.clone(),
            RunningWorker {
                child,
                started_at: now,
            },
        );
        Ok(())
    }
}

impl WorkerIsolation for ProcessWorkerIsolation {
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
        let mut running = self
            .children
            .lock()
            .map_err(|_| p::Error("worker process registry is unavailable".into()))?
            .remove(&job.job_id)
            .ok_or_else(|| p::Error("worker process is not active".into()))?;
        if running
            .child
            .try_wait()
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
        if running.child.kill().is_err() || running.child.wait().is_err() {
            return Self::cancel_receipt(
                job,
                now,
                p::WorkerOutcome::Unknown,
                Some("worker-process-cancel-outcome-unknown".into()),
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
        let Some(running) = children.get_mut(&job.job_id) else {
            return Ok(None);
        };
        let Some(status) = running
            .child
            .try_wait()
            .map_err(|_| p::Error("worker process outcome is unknown".into()))?
        else {
            return Ok(None);
        };
        let started_at = running.started_at;
        let outcome = if status.success() {
            p::WorkerOutcome::Completed
        } else {
            p::WorkerOutcome::Failed
        };
        children.remove(&job.job_id);
        Ok(Some(Self::receipt(job, started_at, now, outcome)?))
    }
}

impl Drop for ProcessWorkerIsolation {
    fn drop(&mut self) {
        if let Ok(mut children) = self.children.lock() {
            for (_, mut running) in std::mem::take(&mut *children) {
                let _ = running.child.kill();
                let _ = running.child.wait();
            }
        }
    }
}

fn validate_program(program: &Path) -> p::Result<()> {
    if !program.is_absolute() {
        return Err(p::Error("worker program path must be absolute".into()));
    }
    reject_linked_ancestors(program)?;
    let metadata = fs::symlink_metadata(program)
        .map_err(|_| p::Error("worker program is unavailable".into()))?;
    if !metadata.is_file() {
        return Err(p::Error("worker program is not a regular file".into()));
    }
    let name = program
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ["cmd.exe", "powershell.exe", "pwsh.exe", "bash", "sh", "zsh"].contains(&name.as_str()) {
        return Err(p::Error("worker launch program cannot be a shell".into()));
    }
    Ok(())
}

pub(crate) fn validate_scope_key(value: &str) -> p::Result<()> {
    if value.is_empty()
        || value.len() > 512
        || !value.starts_with("workspace:")
        || value.contains(['\\', '\0'])
        || value.chars().any(char::is_control)
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(p::Error("worker workspace scope is invalid".into()));
    }
    Ok(())
}

pub(crate) fn set_private_directory(path: &Path) -> p::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| p::Error("worker directory permissions cannot be restricted".into()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn private_file(path: &Path) -> p::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| p::Error("worker private file cannot be created".into()))
}

pub(crate) fn write_private_json<T: Serialize>(path: &Path, value: &T) -> p::Result<()> {
    let mut file = private_file(path)?;
    let bytes = serde_json::to_vec(value)
        .map_err(|_| p::Error("worker launch payload cannot be serialized".into()))?;
    file.write_all(&bytes)
        .map_err(|_| p::Error("worker launch payload cannot be written".into()))?;
    file.sync_all()
        .map_err(|_| p::Error("worker launch payload cannot be synchronized".into()))
}
