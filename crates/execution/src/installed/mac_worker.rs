#![cfg(target_os = "macos")]
#![forbid(unsafe_code)]

//! Managed macOS workers backed by system LaunchDaemon jobs.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use forme_protocol as p;

use super::mac_worker_sandbox::{launchd_plist, sandbox_profile};
use super::posix_worker_fs::{
    chown_readonly, chown_recursive, set_group_directory, write_private_bytes,
};
use super::posix_worker_overlay::PreparedOverlay;
use super::process::{
    validate_scope_key, write_private_json, WorkerLaunchRegistry, WorkerLaunchSpec,
};
use super::{reject_linked_ancestors, WorkerIsolation};

const EXEC_USER: &str = "_formeexec";
const LAUNCHCTL: &str = "/bin/launchctl";
const ID: &str = "/usr/bin/id";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(20);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct RunningWorker {
    started_at: p::Timestamp,
    overlay: PreparedOverlay,
    label: String,
}

/// The adapter is selected only from the installed LaunchDaemon Host path.
/// Workers receive a private copy and the Host owns all writeback decisions.
pub struct MacOsLaunchdWorkerIsolation {
    runtime_root: PathBuf,
    launches: WorkerLaunchRegistry,
    workspace_roots: BTreeMap<String, PathBuf>,
    children: Mutex<BTreeMap<String, RunningWorker>>,
}

impl MacOsLaunchdWorkerIsolation {
    pub fn open(
        runtime_root: PathBuf,
        launches: WorkerLaunchRegistry,
        workspace_roots: BTreeMap<String, PathBuf>,
    ) -> p::Result<Self> {
        require_root()?;
        require_managed_host()?;
        require_command(LAUNCHCTL)?;
        require_command(SANDBOX_EXEC)?;
        require_user(EXEC_USER)?;
        if !runtime_root.is_absolute() || workspace_roots.is_empty() {
            return Err(p::Error(
                "macOS worker isolation requires absolute runtime and workspace paths".into(),
            ));
        }
        reject_linked_ancestors(&runtime_root)?;
        fs::create_dir_all(&runtime_root)
            .map_err(|_| p::Error("macOS worker runtime directory cannot be created".into()))?;
        set_group_directory(&runtime_root, EXEC_USER)?;
        let runtime_identity = fs::canonicalize(&runtime_root)
            .map_err(|_| p::Error("macOS worker runtime directory cannot be resolved".into()))?;
        for (scope, path) in &workspace_roots {
            validate_scope_key(scope)?;
            reject_linked_ancestors(path)?;
            let metadata = fs::symlink_metadata(path)
                .map_err(|_| p::Error("macOS worker workspace is unavailable".into()))?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(p::Error("macOS worker workspace is not a directory".into()));
            }
            let workspace = fs::canonicalize(path)
                .map_err(|_| p::Error("macOS worker workspace cannot be resolved".into()))?;
            if workspace == runtime_identity
                || workspace.starts_with(&runtime_identity)
                || runtime_identity.starts_with(&workspace)
            {
                return Err(p::Error(
                    "macOS worker runtime and workspace paths must be disjoint".into(),
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
            .ok_or_else(|| p::Error("macOS worker workspace scope has no installed path".into()))
    }

    fn run_dir(&self, job: &p::WorkerJob) -> p::Result<PathBuf> {
        let digest =
            p::canonical_digest(&("forme-macos-worker-run-v1", &job.job_id, &job.plan_digest))?.0;
        let path = self
            .runtime_root
            .join(format!("job-{}", digest.trim_start_matches("sha256:")));
        reject_linked_ancestors(&path)?;
        fs::create_dir(&path)
            .map_err(|_| p::Error("macOS worker job directory cannot be created".into()))?;
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
                "macOS worker launch profile is not permit-bound".into(),
            ));
        }
        if fs::metadata(&spec.program)
            .map_err(|_| p::Error("macOS worker program is unavailable".into()))?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(p::Error("macOS worker program is not executable".into()));
        }
        let execution = execution
            .ok_or_else(|| p::Error("macOS managed worker requires an execution spec".into()))?;
        if action_uses_network(execution) {
            return Err(p::Error(
                "macOS managed worker network access is unavailable without a scoped adapter"
                    .into(),
            ));
        }
        let original = self.workspace(&job.workspace_scope)?.to_path_buf();
        let run_dir = self.run_dir(job)?;
        let overlay =
            PreparedOverlay::prepare(&run_dir, &original, job, permit, Some(execution), now)?;
        let workspace = overlay.workspace();
        let temporary = run_dir.join("tmp");
        fs::create_dir(&temporary)
            .map_err(|_| p::Error("macOS worker temporary directory cannot be created".into()))?;
        chown_recursive(&temporary, EXEC_USER)?;
        let job_path = run_dir.join("job.json");
        let permit_path = run_dir.join("permit.json");
        let execution_path = run_dir.join("execution.json");
        write_private_json(&job_path, job)?;
        write_private_json(&permit_path, permit)?;
        write_private_json(&execution_path, execution)?;
        for path in [&job_path, &permit_path, &execution_path] {
            chown_readonly(path, EXEC_USER)?;
        }
        let sandbox_path = run_dir.join("worker.sb");
        let profile = sandbox_profile(
            &workspace,
            &temporary,
            &[&job_path, &permit_path, &execution_path],
            &spec.program,
        )?;
        write_private_bytes(&sandbox_path, profile.as_bytes())?;
        chown_readonly(&sandbox_path, EXEC_USER)?;
        let mut environment = BTreeMap::new();
        environment.insert("FORME_WORKER_PROTOCOL_VERSION".into(), "1".into());
        environment.insert(
            "FORME_WORKER_JOB_PATH".into(),
            job_path.to_string_lossy().into_owned(),
        );
        environment.insert(
            "FORME_WORKER_PERMIT_PATH".into(),
            permit_path.to_string_lossy().into_owned(),
        );
        environment.insert(
            "FORME_WORKER_EXECUTION_PATH".into(),
            execution_path.to_string_lossy().into_owned(),
        );
        environment.insert(
            "FORME_WORKER_WORKSPACE_PATH".into(),
            workspace.to_string_lossy().into_owned(),
        );
        environment.insert(
            "FORME_WORKER_WORKSPACE_SCOPE".into(),
            job.workspace_scope.0.clone(),
        );
        environment.insert("FORME_WORKER_DEADLINE".into(), job.deadline.to_string());
        environment.insert("TMPDIR".into(), temporary.to_string_lossy().into_owned());
        let label = unit_label(job)?;
        let plist_path = run_dir.join("worker.plist");
        let plist = launchd_plist(
            &label,
            &sandbox_path,
            &spec.program,
            &spec.args,
            &workspace,
            &environment,
        )?;
        write_private_bytes(&plist_path, plist.as_bytes())?;
        run_control(
            LAUNCHCTL,
            &[
                "bootstrap".into(),
                "system".into(),
                plist_path.to_string_lossy().into_owned(),
            ],
            CONTROL_TIMEOUT,
        )?;
        Ok(RunningWorker {
            started_at: now,
            overlay,
            label,
        })
    }

    fn observe_unit(&self, label: &str) -> p::Result<Option<i32>> {
        let state = show_unit(label)?;
        if matches!(state.as_str(), "running" | "spawn scheduled") {
            return Ok(None);
        }
        if state == "exited" {
            return last_exit_code(label)?
                .map(Some)
                .ok_or_else(|| p::Error("macOS worker launchd exit status is unavailable".into()));
        }
        Err(p::Error("macOS worker launchd state is unknown".into()))
    }
}

