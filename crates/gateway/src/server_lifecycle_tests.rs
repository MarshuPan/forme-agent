use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use forme_harness::{
    CognitiveRuntimeConfig, DataProtectionConfig, ProductionRuntimeConfig, SchedulerGatewayControl,
};

const TOKEN: &str = "lifecycle-test-token-0123456789abcdef0123456789abcdef";
const KEY_ENV: &str = "FORME_GATEWAY_LIFECYCLE_FIXTURE_KEY";
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[path = "server_lifecycle_tests/installed_restore.rs"]
mod installed_restore;
#[path = "server_lifecycle_tests/web_workbench.rs"]
mod web_workbench;

struct Fixture {
    _lock: std::sync::MutexGuard<'static, ()>,
    old_key: Option<std::ffi::OsString>,
    config: ProductionRuntimeConfig,
    calls: Arc<AtomicUsize>,
    write_target: Arc<Mutex<Option<PathBuf>>>,
    release: mpsc::Sender<()>,
    stop: Arc<AtomicBool>,
    model: Option<std::thread::JoinHandle<()>>,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old_key = std::env::var_os(KEY_ENV);
        std::env::set_var(KEY_ENV, "local-lifecycle-fixture-key");
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).unwrap();
        let root = std::env::temp_dir().join(format!(
            "forme-core-stop-{:x}",
            u128::from_le_bytes(entropy)
        ));
        std::fs::create_dir(&root).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let write_target = Arc::new(Mutex::new(None::<PathBuf>));
        let stop = Arc::new(AtomicBool::new(false));
        let (release, receive) = mpsc::channel();
        let count = calls.clone();
        let target = write_target.clone();
        let flag = stop.clone();
        let model = std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("fixture listener: {error}"),
                };
                // On Windows, a stream accepted from a non-blocking listener can
                // retain the non-blocking mode. Restore blocking I/O before the
                // fixture's request/response exchange.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let request = read_request(&mut stream);
                count.fetch_add(1, Ordering::SeqCst);
                receive.recv_timeout(Duration::from_secs(5)).unwrap();
                let body = if let Some(path) = target.lock().unwrap().as_ref() {
                    let offset = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
                    let request: serde_json::Value =
                        serde_json::from_slice(&request[offset + 4..]).unwrap();
                    let tool = request["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|tool| {
                            tool["function"]["parameters"]["properties"]["File"].is_object()
                        })
                        .expect("file tool must be in the production catalog");
                    let parameters = p::ActionParameters::File {
                        operation: p::FileOperation::Write,
                        path: path.to_str().unwrap().into(),
                        content: Some(b"requires-owner-approval".to_vec()),
                    };
                    serde_json::json!({"choices":[{"message":{"tool_calls":[{
                        "id":"lifecycle-write", "type":"function", "function":{
                            "name":tool["function"]["name"],
                            "arguments":serde_json::to_string(&parameters).unwrap()
                        }
                    }]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}).to_string()
                } else {
                    r#"{"choices":[{"message":{"content":"drained-result"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#.into()
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let config = ProductionRuntimeConfig {
            schema_version: p::SchemaVersion(1),
            owner: p::OwnerPrincipalRef("owner:lifecycle".into()),
            store_path: root.join("brain.sqlite3"),
            workspace: p::WorkspaceRef("workspace:lifecycle".into()),
            agent_profile: p::AgentProfileRef("agent:lifecycle".into()),
            models: vec![serde_json::from_value(serde_json::json!({
                "id": "model:lifecycle", "provider": "lifecycle-fixture",
                "model": "lifecycle-fixture", "base_url": format!("http://{address}/v1"),
                "credential_env": KEY_ENV, "context_window": 32768,
                "default": true, "timeout_ms": 5000
            }))
            .unwrap()],
            capabilities: Default::default(),
            scheduler_enabled: false,
            scheduler: p::SchedulerConfig {
                schema_version: p::SchemaVersion(1),
                tick: p::DurationMs(1000),
                lease: p::DurationMs(30_000),
                max_claims_per_tick: 1,
            },
            cognition: CognitiveRuntimeConfig {
                tick_enabled: false,
                ..Default::default()
            },
            data: DataProtectionConfig {
                schema_version: p::SchemaVersion(1),
                artifact_root: root.join("artifacts"),
            },
            authority_lease: p::DurationMs(30_000),
            context_limits: Default::default(),
        };
        Self {
            _lock: lock,
            old_key,
            config,
            calls,
            write_target,
            release,
            stop,
            model: Some(model),
            root,
        }
    }

    fn build(&self) -> Arc<ProductionRuntime> {
        Arc::new(
            ProductionRuntimeBuilder::from_config(self.config.clone())
                .unwrap()
                .build()
                .unwrap(),
        )
    }

    fn gateway_config(&self) -> GatewayServerConfig {
        GatewayServerConfig {
            schema_version: p::SchemaVersion(1),
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 0,
            owner: p::VerifiedPrincipal(self.config.owner.0.clone()),
            agent_profile: self.config.agent_profile.clone(),
            max_body_bytes: 8192,
            request_timeout: Duration::from_secs(3),
            max_requests_per_minute: 100,
            scheduler_enabled: self.config.scheduler_enabled,
            scheduler: self.config.scheduler.clone(),
            install_ref: None,
            web_bridge_token: None,
            web_assertion_verifying_key: None,
            web_session_fence_path: None,
            token: LocalBearerSecret::new(TOKEN.into()).unwrap(),
            token_path: None,
        }
    }

    async fn wait_for_model(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.calls.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.release.send(());
        if let Some(thread) = self.model.take() {
            thread
                .join()
                .expect("model fixture thread must exit without panicking");
        }
        match &self.old_key {
            Some(value) => std::env::set_var(KEY_ENV, value),
            None => std::env::remove_var(KEY_ENV),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let size = stream.read(&mut buffer).unwrap();
        assert_ne!(size, 0);
        bytes.extend_from_slice(&buffer[..size]);
        assert!(bytes.len() < 1_048_576);
        if let Some(offset) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let length = String::from_utf8_lossy(&bytes[..offset])
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap();
            if bytes.len() >= offset + 4 + length {
                return bytes;
            }
        }
    }
}

async fn submit(address: SocketAddr) -> p::RunId {
    tokio::task::spawn_blocking(move || {
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1), source: p::Source::UserTurn,
            session: p::SessionRef("session:lifecycle".into()), agent_profile: p::AgentProfileRef("agent:lifecycle".into()),
            input: p::RunInput("Return the fixture result".into()), budget: None,
            idempotency_key: Some(p::IdempotencyKey("lifecycle:run".into())),
        };
        let body = serde_json::to_string(&request).unwrap();
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        write!(stream, "POST /v1/runs HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nOrigin: http://{address}\r\nx-forme-csrf: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 202"), "{response}");
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }).await.unwrap()
}

