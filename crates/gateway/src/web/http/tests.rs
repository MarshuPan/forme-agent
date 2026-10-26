//! Real loopback HTTP fixtures. No installed-service or browser acceptance claim.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc;

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};

use super::*;
use crate::web::identity::{SensitivePassword, WebIdentityStore};

mod backup_verification;
mod proposals;

const PASSWORD: &str = "web HTTP fixture password 15";

struct Fixture {
    address: SocketAddr,
    root: PathBuf,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

impl Fixture {
    fn new(claimed: bool) -> Self {
        Self::with_core(claimed, None)
    }

    fn with_core(claimed: bool, core: Option<Arc<crate::web::core::CoreBridgeClient>>) -> Self {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).unwrap();
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("forme-web-http-{:x}", u128::from_le_bytes(bytes)));
        std::fs::create_dir(&root).unwrap();
        let key = SigningKey::from_bytes(&[127; 32]);
        let identity =
            WebIdentityStore::open(&root, "installation:http-fixture", key.verifying_key())
                .unwrap();
        if claimed {
            let commitment = identity
                .prepare_claim(
                    "claim:http-fixture",
                    "fixture",
                    SensitivePassword::new(PASSWORD.into()),
                )
                .unwrap();
            let receipt = p::WebClaimCommit {
                schema_version: p::SchemaVersion(1),
                install_ref: "installation:http-fixture".into(),
                operation_id: "claim:http-fixture".into(),
                candidate_ref: "data:verified-http-fixture".into(),
                owner: p::OwnerPrincipalRef("owner:http-fixture".into()),
                account_commitment: commitment,
                expires_at: now() + 60_000,
            };
            let digest = p::canonical_digest(&("forme-web-claim-commit-v1", &receipt)).unwrap();
            identity
                .commit_claim(&p::SignedWebClaimCommit {
                    schema_version: p::SchemaVersion(1),
                    receipt,
                    signature: key.sign(digest.0.as_bytes()).to_bytes().to_vec(),
                })
                .unwrap();
        }
        let (send, receive) = mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                serve_identity_with_setup_until(
                    connections::BoundedListener::new(listener),
                    Arc::new(identity),
                    None,
                    None,
                    core,
                    |address| {
                        send.send(address).unwrap();
                        Ok(())
                    },
                    async move {
                        stopped.await.unwrap();
                        Ok(())
                    },
                )
                .await
                .unwrap();
            });
        });
        Self {
            address: receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            root,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    fn origin(&self) -> String {
        format!("http://{}", self.address)
    }

    fn request(&self, method: &str, path: &str, headers: &[(&str, String)], body: &str) -> Reply {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\n", body.len());
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("host"))
        {
            request.push_str(&format!("Host: {}\r\n", self.address));
        }
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(body);
        stream.write_all(request.as_bytes()).unwrap();
        let mut result = String::new();
        stream.read_to_string(&mut result).unwrap();
        let (headers, body) = result.split_once("\r\n\r\n").unwrap();
        let mut lines = headers.lines();
        let status = lines
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        let headers = lines
            .map(|line| {
                let (k, v) = line.split_once(':').unwrap();
                (k.to_ascii_lowercase(), v.trim().to_owned())
            })
            .collect();
        Reply {
            status,
            headers,
            body: body.into(),
        }
    }

    fn login(&self) -> (String, String) {
        self.login_password(PASSWORD)
    }

    fn login_password(&self, password: &str) -> (String, String) {
        let challenge = self.request("GET", "/api/v1/auth/login-challenge", &[], "");
        assert_eq!(challenge.status, 200);
        let view = challenge.json();
        let login_cookie = challenge.cookie(security::LOGIN_COOKIE);
        let reply = self.request(
            "POST",
            "/api/v1/auth/login",
            &[
                ("Origin", self.origin()),
                ("Cookie", login_cookie),
                ("X-Forme-Csrf", view["csrf"].as_str().unwrap().into()),
                (
                    "Idempotency-Key",
                    view["attempt_ref"].as_str().unwrap().into(),
                ),
            ],
            &json!({"schema_version":1,"login_name":"fixture","password":password}).to_string(),
        );
        assert_eq!(reply.status, 200);
        let session = reply.cookie(security::SESSION_COOKIE);
        let csrf = reply.json()["csrf"].as_str().unwrap().into();
        (session, csrf)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        self.thread.take().unwrap().join().unwrap();
        assert_eq!(
            self.root.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(self
            .root
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("forme-web-http-"));
        assert_eq!(self.root.canonicalize().unwrap(), self.root);
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}
impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }
    fn header(&self, name: &str) -> &str {
        &self.headers.iter().find(|(key, _)| key == name).unwrap().1
    }
    fn cookie(&self, name: &str) -> String {
        self.headers
            .iter()
            .filter(|(key, _)| key == "set-cookie")
            .map(|(_, value)| value)
            .find(|value| value.starts_with(&format!("{name}=")))
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into()
    }
}

