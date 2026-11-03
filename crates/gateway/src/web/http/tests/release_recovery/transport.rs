//! Real public auth and internal HTTP transport with controlled Host responses.
use super::*;
use axum::{extract::Path, http::HeaderMap, Json};
use std::sync::atomic::{AtomicBool, Ordering};

struct ControlledHost {
    client: Arc<crate::web::configuration::LocalConfigurationClient>,
    calls: Arc<Mutex<Vec<p::ProductReleaseRecoveryRequest>>>,
    wrong_owner: Arc<AtomicBool>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    _root: tempfile::TempDir,
}
impl ControlledHost {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let credential = root.path().join("configuration-token");
        std::fs::write(&credential, "recovery-configuration-test-token-123456789").unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let wrong_owner = Arc::new(AtomicBool::new(false));
        let response_owner = wrong_owner.clone();
        let (send, receive) = mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let recorded_post = recorded.clone();
                let app = axum::Router::new()
                    .route("/v1/host/configuration/release-recovery", post(move |headers: HeaderMap, Json(request): Json<p::ProductReleaseRecoveryRequest>| {
                        request.validate().unwrap();
                        assert!(headers.contains_key("authorization"));
                        recorded_post.lock().unwrap().push(request.clone());
                        async move { Json(recovery_view(request)) }
                    }))
                    .route("/v1/host/configuration/release-recovery/operations/{id}", get(move |Path(_id): Path<String>| {
                        let mut request = recorded.lock().unwrap().last().unwrap().clone();
                        if response_owner.load(Ordering::SeqCst) { request.owner = p::OwnerPrincipalRef("owner:other".into()); request.candidate.owner = request.owner.clone(); }
                        async move { Json(recovery_view(request)) }
                    }));
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                send.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener, app).with_graceful_shutdown(async move { let _ = stopped.await; }).await.unwrap();
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
            calls,
            wrong_owner,
            stop: Some(stop),
            thread: Some(thread),
            _root: root,
        }
    }
}
impl Drop for ControlledHost {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
    }
}
fn recovery_view(request: p::ProductReleaseRecoveryRequest) -> p::ProductReleaseRecoveryView {
    let view = p::ProductReleaseRecoveryView {
        schema_version: p::SchemaVersion(1),
        request_digest: request.digest().unwrap(),
        revision: request.expected_revision + 1,
        request,
        stage: p::ProductReleaseRecoveryStage::Intent,
        receipt_digest: None,
    };
    view.validate().unwrap();
    view
}

#[test]
fn release_recovery_http_forwards_only_session_owner_and_bound_request_after_password_then_reads_original(
) {
    let host = ControlledHost::new();
    let f = Fixture::with_bridges(true, None, Some(host.client.clone()));
    let (cookie, csrf) = f.login();
    let headers = [
        ("Origin", f.origin()),
        ("Cookie", cookie),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "recover:http".into()),
    ];
    let body = recovery();
    let mut wrong = body.clone();
    wrong["password"] = "wrong-fixture-password".into();
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/product/release-recovery",
            &headers,
            &wrong.to_string()
        )
        .status,
        401
    );
    assert!(host.calls.lock().unwrap().is_empty());
    let response = f.request(
        "POST",
        "/api/v1/product/release-recovery",
        &headers,
        &body.to_string(),
    );
    assert_eq!(response.status, 200);
    let view: p::ProductReleaseRecoveryView = serde_json::from_str(&response.body).unwrap();
    let calls = host.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].owner,
        p::OwnerPrincipalRef("owner:http-fixture".into())
    );
    assert_eq!(calls[0].digest().unwrap(), view.request_digest);
    assert!(!serde_json::to_string(&calls[0]).unwrap().contains(PASSWORD));
    drop(calls);
    assert_eq!(view.stage, p::ProductReleaseRecoveryStage::Intent);
    let read = f.request(
        "GET",
        "/api/v1/product/release-recovery/operations/recover:http",
        &headers,
        "",
    );
    assert_eq!(read.status, 200);
    assert_eq!(host.calls.lock().unwrap().len(), 1);
    host.wrong_owner.store(true, Ordering::SeqCst);
    assert_eq!(
        f.request(
            "GET",
            "/api/v1/product/release-recovery/operations/recover:http",
            &headers,
            ""
        )
        .status,
        401
    );
}