fn start(
    fixture: &Fixture,
    runtime: Arc<ProductionRuntime>,
    timeout: Duration,
) -> (
    tokio::task::JoinHandle<p::Result<()>>,
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<SocketAddr>,
) {
    let config = fixture.gateway_config();
    let (stop, receive) = tokio::sync::oneshot::channel();
    let (ready, address) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve_runtime_until(
        config,
        runtime,
        GatewayLifecycleConfig {
            schema_version: p::SchemaVersion(1),
            drain_timeout: timeout,
        },
        move |address| {
            ready.send(address).unwrap();
            Ok(())
        },
        async {
            receive
                .await
                .map_err(|_| p::Error("fixture control disconnected".into()))
        },
    ));
    (server, stop, address)
}

fn events(runtime: &ProductionRuntime, run: &p::RunId) -> Vec<p::Event> {
    GatewayControl::stream_event_page(
        runtime.harness().as_ref(),
        p::EventCursor {
            schema_version: p::SchemaVersion(1),
            run: run.clone(),
            after_stream_seq: 0,
        },
    )
    .unwrap()
    .events
}

#[tokio::test]
async fn controlled_shutdown_waits_for_the_real_model_run_and_releases_the_listener() {
    let fixture = Fixture::new();
    let runtime = fixture.build();
    let (mut server, stop, ready) = start(&fixture, runtime.clone(), Duration::from_secs(3));
    let address = ready.await.unwrap();
    let run = submit(address).await;
    fixture.wait_for_model().await;
    stop.send(()).unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut server)
        .await
        .is_err());
    assert!(TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err());
    fixture.release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err());
    let before = events(&runtime, &run);
    assert_eq!(
        before
            .iter()
            .filter(|e| e.kind == p::EventKind::RunComplete)
            .count(),
        1
    );
    assert!(!before.iter().any(|e| e.kind == p::EventKind::RunAborted));
    drop(runtime);
    let reopened = fixture.build();
    assert_eq!(before, events(&reopened, &run));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn shutdown_timeout_preserves_inflight_facts_without_fabricating_a_terminal() {
    let fixture = Fixture::new();
    let runtime = fixture.build();
    let (server, stop, ready) = start(&fixture, runtime.clone(), Duration::from_millis(25));
    let run = submit(ready.await.unwrap()).await;
    fixture.wait_for_model().await;
    let before_stop = events(&runtime, &run);
    stop.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    assert!(result.unwrap_err().0.contains("deadline"));
    let saved = events(&runtime, &run);
    assert_eq!(saved, before_stop);
    assert!(saved.iter().any(|e| e.kind == p::EventKind::RunAccepted));
    assert!(!saved.iter().any(|e| matches!(
        e.kind,
        p::EventKind::RunComplete | p::EventKind::RunAborted | p::EventKind::RunFailed
    )));
    fixture.release.send(()).unwrap();
    let harness = runtime.harness();
    let wait_run = run.clone();
    tokio::task::spawn_blocking(move || harness.wait(wait_run))
        .await
        .unwrap()
        .unwrap();
    assert!(events(&runtime, &run)
        .iter()
        .any(|e| e.kind == p::EventKind::ModelCallStarted));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelled_request_keeps_its_blocking_admission_until_the_operation_finishes() {
    let fixture = Fixture::new();
    let runtime = fixture.build();
    let state = Arc::new(HttpState {
        gateway: Arc::new(LocalGateway::from_production_runtime(runtime.clone()).unwrap()),
        runtime,
        owner: fixture.gateway_config().owner,
        token: LocalBearerSecret::new(TOKEN.into()).unwrap(),
        origin: "http://127.0.0.1".into(),
        rate: Mutex::new(RateWindow::new()),
        max_requests_per_minute: 100,
        max_body_bytes: 8192,
        admission: Arc::new(WorkAdmission::default()),
        web_bridge: super::web_bridge::WebBridgeState::open(
            None,
            None,
            None,
            None,
            &p::VerifiedPrincipal("owner:test".into()),
        )
        .unwrap(),
    });
    let (release, wait) = mpsc::channel();
    let (entered, entry) = tokio::sync::oneshot::channel();
    let worker_state = state.clone();
    let worker = tokio::spawn(async move {
        worker_state
            .blocking(move || {
                entered.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(3)).unwrap();
                Ok(())
            })
            .await
    });
    entry.await.unwrap();
    worker.abort();
    let _ = worker.await;
    state.admission.close().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    assert_eq!(
        state
            .blocking(move || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap_err()
            .status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), state.admission.idle())
            .await
            .is_err()
    );
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), state.admission.idle())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn stop_during_startup_never_announces_ready_or_claims_background_work() {
    let fixture = Fixture::new();
    let runtime = fixture.build();
    let announced = Arc::new(AtomicBool::new(false));
    let marker = announced.clone();
    serve_runtime_until(
        fixture.gateway_config(),
        runtime.clone(),
        GatewayLifecycleConfig::default(),
        move |_| {
            marker.store(true, Ordering::SeqCst);
            Ok(())
        },
        std::future::ready(Ok(())),
    )
    .await
    .unwrap();
    assert!(!announced.load(Ordering::SeqCst));
    assert!(GatewayControl::list_runs(runtime.harness().as_ref())
        .unwrap()
        .is_empty());
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn shutdown_preserves_the_exact_pending_approval_across_restart_without_an_effect() {
    let mut fixture = Fixture::new();
    let workspace = fixture.root.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let target = workspace.join("approved-only.txt");
    fixture.config.capabilities.file.enabled = true;
    fixture.config.capabilities.file.roots = vec![workspace.to_str().unwrap().into()];
    fixture.config.models[0].tool_use = true;
    fixture.config.models[0].tool_schema_versions = vec![1];
    *fixture.write_target.lock().unwrap() = Some(target.clone());
    let runtime = fixture.build();
    let (server, stop, ready) = start(&fixture, runtime.clone(), Duration::from_secs(3));
    let run = submit(ready.await.unwrap()).await;
    fixture.wait_for_model().await;
    fixture.release.send(()).unwrap();
    let harness = runtime.harness();
    let wait_run = run.clone();
    let result = tokio::task::spawn_blocking(move || harness.wait(wait_run))
        .await
        .unwrap();
    assert!(result.unwrap_err().0.contains("suspended"));
    let summary = GatewayControl::run_summary(runtime.harness().as_ref(), run.clone()).unwrap();
    assert_eq!(summary.status, p::RunStatus::Waiting);
    let session = summary.session;
    let before = events(&runtime, &run);
    let approvals = runtime
        .harness()
        .pending_approvals(session.clone())
        .unwrap();
    assert_eq!(approvals.len(), 1);
    assert!(before
        .iter()
        .any(|e| e.kind == p::EventKind::ApprovalRequested));
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(events(&runtime, &run), before);
    assert!(!target.exists());
    drop(runtime);
    let reopened = fixture.build();
    assert_eq!(events(&reopened, &run), before);
    assert_eq!(
        GatewayControl::run_summary(reopened.harness().as_ref(), run.clone())
            .unwrap()
            .status,
        p::RunStatus::Waiting
    );
    assert_eq!(
        reopened
            .harness()
            .pending_approvals(session.clone())
            .unwrap(),
        approvals
    );
    let projected =
        GatewayControl::pending_approvals(reopened.harness().as_ref(), session.clone()).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].approval_id, approvals[0].approval_id);
    assert_eq!(
        events(&reopened, &run),
        before,
        "reading pending work appends no new facts"
    );
    assert!(!target.exists());
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    let original = &approvals[0];
    let decision = p::ApprovalDecision {
        schema_version: p::SchemaVersion(1),
        approval_id: original.approval_id.clone(),
        outcome: p::ApprovalOutcome::Granted,
        approver: fixture.gateway_config().owner,
        bound_plan_digest: original.plan_digest.clone(),
        normalized_action_digest: original.normalized_action_digest.clone(),
        affected_resources: original.affected_resources.clone(),
        policy_version: original.policy_version,
        tool_schema_version: original.tool_schema_version,
        nonce: p::Nonce("lifecycle-one-shot".into()),
        use_by: original.expires_at,
    };
    let mut stale = decision.clone();
    stale.bound_plan_digest = p::PlanDigest("sha256:changed-plan".into());
    assert!(GatewayControl::control(
        reopened.harness().as_ref(),
        run.clone(),
        p::RunControl::ResolveApproval(stale)
    )
    .is_err());
    assert!(!target.exists());
    assert_eq!(events(&reopened, &run), before);
    *fixture.write_target.lock().unwrap() = None;
    fixture.release.send(()).unwrap();
    let harness = reopened.harness();
    let resume_run = run.clone();
    let approval = decision.clone();
    tokio::task::spawn_blocking(move || {
        GatewayControl::control(
            harness.as_ref(),
            resume_run,
            p::RunControl::ResolveApproval(approval),
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"requires-owner-approval");
    assert_eq!(
        events(&reopened, &run)
            .iter()
            .filter(|e| e.kind == p::EventKind::ActionStarted)
            .count(),
        1
    );
    assert!(
        GatewayControl::pending_approvals(reopened.harness().as_ref(), session.clone())
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 2);
    let completed = events(&reopened, &run);
    drop(reopened);
    let last = fixture.build();
    assert!(GatewayControl::control(
        last.harness().as_ref(),
        run.clone(),
        p::RunControl::ResolveApproval(decision)
    )
    .is_err());
    assert_eq!(events(&last, &run), completed);
    assert!(last
        .harness()
        .pending_approvals(session)
        .unwrap()
        .is_empty());
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 2);
}

