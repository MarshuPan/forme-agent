use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use forme_execution::installed::{
    DesktopHelperAdapter, HostCredentials, HostManagement, IsolatedExecutorHost,
    NativeServiceManager, ProductHostBroker, WorkerExecutorHost, WorkerIsolation,
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

#[derive(Clone)]
struct FakeDesktopHelper {
    observations: Arc<AtomicU64>,
    executions: Arc<AtomicU64>,
    lose_after_first_observation: bool,
}

impl FakeDesktopHelper {
    fn live_observation(
        binding: &p::DesktopSessionBinding,
        now: p::Timestamp,
    ) -> p::DesktopSessionObservation {
        p::DesktopSessionObservation {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            helper_ref: binding.helper_ref.clone(),
            grant_ref: Some(binding.grant_ref.clone()),
            state: p::DesktopSessionState::Ready,
            liveness: p::DesktopSessionLiveness::Live,
            os_user_ref: Some(binding.os_user_ref.clone()),
            os_session_ref: Some(binding.os_session_ref.clone()),
            session_generation: Some(binding.session_generation),
            platform: binding.platform,
            desktop_ref: Some(binding.desktop_ref.clone()),
            peer_ref: Some("peer:desktop".into()),
            receipt_ref: None,
            reason_ref: None,
            observed_at: now,
        }
    }
}

impl DesktopHelperAdapter for FakeDesktopHelper {
    fn observe(
        &self,
        binding: &p::DesktopSessionBinding,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopSessionObservation> {
        let call = self.observations.fetch_add(1, Ordering::SeqCst) + 1;
        if self.lose_after_first_observation && call > 1 {
            return Ok(p::DesktopSessionObservation {
                schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
                helper_ref: binding.helper_ref.clone(),
                grant_ref: None,
                state: p::DesktopSessionState::Unavailable,
                liveness: p::DesktopSessionLiveness::Lost,
                os_user_ref: None,
                os_session_ref: None,
                session_generation: None,
                platform: binding.platform,
                desktop_ref: None,
                peer_ref: None,
                receipt_ref: None,
                reason_ref: Some("reason:desktop-session-lost".into()),
                observed_at: now,
            });
        }
        Ok(Self::live_observation(binding, now))
    }

    fn execute(
        &self,
        request: &p::DesktopActionRequest,
        _intent: &p::ActionIntent,
    ) -> p::Result<p::DesktopActionReceipt> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        Ok(p::DesktopActionReceipt {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            job_id: request.job_id.clone(),
            permit_ref: request.permit_ref.clone(),
            action_ref: request.binding.action_ref.clone(),
            plan_digest: request.binding.plan_digest.clone(),
            nonce: request.binding.nonce.clone(),
            normalized_action_digest: request.normalized_action_digest.clone(),
            outcome: p::DesktopActionOutcome::Committed,
            result_ref: Some("result:desktop".into()),
            observed_at: request.deadline - 1,
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

fn desktop_execution(
    job: &mut p::WorkerJob,
    permit: &mut p::WorkerPermit,
) -> p::WorkerExecutionSpec {
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: job.action_ref.clone(),
        source: p::Source::Internal,
        goal: p::GoalRef("goal:desktop".into()),
        backend_hint: p::BackendKind::Computer,
        capability_ref: p::CapabilityRef("computer:desktop".into()),
        action_type: p::ActionType::Execute,
        scope: job.workspace_scope.clone(),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Computer(p::ComputerActionSpec {
            schema_version: p::SchemaVersion(1),
            driver: p::ProviderId("driver:desktop-fixture".into()),
            surface: p::SurfaceRef("surface:desktop".into()),
            bounds: p::CoordinateBounds {
                schema_version: p::SchemaVersion(1),
                min_x: 0,
                min_y: 0,
                max_x_exclusive: 800,
                max_y_exclusive: 600,
            },
            operation: p::ComputerOperation::Click {
                x: 20,
                y: 30,
                button: p::PointerButton::Primary,
            },
            artifact_scope: job.workspace_scope.clone(),
        }),
        requested_permissions: Vec::new(),
        requested_at: 2,
        estimated_output_bytes: 128,
        estimated_duration: p::DurationMs(50),
    };
    let resources = p::affected_resources_for(&intent);
    job.affected_resources = resources.clone();
    permit.affected_resources = resources.clone();
    p::WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: job.plan_digest.clone(),
        intent: intent.clone(),
        backend: p::BackendKind::Computer,
        scope: job.workspace_scope.clone(),
        affected_resources: resources,
        normalized_action_digest: p::normalized_action_digest(&intent).unwrap(),
        timeout: p::DurationMs(100),
        output_budget_bytes: 4 * 1024,
        desktop_session: Some(p::DesktopSessionBinding {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            helper_ref: "helper:desktop".into(),
            grant_ref: "grant:desktop".into(),
            owner_ref: job.owner_ref.clone(),
            os_user_ref: "os-user:desktop".into(),
            os_session_ref: "os-session:desktop".into(),
            session_generation: 4,
            platform: p::DesktopPlatform::Windows,
            desktop_ref: "desktop:primary".into(),
            action_ref: job.action_ref.clone(),
            plan_digest: job.plan_digest.clone(),
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            capabilities: [p::DesktopCapability::PointerClick].into_iter().collect(),
            issued_at: 1,
            expires_at: 900,
        }),
    }
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

#[derive(Clone)]
struct SealLifecycle {
    root: PathBuf,
    reject_cleanup: bool,
    cancel_only: bool,
    cleanups: Arc<AtomicU64>,
}

impl WorkerIsolation for SealLifecycle {
    fn admit(&self, _job: &p::WorkerJob, _permit: &p::WorkerPermit) -> p::Result<()> {
        fs::write(self.root.join("source.bin"), b"retained result").unwrap();
        Ok(())
    }

    fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
        Ok(p::WorkerCancelReceipt {
            schema_version: p::SchemaVersion(1),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            nonce: job.nonce.clone(),
            outcome: p::WorkerOutcome::Cancelled,
            observed_at: 20,
            receipt_ref: Some("receipt:cancel-seal".into()),
            reason_ref: None,
        })
    }

    fn observe(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        if self.cancel_only {
            return Ok(None);
        }
        Ok(Some(p::WorkerReceipt {
            schema_version: p::SchemaVersion(1),
            receipt_ref: "receipt:sealed".into(),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            observed_outcome: p::WorkerOutcome::Completed,
            safe_result_ref: Some("result:sealed".into()),
            ground_truth_refs: vec!["fixture:exit".into()],
            started_at: 10,
            finished_at: Some(now),
        }))
    }

    fn result_path(&self, _job: &p::WorkerJob) -> p::Result<Option<PathBuf>> {
        Ok(Some(self.root.join("source.bin")))
    }

    fn result_sealed(&self, _job: &p::WorkerJob, _now: p::Timestamp) -> p::Result<()> {
        let artifacts = fs::read_dir(self.root.join("worker.results"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        assert!(artifacts
            .iter()
            .any(|path| path.extension().is_some_and(|value| value == "json")));
        assert!(artifacts
            .iter()
            .any(|path| path.extension().is_some_and(|value| value == "bin")));
        self.cleanups.fetch_add(1, Ordering::SeqCst);
        if self.reject_cleanup {
            return Err(p::Error("fixture cleanup failed".into()));
        }
        fs::remove_file(self.root.join("source.bin")).unwrap();
        Ok(())
    }
}

#[test]
fn result_handoff_orders_manifest_cleanup_receipt_and_never_retries_failed_cleanup() {
    for reject_cleanup in [false, true] {
        let temp = Temp::new();
        let (_broker, _maintenance, agent) = auths(&temp);
        let cleanups = Arc::new(AtomicU64::new(0));
        let host = host(
            &temp,
            SealLifecycle {
                root: temp.0.clone(),
                reject_cleanup,
                cancel_only: false,
                cleanups: cleanups.clone(),
            },
        );
        let (job, permit) = make_job(100);
        host.dispatch(&agent, job.clone(), permit.clone(), 10)
            .unwrap();
        let observed = if reject_cleanup {
            host.result_manifest(&agent, &job.job_id, 20)
                .map(|_| unreachable!())
        } else {
            host.probe(&agent, &job.job_id, 20)
        };
        if reject_cleanup {
            assert!(observed.is_err());
            assert_eq!(
                host.probe(&agent, &job.job_id, 21).unwrap().state,
                p::WorkerState::Unknown
            );
            assert_eq!(
                host.dispatch(&agent, job, permit, 22).unwrap().state,
                p::WorkerState::Unknown
            );
            assert!(temp.0.join("source.bin").is_file());
        } else {
            assert_eq!(observed.unwrap().state, p::WorkerState::Completed);
            assert!(!temp.0.join("source.bin").exists());
        }
        assert_eq!(cleanups.load(Ordering::SeqCst), 1);
        drop(host);
        let records = fs::read_to_string(temp.worker_journal()).unwrap();
        assert!(records.contains("\"record\":\"result_manifest\""));
        let receipt = records.find("\"record\":\"receipt\"");
        assert_eq!(receipt.is_none(), reject_cleanup);
        if let Some(receipt) = receipt {
            assert!(records.find("\"record\":\"result_manifest\"").unwrap() < receipt);
        }
    }
}

#[test]
fn cancellation_seals_existing_source_without_fabricating_completion() {
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let host = host(
        &temp,
        SealLifecycle {
            root: temp.0.clone(),
            reject_cleanup: false,
            cancel_only: true,
            cleanups: Arc::new(AtomicU64::new(0)),
        },
    );
    let (job, permit) = make_job(100);
    host.dispatch(&agent, job.clone(), permit, 10).unwrap();
    assert_eq!(
        host.cancel(&agent, &job.job_id, 20).unwrap().outcome,
        p::WorkerOutcome::Cancelled
    );
    assert_eq!(
        host.probe(&agent, &job.job_id, 21).unwrap().state,
        p::WorkerState::Cancelled
    );
    let manifest = host.result_manifest(&agent, &job.job_id, 22).unwrap();
    assert_eq!(manifest.status, p::WorkerResultStatus::Available);
    assert_eq!(manifest.outcome, Some(p::WorkerOutcome::Cancelled));
    assert!(!temp.0.join("source.bin").exists());
}

#[test]
fn cancel_result_cleanup_failure_stays_unknown_without_repeating_cleanup() {
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let cleanups = Arc::new(AtomicU64::new(0));
    let host = host(
        &temp,
        SealLifecycle {
            root: temp.0.clone(),
            reject_cleanup: true,
            cancel_only: true,
            cleanups: cleanups.clone(),
        },
    );
    let (job, permit) = make_job(100);
    host.dispatch(&agent, job.clone(), permit, 10).unwrap();
    assert!(host.cancel(&agent, &job.job_id, 20).is_err());
    assert_eq!(
        host.probe(&agent, &job.job_id, 21).unwrap().state,
        p::WorkerState::Unknown
    );
    assert_eq!(
        host.cancel(&agent, &job.job_id, 22).unwrap().outcome,
        p::WorkerOutcome::Unknown
    );
    assert_eq!(cleanups.load(Ordering::SeqCst), 1);
    assert!(temp.0.join("source.bin").exists());
    drop(host);
    let records = fs::read_to_string(temp.worker_journal()).unwrap();
    assert!(!records.contains("\"record\":\"cancel_receipt\""));
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

#[test]
fn desktop_helper_dispatch_is_exact_and_terminal_receipt_is_idempotent() {
    let temp = Temp::new();
    let (_broker, maintenance, agent) = auths(&temp);
    let helper = FakeDesktopHelper {
        observations: Arc::new(AtomicU64::new(0)),
        executions: Arc::new(AtomicU64::new(0)),
        lose_after_first_observation: false,
    };
    let host = WorkerExecutorHost::open_with_adapters(
        &temp.worker_journal(),
        p::canonical_digest(&AGENT_TOKEN).unwrap(),
        p::AuthorityEpoch(1),
        [p::Scope("workspace:one".into())].into_iter().collect(),
        FakeIsolation::default(),
        helper.clone(),
    )
    .unwrap();
    let (mut job, mut permit) = make_job(1_000);
    let execution = desktop_execution(&mut job, &mut permit);
    let first = host
        .dispatch_with_execution(
            &agent,
            job.clone(),
            permit.clone(),
            Some(execution.clone()),
            10,
        )
        .unwrap();
    assert_eq!(first.state, p::WorkerState::Completed);
    assert_eq!(first.outcome, Some(p::WorkerOutcome::Completed));
    assert_eq!(helper.executions.load(Ordering::SeqCst), 1);
    assert_eq!(helper.observations.load(Ordering::SeqCst), 2);

    let duplicate = host
        .dispatch_with_execution(
            &agent,
            job.clone(),
            permit.clone(),
            Some(execution.clone()),
            11,
        )
        .unwrap();
    assert_eq!(duplicate.receipt_ref, first.receipt_ref);
    assert_eq!(helper.executions.load(Ordering::SeqCst), 1);
    assert_eq!(helper.observations.load(Ordering::SeqCst), 2);

    let mut changed = execution;
    changed.desktop_session.as_mut().unwrap().session_generation = 5;
    assert!(host
        .dispatch_with_execution(&agent, job.clone(), permit.clone(), Some(changed), 11)
        .is_err());
    assert_eq!(helper.executions.load(Ordering::SeqCst), 1);

    assert!(host
        .dispatch_with_execution(&maintenance, job, permit, None, 12)
        .is_err());
}

#[test]
fn desktop_helper_loss_after_start_is_unknown_and_never_retried() {
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let helper = FakeDesktopHelper {
        observations: Arc::new(AtomicU64::new(0)),
        executions: Arc::new(AtomicU64::new(0)),
        lose_after_first_observation: true,
    };
    let host = WorkerExecutorHost::open_with_adapters(
        &temp.worker_journal(),
        p::canonical_digest(&AGENT_TOKEN).unwrap(),
        p::AuthorityEpoch(1),
        [p::Scope("workspace:one".into())].into_iter().collect(),
        FakeIsolation::default(),
        helper.clone(),
    )
    .unwrap();
    let (mut job, mut permit) = make_job(1_000);
    let execution = desktop_execution(&mut job, &mut permit);
    assert!(host
        .dispatch_with_execution(
            &agent,
            job.clone(),
            permit.clone(),
            Some(execution.clone()),
            10
        )
        .is_err());
    let unknown = host.probe(&agent, &job.job_id, 11).unwrap();
    assert_eq!(unknown.state, p::WorkerState::Unknown);
    assert_eq!(unknown.outcome, Some(p::WorkerOutcome::Unknown));
    assert_eq!(helper.executions.load(Ordering::SeqCst), 0);
    assert_eq!(helper.observations.load(Ordering::SeqCst), 2);

    let retry = host
        .dispatch_with_execution(&agent, job, permit, Some(execution), 12)
        .unwrap();
    assert_eq!(retry.state, p::WorkerState::Unknown);
    assert_eq!(helper.executions.load(Ordering::SeqCst), 0);
    assert_eq!(helper.observations.load(Ordering::SeqCst), 2);
}

#[test]
fn desktop_grant_expiry_and_capability_mismatch_are_rejected_before_helper_call() {
    let temp = Temp::new();
    let (_broker, _maintenance, agent) = auths(&temp);
    let helper = FakeDesktopHelper {
        observations: Arc::new(AtomicU64::new(0)),
        executions: Arc::new(AtomicU64::new(0)),
        lose_after_first_observation: false,
    };
    let host = WorkerExecutorHost::open_with_adapters(
        &temp.worker_journal(),
        p::canonical_digest(&AGENT_TOKEN).unwrap(),
        p::AuthorityEpoch(1),
        [p::Scope("workspace:one".into())].into_iter().collect(),
        FakeIsolation::default(),
        helper.clone(),
    )
    .unwrap();

    let (mut job, mut permit) = make_job(1_000);
    let mut expired = desktop_execution(&mut job, &mut permit);
    expired.desktop_session.as_mut().unwrap().expires_at = 9;
    assert!(host
        .dispatch_with_execution(&agent, job.clone(), permit.clone(), Some(expired), 10)
        .is_err());
    assert_eq!(helper.observations.load(Ordering::SeqCst), 0);

    let (mut job, mut permit) = make_job(1_000);
    let mut wrong_capability = desktop_execution(&mut job, &mut permit);
    wrong_capability
        .desktop_session
        .as_mut()
        .unwrap()
        .capabilities = [p::DesktopCapability::Screenshot].into_iter().collect();
    assert!(host
        .dispatch_with_execution(&agent, job, permit, Some(wrong_capability), 10)
        .is_err());
    assert_eq!(helper.observations.load(Ordering::SeqCst), 0);
    assert_eq!(helper.executions.load(Ordering::SeqCst), 0);
}
