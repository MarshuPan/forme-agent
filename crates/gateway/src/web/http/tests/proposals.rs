//! Controlled-core HTTP evidence; the real signed bridge is tested separately.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct ControlledCore {
    client: Arc<crate::web::core::CoreBridgeClient>,
    mode: Arc<AtomicUsize>,
    calls: Arc<Mutex<Vec<p::WorkbenchProposalCommand>>>,
    root: PathBuf,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ControlledCore {
    fn new() -> Self {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).unwrap();
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "forme-proposal-core-{:x}",
            u128::from_le_bytes(bytes)
        ));
        std::fs::create_dir(&root).unwrap();
        let credential_path = root.join("service-token");
        let signing_key_path = root.join("assertion-key");
        std::fs::write(
            &credential_path,
            "fixture-proposal-service-token-1234567890",
        )
        .unwrap();
        std::fs::write(&signing_key_path, [113; 32]).unwrap();
        let mode = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let response_mode = mode.clone();
        let recorded_calls = calls.clone();
        let (send, receive) = mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
                .block_on(async move {
                    let app = axum::Router::new().route("/v1/brain/workbench/proposal", post(
                        move |headers: axum::http::HeaderMap, axum::Json(command): axum::Json<p::WorkbenchProposalCommand>| {
                            let mode = response_mode.clone();
                            let calls = recorded_calls.clone();
                            async move {
                                assert!(headers.contains_key("x-forme-core-assertion"));
                                calls.lock().unwrap().push(command.clone());
                                let mode = mode.load(Ordering::SeqCst);
                                if mode == 3 {
                                    return (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"error":"fixture offline"})));
                                }
                                let mut binding = p::canonical_digest(&(
                                    p::VerifiedPrincipal("owner:http-fixture".into()), &command
                                )).unwrap();
                                if mode == 1 { binding = p::sha256_content_digest(b"wrong fixture binding"); }
                                let status = if matches!(mode, 1 | 2) { "Confirmed" } else { "Unknown" };
                                (axum::http::StatusCode::OK, axum::Json(json!({
                                    "schema_version":1,"command":command.command,"binding":binding,
                                    "status":status,"event":"event:fixture-proposal-receipt"
                                })))
                            }
                        }
                    ));
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    send.send(listener.local_addr().unwrap()).unwrap();
                    axum::serve(listener, app).with_graceful_shutdown(async move { let _ = stopped.await; }).await.unwrap();
                });
        });
        let config = crate::web::config::WebCoreBridgeConfig {
            schema_version: p::SchemaVersion(1),
            address: receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            credential_path,
            signing_key_path,
        };
        let client = Arc::new(
            crate::web::core::CoreBridgeClient::connect(&config, "installation:http-fixture")
                .unwrap(),
        );
        Self {
            client,
            mode,
            calls,
            root,
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}

impl Drop for ControlledCore {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
        assert_eq!(
            self.root.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(self
            .root
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("forme-proposal-core-"));
        assert_eq!(self.root.canonicalize().unwrap(), self.root);
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn web_unknown_proposal_preserves_exact_command_and_only_explicit_verification_forwards() {
    let core = ControlledCore::new();
    let fixture = Fixture::with_core(true, Some(core.client.clone()));
    let (cookie, csrf) = fixture.login();
    let pending = || {
        fixture.request(
            "GET",
            "/api/v1/workbench/proposals/pending",
            &[("Cookie", cookie.clone())],
            "",
        )
    };
    let command = json!({"schema_version":1,"command":"proposal:original","run":"run:original",
        "proposal":"proposal:original","outcome":"Adopt","defer_until":null});
    let submit = |body: &Value| {
        fixture.request(
            "POST",
            "/api/v1/brain/workbench/proposal",
            &[
                ("Origin", fixture.origin()),
                ("Cookie", cookie.clone()),
                ("X-Forme-Csrf", csrf.clone()),
                ("Idempotency-Key", body["command"].as_str().unwrap().into()),
            ],
            &body.to_string(),
        )
    };
    assert_eq!(pending().json()["commands"], json!([]));
    assert!(core.calls.lock().unwrap().is_empty());
    let first = submit(&command);
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["status"], "Unknown");
    for _ in 0..2 {
        let recovered = pending();
        assert_eq!(recovered.status, 200, "{}", recovered.body);
        assert_eq!(recovered.json()["commands"], json!([command]));
    }
    assert_eq!(core.calls.lock().unwrap().len(), 1);

    let mut replaced = command.clone();
    replaced["command"] = json!("proposal:new-key");
    assert_eq!(submit(&replaced).status, 409);
    let mut rebound = command.clone();
    rebound["outcome"] = json!("Reject");
    assert_eq!(submit(&rebound).status, 409);
    assert_eq!(core.calls.lock().unwrap().len(), 1);

    core.mode.store(3, Ordering::SeqCst);
    assert_eq!(submit(&command).status, 503);
    assert_eq!(pending().json()["commands"], json!([command]));
    core.mode.store(1, Ordering::SeqCst);
    assert_eq!(submit(&command).status, 502);
    assert_eq!(pending().json()["commands"], json!([command]));
    core.mode.store(0, Ordering::SeqCst);
    assert_eq!(submit(&command).json()["status"], "Unknown");
    core.mode.store(2, Ordering::SeqCst);
    assert_eq!(submit(&command).json()["status"], "Confirmed");
    assert_eq!(pending().json()["commands"], json!([]));
    let calls = core.calls.lock().unwrap();
    assert_eq!(calls.len(), 5);
    for call in calls.iter() {
        assert_eq!(serde_json::to_value(call).unwrap(), command);
    }
}