#[test]
fn web_unclaimed_ingress_is_reachable_without_a_brain_and_never_exposes_owner() {
    let fixture = Fixture::new(false);
    let reply = fixture.request("GET", "/api/v1/bootstrap/status", &[], "");
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.json(),
        json!({"schema_version":1,"claimed":false,"setup_needed":true,"protocol_version":1})
    );
    assert_eq!(reply.header("cache-control"), "no-store");
    assert_eq!(reply.header("x-content-type-options"), "nosniff");
    assert!(reply
        .header("content-security-policy")
        .contains("frame-ancestors 'none'"));
    assert_eq!(
        fixture
            .request("GET", "/api/v1/auth/session", &[], "")
            .status,
        401
    );
    assert_eq!(fixture.request("GET", "/v1/runs", &[], "").status, 404);
    assert_eq!(
        fixture
            .request(
                "POST",
                "/api/v1/bootstrap/claim",
                &[("Origin", fixture.origin())],
                "{}"
            )
            .status,
        503
    );
    assert!(!fixture.root.join("brain.sqlite").exists());
}

#[test]
fn web_login_logout_and_reauth_revoke_enforce_live_sessions() {
    let fixture = Fixture::new(true);
    let (cookie, csrf) = fixture.login();
    let view = fixture.request(
        "GET",
        "/api/v1/auth/session",
        &[("Cookie", cookie.clone())],
        "",
    );
    assert_eq!(view.status, 200);
    assert_eq!(view.json()["session"]["owner"], "owner:http-fixture");
    for secret in [
        "password_hash",
        "credential_revision",
        "claim_verifying_key",
        PASSWORD,
    ] {
        assert!(!view.body.contains(secret));
    }
    let headers = [
        ("Origin", fixture.origin()),
        ("Cookie", cookie.clone()),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "logout:fixture".into()),
    ];
    let reply = fixture.request("POST", "/api/v1/auth/logout", &headers, "");
    assert_eq!(reply.status, 202);
    assert_eq!(reply.json()["state"], "local_revoked_core_sync_pending");
    assert!(reply
        .header("set-cookie")
        .contains("HttpOnly; SameSite=Strict; Max-Age=0"));
    assert_eq!(
        fixture
            .request("GET", "/api/v1/auth/session", &[("Cookie", cookie)], "")
            .status,
        401
    );
    let (cookie, csrf) = fixture.login();
    let session = fixture
        .request(
            "GET",
            "/api/v1/auth/session",
            &[("Cookie", cookie.clone())],
            "",
        )
        .json();
    let headers = [
        ("Origin", fixture.origin()),
        ("Cookie", cookie.clone()),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "revoke:fixture".into()),
    ];
    let proof=fixture.request("POST","/api/v1/auth/reauth",&headers,&json!({"schema_version":1,"password":PASSWORD,"operation_digest":session["revoke_operation"]}).to_string());
    assert_eq!(proof.status, 200);
    let revoke=fixture.request("DELETE","/api/v1/auth/sessions",&headers,&json!({"schema_version":1,"operation_digest":session["revoke_operation"],"proof":proof.json()["proof"]}).to_string());
    assert_eq!(revoke.status, 202);
    assert_eq!(revoke.json()["auth_epoch"], 2);
    assert_eq!(
        fixture
            .request("GET", "/api/v1/auth/session", &[("Cookie", cookie)], "")
            .status,
        401
    );
}

