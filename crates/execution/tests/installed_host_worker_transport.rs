//! Real hostd AgentExecution transport tests with a managed worker config.
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forme_execution::installed::{
    WorkerBrokerClient, WorkerBrokerTransport, WorkerLaunchProfileConfig, WorkerRuntimeConfig,
};
use forme_protocol as p;
use forme_protocol::WorkerExecutionSpec;

const MAINTENANCE_TOKEN: &str = "repository-owned-maintenance-token-0123456789";
const AGENT_TOKEN: &str = "repository-owned-agent-token-0123456789012345";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Daemon {
    child: Child,
    root: PathBuf,
    address: String,
    agent_address: String,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Daemon {
    fn start(args: Vec<String>) -> Self {
        Self::start_mode(args, true)
    }

    fn start_mode(args: Vec<String>, development: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-host-worker-http-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("workspace")).unwrap();
        std::fs::create_dir_all(root.join("runtime")).unwrap();
        let config = WorkerRuntimeConfig {
            schema_version: p::SchemaVersion(1),
            authority_epoch: p::AuthorityEpoch(1),
            journal_path: root.join("worker.jsonl"),
            runtime_root: root.join("runtime"),
            allowed_scopes: vec![p::Scope("workspace:one".into())],
            workspace_roots: [("workspace:one".into(), root.join("workspace"))]
                .into_iter()
                .collect(),
            launch_profiles: vec![WorkerLaunchProfileConfig {
                schema_version: p::SchemaVersion(1),
                sandbox_profile_ref: "sandbox:process".into(),
                program: PathBuf::from(env!("CARGO_BIN_EXE_forme-worker-fixture")),
                args,
            }],
        };
        let config_path = root.join("worker-config.json");
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let target = if cfg!(windows) {
            "windows_x86_64"
        } else if cfg!(target_os = "macos") {
            "macos_arm64"
        } else {
            "ubuntu_2404_x86_64"
        };
        let mut command = Command::new(env!("CARGO_BIN_EXE_forme-hostd"));
        if development {
            command.arg("--development-process-workers");
        }
        let mut child = command
            .env_remove("FORME_HOST_STATE_ROOT")
            .env_remove("FORME_HOST_TOKEN_PATH")
            .env_remove("FORME_HOST_MAINTENANCE_TOKEN_PATH")
            .env_remove("FORME_HOST_AGENT_TOKEN_PATH")
            .env("FORME_HOST_BIND", "127.0.0.1:0")
            .env("FORME_AGENT_EXECUTION_BIND", "127.0.0.1:0")
            .env("FORME_HOST_JOURNAL_PATH", root.join("host.jsonl"))
            .env("FORME_INSTALL_ID", "worker-http-install")
            .env("FORME_ACTIVE_RELEASE", "0.1.0")
            .env("FORME_PRODUCT_TARGET", target)
            .env("FORME_HOST_TOKEN", MAINTENANCE_TOKEN)
            .env("FORME_AGENT_EXECUTION_TOKEN", AGENT_TOKEN)
            .env("FORME_WORKER_CONFIG_PATH", &config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            let mut lines = Vec::new();
            for _ in 0..2 {
                let mut line = String::new();
                let _ = output.read_line(&mut line);
                lines.push(line);
            }
            let _ = send.send(lines);
        });
        let mut daemon = Self {
            child,
            root,
            address: String::new(),
            agent_address: String::new(),
        };
        let lines = receive
            .recv_timeout(Duration::from_secs(15))
            .expect("worker host did not start");
        reader.join().unwrap();
        daemon.address = lines[0]
            .trim()
            .strip_prefix("FORME_HOST_URL=http://")
            .expect("worker host did not announce its endpoint")
            .to_owned();
        daemon.agent_address = lines[1]
            .trim()
            .strip_prefix("FORME_AGENT_EXECUTION_URL=http://")
            .expect("worker host did not announce its execution endpoint")
            .to_owned();
        assert_ne!(daemon.address, daemon.agent_address);
        daemon
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: &[u8],
    ) -> (u16, serde_json::Value) {
        self.request_at(&self.agent_address, method, path, token, body)
    }

    fn request_at(
        &self,
        address: &str,
        method: &str,
        path: &str,
        token: &str,
        body: &[u8],
    ) -> (u16, serde_json::Value) {
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            address,
            body.len()
        )
        .unwrap();
        stream.write_all(body).unwrap();
        let mut bytes = String::new();
        stream.read_to_string(&mut bytes).unwrap();
        let (head, body) = bytes.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        assert!(head.contains("Cache-Control: no-store"));
        (status, serde_json::from_str(body).unwrap())
    }
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn job(job_id: &str, deadline: p::Timestamp) -> p::WorkerDispatchRequest {
    let plan = p::PlanDigest(
        p::canonical_digest(&("host-worker-plan", job_id))
            .unwrap()
            .0,
    );
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: job_id.into(),
        owner_ref: p::VerifiedPrincipal("owner:worker-http".into()),
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
        issued_at: now_ms() - 1,
    };
    p::WorkerDispatchRequest {
        schema_version: p::SchemaVersion(1),
        job,
        permit,
        execution: None,
    }
}