fn reminder(id: &str, due: p::Timestamp) -> p::ScheduleCommand {
    p::ScheduleCommand {
        schema_version: p::SchemaVersion(1),
        intention: p::ProspectiveIntention {
            schema_version: p::SchemaVersion(1),
            id: p::IntentionId(id.into()),
            source: p::IntentionSource::Commitment,
            trigger: p::IntentionTrigger::At(due),
            state: p::IntentionState::Pending,
            seed: p::SeedRef(format!("lifecycle reminder {id}")),
            provenance: p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            expires_at: Some(due + 60_000),
        },
        session: p::SessionId(format!("session:{id}")),
        envelope: p::AutonomyEnvelope {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("workspace:lifecycle".into()),
            capability: p::CapabilitySet {
                schema_version: p::SchemaVersion(1),
                capabilities: vec![p::CapabilityRef("capability:local-notification".into())],
                permissions: vec![p::PermissionRef("permission:local-notification".into())],
            },
            action_type: vec![p::ActionType::Deliver],
            risk_limit: p::Risk::Low,
            approval_rule: p::ApprovalRule::Allow,
            budget: p::Budget("units:1".into()),
            timebox: p::Timebox {
                schema_version: p::SchemaVersion(1),
                starts_at: due,
                expires_at: due + 60_000,
                max_turns: 2,
            },
            rollback: p::RollbackReq {
                schema_version: p::SchemaVersion(1),
                required: false,
                boundary: None,
            },
        },
        budget: p::Budget("units:1".into()),
    }
}

