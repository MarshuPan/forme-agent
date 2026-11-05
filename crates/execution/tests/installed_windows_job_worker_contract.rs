#![cfg(windows)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use forme_execution::installed::{
    HostCredentials, IsolatedExecutorHost, NativeServiceManager, ProductHostBroker,
    WindowsJobWorkerIsolation, WorkerExecutorHost, WorkerLaunchRegistry, WorkerLaunchSpec,
};
use forme_protocol as p;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-windows-job-worker-contract-{}-{}",
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
        "windows-job-install",
        HostCredentials::new(
            "windows-job-maintenance-abcdefghijklmnopqrstuvwxyz".into(),
            Some("windows-job-agent-abcdefghijklmnopqrstuvwxyz".into()),
        )
        .unwrap(),
        p::ProductTarget::WindowsX86_64,
        "0.1.0".into(),
        FakeServices,
    )
    .unwrap();
    broker
        .authenticate_agent("windows-job-agent-abcdefghijklmnopqrstuvwxyz")
        .unwrap()
}

fn job(job_id: &str, deadline: p::Timestamp) -> (p::WorkerJob, p::WorkerPermit) {
    let plan = p::PlanDigest(
        p::canonical_digest(&("windows-job-plan", job_id))
            .unwrap()
            .0,
    );
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: job_id.into(),
        owner_ref: p::VerifiedPrincipal("owner:windows-job".into()),
        action_ref: p::ActionId(format!("action-{job_id}")),
        affected_resources: vec![p::ResourceRef("workspace:one".into())],
        execution_plan_ref: format!("plan-{job_id}"),
        plan_digest: plan.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: format!("permit-{job_id}"),
        sandbox_profile_ref: "sandbox:windows-job".into(),
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

fn host(temp: &Temp, args: Vec<String>) -> WorkerExecutorHost<WindowsJobWorkerIsolation> {
    let launch = WorkerLaunchSpec::new(
        "sandbox:windows-job",
        PathBuf::from(env!("CARGO_BIN_EXE_forme-worker-fixture")),
        args,
    )
    .unwrap();
    let launches = WorkerLaunchRegistry::from_specs([launch]).unwrap();
    let roots = BTreeMap::from([(String::from("workspace:one"), temp.workspace())]);
    let isolation =
        WindowsJobWorkerIsolation::open(temp.0.join("runtime"), launches, roots).unwrap();
    WorkerExecutorHost::open_with_isolation(
        &temp.journal(),
        p::canonical_digest(&"windows-job-agent-abcdefghijklmnopqrstuvwxyz").unwrap(),
        p::AuthorityEpoch(1),
        [p::Scope("workspace:one".into())].into_iter().collect(),
        isolation,
    )
    .unwrap()
}

fn wait_for_terminal(
    host: &WorkerExecutorHost<WindowsJobWorkerIsolation>,
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
fn windows_job_worker_reports_real_completion_and_failure() {
    let temp = Temp::new();
    let auth = agent_auth(&temp);
    let success_host = host(&temp, Vec::new());
    let (success_job, permit) = job("job-windows-job-success", 500);

    assert_eq!(
        success_host
            .dispatch(&auth, success_job.clone(), permit, 10)
            .unwrap()
            .state,
        p::WorkerState::Running
    );
    let observation = wait_for_terminal(&success_host, &auth, &success_job.job_id);
    assert_eq!(observation.state, p::WorkerState::Completed);
    assert_eq!(observation.outcome, Some(p::WorkerOutcome::Completed));
    assert!(temp.workspace().join("worker-fixture-ran").is_file());

    drop(success_host);
    let failing = host(&temp, vec!["--fail".into()]);
    let (job, permit) = job("job-windows-job-failure", 500);
    failing.dispatch(&auth, job.clone(), permit, 10).unwrap();
    let observation = wait_for_terminal(&failing, &auth, &job.job_id);
    assert_eq!(observation.state, p::WorkerState::Failed);
    assert_eq!(observation.outcome, Some(p::WorkerOutcome::Failed));
}

#[test]
fn a_caller_receipt_cannot_settle_a_live_native_worker_or_unlock_release_recovery() {
    let temp = Temp::new();
    let auth = agent_auth(&temp);
    let host = host(&temp, vec!["--sleep-ms=10000".into()]);
    let (job, permit) = job("job-unobserved-receipt", 20000);
    host.dispatch(&auth, job.clone(), permit, 10).unwrap();
    for _ in 0..200 {
        if temp.workspace().join("worker-fixture-ran").is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(temp.workspace().join("worker-fixture-ran").is_file());
    let before = fs::metadata(temp.journal()).unwrap().len();
    let claimed = p::WorkerReceipt {
        schema_version: p::SchemaVersion(1),
        receipt_ref: "receipt:unobserved-native-worker".into(),
        job_id: job.job_id.clone(),
        plan_digest: job.plan_digest.clone(),
        authority_epoch: job.authority_epoch,
        nonce: job.nonce.clone(),
        observed_outcome: p::WorkerOutcome::Completed,
        safe_result_ref: None,
        ground_truth_refs: Vec::new(),
        started_at: 10,
        finished_at: Some(11),
    };
    let rejected = host.record_receipt(&auth, claimed, 11).is_err();
    let mut callbacks = 0;
    let blocked = host
        .with_quiescent_workers(&mut |_| {
            callbacks += 1;
            Ok(())
        })
        .is_err();
    assert!(
        rejected,
        "a supplied receipt is not an OS terminal observation"
    );
    assert!(blocked);
    assert_eq!(callbacks, 0);
    assert_eq!(fs::metadata(temp.journal()).unwrap().len(), before);
    assert_eq!(
        host.probe(&auth, &job.job_id, 12).unwrap().state,
        p::WorkerState::Running
    );
    assert_eq!(
        host.cancel(&auth, &job.job_id, 13).unwrap().outcome,
        p::WorkerOutcome::Cancelled
    );
    host.with_quiescent_workers(&mut |_| {
        callbacks += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(callbacks, 1);
}

#[test]
fn windows_job_terminates_a_descendant_without_replaying_the_job() {
    let temp = Temp::new();
    let auth = agent_auth(&temp);
    let host = host(
        &temp,
        vec!["--spawn-descendant".into(), "--sleep-ms=5000".into()],
    );
    let (job, permit) = job("job-windows-job-tree-cancel", 5000);

    host.dispatch(&auth, job.clone(), permit, 10).unwrap();
    let receipt = host.cancel(&auth, &job.job_id, 11).unwrap();
    assert_eq!(receipt.outcome, p::WorkerOutcome::Cancelled);
    assert_eq!(
        host.probe(&auth, &job.job_id, 12).unwrap().state,
        p::WorkerState::Cancelled
    );
    thread::sleep(Duration::from_millis(1000));
    assert!(!temp
        .workspace()
        .join("worker-fixture-descendant-ran")
        .exists());
    assert!(host.cancel(&auth, &job.job_id, 13).is_err());
}
