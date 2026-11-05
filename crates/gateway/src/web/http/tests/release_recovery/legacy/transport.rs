//! Authenticated real sockets with controlled Host facts, not native evidence.
use super::*;
use axum::{
    body::to_bytes,
    extract::{Request, State},
    Json,
};
use std::sync::atomic::{AtomicU8, Ordering};

const INTERNAL: &str = "/v1/host/configuration/legacy-release-recovery";
const CREDENTIAL: &str = "legacy-test-configuration-token-0123456789";
#[derive(Clone)]
struct Control {
    calls: Arc<Mutex<Vec<(String, Value)>>>,
    saved: Arc<Mutex<p::LegacyReleaseRecoveryRequest>>,
    fault: Arc<AtomicU8>,
}
struct ControlledHost {
    client: Arc<crate::web::configuration::LocalConfigurationClient>,
    control: Control,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    _root: tempfile::TempDir,
}
impl ControlledHost {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let credential = root.path().join("configuration.token");
        std::fs::write(&credential, CREDENTIAL).unwrap();
        let mut raw = body();
        raw.as_object_mut().unwrap().remove("password");
        raw["owner"] = "owner:http-fixture".into();
        let control = Control {
            calls: Arc::default(),
            saved: Arc::new(Mutex::new(serde_json::from_value(raw).unwrap())),
            fault: Arc::default(),
        };
        let state = control.clone();
        let (send, receive) = mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let app = axum::Router::new().fallback(reply).with_state(state);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                send.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = stopped.await;
                    })
                    .await
                    .unwrap();
            });
        });
        let client = Arc::new(
            crate::web::configuration::LocalConfigurationClient::connect(
                receive.recv_timeout(Duration::from_secs(5)).unwrap(),
                &credential,
            )
            .unwrap(),
        );
        Self {
            client,
            control,
            stop: Some(stop),
            thread: Some(thread),
            _root: root,
        }
    }
    fn calls(&self) -> Vec<(String, Value)> {
        self.control.calls.lock().unwrap().clone()
    }
}
impl Drop for ControlledHost {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
    }
}
fn report() -> p::LegacyReleaseReview {
    let hash = p::sha256_content_digest(b"controlled-legacy-http-evidence");
    let binding = p::LegacyReleaseReviewBinding {
        schema_version: p::SchemaVersion(1),
        review_id: "review:http".into(),
        install_ref: "install:http".into(),
        owner: p::OwnerPrincipalRef("owner:http-fixture".into()),
        original_operation_id: "upgrade:original".into(),
        original_command_digest: hash.clone(),
        original_release_ref: "0.1.0".into(),
        original_release_digest: hash.clone(),
        config_revision: 1,
        config_digest: hash.clone(),
        journal_seq: 28,
        journal_head: hash.clone(),
        expires_at: now() + 90_000,
    };
    p::LegacyReleaseReview {
        schema_version: p::SchemaVersion(1),
        evidence: p::LegacyReleaseEvidenceKind::ALL
            .into_iter()
            .map(|kind| p::LegacyReleaseEvidence {
                schema_version: p::SchemaVersion(1),
                kind,
                artifact_digest: hash.clone(),
                binding_digest: binding.digest().unwrap(),
                observation_digest: hash.clone(),
            })
            .collect(),
        binding,
        status: p::LegacyReleaseReviewStatus::Verified,
        issues: vec![],
    }
}
fn view(request: p::LegacyReleaseRecoveryRequest, committed: bool) -> p::LegacyReleaseRecoveryView {
    p::LegacyReleaseRecoveryView {
        schema_version: p::SchemaVersion(1),
        request_digest: request.digest().unwrap(),
        revision: request.expected_revision + if committed { 2 } else { 1 },
        review_digest: request.review_digest.clone(),
        request,
        stage: if committed {
            p::LegacyReleaseRecoveryStage::Committed
        } else {
            p::LegacyReleaseRecoveryStage::Intent
        },
        receipt_digest: committed.then(|| p::sha256_content_digest(b"controlled-receipt")),
    }
}
async fn reply(State(control): State<Control>, request: Request) -> Json<Value> {
    assert_eq!(
        request.headers().get("authorization").unwrap(),
        &format!("Bearer {CREDENTIAL}")
    );
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    let bytes = to_bytes(request.into_body(), 16_384).await.unwrap();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    assert!(body.get("password").is_none());
    control
        .calls
        .lock()
        .unwrap()
        .push((format!("{method} {path}"), body.clone()));
    let fault = control.fault.load(Ordering::SeqCst);
    let mut response = if method == "POST" && path == INTERNAL {
        let request: p::LegacyReleaseRecoveryRequest = serde_json::from_value(body).unwrap();
        request.validate().unwrap();
        *control.saved.lock().unwrap() = request.clone();
        serde_json::to_value(view(request, false)).unwrap()
    } else if method == "POST" && path == format!("{INTERNAL}/verify") {
        let verify: p::LegacyReleaseRecoveryVerification = serde_json::from_value(body).unwrap();
        verify.validate().unwrap();
        let original = control.saved.lock().unwrap().clone();
        assert_eq!(verify.owner, original.owner);
        assert_eq!(verify.request_digest, original.digest().unwrap());
        serde_json::to_value(view(original, true)).unwrap()
    } else if path == format!("{INTERNAL}/review/upgrade:original") {
        let mut report = report();
        if fault == 1 {
            report.binding.owner = p::OwnerPrincipalRef("owner:foreign".into());
            let digest = report.binding.digest().unwrap();
            for item in &mut report.evidence {
                item.binding_digest = digest.clone();
            }
        }
        serde_json::to_value(report).unwrap()
    } else {
        let mut original = control.saved.lock().unwrap().clone();
        if fault == 1 {
            original.owner = p::OwnerPrincipalRef("owner:foreign".into());
        }
        if fault == 3 {
            original.operation_id = "legacy:foreign".into();
        }
        let view = view(original, false);
        if path == format!("{INTERNAL}/operations") {
            serde_json::to_value(p::LegacyReleaseRecoveryList {
                schema_version: p::SchemaVersion(1),
                product_revision: view.revision,
                owner: view.request.owner.clone(),
                operations: vec![view],
            })
            .unwrap()
        } else {
            serde_json::to_value(view).unwrap()
        }
    };
    if fault == 2 {
        response["request_digest"] = p::sha256_content_digest(b"mutated").0.into();
    }
    if fault == 4 {
        response["evidence_path"] = "forged-path".into();
    }
    if fault == 5 {
        let mut changed: p::LegacyReleaseRecoveryView = serde_json::from_value(response).unwrap();
        changed.request.requested_deadline += 1;
        changed.request_digest = changed.request.digest().unwrap();
        response = serde_json::to_value(changed).unwrap();
    }
    Json(response)
}

