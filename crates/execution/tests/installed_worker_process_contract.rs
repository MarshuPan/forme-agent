use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use forme_execution::installed::{
    HostCredentials, IsolatedExecutorHost, NativeServiceManager, ProcessWorkerIsolation,
    ProductHostBroker, WorkerExecutorHost, WorkerLaunchRegistry, WorkerLaunchSpec,
};
use forme_protocol as p;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-worker-process-contract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("workspace")).unwrap();
        Self(root)
    }

    fn workspace(&self) -> PathBuf {
        self.0.join("workspace")
    }

    fn journal(&self) -> PathBuf {
        self.0.join("worker.jsonl")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Clone, Default)]
struct FakeServices;

impl NativeServiceManager for FakeServices {
    fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service,
            state: p::NativeServiceState::Stopped,
        })
    }

    fn apply(&self, change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        Ok(p::NativeReceipt {
            schema_version: p::SchemaVersion(1),
            service: change.service,
            requested: change.action,
            observed: p::NativeServiceState::Stopped,
        })
    }
}

fn agent_auth(temp: &Temp) -> forme_execution::installed::HostAuth {
    let broker = ProductHostBroker::open_with_service_manager(
        &temp.0.join("host.jsonl"),
        "process-install",
        HostCredentials::new(
            "process-maintenance-token-abcdefghijklmnopqrstuvwxyz".into(),
            Some("process-agent-token-abcdefghijklmnopqrstuvwxyz".into()),
        )
        .unwrap(),
        p::ProductTarget::WindowsX86_64,
        "0.1.0".into(),
        FakeServices,
    )
    .unwrap();
    broker
        .authenticate_agent("process-agent-token-abcdefghijklmnopqrstuvwxyz")
        .unwrap()
}

fn job(job_id: &str, deadline: p::Timestamp) -> (p::WorkerJob, p::WorkerPermit) {
    let plan = p::PlanDigest(p::canonical_digest(&("process-plan", job_id)).unwrap().0);
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: job_id.into(),
        owner_ref: p::VerifiedPrincipal("owner:process".into()),
        action_ref: p::ActionId(format!("action-{job_id}")),
        affected_resources: vec![p::ResourceRef("workspace:one".into())],
        execution_plan_ref: format!("plan-{job_id}"),
        plan_digest: plan.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: format!("permit-{job_id}"),
        sandbox_profile_ref: "sandbox:process".into(),
        workspace_scope: p::Scope("workspace:one".into()),
        deadline,
        nonce: p::Nonce(format!("nonce-{job_id}")),
        cancel_ref: format!("cancel-{job_id}"),
    };
    let permit = p::WorkerPermit {
        schema_version: p::SchemaVersion(1),
        permit_ref: job.permit_ref.clone(),
        job_id: job.job_id.clone(),
        owner_ref: job.owner_ref.clone(),
        action_ref: job.action_ref.clone(),
        affected_resources: job.affected_resources.clone(),
        plan_digest: plan,
        authority_epoch: job.authority_epoch,
        workspace_scope: job.workspace_scope.clone(),
        sandbox_profile_ref: job.sandbox_profile_ref.clone(),
        deadline,
        nonce: job.nonce.clone(),
        issued_at: 1,
    };
    (job, permit)
}

fn host(temp: &Temp, args: Vec<String>) -> WorkerExecutorHost<ProcessWorkerIsolation> {
    let launch = WorkerLaunchSpec::new(
        "sandbox:process",
        PathBuf::from(env!("CARGO_BIN_EXE_forme-worker-fixture")),
        args,
    )
    .unwrap();
    let launches = WorkerLaunchRegistry::from_specs([launch]).unwrap();
    let roots = BTreeMap::from([(String::from("workspace:one"), temp.workspace())]);
    let isolation = ProcessWorkerIsolation::open(temp.0.join("runtime"), launches, roots).unwrap();
    WorkerExecutorHost::open_with_isolation(
        &temp.journal(),
        p::canonical_digest(&"process-agent-token-abcdefghijklmnopqrstuvwxyz").unwrap(),
        p::AuthorityEpoch(1),
        [p::Scope("workspace:one".into())].into_iter().collect(),
        isolation,
    )
    .unwrap()
}