fn observe_until_terminal(daemon: &Daemon, job_id: &str) -> p::WorkerObservation {
    for _ in 0..100 {
        let (status, body) = daemon.request(
            "GET",
            &format!("/v1/agent/workers/{job_id}"),
            AGENT_TOKEN,
            &[],
        );
        assert_eq!(status, 200);
        let observation: p::WorkerObservation = serde_json::from_value(body).unwrap();
        observation.validate().unwrap();
        if matches!(
            observation.state,
            p::WorkerState::Completed | p::WorkerState::Failed | p::WorkerState::Cancelled
        ) {
            return observation;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("worker did not reach a terminal state");
}

fn typed_execution_request(job_id: &str, deadline: p::Timestamp) -> p::WorkerDispatchRequest {
    let mut request = job(job_id, deadline);
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: request.job.action_ref.clone(),
        source: p::Source::Internal,
        goal: p::GoalRef(format!("goal-{job_id}")),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("shell:worker-fixture".into()),
        action_type: p::ActionType::Execute,
        scope: request.job.workspace_scope.clone(),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: "worker-fixture".into(),
            args: Vec::new(),
            cwd: None,
            network: false,
        },
        requested_permissions: Vec::new(),
        requested_at: now_ms(),
        estimated_output_bytes: 1_024,
        estimated_duration: p::DurationMs(1_000),
    };
    let resources = p::affected_resources_for(&intent);
    request.job.affected_resources = resources.clone();
    request.permit.affected_resources = resources.clone();
    request.execution = Some(WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: request.job.plan_digest.clone(),
        intent: intent.clone(),
        backend: p::BackendKind::Shell,
        scope: request.job.workspace_scope.clone(),
        affected_resources: resources,
        normalized_action_digest: p::normalized_action_digest(&intent).unwrap(),
        timeout: p::DurationMs(5_000),
        output_budget_bytes: 4 * 1024,
        desktop_session: None,
    });
    request
}

#[test]
fn hostd_dispatches_registered_worker_and_keeps_maintenance_domain_out() {
    let daemon = Daemon::start(Vec::new());
    let request = job("job-http-success", now_ms() + 30_000);
    let body = serde_json::to_vec(&request).unwrap();
    let (status, response) =
        daemon.request("POST", "/v1/agent/workers/dispatch", AGENT_TOKEN, &body);
    assert_eq!(status, 200);
    let admitted: p::WorkerObservation = serde_json::from_value(response).unwrap();
    assert_eq!(admitted.state, p::WorkerState::Running);
    admitted.validate().unwrap();

    let (status, _) = daemon.request(
        "POST",
        "/v1/agent/workers/dispatch",
        MAINTENANCE_TOKEN,
        &body,
    );
    assert_eq!(status, 401);
    // Credentials do not grant access on the other domain's listener.
    assert_eq!(
        daemon
            .request_at(
                &daemon.address,
                "POST",
                "/v1/agent/workers/dispatch",
                AGENT_TOKEN,
                &body
            )
            .0,
        401
    );
    assert_eq!(
        daemon
            .request_at(
                &daemon.agent_address,
                "GET",
                "/v1/host/status",
                MAINTENANCE_TOKEN,
                &[]
            )
            .0,
        401
    );
    assert_eq!(
        daemon
            .request_at(
                &daemon.address,
                "GET",
                "/v1/host/status",
                MAINTENANCE_TOKEN,
                &[]
            )
            .0,
        200
    );

    let observation = observe_until_terminal(&daemon, &request.job.job_id);
    assert_eq!(observation.state, p::WorkerState::Completed);
    assert_eq!(observation.outcome, Some(p::WorkerOutcome::Completed));
    assert!(daemon.root.join("workspace/worker-fixture-ran").is_file());
}