impl WorkerIsolation for MacOsLaunchdWorkerIsolation {
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
            .map_err(|_| p::Error("macOS worker registry is unavailable".into()))?;
        if children.contains_key(&job.job_id) {
            return Err(p::Error("macOS worker is already admitted".into()));
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
        let label = self
            .children
            .lock()
            .map_err(|_| p::Error("macOS worker registry is unavailable".into()))?
            .get(&job.job_id)
            .map(|worker| worker.label.clone())
            .ok_or_else(|| p::Error("macOS worker is not active".into()))?;
        if let Some(status) = self.observe_unit(&label)? {
            return cancel_receipt(
                job,
                now,
                p::WorkerOutcome::Unknown,
                Some(if status == 0 {
                    "worker-finished-before-cancel".into()
                } else {
                    "worker-failed-before-cancel".into()
                }),
            );
        }
        if run_control(
            LAUNCHCTL,
            &["kill".into(), "SIGTERM".into(), format!("system/{label}")],
            CONTROL_TIMEOUT,
        )
        .is_err()
        {
            return cancel_receipt(
                job,
                now,
                p::WorkerOutcome::Unknown,
                Some("worker-launchd-stop-unknown".into()),
            );
        }
        let deadline = Instant::now() + CANCEL_TIMEOUT;
        loop {
            if let Some(status) = self.observe_unit(&label)? {
                let outcome = if status == 0 {
                    p::WorkerOutcome::Unknown
                } else {
                    p::WorkerOutcome::Cancelled
                };
                let unload = unload_unit(&label);
                if unload.is_err() {
                    return cancel_receipt(
                        job,
                        now,
                        p::WorkerOutcome::Unknown,
                        Some("worker-launchd-unload-unknown".into()),
                    );
                }
                let receipt = cancel_receipt(job, now, outcome, None)?;
                self.children
                    .lock()
                    .map_err(|_| p::Error("macOS worker registry is unavailable".into()))?
                    .remove(&job.job_id);
                return Ok(receipt);
            }
            if Instant::now() >= deadline {
                return cancel_receipt(
                    job,
                    now,
                    p::WorkerOutcome::Unknown,
                    Some("worker-launchd-cancel-timeout".into()),
                );
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
            .map_err(|_| p::Error("macOS worker registry is unavailable".into()))?;
        let Some(worker) = children.get(&job.job_id) else {
            return Ok(None);
        };
        let Some(status) = self.observe_unit(&worker.label)? else {
            return Ok(None);
        };
        unload_unit(&worker.label)?;
        if status == 0 {
            worker.overlay.complete(job, now)?;
        }
        let started_at = worker.started_at;
        children.remove(&job.job_id);
        Ok(Some(receipt(
            job,
            started_at,
            now,
            if status == 0 {
                p::WorkerOutcome::Completed
            } else {
                p::WorkerOutcome::Failed
            },
        )?))
    }
}

impl Drop for MacOsLaunchdWorkerIsolation {
    fn drop(&mut self) {
        if let Ok(children) = self.children.lock() {
            for worker in children.values() {
                let _ = run_control(
                    LAUNCHCTL,
                    &[
                        "kill".into(),
                        "SIGKILL".into(),
                        format!("system/{}", worker.label),
                    ],
                    CONTROL_TIMEOUT,
                );
                let _ = unload_unit(&worker.label);
            }
        }
    }
}

fn action_uses_network(execution: &p::WorkerExecutionSpec) -> bool {
    matches!(
        execution.intent.parameters,
        p::ActionParameters::Shell { network: true, .. }
    )
}

fn unit_label(job: &p::WorkerJob) -> p::Result<String> {
    let digest =
        p::canonical_digest(&("forme-macos-worker-label-v1", &job.job_id, &job.plan_digest))?.0;
    Ok(format!(
        "com.forme.worker.{}",
        digest.trim_start_matches("sha256:")
    ))
}

fn require_command(path: &str) -> p::Result<()> {
    if Path::new(path).is_file() {
        Ok(())
    } else {
        Err(p::Error(format!("macOS managed worker requires {path}")))
    }
}

fn require_root() -> p::Result<()> {
    if run_control(ID, &["-u".into()], CONTROL_TIMEOUT)?.trim() == "0" {
        Ok(())
    } else {
        Err(p::Error(
            "macOS managed worker host must run as root".into(),
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
            "macOS managed worker execution account is unavailable".into(),
        ))
    }
}

fn require_managed_host() -> p::Result<()> {
    let output = run_control(
        LAUNCHCTL,
        &["print".into(), "system/com.forme.host".into()],
        CONTROL_TIMEOUT,
    )?;
    let current = std::process::id().to_string();
    if output.lines().any(|line| {
        line.trim()
            .strip_prefix("pid = ")
            .is_some_and(|pid| pid == current)
    }) {
        Ok(())
    } else {
        Err(p::Error(
            "macOS managed worker host is not bound to com.forme.host".into(),
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
        .map_err(|_| p::Error("macOS worker control command is unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| p::Error("macOS worker control output is unavailable".into()))?;
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
                    "macOS worker control command outcome is unknown".into(),
                ));
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| p::Error("macOS worker control output failed".into()))?
        .map_err(|_| p::Error("macOS worker control output failed".into()))?;
    if bytes.len() > MAX_OUTPUT_BYTES {
        return Err(p::Error(
            "macOS worker control output exceeded its bound".into(),
        ));
    }
    if !status.success() {
        return Err(p::Error("macOS worker control command was rejected".into()));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn show_unit(label: &str) -> p::Result<String> {
    let output = run_control(
        LAUNCHCTL,
        &["print".into(), format!("system/{label}")],
        CONTROL_TIMEOUT,
    )?;
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("state = ").map(str::to_owned))
        .ok_or_else(|| p::Error("macOS worker launchd state is incomplete".into()))
}

fn last_exit_code(label: &str) -> p::Result<Option<i32>> {
    let output = run_control(
        LAUNCHCTL,
        &["print".into(), format!("system/{label}")],
        CONTROL_TIMEOUT,
    )?;
    Ok(output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("last exit code = ")
            .and_then(|value| value.parse::<i32>().ok())
    }))
}

fn unload_unit(label: &str) -> p::Result<()> {
    run_control(
        LAUNCHCTL,
        &["bootout".into(), format!("system/{label}")],
        CONTROL_TIMEOUT,
    )
    .map(|_| ())
}

fn receipt(
    job: &p::WorkerJob,
    started_at: p::Timestamp,
    finished_at: p::Timestamp,
    outcome: p::WorkerOutcome,
) -> p::Result<p::WorkerReceipt> {
    let digest = p::canonical_digest(&("macos-worker-receipt", &job.job_id, &job.plan_digest))?.0;
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
    let digest = p::canonical_digest(&("macos-worker-cancel", &job.job_id, &job.plan_digest))?.0;
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