fn wait_for_terminal(
    host: &WorkerExecutorHost<ProcessWorkerIsolation>,
    auth: &forme_execution::installed::HostAuth,
    job_id: &str,
) -> p::WorkerObservation {
    for now in 11..200 {
        let observation = host.probe(auth, job_id, now).unwrap();
        if matches!(
            observation.state,
            p::WorkerState::Completed | p::WorkerState::Failed | p::WorkerState::Unknown
        ) {
            return observation;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("worker did not reach a terminal state");
}

#[test]
fn process_worker_uses_registered_program_and_reports_completion() {
    let temp = Temp::new();
    let auth = agent_auth(&temp);
    let host = host(&temp, Vec::new());
    let (job, permit) = job("job-process-success", 500);

    assert_eq!(
        host.dispatch(&auth, job.clone(), permit, 10).unwrap().state,
        p::WorkerState::Running
    );
    let observation = wait_for_terminal(&host, &auth, &job.job_id);
    assert_eq!(observation.state, p::WorkerState::Completed);
    assert_eq!(observation.outcome, Some(p::WorkerOutcome::Completed));
    assert!(observation.safe_result_ref.is_some());
    assert!(temp.workspace().join("worker-fixture-ran").is_file());
    let manifest = host.result_manifest(&auth, &job.job_id, 200).unwrap();
    assert_eq!(manifest.status, p::WorkerResultStatus::Available);
    assert_eq!(manifest.bytes, b"worker fixture result\n".len() as u64);
    let chunk = host
        .read_result(
            &auth,
            p::WorkerResultRead {
                schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
                binding: manifest.binding.clone(),
                manifest_digest: manifest.manifest_digest.clone(),
                offset: 0,
                max_bytes: p::WORKER_RESULT_MAX_CHUNK_BYTES,
            },
            201,
        )
        .unwrap();
    assert_eq!(chunk.bytes, b"worker fixture result\n");
    assert!(chunk.finished);

    let artifact = temp.0.join("worker.results").join(format!(
        "data-{}.bin",
        manifest.manifest_digest.0.strip_prefix("sha256:").unwrap()
    ));
    fs::write(artifact, b"tampered").unwrap();
    assert!(host
        .read_result(
            &auth,
            p::WorkerResultRead {
                schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
                binding: manifest.binding,
                manifest_digest: manifest.manifest_digest,
                offset: 0,
                max_bytes: p::WORKER_RESULT_MAX_CHUNK_BYTES,
            },
            202,
        )
        .is_err());
}

#[test]
fn process_worker_missing_result_is_not_reported_as_available() {
    let temp = Temp::new();
    let auth = agent_auth(&temp);
    let host = host(&temp, vec!["--no-result".into()]);
    let (job, permit) = job("job-process-missing-result", 500);

    host.dispatch(&auth, job.clone(), permit, 10).unwrap();
    let observation = wait_for_terminal(&host, &auth, &job.job_id);
    assert_eq!(observation.state, p::WorkerState::Completed);
    let manifest = host.result_manifest(&auth, &job.job_id, 200).unwrap();
    assert_eq!(manifest.status, p::WorkerResultStatus::Missing);
    assert_eq!(manifest.outcome, Some(p::WorkerOutcome::Completed));
    assert!(host
        .read_result(
            &auth,
            p::WorkerResultRead {
                schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
                binding: manifest.binding,
                manifest_digest: manifest.manifest_digest,
                offset: 0,
                max_bytes: p::WORKER_RESULT_MAX_CHUNK_BYTES,
            },
            201,
        )
        .is_err());
}

#[test]
fn process_worker_preserves_failed_exit_as_failed_receipt() {
    let temp = Temp::new();
    let auth = agent_auth(&temp);
    let host = host(&temp, vec!["--fail".into()]);
    let (job, permit) = job("job-process-failure", 500);

    host.dispatch(&auth, job.clone(), permit, 10).unwrap();
    let observation = wait_for_terminal(&host, &auth, &job.job_id);
    assert_eq!(observation.state, p::WorkerState::Failed);
    assert_eq!(observation.outcome, Some(p::WorkerOutcome::Failed));
}

#[test]
fn process_worker_cancel_kills_running_child_without_retry() {
    let temp = Temp::new();
    let auth = agent_auth(&temp);
    let host = host(&temp, vec!["--sleep-ms=5000".into()]);
    let (job, permit) = job("job-process-cancel", 5000);

    host.dispatch(&auth, job.clone(), permit, 10).unwrap();
    let receipt = host.cancel(&auth, &job.job_id, 11).unwrap();
    assert_eq!(receipt.outcome, p::WorkerOutcome::Cancelled);
    assert_eq!(
        host.probe(&auth, &job.job_id, 12).unwrap().state,
        p::WorkerState::Cancelled
    );
    assert!(host.cancel(&auth, &job.job_id, 13).is_err());
}