#[test]
fn worker_broker_client_dispatches_typed_spec_and_reuses_original_job() {
    let daemon = Daemon::start(Vec::new());
    let credential_path = daemon.root.join("agent.token");
    std::fs::write(&credential_path, format!("{AGENT_TOKEN}\n")).unwrap();
    let client =
        WorkerBrokerClient::connect(daemon.agent_address.parse().unwrap(), &credential_path)
            .unwrap();
    let request = typed_execution_request("job-client-typed", now_ms() + 30_000);
    let expected_execution = request.execution.clone().unwrap();
    let admitted = client.dispatch(request.clone()).unwrap();
    assert_eq!(admitted.state, p::WorkerState::Running);
    let run_digest =
        p::canonical_digest(&("worker-run", &request.job.job_id, &request.job.plan_digest))
            .unwrap()
            .0;
    let execution_path = daemon
        .root
        .join("runtime")
        .join(format!("job-{}", run_digest.trim_start_matches("sha256:")))
        .join("execution.json");
    for _ in 0..100 {
        if execution_path.is_file() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let encoded = std::fs::read(&execution_path).unwrap();
    let stored: WorkerExecutionSpec = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(stored, expected_execution);

    let mut changed = request.clone();
    changed.execution.as_mut().unwrap().timeout = p::DurationMs(4_000);
    assert_eq!(
        client.dispatch(changed).unwrap_err().0,
        "agent execution request was rejected before admission"
    );

    let duplicate = client.dispatch(request).unwrap();
    assert_eq!(duplicate.job_id, admitted.job_id);
    let observation = loop {
        let observation = client.probe(&duplicate.job_id).unwrap();
        if matches!(
            observation.state,
            p::WorkerState::Completed | p::WorkerState::Failed | p::WorkerState::Cancelled
        ) {
            break observation;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(observation.state, p::WorkerState::Completed);
    assert_eq!(observation.outcome, Some(p::WorkerOutcome::Completed));
}

#[test]
fn worker_broker_client_cannot_use_maintenance_listener() {
    let daemon = Daemon::start_mode(Vec::new(), false);
    let credential_path = daemon.root.join("agent.token");
    std::fs::write(&credential_path, format!("{AGENT_TOKEN}\n")).unwrap();
    let client =
        WorkerBrokerClient::connect(daemon.address.parse().unwrap(), &credential_path).unwrap();
    let error = client.probe("job-not-on-maintenance").unwrap_err();
    assert_eq!(error.0, "agent execution authorization was rejected");
}

#[test]
fn hostd_cancels_the_original_worker_job_without_a_second_dispatch() {
    let daemon = Daemon::start(vec!["--sleep-ms=5000".into()]);
    let request = job("job-http-cancel", now_ms() + 30_000);
    let body = serde_json::to_vec(&request).unwrap();
    let (status, _) = daemon.request("POST", "/v1/agent/workers/dispatch", AGENT_TOKEN, &body);
    assert_eq!(status, 200);
    let (status, response) = daemon.request(
        "POST",
        &format!("/v1/agent/workers/{}/cancel", request.job.job_id),
        AGENT_TOKEN,
        &[],
    );
    assert_eq!(status, 200);
    let receipt: p::WorkerCancelReceipt = serde_json::from_value(response).unwrap();
    assert_eq!(receipt.outcome, p::WorkerOutcome::Cancelled);
    receipt.validate_for(&request.job).unwrap();

    let (status, response) = daemon.request(
        "GET",
        &format!("/v1/agent/workers/{}", request.job.job_id),
        AGENT_TOKEN,
        &[],
    );
    assert_eq!(status, 200);
    let observation: p::WorkerObservation = serde_json::from_value(response).unwrap();
    assert_eq!(observation.state, p::WorkerState::Cancelled);
    observation.validate().unwrap();
    assert_eq!(observation.receipt_ref, receipt.receipt_ref);

    let (status, _) = daemon.request(
        "POST",
        &format!("/v1/agent/workers/{}/cancel", request.job.job_id),
        AGENT_TOKEN,
        &[],
    );
    assert_eq!(status, 400);
}

#[test]
fn hostd_default_config_cannot_enable_an_unsandboxed_worker() {
    let daemon = Daemon::start_mode(Vec::new(), false);
    let request = job("job-service-blocked", now_ms() + 30_000);
    let body = serde_json::to_vec(&request).unwrap();
    assert_eq!(
        daemon
            .request("POST", "/v1/agent/workers/dispatch", AGENT_TOKEN, &body)
            .0,
        400
    );
    assert!(!daemon.root.join("workspace/worker-fixture-ran").exists());
    let (status, body) = daemon.request(
        "GET",
        "/v1/agent/workers/job-service-blocked",
        AGENT_TOKEN,
        &[],
    );
    assert_eq!(status, 200);
    let observation: p::WorkerObservation = serde_json::from_value(body).unwrap();
    observation.validate().unwrap();
    assert_eq!(observation.state, p::WorkerState::Unknown);
    // Maintenance stays available while the execution capability is blocked.
    assert_eq!(
        daemon
            .request_at(
                &daemon.address,
                "GET",
                "/v1/host/status",
                MAINTENANCE_TOKEN,
                &[]
            )
            .0,
        200
    );
}

#[test]
fn hostd_accepts_delayed_fragmented_worker_requests_without_redispatch() {
    let daemon = Daemon::start(Vec::new());
    let request = job("job-http-fragmented", now_ms() + 30_000);
    let body = serde_json::to_vec(&request).unwrap();
    let mut stream = std::net::TcpStream::connect(&daemon.agent_address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    // A blocking handler must wait for data after accept, including when the
    // Windows socket inherited the listener's nonblocking mode.
    std::thread::sleep(Duration::from_millis(100));
    stream
        .write_all(b"POST /v1/agent/workers/dispatch HTTP/1.1\r\n")
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    write!(
        stream,
        "Host: {}\r\nAuthorization: Bearer {AGENT_TOKEN}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        daemon.agent_address,
        body.len(),
    )
    .unwrap();
    let middle = body.len() / 2;
    stream.write_all(&body[..middle]).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    stream.write_all(&body[middle..]).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
    let observation: p::WorkerObservation = serde_json::from_str(body).unwrap();
    observation.validate().unwrap();
    assert_eq!(
        observe_until_terminal(&daemon, &request.job.job_id).state,
        p::WorkerState::Completed
    );
    let marker = std::fs::read_to_string(daemon.root.join("workspace/worker-fixture-ran")).unwrap();
    assert_eq!(marker.lines().count(), 1);
}

#[test]
fn concurrent_http_dispatches_run_the_original_worker_once() {
    let daemon = Daemon::start(vec!["--sleep-ms=100".into()]);
    let request = job("job-http-concurrent", now_ms() + 30_000);
    let body = serde_json::to_vec(&request).unwrap();
    std::thread::scope(|scope| {
        let mut calls = Vec::new();
        for _ in 0..8 {
            calls.push(scope.spawn(|| {
                let (status, body) =
                    daemon.request("POST", "/v1/agent/workers/dispatch", AGENT_TOKEN, &body);
                assert_eq!(status, 200);
                let observation: p::WorkerObservation = serde_json::from_value(body).unwrap();
                observation.validate().unwrap();
            }));
        }
        for call in calls {
            call.join().unwrap();
        }
    });
    assert_eq!(
        observe_until_terminal(&daemon, &request.job.job_id).state,
        p::WorkerState::Completed
    );
    let marker = std::fs::read_to_string(daemon.root.join("workspace/worker-fixture-ran")).unwrap();
    assert_eq!(marker.lines().count(), 1);
}

#[test]
fn invalid_worker_requests_and_routes_have_no_admission_or_side_effect() {
    let daemon = Daemon::start(Vec::new());
    let request = job("job-http-rejected", now_ms() + 30_000);
    let journal = daemon.root.join("worker.jsonl");
    let baseline = std::fs::metadata(&journal).unwrap().len();
    let mut unknown = serde_json::to_value(&request).unwrap();
    unknown["program"] = "untrusted.exe".into();
    let mut wrong_schema = request.clone();
    wrong_schema.schema_version = p::SchemaVersion(2);
    let mut wrong_plan = request.clone();
    wrong_plan.permit.plan_digest = p::PlanDigest(p::canonical_digest(&"changed-plan").unwrap().0);
    let mut expired = request.clone();
    expired.job.deadline = now_ms() - 1;
    expired.permit.deadline = expired.job.deadline;
    for body in [
        serde_json::to_vec(&unknown).unwrap(),
        serde_json::to_vec(&wrong_schema).unwrap(),
        serde_json::to_vec(&wrong_plan).unwrap(),
        serde_json::to_vec(&expired).unwrap(),
    ] {
        assert_eq!(
            daemon
                .request("POST", "/v1/agent/workers/dispatch", AGENT_TOKEN, &body)
                .0,
            400
        );
    }
    for path in [
        "/v1/agent/workers/a/b",
        "/v1/agent/workers/",
        "/v1/agent/workers/a%2fb",
    ] {
        assert_eq!(daemon.request("GET", path, AGENT_TOKEN, &[]).0, 400);
    }
    assert_eq!(std::fs::metadata(&journal).unwrap().len(), baseline);
    assert!(!daemon.root.join("workspace/worker-fixture-ran").exists());
}