#[tokio::test]
async fn shutdown_stops_background_claims_and_leaves_later_intentions_pending() {
    let mut fixture = Fixture::new();
    fixture.config.scheduler_enabled = true;
    fixture.config.scheduler.tick = p::DurationMs(10);
    fixture.config.capabilities.notification_enabled = true;
    let runtime = fixture.build();
    let harness = runtime.harness();
    let due = now_ms() - 1;
    let first =
        SchedulerGatewayControl::schedule(harness.as_ref(), reminder("a:first", due)).unwrap();
    let later = now_ms() + 1000;
    let remaining =
        SchedulerGatewayControl::schedule(harness.as_ref(), reminder("b:next", later)).unwrap();
    drop(harness);
    let (server, stop, ready) = start(&fixture, runtime.clone(), Duration::from_secs(3));
    let _address = ready.await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if runtime.harness().list_jobs().unwrap().iter().any(|job| {
                job.intention.id == first && job.intention.state == p::IntentionState::Done
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    while now_ms() <= later + 30 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let jobs = runtime.harness().list_jobs().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!(
        jobs.iter()
            .find(|job| job.intention.id == remaining)
            .unwrap()
            .intention
            .state,
        p::IntentionState::Pending
    );
    let runs = GatewayControl::list_runs(runtime.harness().as_ref()).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    assert!(events(&runtime, &runs[0].run)
        .iter()
        .any(|e| e.kind == p::EventKind::RunComplete));
}
