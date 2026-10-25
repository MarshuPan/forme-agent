#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

//! Managed Linux workers backed by systemd transient units.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use forme_protocol as p;

use super::posix_worker_fs::{chown_readonly, set_group_directory};
use super::posix_worker_overlay::PreparedOverlay;
use super::process::{
    validate_scope_key, write_private_json, WorkerLaunchRegistry, WorkerLaunchSpec,
};
use super::{reject_linked_ancestors, WorkerIsolation};

const EXEC_USER: &str = "forme-exec";
const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
const SYSTEMCTL: &str = "/usr/bin/systemctl";
const ID: &str = "/usr/bin/id";
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(20);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct RunningWorker {
    started_at: p::Timestamp,
    overlay: PreparedOverlay,
    unit: String,
}

/// The adapter is selected only from the managed Host path. The original
/// workspace never becomes writable by the worker; Host writeback is the only
/// path back to brain-owned project data.
pub struct LinuxSystemdWorkerIsolation {
    runtime_root: PathBuf,
    launches: WorkerLaunchRegistry,
    workspace_roots: BTreeMap<String, PathBuf>,
    children: Mutex<BTreeMap<String, RunningWorker>>,
}

impl LinuxSystemdWorkerIsolation {
    pub fn open(
        runtime_root: PathBuf,
        launches: WorkerLaunchRegistry,
        workspace_roots: BTreeMap<String, PathBuf>,
    ) -> p::Result<Self> {
        require_root()?;
        require_managed_host_unit()?;
        require_command(SYSTEMD_RUN)?;
        require_command(SYSTEMCTL)?;
        require_user(EXEC_USER)?;
        if !runtime_root.is_absolute() || workspace_roots.is_empty() {
            return Err(p::Error(
                "Linux worker isolation requires absolute runtime and workspace paths".into(),
            ));
        }
        reject_linked_ancestors(&runtime_root)?;
        fs::create_dir_all(&runtime_root)
            .map_err(|_| p::Error("Linux worker runtime directory cannot be created".into()))?;
        set_group_directory(&runtime_root, EXEC_USER)?;
        let runtime_identity = fs::canonicalize(&runtime_root)
            .map_err(|_| p::Error("Linux worker runtime directory cannot be resolved".into()))?;
        for (scope, path) in &workspace_roots {
            validate_scope_key(scope)?;
            reject_linked_ancestors(path)?;
            let metadata = fs::symlink_metadata(path)
                .map_err(|_| p::Error("Linux worker workspace is unavailable".into()))?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(p::Error("Linux worker workspace is not a directory".into()));
            }
            let workspace = fs::canonicalize(path)
                .map_err(|_| p::Error("Linux worker workspace cannot be resolved".into()))?;
            if workspace == runtime_identity
                || workspace.starts_with(&runtime_identity)
                || runtime_identity.starts_with(&workspace)
            {
                return Err(p::Error(
                    "Linux worker runtime and workspace paths must be disjoint".into(),
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
            .ok_or_else(|| p::Error("Linux worker workspace scope has no installed path".into()))
    }

    fn run_dir(&self, job: &p::WorkerJob) -> p::Result<PathBuf> {
        let digest =
            p::canonical_digest(&("forme-linux-worker-run-v1", &job.job_id, &job.plan_digest))?.0;
        let path = self
            .runtime_root
            .join(format!("job-{}", digest.trim_start_matches("sha256:")));
        reject_linked_ancestors(&path)?;
        fs::create_dir(&path)
            .map_err(|_| p::Error("Linux worker job directory cannot be created".into()))?;
        set_group_directory(&path, EXEC_USER)?;
        Ok(path)
    }

    fn spawn_worker(
        &self,
        job: &p::WorkerJob,
        permit: &p::WorkerPermit,
        execution: Option<&p::WorkerExecutionSpec>,
        now: p::Timestamp,
    ) -> p::Result<RunningWorker> {
        let spec: WorkerLaunchSpec = self.launches.get(&job.sandbox_profile_ref)?.clone();
        spec.validate()?;
        if spec.sandbox_profile_ref != permit.sandbox_profile_ref {
            return Err(p::Error(
                "Linux worker launch profile is not permit-bound".into(),
            ));
        }
        if execution.is_some_and(action_uses_network) {
            return Err(p::Error(
                "Linux managed worker network access is unavailable without a scoped network adapter".into(),
            ));
        }
        let original = self.workspace(&job.workspace_scope)?.to_path_buf();
        let run_dir = self.run_dir(job)?;
        let overlay = PreparedOverlay::prepare(&run_dir, &original, job, permit, execution, now)?;
        let job_path = run_dir.join("job.json");
        let permit_path = run_dir.join("permit.json");
        let execution_path = run_dir.join("execution.json");
        write_private_json(&job_path, job)?;
        write_private_json(&permit_path, permit)?;
        chown_readonly(&job_path, EXEC_USER)?;
        chown_readonly(&permit_path, EXEC_USER)?;
        if let Some(value) = execution {
            write_private_json(&execution_path, value)?;
            chown_readonly(&execution_path, EXEC_USER)?;
        }

        let unit = unit_name(job)?;
        let mut args = systemd_args(
            &self.runtime_root,
            &overlay.workspace(),
            &unit,
            &job_path,
            &permit_path,
            &execution_path,
            execution.is_some(),
            job,
        );
        for sibling in self.sibling_paths(&run_dir)? {
            args.push(format!(
                "--property=InaccessiblePaths={}",
                sibling.display()
            ));
        }
        args.push("--no-block".into());
        args.push("--".into());
        args.push(spec.program.to_string_lossy().into_owned());
        args.extend(spec.args);
        run_control(SYSTEMD_RUN, &args, CONTROL_TIMEOUT)?;
        Ok(RunningWorker {
            started_at: now,
            overlay,
            unit,
        })
    }

    fn sibling_paths(&self, current: &Path) -> p::Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in fs::read_dir(&self.runtime_root)
            .map_err(|_| p::Error("Linux worker runtime cannot be inspected".into()))?
        {
            let path = entry
                .map_err(|_| p::Error("Linux worker runtime entry cannot be inspected".into()))?
                .path();
            if path != current {
                reject_linked_ancestors(&path)?;
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }

    fn observe_unit(&self, unit: &str) -> p::Result<Option<i32>> {
        let state = show_unit(unit)?;
        if matches!(
            state.active.as_str(),
            "activating" | "active" | "deactivating"
        ) {
            return Ok(None);
        }
        if !matches!(state.active.as_str(), "inactive" | "failed") {
            return Err(p::Error("Linux worker unit outcome is unknown".into()));
        }
        state
            .exit_status
            .map(Some)
            .ok_or_else(|| p::Error("Linux worker unit exit status is unavailable".into()))
    }
}

impl WorkerIsolation for LinuxSystemdWorkerIsolation {
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
            .map_err(|_| p::Error("Linux worker registry is unavailable".into()))?;
        if children.contains_key(&job.job_id) {
            return Err(p::Error("Linux worker is already admitted".into()));
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
        let unit = self
            .children
            .lock()
            .map_err(|_| p::Error("Linux worker registry is unavailable".into()))?
            .get(&job.job_id)
            .map(|worker| worker.unit.clone())
            .ok_or_else(|| p::Error("Linux worker is not active".into()))?;
        if self.observe_unit(&unit)?.is_some() {
            return Ok(cancel_receipt(
                job,
                now,
                p::WorkerOutcome::Unknown,
                Some("worker-finished-before-cancel".into()),
            )?);
        }
        if run_control(SYSTEMCTL, &["stop".into(), unit.clone()], CONTROL_TIMEOUT).is_err() {
            return Ok(cancel_receipt(
                job,
                now,
                p::WorkerOutcome::Unknown,
                Some("worker-systemd-stop-unknown".into()),
            )?);
        }
        let deadline = Instant::now() + CANCEL_TIMEOUT;
        loop {
            if let Some(status) = self.observe_unit(&unit)? {
                let outcome = if status == 0 {
                    p::WorkerOutcome::Unknown
                } else {
                    p::WorkerOutcome::Cancelled
                };
                let reason = (outcome == p::WorkerOutcome::Unknown)
                    .then(|| "worker-exited-before-cancel".into());
                let receipt = cancel_receipt(job, now, outcome, reason)?;
                self.children
                    .lock()
                    .map_err(|_| p::Error("Linux worker registry is unavailable".into()))?
                    .remove(&job.job_id);
                return Ok(receipt);
            }
            if Instant::now() >= deadline {
                return Ok(cancel_receipt(
                    job,
                    now,
                    p::WorkerOutcome::Unknown,
                    Some("worker-systemd-cancel-timeout".into()),
                )?);
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn observe(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        let mut children = self
            .children
            .lock()
            .map_err(|_| p::Error("Linux worker registry is unavailable".into()))?;
        let Some(worker) = children.get(&job.job_id) else {
            return Ok(None);
        };
        let Some(status) = self.observe_unit(&worker.unit)? else {
            return Ok(None);
        };
        if status != 0 {
            let started_at = worker.started_at;
            children.remove(&job.job_id);
            return Ok(Some(receipt(
                job,
                started_at,
                now,
                p::WorkerOutcome::Failed,
            )?));
        }
        worker.overlay.complete(job, now)?;
        let started_at = worker.started_at;
        children.remove(&job.job_id);
        Ok(Some(receipt(
            job,
            started_at,
            now,
            p::WorkerOutcome::Completed,
        )?))
    }
}

impl Drop for LinuxSystemdWorkerIsolation {
    fn drop(&mut self) {
        if let Ok(children) = self.children.lock() {
            for worker in children.values() {
                let _ = run_control(
                    SYSTEMCTL,
                    &["stop".into(), worker.unit.clone()],
                    CONTROL_TIMEOUT,
                );
            }
        }
    }
}

fn systemd_args(
    runtime_root: &Path,
    workspace: &Path,
    unit: &str,
    job_path: &Path,
    permit_path: &Path,
    execution_path: &Path,
    has_execution: bool,
    job: &p::WorkerJob,
) -> Vec<String> {
    let mut args = vec![
        "--system".into(),
        "--unit".into(),
        unit.into(),
        "--property=Type=exec".into(),
        format!("--property=User={EXEC_USER}"),
        format!("--property=Group={EXEC_USER}"),
        "--property=NoNewPrivileges=yes".into(),
        "--property=PrivateTmp=yes".into(),
        "--property=PrivateDevices=yes".into(),
        "--property=ProtectSystem=strict".into(),
        "--property=ProtectHome=yes".into(),
        "--property=ProtectKernelTunables=yes".into(),
        "--property=ProtectControlGroups=yes".into(),
        "--property=RestrictSUIDSGID=yes".into(),
        "--property=LockPersonality=yes".into(),
        "--property=MemoryDenyWriteExecute=yes".into(),
        "--property=RestrictAddressFamilies=AF_UNIX".into(),
        format!("--property=ReadOnlyPaths={}", runtime_root.display()),
        format!("--property=ReadWritePaths={}", workspace.display()),
        format!("--working-directory={}", workspace.display()),
        "--setenv=FORME_WORKER_PROTOCOL_VERSION=1".into(),
        format!("--setenv=FORME_WORKER_JOB_PATH={}", job_path.display()),
        format!(
            "--setenv=FORME_WORKER_PERMIT_PATH={}",
            permit_path.display()
        ),
        format!(
            "--setenv=FORME_WORKER_WORKSPACE_PATH={}",
            workspace.display()
        ),
        format!(
            "--setenv=FORME_WORKER_WORKSPACE_SCOPE={}",
            job.workspace_scope.0
        ),
        format!("--setenv=FORME_WORKER_DEADLINE={}", job.deadline),
    ];
    if has_execution {
        args.push(format!(
            "--setenv=FORME_WORKER_EXECUTION_PATH={}",
            execution_path.display()
        ));
    }
    args
}

fn action_uses_network(execution: &p::WorkerExecutionSpec) -> bool {
    matches!(
        execution.intent.parameters,
        p::ActionParameters::Shell { network: true, .. }
    )
}

fn unit_name(job: &p::WorkerJob) -> p::Result<String> {
    let digest =
        p::canonical_digest(&("forme-linux-worker-unit-v1", &job.job_id, &job.plan_digest))?.0;
    Ok(format!(
        "forme-worker-{}.service",
        digest.trim_start_matches("sha256:")
    ))
}

fn require_command(path: &str) -> p::Result<()> {
    if Path::new(path).is_file() {
        Ok(())
    } else {
        Err(p::Error(format!("Linux managed worker requires {path}")))
    }
}

fn require_root() -> p::Result<()> {
    if run_control(ID, &["-u".into()], CONTROL_TIMEOUT)?.trim() == "0" {
        Ok(())
    } else {
        Err(p::Error(
            "Linux managed worker host must run as root".into(),
        ))
    }
}

fn require_user(user: &str) -> p::Result<()> {
    if run_control(ID, &["-u".into(), user.into()], CONTROL_TIMEOUT)?
        .trim()
        .parse::<u32>()
        .is_ok()
    {
        Ok(())
    } else {
        Err(p::Error(
            "Linux managed worker execution account is unavailable".into(),
        ))
    }
}

fn run_control(program: &str, args: &[String], timeout: Duration) -> p::Result<String> {
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| p::Error("Linux worker control command is unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| p::Error("Linux worker control output is unavailable".into()))?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take((MAX_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(p::Error(
                    "Linux worker control command outcome is unknown".into(),
                ));
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| p::Error("Linux worker control output failed".into()))?
        .map_err(|_| p::Error("Linux worker control output failed".into()))?;
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(p::Error(
            "Linux worker control output exceeded its bound".into(),
        ));
    }
    if !status.success() {
        return Err(p::Error("Linux worker control command was rejected".into()));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn require_managed_host_unit() -> p::Result<()> {
    let cgroup = fs::read_to_string("/proc/self/cgroup")
        .map_err(|_| p::Error("Linux managed worker host cgroup is unavailable".into()))?;
    if cgroup
        .lines()
        .any(|line| line.ends_with("/forme-host.service"))
    {
        Ok(())
    } else {
        Err(p::Error(
            "Linux managed worker host is not bound to forme-host.service".into(),
        ))
    }
}

struct UnitState {
    active: String,
    exit_status: Option<i32>,
}

fn show_unit(unit: &str) -> p::Result<UnitState> {
    let output = run_control(
        SYSTEMCTL,
        &[
            "show".into(),
            "--no-pager".into(),
            "--property=ActiveState".into(),
            "--property=ExecMainStatus".into(),
            unit.into(),
        ],
        CONTROL_TIMEOUT,
    )?;
    let mut active = None;
    let mut exit_status = None;
    for line in output.lines() {
        if let Some(value) = line.strip_prefix("ActiveState=") {
            active = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("ExecMainStatus=") {
            exit_status = value.parse::<i32>().ok();
        }
    }
    Ok(UnitState {
        active: active.ok_or_else(|| p::Error("Linux worker unit state is incomplete".into()))?,
        exit_status,
    })
}

fn receipt(
    job: &p::WorkerJob,
    started_at: p::Timestamp,
    finished_at: p::Timestamp,
    outcome: p::WorkerOutcome,
) -> p::Result<p::WorkerReceipt> {
    let digest = p::canonical_digest(&("linux-worker-receipt", &job.job_id, &job.plan_digest))?.0;
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
    reason_ref: Option<String>,
) -> p::Result<p::WorkerCancelReceipt> {
    let digest = p::canonical_digest(&("linux-worker-cancel", &job.job_id, &job.plan_digest))?.0;
    Ok(p::WorkerCancelReceipt {
        schema_version: p::SchemaVersion(1),
        job_id: job.job_id.clone(),
        plan_digest: job.plan_digest.clone(),
        nonce: job.nonce.clone(),
        outcome,
        observed_at: now,
        receipt_ref: Some(format!("worker-cancel-{digest}")),
        reason_ref,
    })
}
