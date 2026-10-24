//! Real router/listener checks, using controlled readiness, not a native release.
use super::*;
use crate::server::release_health::ReleaseHealth;
use ed25519_dalek::{Signature, Signer, SigningKey};

const CREDENTIAL: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn ready() -> p::ProductReleaseReadiness {
    let hash = |s: &str| p::sha256_content_digest(s.as_bytes());
    let binding = p::ProductReleaseBinding {
        schema_version: p::SchemaVersion(1),
        install_ref: "install:router-fixture".into(),
        operation_id: "upgrade:router-fixture".into(),
        command_digest: hash("command"),
        target_release_ref: "0.0.2".into(),
        target_release_digest: hash("release"),
        core_image_digest: hash("image"),
        owner_digest: hash("owner"),
        store_identity: hash("store"),
        config_revision: 2,
        readiness_nonce: "a".repeat(64),
    };
    p::ProductReleaseReadiness {
        schema_version: p::SchemaVersion(1),
        config_revision: 2,
        actual_image_digest: binding.core_image_digest.clone(),
        data_identity: p::ProductDataAcknowledgement {
            schema_version: p::SchemaVersion(1),
            owner_digest: binding.owner_digest.clone(),
            store_identity: binding.store_identity.clone(),
            execution_fence_digest: None,
            composition_digest: hash("composition"),
        },
        binding,
    }
}

fn signed(key: &SigningKey) -> p::ProductReleaseReadinessProbe {
    let mut request = p::ProductReleaseReadinessProbe {
        schema_version: p::SchemaVersion(1),
        install_ref: ready().binding.install_ref,
        nonce: "b".repeat(64),
        signature: Vec::new(),
    };
    request.signature = key
        .sign(request.signing_digest().unwrap().0.as_bytes())
        .to_bytes()
        .to_vec();
    request
}

async fn send(address: SocketAddr, host: String, headers: &str, body: Vec<u8>) -> String {
    let headers = headers.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        write!(stream, "POST /v1/product/release/readiness HTTP/1.1\r\nHost: {host}\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
        stream.write_all(&body).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    })
    .await
    .unwrap()
}

async fn listener(
    fixture: &Fixture,
    health: Option<ReleaseHealth>,
) -> (
    SocketAddr,
    Arc<HttpState>,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let runtime = fixture.build();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let owner = fixture.gateway_config().owner;
    let state = Arc::new(HttpState {
        gateway: Arc::new(LocalGateway::from_production_runtime(runtime.clone()).unwrap()),
        runtime,
        owner: owner.clone(),
        token: LocalBearerSecret::new(CREDENTIAL.into()).unwrap(),
        origin: format!("http://{address}"),
        // Normal work traffic is deliberately already exhausted.
        rate: Mutex::new(RateWindow {
            started: Instant::now(),
            count: 1,
        }),
        max_requests_per_minute: 1,
        max_body_bytes: 8192,
        admission: Arc::new(WorkAdmission::default()),
        web_bridge: web_bridge::WebBridgeState::open(None, None, None, None, &owner).unwrap(),
        release_health: health,
    });
    state.admission.close().unwrap();
    let app = router(state.clone(), &fixture.gateway_config());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    (address, state, stop, server)
}

#[tokio::test]
async fn live_release_router_enforces_http_boundaries_and_independent_rate_without_brain_effects() {
    let fixture = Fixture::new();
    let token = LocalBearerSecret::new(CREDENTIAL.into()).unwrap();
    let key =
        SigningKey::from_bytes(&p::product_release_probe_seed(CREDENTIAL.as_bytes()).unwrap());
    let health = ReleaseHealth::new(ready(), &token).unwrap();
    let (address, state, stop, server) = listener(&fixture, Some(health)).await;
    let initial_store = fs::read(&fixture.config.store_path).unwrap();
    let probe = signed(&key);
    let body = serde_json::to_vec(&probe).unwrap();
    for (host, headers) in [
        ("foreign.test".into(), String::new()),
        (address.to_string(), format!("Origin: http://{address}\r\n")),
        (
            address.to_string(),
            format!("Authorization: Bearer {CREDENTIAL}\r\n"),
        ),
    ] {
        let response = send(address, host, &headers, body.clone()).await;
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    }
    let mut oversized = vec![b' '; 5000];
    oversized.extend_from_slice(&body);
    let response = send(address, address.to_string(), "", oversized).await;
    assert!(response.starts_with("HTTP/1.1 413"), "{response}");
    let response = send(address, address.to_string(), "", b"{}".to_vec()).await;
    assert!(response.starts_with("HTTP/1.1 422"), "{response}");
    let mut foreign = probe.clone();
    foreign.signature = vec![0; 64];
    let response = send(
        address,
        address.to_string(),
        "",
        serde_json::to_vec(&foreign).unwrap(),
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 401"), "{response}");
    let response = send(address, address.to_string(), "", body.clone()).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let (headers, data) = response.split_once("\r\n\r\n").unwrap();
    assert!(headers
        .to_ascii_lowercase()
        .contains("cache-control: no-store"));
    let proof: p::ProductReleaseReadinessProof = serde_json::from_str(data).unwrap();
    proof.validate_for(&probe).unwrap();
    key.verifying_key()
        .verify_strict(
            proof.signing_digest().unwrap().0.as_bytes(),
            &Signature::from_slice(&proof.signature).unwrap(),
        )
        .unwrap();
    assert_eq!(proof.readiness, ready());
    // One unauthorized challenge plus 29 valid probes consume only this endpoint's budget.
    for _ in 0..28 {
        let response = send(address, address.to_string(), "", body.clone()).await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }
    let response = send(address, address.to_string(), "", body).await;
    assert!(response.starts_with("HTTP/1.1 429"), "{response}");
    assert_eq!(state.rate.lock().unwrap().count, 1);
    assert_eq!(fs::read(&fixture.config.store_path).unwrap(), initial_store);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    stop.send(()).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn ordinary_runtime_has_no_release_readiness_endpoint_authority() {
    let fixture = Fixture::new();
    let (address, _state, stop, server) = listener(&fixture, None).await;
    let initial_store = fs::read(&fixture.config.store_path).unwrap();
    let key =
        SigningKey::from_bytes(&p::product_release_probe_seed(CREDENTIAL.as_bytes()).unwrap());
    let response = send(
        address,
        address.to_string(),
        "",
        serde_json::to_vec(&signed(&key)).unwrap(),
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    assert_eq!(fs::read(&fixture.config.store_path).unwrap(), initial_store);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    stop.send(()).unwrap();
    server.await.unwrap();
}
