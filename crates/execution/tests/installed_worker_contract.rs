use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use forme_execution::installed::{
    HostCredentials, HostManagement, IsolatedExecutorHost, NativeServiceManager, ProductHostBroker,
    WorkerExecutorHost, WorkerIsolation,
};
use forme_protocol as p;

const HOST_TOKEN: &str = "worker-contract-host-token-abcdefghijklmnopqrstuvwxyz";
const AGENT_TOKEN: &str = "worker-contract-agent-token-abcdefghijklmnopqrstuvwxyz";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-worker-contract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn host_journal(&self) -> PathBuf {
        self.0.join("host.jsonl")
    }

    fn worker_journal(&self) -> PathBuf {
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

#[derive(Clone, Default)]
struct FakeIsolation {
    admitted: Arc<AtomicU64>,
    cancelled: Arc<AtomicU64>,
}

impl WorkerIsolation for FakeIsolation {
    fn admit(&self, _job: &p::WorkerJob, _permit: &p::WorkerPermit) -> p::Result<()> {
        self.admitted.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
        self.cancelled.fetch_add(1, Ordering::SeqCst);
        Ok(p::WorkerCancelReceipt {
            schema_version: p::SchemaVersion(1),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            nonce: job.nonce.clone(),
            outcome: p::WorkerOutcome::Cancelled,
            observed_at: 20,
            receipt_ref: Some("receipt:cancelled".into()),
            reason_ref: None,
        })
    }
}

fn auths(
    temp: &Temp,
) -> (
    ProductHostBroker<FakeServices>,
    forme_execution::installed::HostAuth,
    forme_execution::installed::HostAuth,
) {
    let broker = ProductHostBroker::open_with_service_manager(
        &temp.host_journal(),
        "worker-install",
        HostCredentials::new(HOST_TOKEN.into(), Some(AGENT_TOKEN.into())).unwrap(),
        p::ProductTarget::WindowsX86_64,
        "0.1.0".into(),
        FakeServices,
    )
    .unwrap();
    let maintenance = broker.authenticate(HOST_TOKEN).unwrap();
    let agent = broker.authenticate_agent(AGENT_TOKEN).unwrap();
    (broker, maintenance, agent)
}

fn make_job(deadline: p::Timestamp) -> (p::WorkerJob, p::WorkerPermit) {
    let plan = p::PlanDigest(p::canonical_digest(&"worker-plan").unwrap().0);
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: "job-1".into(),
        owner_ref: p::VerifiedPrincipal("owner:worker".into()),
        action_ref: p::ActionId("action-1".into()),
        affected_resources: vec![p::ResourceRef("workspace:one".into())],
        execution_plan_ref: "plan-ref-1".into(),
        plan_digest: plan.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: "permit-1".into(),
        sandbox_profile_ref: "sandbox:strict".into(),
        workspace_scope: p::Scope("workspace:one".into()),
        deadline,
        nonce: p::Nonce("nonce-1".into()),
        cancel_ref: "cancel-1".into(),
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

fn host<I: WorkerIsolation>(temp: &Temp, isolation: I) -> WorkerExecutorHost<I> {
    WorkerExecutorHost::open_with_isolation(
        &temp.worker_journal(),
        p::canonical_digest(&AGENT_TOKEN).unwrap(),
        p::AuthorityEpoch(1),
        [p::Scope("workspace:one".into())].into_iter().collect(),
        isolation,
    )
    .unwrap()
}

#[test]
fn worker_requires_agent_domain_and_real_isolation() {
    let temp = Temp::new();
    let (_broker, maintenance, agent) = auths(&temp);
    let default_host = WorkerExecutorHost::open(
        &temp.worker_journal(),
        p::canonical_digest(&AGENT_TOKEN).unwrap(),
        p::AuthorityEpoch(1),
        [p::Scope("workspace:one".into())].into_iter().collect(),
    )
    .unwrap();
    let (job, permit) = make_job(100);
    assert!(default_host
        .dispatch(&agent, job.clone(), permit.clone(), 10)
        .is_err());
    assert!(default_host
        .dispatch(&maintenance, job, permit, 10)
        .is_err());
    assert_eq!(
        default_host.probe(&agent, "job-1", 10).unwrap().state,
        p::WorkerState::Unknown
    );
}

#[test]
fn worker_binds_plan_scope_epoch_deadline_and_nonce_before_side_effect() {
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let isolation = FakeIsolation::default();
    let host = host(&temp, isolation.clone());
    let (job, permit) = make_job(100);
    assert_eq!(
        host.dispatch(&agent, job.clone(), permit.clone(), 10)
            .unwrap()
            .state,
        p::WorkerState::Running
    );
    assert_eq!(isolation.admitted.load(Ordering::SeqCst), 1);

    let mut changed_plan = job.clone();
    changed_plan.plan_digest = p::PlanDigest(p::canonical_digest(&"different-plan").unwrap().0);
    assert!(host
        .dispatch(&agent, changed_plan, permit.clone(), 10)
        .is_err());
    let mut changed_scope = job.clone();
    changed_scope.workspace_scope = p::Scope("workspace:two".into());
    assert!(host
        .dispatch(&agent, changed_scope, permit.clone(), 10)
        .is_err());
    let mut changed_epoch = job.clone();
    changed_epoch.authority_epoch = p::AuthorityEpoch(2);
    assert!(host
        .dispatch(&agent, changed_epoch, permit.clone(), 10)
        .is_err());
    let (expired_job, expired_permit) = make_job(10);
    assert!(host
        .dispatch(&agent, expired_job, expired_permit, 10)
        .is_err());
    assert_eq!(isolation.admitted.load(Ordering::SeqCst), 1);
}

#[test]
fn worker_admission_is_idempotent_and_restart_marks_exact_job_unknown() {
    let temp = Temp::new();
    let (broker, _maintenance, agent) = auths(&temp);
    let isolation = FakeIsolation::default();
    let (job, permit) = make_job(100);
    {
        let host = host(&temp, isolation.clone());
        host.dispatch(&agent, job.clone(), permit.clone(), 10)
            .unwrap();
        assert_eq!(
            host.dispatch(&agent, job.clone(), permit.clone(), 11)
                .unwrap()
                .state,
            p::WorkerState::Running
        );
        assert_eq!(isolation.admitted.load(Ordering::SeqCst), 1);
    }
    drop(broker);

    let (_broker, _maintenance, agent) = auths(&temp);
    let host = host(&temp, isolation.clone());
    let observation = host.probe(&agent, &job.job_id, 20).unwrap();
    assert_eq!(observation.state, p::WorkerState::Unknown);
    assert_eq!(observation.outcome, Some(p::WorkerOutcome::Unknown));
    assert!(observation.reason_ref.is_some());
    assert_eq!(isolation.admitted.load(Ordering::SeqCst), 1);
    assert_eq!(
        host.dispatch(&agent, job, permit, 21).unwrap().state,
        p::WorkerState::Unknown
    );
    assert_eq!(isolation.admitted.load(Ordering::SeqCst), 1);
}

#[test]
fn worker_receipt_and_cancel_are_bound_to_the_original_job() {
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let isolation = FakeIsolation::default();
    let host = host(&temp, isolation.clone());
    let (job, permit) = make_job(100);
    host.dispatch(&agent, job.clone(), permit, 10).unwrap();
    let receipt = p::WorkerReceipt {
        schema_version: p::SchemaVersion(1),
        receipt_ref: "receipt:completed".into(),
        job_id: job.job_id.clone(),
        plan_digest: job.plan_digest.clone(),
        authority_epoch: job.authority_epoch,
        nonce: job.nonce.clone(),
        observed_outcome: p::WorkerOutcome::Completed,
        safe_result_ref: Some("result:completed".into()),
        ground_truth_refs: vec!["ground-truth:1".into()],
        started_at: 10,
        finished_at: Some(20),
    };
    assert_eq!(
        host.record_receipt(&agent, receipt, 20).unwrap().state,
        p::WorkerState::Completed
    );
    assert!(host.cancel(&agent, &job.job_id, 21).is_err());
    assert_eq!(isolation.cancelled.load(Ordering::SeqCst), 0);
}

#[test]
fn repeated_cancel_after_an_unsettled_cancel_never_calls_the_isolator_twice() {
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let isolation = FakeIsolation::default();
    let host = host(&temp, isolation.clone());
    let (job, permit) = make_job(100);
    host.dispatch(&agent, job.clone(), permit, 10).unwrap();
    let first = host.cancel(&agent, &job.job_id, 11).unwrap();
    assert_eq!(first.outcome, p::WorkerOutcome::Cancelled);
    assert_eq!(isolation.cancelled.load(Ordering::SeqCst), 1);

    // The durable receipt is already terminal, so a second cancel cannot
    // invoke the underlying isolator again.
    assert!(host.cancel(&agent, &job.job_id, 12).is_err());
    assert_eq!(isolation.cancelled.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelled_observation_replays_the_exact_cancel_receipt() {
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let isolation = FakeIsolation::default();
    let (job, permit) = make_job(100);
    let receipt;
    {
        let host = host(&temp, isolation.clone());
        host.dispatch(&agent, job.clone(), permit, 10).unwrap();
        receipt = host.cancel(&agent, &job.job_id, 20).unwrap();
        let observation = host.probe(&agent, &job.job_id, 21).unwrap();
        observation.validate().unwrap();
        assert_eq!(observation.state, p::WorkerState::Cancelled);
        assert_eq!(observation.outcome, Some(receipt.outcome));
        assert_eq!(observation.receipt_ref, receipt.receipt_ref);
    }
    let host = host(&temp, isolation.clone());
    let observation = host.probe(&agent, &job.job_id, 22).unwrap();
    observation.validate().unwrap();
    assert_eq!(observation.state, p::WorkerState::Cancelled);
    assert_eq!(observation.receipt_ref, receipt.receipt_ref);
    assert_eq!(isolation.admitted.load(Ordering::SeqCst), 1);
    assert_eq!(isolation.cancelled.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_cancel_and_probe_cannot_duplicate_the_platform_effect() {
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let isolation = FakeIsolation::default();
    let host = host(&temp, isolation.clone());
    let (job, permit) = make_job(100);
    host.dispatch(&agent, job.clone(), permit, 10).unwrap();
    std::thread::scope(|scope| {
        let mut calls = Vec::new();
        for _ in 0..12 {
            calls.push(scope.spawn(|| {
                let _ = host.cancel(&agent, &job.job_id, 20);
                host.probe(&agent, &job.job_id, 21)
                    .unwrap()
                    .validate()
                    .unwrap();
            }));
        }
        for call in calls {
            call.join().unwrap();
        }
    });
    let final_state = host.probe(&agent, &job.job_id, 22).unwrap();
    final_state.validate().unwrap();
    assert_eq!(final_state.state, p::WorkerState::Cancelled);
    assert_eq!(isolation.cancelled.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_waits_for_admission_to_finish_before_touching_the_platform() {
    struct BlockingIsolation {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
        cancelled: std::sync::mpsc::Sender<()>,
    }
    impl WorkerIsolation for BlockingIsolation {
        fn admit(&self, _: &p::WorkerJob, _: &p::WorkerPermit) -> p::Result<()> {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            Ok(())
        }
        fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
            self.cancelled.send(()).unwrap();
            FakeIsolation::default().cancel(job)
        }
    }
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (cancelled_tx, cancelled_rx) = std::sync::mpsc::channel();
    let host = host(
        &temp,
        BlockingIsolation {
            entered: entered_tx,
            release: std::sync::Mutex::new(release_rx),
            cancelled: cancelled_tx,
        },
    );
    let (job, permit) = make_job(100);
    std::thread::scope(|scope| {
        let admission = scope.spawn(|| host.dispatch(&agent, job.clone(), permit, 10).unwrap());
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let (calling_tx, calling_rx) = std::sync::mpsc::channel();
        let host_ref = &host;
        let agent_ref = &agent;
        let job_id = &job.job_id;
        let cancellation = scope.spawn(move || {
            calling_tx.send(()).unwrap();
            host_ref.cancel(agent_ref, job_id, 20).unwrap()
        });
        calling_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let premature = cancelled_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_ok();
        release_tx.send(()).unwrap();
        admission.join().unwrap().validate().unwrap();
        cancellation.join().unwrap().validate_for(&job).unwrap();
        assert!(
            !premature,
            "cancel reached the platform while admission was unfinished"
        );
    });
    host.probe(&agent, &job.job_id, 21)
        .unwrap()
        .validate()
        .unwrap();
}