#[test]
fn legacy_http_transports_only_bound_owner_commands_and_keeps_refresh_read_only() {
    let host = ControlledHost::new();
    let f = Fixture::with_bridges(true, None, Some(host.client.clone()));
    let headers = headers(&f);
    let mut body = body();
    body["review_digest"] = report().digest().unwrap().0.into();
    let mut wrong = body.clone();
    wrong["password"] = "wrong-password".into();
    assert_eq!(
        f.request("POST", PREFIX, &headers, &wrong.to_string())
            .status,
        401
    );
    assert!(host.calls().is_empty());
    let read = f.request(
        "GET",
        &format!("{PREFIX}/review/upgrade:original"),
        &headers,
        "",
    );
    assert_eq!(read.status, 200);
    serde_json::from_str::<p::LegacyReleaseReview>(&read.body)
        .unwrap()
        .validate()
        .unwrap();
    let response = f.request("POST", PREFIX, &headers, &body.to_string());
    assert_eq!(response.status, 200);
    let intent: p::LegacyReleaseRecoveryView = serde_json::from_str(&response.body).unwrap();
    assert_eq!(intent.request.owner.0, "owner:http-fixture");
    assert!(!response.body.contains(PASSWORD));
    for suffix in [
        "/operations",
        "/operations/legacy:http",
        "/review/upgrade:original",
    ] {
        assert_eq!(
            f.request("GET", &format!("{PREFIX}{suffix}"), &headers, "")
                .status,
            200
        );
    }
    assert_eq!(
        host.calls()
            .iter()
            .filter(|(line, _)| line.starts_with("POST "))
            .count(),
        1
    );
    let mut verify = verification();
    verify["request_digest"] = intent.request_digest.0.into();
    wrong = verify.clone();
    wrong["password"] = "wrong-password".into();
    assert_eq!(
        f.request(
            "POST",
            &format!("{PREFIX}/verify"),
            &headers,
            &wrong.to_string()
        )
        .status,
        401
    );
    assert_eq!(host.calls().len(), 5);
    let done = f.request(
        "POST",
        &format!("{PREFIX}/verify"),
        &headers,
        &verify.to_string(),
    );
    assert_eq!(done.status, 200);
    let done: p::LegacyReleaseRecoveryView = serde_json::from_str(&done.body).unwrap();
    assert_eq!(done.stage, p::LegacyReleaseRecoveryStage::Committed);
    assert_eq!(host.calls().len(), 6);
    assert_eq!(host.calls()[5].0, format!("POST {INTERNAL}/verify"));
    assert!(host
        .calls()
        .iter()
        .all(|(_, body)| !body.to_string().contains(PASSWORD)));
}

#[test]
fn legacy_http_rejects_cross_owner_rebound_or_unclosed_host_responses() {
    let host = ControlledHost::new();
    let f = Fixture::with_bridges(true, None, Some(host.client.clone()));
    let headers = headers(&f);
    host.control.fault.store(1, Ordering::SeqCst);
    for suffix in [
        "/review/upgrade:original",
        "/operations/legacy:http",
        "/operations",
    ] {
        assert_eq!(
            f.request("GET", &format!("{PREFIX}{suffix}"), &headers, "")
                .status,
            401
        );
    }
    for fault in [2, 3, 4] {
        host.control.fault.store(fault, Ordering::SeqCst);
        let reply = f.request(
            "GET",
            &format!("{PREFIX}/operations/legacy:http"),
            &headers,
            "",
        );
        assert_ne!(reply.status, 200, "fault {fault}");
    }
    for fault in [2, 4, 5] {
        host.control.fault.store(fault, Ordering::SeqCst);
        assert_eq!(
            f.request("POST", PREFIX, &headers, &body().to_string())
                .status,
            if fault == 4 { 502 } else { 503 },
            "fault {fault}"
        );
    }
}
