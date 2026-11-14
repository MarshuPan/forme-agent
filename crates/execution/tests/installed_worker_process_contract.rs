use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use forme_execution::installed::{
    HostCredentials, IsolatedExecutorHost, ManagedWorkerInputResolver, NativeServiceManager,
    ProcessWorkerIsolation, ProductHostBroker, WorkerExecutorHost, WorkerLaunchRegistry,
    WorkerLaunchSpec,
};
use forme_execution::{InMemoryContentResolver, RejectingSecretResolver};
use forme_protocol as p;

static NEXT: AtomicU64 = AtomicU64::new(0);
const AGENT_TOKEN: &str = "process-agent-token-abcdefghijklmnopqrstuvwxyz";

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

fn input_execution(
    job: &mut p::WorkerJob,
    permit: &mut p::WorkerPermit,
) -> (p::WorkerExecutionSpec, Arc<ManagedWorkerInputResolver>) {
    let content_ref = p::ContentRef("content:worker-input".into());
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action-worker-input".into()),
        source: p::Source::Internal,
        goal: p::GoalRef("worker input delivery".into()),
        backend_hint: p::BackendKind::AppApi,
        capability_ref: p::CapabilityRef("connector:worker-input".into()),
        action_type: p::ActionType::Execute,
        scope: job.workspace_scope.clone(),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::AppApi(p::AppApiActionSpec {
            schema_version: p::SchemaVersion(1),
            connector: p::ProviderId("connector:worker-input".into()),
            endpoint: "http://127.0.0.1/input".into(),
            schema_digest: p::SchemaDigest(
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            ),
            credential: None,
            operation: p::AppApiOperation::Mutation {
                method: p::AppApiMutationMethod::Post,
                body: Some(p::ExternalInput::Content(content_ref.clone())),
                idempotency_key: p::IdempotencyKey("worker-input-once".into()),
            },
            timeout: p::DurationMs(1_000),
            participant: None,
            representation: None,
            disclosure_request: None,
        }),
        requested_permissions: Vec::new(),
        requested_at: 2,
        estimated_output_bytes: 128,
        estimated_duration: p::DurationMs(100),
    };
    job.action_ref = intent.intent_id.clone();
    permit.action_ref = intent.intent_id.clone();
    let resources = p::affected_resources_for(&intent);
    job.affected_resources = resources.clone();
    permit.affected_resources = resources.clone();
    let agent_digest = p::canonical_digest(&AGENT_TOKEN).unwrap();
    let installation_ref = format!(
        "install-auth-{}",
        agent_digest.0.strip_prefix("sha256:").unwrap()
    );
    let mut grant = p::WorkerInputGrantV1 {
        schema_version: p::WORKER_INPUT_SCHEMA_VERSION,
        grant_ref: "worker-input-grant:placeholder".into(),
        installation_ref,
        owner_ref: job.owner_ref.clone(),
        job_id: job.job_id.clone(),
        action_ref: job.action_ref.clone(),
        plan_digest: job.plan_digest.clone(),
        normalized_action_digest: p::normalized_action_digest(&intent).unwrap(),
        permit_digest: p::canonical_digest(permit).unwrap(),
        authority_epoch: job.authority_epoch,
        nonce: job.nonce.clone(),
        workspace_scope: job.workspace_scope.clone(),
        input_path: "app_api.body".into(),
        input_kind: p::WorkerInputKind::Content,
        content_ref: Some(content_ref.clone()),
        content_digest: Some(p::sha256_content_digest(b"managed input")),
        content_bytes: Some(13),
        secret_slot_ref: None,
        issued_at: 2,
        expires_at: 400,
    };
    grant.grant_ref = grant.expected_ref().unwrap();
    let contents = Arc::new(InMemoryContentResolver::default());
    contents
        .insert(content_ref, b"managed input".to_vec())
        .unwrap();
    let resolver = Arc::new(
        ManagedWorkerInputResolver::new(contents, Arc::new(RejectingSecretResolver), []).unwrap(),
    );
    (
        p::WorkerExecutionSpec {
            schema_version: p::SchemaVersion(1),
            plan_digest: job.plan_digest.clone(),
            intent,
            backend: p::BackendKind::AppApi,
            scope: job.workspace_scope.clone(),
            affected_resources: resources,
            normalized_action_digest: grant.normalized_action_digest.clone(),
            timeout: p::DurationMs(100),
            output_budget_bytes: 4096,
            desktop_session: None,
            input_grants: vec![grant],
        },
        resolver,
    )
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
fn result_ciphertext_survives_restart_but_wrong_missing_keys_and_tampering_never_read() {
    let temp = Temp::new();
    let auth = agent_auth(&temp);
    let worker = host(&temp, Vec::new());
    let (job, permit) = job("job-encrypted-result", 500);
    worker.dispatch(&auth, job.clone(), permit, 10).unwrap();
    assert_eq!(
        wait_for_terminal(&worker, &auth, &job.job_id).state,
        p::WorkerState::Completed
    );
    let manifest = worker.result_manifest(&auth, &job.job_id, 201).unwrap();
    let suffix = manifest.manifest_digest.0.strip_prefix("sha256:").unwrap();
    let artifact = temp
        .0
        .join("worker.results")
        .join(format!("data-{suffix}.bin"));
    let key_path = temp
        .0
        .join("worker.result-keys")
        .join(format!("{suffix}.key"));
    let envelope = fs::read(&artifact).unwrap();
    let key = fs::read(&key_path).unwrap();
    assert_eq!(key.len(), 32);
    assert!(envelope.starts_with(b"FRMRES01"));
    assert!(!envelope
        .windows(b"worker fixture result".len())
        .any(|part| part == b"worker fixture result"));
    drop(worker);
    let journal = fs::read(temp.journal()).unwrap();
    assert!(!journal
        .windows(key.len())
        .any(|part| part == key.as_slice()));
    assert!(!journal
        .windows(b"worker fixture result".len())
        .any(|part| part == b"worker fixture result"));
    let read = p::WorkerResultRead {
        schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
        binding: manifest.binding.clone(),
        manifest_digest: manifest.manifest_digest.clone(),
        offset: 0,
        max_bytes: p::WORKER_RESULT_MAX_CHUNK_BYTES,
    };
    let worker = host(&temp, Vec::new());
    assert_eq!(
        worker.read_result(&auth, read.clone(), 202).unwrap().bytes,
        b"worker fixture result\n"
    );
    let mut damaged = envelope.clone();
    *damaged.last_mut().unwrap() ^= 1;
    fs::write(&artifact, damaged).unwrap();
    assert!(worker.read_result(&auth, read.clone(), 203).is_err());
    fs::write(&artifact, &envelope).unwrap();
    let mut wrong_key = key.clone();
    wrong_key[0] ^= 1;
    fs::write(&key_path, wrong_key).unwrap();
    assert!(worker.read_result(&auth, read.clone(), 204).is_err());
    fs::write(&key_path, &key).unwrap();
    assert_eq!(
        worker.read_result(&auth, read.clone(), 205).unwrap().bytes,
        b"worker fixture result\n"
    );
    fs::remove_file(&key_path).unwrap();
    drop(worker);
    let worker = host(&temp, Vec::new());
    assert!(worker.read_result(&auth, read, 206).is_err());
    assert!(!key_path.exists());
    assert_eq!(
        fs::read_to_string(temp.workspace().join("worker-fixture-ran"))
            .unwrap()
            .lines()
            .count(),
        1
    );
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

#[test]
fn process_worker_receives_resolved_content_as_one_shot_stdin_frame() {
    let temp = Temp::new();
    let auth = agent_auth(&temp);
    let mut host = host(&temp, vec!["--read-input".into()]);
    let (mut job, mut permit) = job("job-process-input", 500);
    let (execution, resolver) = input_execution(&mut job, &mut permit);
    host = host.with_input_resolver(resolver);
    host.dispatch_with_execution(&auth, job.clone(), permit, Some(execution), 10)
        .unwrap();
    assert_eq!(
        wait_for_terminal(&host, &auth, &job.job_id).state,
        p::WorkerState::Completed
    );
    let frame = fs::read(temp.workspace().join("worker-input-frame.bin")).unwrap();
    assert!(frame.starts_with(b"FORME-WORKER-INPUT-V1\0"));
    assert!(frame
        .windows(b"managed input".len())
        .any(|part| part == b"managed input"));
}