#[test]
fn web_host_origin_forwarding_and_csrf_rejections_preserve_the_session() {
    let fixture = Fixture::new(true);
    let (cookie, csrf) = fixture.login();
    assert_eq!(
        fixture
            .request(
                "GET",
                "/api/v1/product/status",
                &[("Cookie", cookie.clone())],
                ""
            )
            .status,
        503
    );
    for headers in [
        vec![("Host", "attacker.invalid".into())],
        vec![("Origin", "https://attacker.invalid".into())],
        vec![("X-Forwarded-Host", fixture.address.to_string())],
        vec![("Sec-Fetch-Site", "cross-site".into())],
    ] {
        assert_eq!(
            fixture
                .request("GET", "/api/v1/bootstrap/status", &headers, "")
                .status,
            403
        );
    }
    assert_eq!(
        fixture
            .request(
                "POST",
                "/api/v1/auth/logout",
                &[("Cookie", cookie.clone())],
                ""
            )
            .status,
        403
    );
    let bad = [
        ("Origin", fixture.origin()),
        ("Cookie", cookie.clone()),
        ("X-Forme-Csrf", "0".repeat(64)),
        ("Idempotency-Key", "logout:rejected".into()),
    ];
    assert_eq!(
        fixture
            .request("POST", "/api/v1/auth/logout", &bad, "")
            .status,
        403
    );
    let duplicate = format!("{cookie}; {cookie}");
    assert_eq!(
        fixture
            .request("GET", "/api/v1/auth/session", &[("Cookie", duplicate)], "")
            .status,
        400
    );
    let live = fixture.request("GET", "/api/v1/auth/session", &[("Cookie", cookie)], "");
    assert_eq!(live.status, 200);
    assert_eq!(live.json()["csrf"], csrf);
}

#[test]
fn web_password_change_revokes_old_cookies_without_changing_owner() {
    let fixture = Fixture::new(true);
    let (cookie, csrf) = fixture.login();
    let replacement = "HTTP replacement fixture password 28";
    let headers = [
        ("Origin", fixture.origin()),
        ("Cookie", cookie.clone()),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "password:change-fixture".into()),
    ];
    let changed = fixture.request(
        "PUT",
        "/api/v1/auth/password",
        &headers,
        &json!({"schema_version":1,"old_password":PASSWORD,"new_password":replacement}).to_string(),
    );
    assert_eq!(changed.status, 202);
    assert_eq!(changed.json()["state"], "local_revoked_core_sync_pending");
    assert_eq!(changed.json()["credential_revision"], 2);
    assert_eq!(
        fixture
            .request("GET", "/api/v1/auth/session", &[("Cookie", cookie)], "")
            .status,
        401
    );
    let (cookie, _) = fixture.login_password(replacement);
    let session = fixture.request("GET", "/api/v1/auth/session", &[("Cookie", cookie)], "");
    assert_eq!(session.status, 200);
    assert_eq!(session.json()["session"]["owner"], "owner:http-fixture");
}

#[test]
fn web_password_change_rejects_reusing_an_operation_key_for_another_password() {
    let fixture = Fixture::new(true);
    let (cookie, csrf) = fixture.login();
    let headers = [
        ("Origin", fixture.origin()),
        ("Cookie", cookie.clone()),
        ("X-Forme-Csrf", csrf),
        ("Idempotency-Key", "password:stable-fixture".into()),
    ];
    let original = "first HTTP replacement password 35";
    let rejected = fixture.request(
        "PUT",
        "/api/v1/auth/password",
        &headers,
        &json!({"schema_version":1,"old_password":"wrong fixture password 46","new_password":original}).to_string(),
    );
    assert_eq!(rejected.status, 401);

    let different = fixture.request(
        "PUT",
        "/api/v1/auth/password",
        &headers,
        &json!({"schema_version":1,"old_password":PASSWORD,"new_password":"different HTTP replacement 82"}).to_string(),
    );
    assert_eq!(different.status, 409);

    let retry = fixture.request(
        "PUT",
        "/api/v1/auth/password",
        &headers,
        &json!({"schema_version":1,"old_password":PASSWORD,"new_password":original}).to_string(),
    );
    assert_eq!(retry.status, 202);
}

#[test]
fn web_read_quota_is_separate_and_rate_rejections_have_retry_after() {
    let fixture = Fixture::new(true);
    for _ in 0..40 {
        assert_eq!(
            fixture
                .request("GET", "/api/v1/bootstrap/status", &[], "")
                .status,
            200
        );
    }
    for _ in 0..16 {
        assert_eq!(
            fixture
                .request("GET", "/api/v1/auth/login-challenge", &[], "")
                .status,
            200
        );
    }
    let reply = fixture.request("GET", "/api/v1/auth/login-challenge", &[], "");
    assert_eq!(reply.status, 429);
    assert_eq!(reply.header("retry-after"), "60");
    assert_eq!(
        fixture
            .request("GET", "/api/v1/bootstrap/status", &[], "")
            .status,
        200
    );
    assert_eq!(
        fixture
            .request(
                "POST",
                "/api/v1/auth/logout",
                &[("Origin", fixture.origin())],
                ""
            )
            .status,
        401
    );
}

#[test]
fn web_configuration_rejects_plaintext_lan_and_malformed_secret_bodies_are_not_echoed() {
    let fixture = Fixture::new(false);
    let mut config = WebServerConfig {
        network_config_path: None,
        network_material_directory: None,
        tls: None,
        bootstrap_bridge: None,
        configuration_bridge: None,
        core_bridge: None,
        schema_version: p::SchemaVersion(1),
        install_ref: "installation:fixture".into(),
        identity_directory: fixture.root.clone(),
        claim_verifying_key: SigningKey::from_bytes(&[127; 32])
            .verifying_key()
            .to_bytes(),
        bind: "0.0.0.0:8765".parse().unwrap(),
    };
    assert!(config.validate().is_err());
    config.bind = fixture.address;
    config.validate().unwrap();
    let bad = "private-fixture-value-do-not-echo";
    let reply = fixture.request(
        "POST",
        "/api/v1/auth/login",
        &[("Origin", fixture.origin())],
        &format!("{{\"password\":\"{bad}\""),
    );
    assert_eq!(reply.status, 400);
    assert!(!reply.body.contains(bad));
    let oversized =
        json!({"schema_version":1,"login_name":"fixture","password":"x".repeat(9000)}).to_string();
    assert_eq!(
        fixture
            .request(
                "POST",
                "/api/v1/auth/login",
                &[("Origin", fixture.origin())],
                &oversized
            )
            .status,
        400
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn web_cancelled_request_keeps_password_capacity_until_real_work_finishes() {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).unwrap();
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "forme-web-http-work-{:x}",
        u128::from_le_bytes(bytes)
    ));
    std::fs::create_dir(&root).unwrap();
    let identity = Arc::new(
        WebIdentityStore::open(
            &root,
            "installation:work",
            SigningKey::from_bytes(&[127; 32]).verifying_key(),
        )
        .unwrap(),
    );
    let admission = Arc::new(WorkAdmission::default());
    let state = Arc::new(WebState {
        core: None,
        bootstrap: None,
        configuration: None,
        identity,
        authorities: vec!["127.0.0.1:8765".into()],
        secure: false,
        listener_digest: p::product_network_listener_digest(
            "127.0.0.1:8765".parse().unwrap(),
            &["127.0.0.1:8765".into()],
            false,
        )
        .unwrap(),
        rate: Mutex::new(security::RateWindow::new()),
        reads: Arc::new(Semaphore::new(16)),
        passwords: Arc::new(Semaphore::new(1)),
        admission: admission.clone(),
        fence_gate: Arc::new(tokio::sync::Mutex::new(())),
    });
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, waiting) = mpsc::channel();
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        task_state
            .blocking(true, move |_| {
                started.send(()).unwrap();
                waiting.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            })
            .await
    });
    ready.await.unwrap();
    task.abort();
    assert!(state.passwords.clone().try_acquire_owned().is_err());
    admission.close().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), admission.idle())
            .await
            .is_err()
    );
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), admission.idle())
        .await
        .unwrap()
        .unwrap();
    assert!(state.passwords.clone().try_acquire_owned().is_ok());
    drop(state);
    assert_eq!(
        root.parent(),
        Some(std::env::temp_dir().canonicalize().unwrap().as_path())
    );
    assert_eq!(root.canonicalize().unwrap(), root);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn web_idle_socket_capacity_is_bounded_and_recovers_when_a_connection_closes() {
    use axum::serve::Listener;
    use tokio::io::AsyncRead;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut listener = connections::BoundedListener::new(listener);
    let mut held = Vec::new();
    for _ in 0..64 {
        let client = tokio::net::TcpStream::connect(address).await.unwrap();
        let (server, _) = listener.accept().await;
        held.push((client, server));
    }
    let mut overflow = tokio::net::TcpStream::connect(address).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), listener.accept())
            .await
            .is_err()
    );
    let mut bytes = [0; 1];
    let mut buffer = tokio::io::ReadBuf::new(&mut bytes);
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        std::future::poll_fn(|context| {
            std::pin::Pin::new(&mut overflow).poll_read(context, &mut buffer)
        }),
    )
    .await
    .unwrap();
    assert!(result.is_err() || buffer.filled().is_empty());
    drop(held.pop());
    let _next = tokio::net::TcpStream::connect(address).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap();
}
