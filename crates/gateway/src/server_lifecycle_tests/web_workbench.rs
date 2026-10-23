//! Real Web + gateway + ProductionRuntime; model is a counted local fixture.
use super::*;
use crate::web::identity::{SensitivePassword, WebIdentityStore};
use crate::web::{http, WebCoreBridgeConfig, WebServerConfig};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};

const INSTALL: &str = "installation:workbench-fixture";
const PASSWORD: &str = "workbench fixture password 25";
const BRIDGE_TOKEN: &str = "workbench-bridge-0123456789abcdef0123456789abcdef";

#[path = "web_workbench/brain_views.rs"]
mod brain_views;
#[path = "web_workbench/data_views.rs"]
mod data_views;
#[path = "web_workbench/memory_views.rs"]
mod memory_views;
#[path = "web_workbench/project_views.rs"]
mod project_views;
#[path = "web_workbench/proposal_views.rs"]
mod proposal_views;
#[path = "web_workbench/runtime_views.rs"]
mod runtime_views;

struct Reply {
    status: u16,
    head: String,
    body: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }
    fn cookie(&self, name: &str) -> String {
        self.head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, v)| v.trim().split(';').next().unwrap())
            .find(|value| value.starts_with(&format!("{name}=")))
            .unwrap()
            .into()
    }
}

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: Vec<(&str, String)>,
    body: String,
) -> Reply {
    let method = method.to_owned();
    let path = path.to_owned();
    let headers: Vec<(String, String)> = headers.into_iter().map(|(k, v)| (k.into(), v)).collect();
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", body.len()).unwrap();
        for (name, value) in headers { write!(stream, "{name}: {value}\r\n").unwrap(); }
        write!(stream, "\r\n{body}").unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        let status = head.lines().next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
        Reply { status, head: head.into(), body: body.into() }
    }).await.unwrap()
}

fn mutation_headers(
    address: SocketAddr,
    cookie: &str,
    csrf: &str,
    key: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("Origin", format!("http://{address}")),
        ("Cookie", cookie.into()),
        ("X-Forme-Csrf", csrf.into()),
        ("Idempotency-Key", key.into()),
    ]
}

async fn login(address: SocketAddr) -> (String, String) {
    let challenge = request(
        address,
        "GET",
        "/api/v1/auth/login-challenge",
        vec![],
        String::new(),
    )
    .await;
    assert_eq!(challenge.status, 200);
    let value = challenge.json();
    let response = request(
        address,
        "POST",
        "/api/v1/auth/login",
        mutation_headers(
            address,
            &challenge.cookie("forme_local_login"),
            value["csrf"].as_str().unwrap(),
            value["attempt_ref"].as_str().unwrap(),
        ),
        json!({"schema_version":1,"login_name":"fixture","password":PASSWORD}).to_string(),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.body);
    (
        response.cookie("forme_local_session"),
        response.json()["csrf"].as_str().unwrap().into(),
    )
}

fn proof_headers(key: &SigningKey, cookie: &str, nonce: &str) -> Vec<(&'static str, String)> {
    let digest = p::canonical_digest(&(
        p::SchemaVersion(1),
        "forme-web-core-request-v1",
        "GET",
        "/v1/runs",
        b"".as_slice(),
    ))
    .unwrap();
    let session = p::canonical_digest(&(
        p::SchemaVersion(1),
        "web-session-secret",
        cookie.split_once('=').unwrap().1,
    ))
    .unwrap();
    let mut assertion = p::CoreSessionAssertion {
        schema_version: p::SchemaVersion(1),
        install_ref: INSTALL.into(),
        owner: p::OwnerPrincipalRef("owner:lifecycle".into()),
        session_digest: session,
        auth_epoch: 1,
        audience: "forme-core".into(),
        request_digest: digest.clone(),
        expires_at: now_ms() + 30_000,
        nonce: nonce.into(),
        signature: vec![0; 64],
    };
    assertion.signature = key
        .sign(assertion.signing_digest().unwrap().0.as_bytes())
        .to_bytes()
        .to_vec();
    let encoded: String = serde_json::to_vec(&assertion)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    vec![
        ("Authorization", format!("Bearer {BRIDGE_TOKEN}")),
        ("X-Forme-Core-Assertion", encoded),
        ("X-Forme-Request-Digest", digest.0),
    ]
}

fn claim(fixture: &Fixture, key: &SigningKey, directory: &std::path::Path) {
    std::fs::create_dir(directory).unwrap();
    let identity = WebIdentityStore::open(directory, INSTALL, key.verifying_key()).unwrap();
    let commitment = identity
        .prepare_claim(
            "claim:workbench",
            "fixture",
            SensitivePassword::new(PASSWORD.into()),
        )
        .unwrap();
    let receipt = p::WebClaimCommit {
        schema_version: p::SchemaVersion(1),
        install_ref: INSTALL.into(),
        operation_id: "claim:workbench".into(),
        candidate_ref: "data:workbench".into(),
        owner: fixture.config.owner.clone(),
        account_commitment: commitment,
        expires_at: now_ms() + 60_000,
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

#[tokio::test]
async fn web_workbench_runs_through_the_brain_once_and_logout_fences_core_proofs() {
    let fixture = Fixture::new();
    let runtime = fixture.build();
    let associated_workspace = p::WorkspaceRef("workspace:web-associated".into());
    let charter = runtime
        .prepare_workspace_charter(
            &p::VerifiedPrincipal(fixture.config.owner.0.clone()),
            p::WorkspaceCharterRecord {
                schema_version: p::SchemaVersion(1),
                workspace: associated_workspace.clone(),
                version: 1,
                goals: vec![],
                constraints: vec![p::Constraint("max-nodes:8".into())],
                prohibitions: vec![],
                done_contract: None,
                review_cadence: Some(p::DurationMs(60_000)),
                actor: p::Actor::Owner,
                digest: p::SchemaDigest(String::new()),
            },
            0,
        )
        .unwrap();
    runtime
        .apply_workspace_charter(
            &p::VerifiedPrincipal(fixture.config.owner.0.clone()),
            p::IdempotencyKey("charter:web-associated".into()),
            charter,
        )
        .unwrap();
    let key = SigningKey::from_bytes(&[107; 32]);
    let identity_path = fixture.root.join("web-auth");
    claim(&fixture, &key, &identity_path);
    let token_path = fixture.root.join("bridge-token");
    let key_path = fixture.root.join("bridge-signing-key");
    std::fs::write(&token_path, BRIDGE_TOKEN).unwrap();
    std::fs::write(&key_path, key.to_bytes()).unwrap();

    let mut config = fixture.gateway_config();
    config.install_ref = Some(INSTALL.into());
    config.web_bridge_token = Some(LocalBearerSecret::new(BRIDGE_TOKEN.into()).unwrap());
    config.web_assertion_verifying_key = Some(key.verifying_key());
    config.web_session_fence_path = Some(fixture.root.join("core-auth.sqlite"));
    let (core_stop, core_stopped) = tokio::sync::oneshot::channel();
    let (ready, address) = tokio::sync::oneshot::channel();
    let core = tokio::spawn(serve_runtime_until(
        config,
        runtime.clone(),
        GatewayLifecycleConfig {
            schema_version: p::SchemaVersion(1),
            drain_timeout: Duration::from_secs(5),
        },
        move |address| {
            ready.send(address).unwrap();
            Ok(())
        },
        async {
            core_stopped.await.unwrap();
            Ok(())
        },
    ));
    let core_address = address.await.unwrap();
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let web_address = probe.local_addr().unwrap();
    drop(probe);
    let web_config = WebServerConfig {
        tls: None,
        schema_version: p::SchemaVersion(1),
        install_ref: INSTALL.into(),
        identity_directory: identity_path,
        claim_verifying_key: key.verifying_key().to_bytes(),
        bind: web_address,
        bootstrap_bridge: None,
        configuration_bridge: None,
        core_bridge: Some(WebCoreBridgeConfig {
            schema_version: p::SchemaVersion(1),
            address: core_address,
            credential_path: token_path,
            signing_key_path: key_path,
        }),
    };
    let (web_stop, web_stopped) = tokio::sync::oneshot::channel();
    let (ready, address) = tokio::sync::oneshot::channel();
    let web = tokio::spawn(http::serve_until(
        web_config,
        move |address| {
            ready.send(address).unwrap();
            Ok(())
        },
        async {
            web_stopped.await.unwrap();
            Ok(())
        },
    ));
    assert_eq!(address.await.unwrap(), web_address);

    let unauthenticated_request = p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef("session:unregistered".into()),
        agent_profile: fixture.config.agent_profile.clone(),
        input: p::RunInput("Return the fixture result".into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey("command:unauthenticated".into())),
    };
    let unauthenticated_body = serde_json::to_string(&unauthenticated_request).unwrap();
    assert_eq!(
        request(
            web_address,
            "POST",
            "/api/v1/brain/runs",
            vec![
                ("Origin", format!("http://{web_address}")),
                ("Idempotency-Key", "command:workbench-one".into())
            ],
            unauthenticated_body
        )
        .await
        .status,
        403
    );
    let (cookie, csrf) = login(web_address).await;
    runtime_views::verify(web_address, &runtime, &fixture.calls, &cookie, &csrf).await;
    let created_session = request(
        web_address,
        "POST",
        "/api/v1/sessions",
        mutation_headers(web_address, &cookie, &csrf, "command:session-create"),
        json!({
            "schema_version": 1,
            "command": "command:session-create",
            "title": "项目对话",
            "workspace": null
        })
        .to_string(),
    )
    .await;
    assert_eq!(created_session.status, 200, "{}", created_session.body);
    let session: p::SessionId =
        serde_json::from_value(created_session.json()["session"].clone()).unwrap();
    let associated_session = request(
        web_address,
        "PUT",
        &format!("/api/v1/sessions/{}", session.0),
        mutation_headers(web_address, &cookie, &csrf, "command:session-associate"),
        json!({
            "schema_version": 1,
            "command": "command:session-associate",
            "session": session.0,
            "expected_updated_at": created_session.json()["updated_at"],
            "mutation": {"operation": "associate_workspace", "workspace": associated_workspace.0}
        })
        .to_string(),
    )
    .await;
    assert_eq!(
        associated_session.status, 200,
        "{}",
        associated_session.body
    );
    let run_request = p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef(session.0.clone()),
        agent_profile: fixture.config.agent_profile.clone(),
        input: p::RunInput("Return the fixture result".into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey("command:workbench-one".into())),
    };
    let body = serde_json::to_string(&run_request).unwrap();
    brain_views::verify(web_address, &runtime, &cookie, &csrf).await;
    memory_views::verify(web_address, &runtime, &fixture.calls, &cookie, &csrf).await;
    proposal_views::verify(web_address, &runtime, &cookie, &csrf).await;
    project_views::verify(web_address, &runtime, &cookie, &csrf).await;
    data_views::verify(web_address, &cookie, &csrf).await;
    let proof = proof_headers(&key, &cookie, "proof:before-logout");
    assert_eq!(
        request(
            core_address,
            "GET",
            "/v1/runs",
            proof.clone(),
            String::new()
        )
        .await
        .status,
        200
    );
    assert_eq!(
        request(core_address, "GET", "/v1/runs", proof, String::new())
            .await
            .status,
        401
    );
    let stale = proof_headers(&key, &cookie, "proof:after-logout");
    assert_eq!(
        request(
            web_address,
            "POST",
            "/api/v1/brain/runs",
            mutation_headers(web_address, &cookie, "wrong", "command:workbench-one"),
            body.clone()
        )
        .await
        .status,
        403
    );
    let accepted = request(
        web_address,
        "POST",
        "/api/v1/brain/runs",
        mutation_headers(web_address, &cookie, &csrf, "command:workbench-one"),
        body.clone(),
    )
    .await;
    assert_eq!(accepted.status, 202, "{}", accepted.body);
    let run: p::RunId = serde_json::from_value(accepted.json()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if events(&runtime, &run)
                .iter()
                .any(|event| matches!(&event.payload, p::EventPayload::SessionBound(_)))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    assert!(events(&runtime, &run).iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::SessionBound(bound) if bound.workspace == associated_workspace
    )));
    let defaulted_session = request(
        web_address,
        "PUT",
        &format!("/api/v1/sessions/{}", session.0),
        mutation_headers(web_address, &cookie, &csrf, "command:session-default"),
        json!({
            "schema_version": 1,
            "command": "command:session-default",
            "session": session.0,
            "expected_updated_at": associated_session.json()["updated_at"],
            "mutation": {"operation": "associate_workspace", "workspace": null}
        })
        .to_string(),
    )
    .await;
    assert_eq!(defaulted_session.status, 200, "{}", defaulted_session.body);
    let archived_session = request(
        web_address,
        "PUT",
        &format!("/api/v1/sessions/{}", session.0),
        mutation_headers(web_address, &cookie, &csrf, "command:session-archive"),
        json!({
            "schema_version": 1,
            "command": "command:session-archive",
            "session": session.0,
            "expected_updated_at": defaulted_session.json()["updated_at"],
            "mutation": {"operation": "archive", "archived": true}
        })
        .to_string(),
    )
    .await;
    assert_eq!(archived_session.status, 200, "{}", archived_session.body);
    let repeated = request(
        web_address,
        "POST",
        "/api/v1/brain/runs",
        mutation_headers(web_address, &cookie, &csrf, "command:workbench-one"),
        body,
    )
    .await;
    assert_eq!(repeated.status, 202, "{}", repeated.body);
    assert_eq!(repeated.json(), accepted.json());
    let mut changed_request = run_request.clone();
    changed_request.input = p::RunInput("A different request with the same key".into());
    let conflicting = request(
        web_address,
        "POST",
        "/api/v1/brain/runs",
        mutation_headers(web_address, &cookie, &csrf, "command:workbench-one"),
        serde_json::to_string(&changed_request).unwrap(),
    )
    .await;
    assert_eq!(conflicting.status, 409, "{}", conflicting.body);
    let mut archived_request = run_request.clone();
    archived_request.idempotency_key = Some(p::IdempotencyKey("command:archived-session".into()));
    let archived = request(
        web_address,
        "POST",
        "/api/v1/brain/runs",
        mutation_headers(web_address, &cookie, &csrf, "command:archived-session"),
        serde_json::to_string(&archived_request).unwrap(),
    )
    .await;
    assert_eq!(archived.status, 409, "{}", archived.body);
    fixture.wait_for_model().await;
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    let old_cookie = cookie.clone();
    let logout = request(
        web_address,
        "POST",
        "/api/v1/auth/logout",
        mutation_headers(web_address, &cookie, &csrf, "command:logout"),
        String::new(),
    )
    .await;
    assert_eq!(logout.status, 204, "{}", logout.body);
    assert_eq!(
        request(core_address, "GET", "/v1/runs", stale, String::new())
            .await
            .status,
        401
    );
    assert_eq!(
        request(
            web_address,
            "GET",
            "/api/v1/brain/runs",
            vec![("Cookie", old_cookie)],
            String::new()
        )
        .await
        .status,
        401
    );
    // Closing the entire Web listener must not stop the accepted core run.
    web_stop.send(()).unwrap();
    web.await.unwrap().unwrap();
    fixture.release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if events(&runtime, &run)
                .iter()
                .any(|e| e.kind == p::EventKind::RunComplete)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    let recorded = events(&runtime, &run);
    assert_eq!(
        recorded
            .iter()
            .filter(|e| e.kind == p::EventKind::RunAccepted)
            .count(),
        1
    );
    assert!(recorded
        .iter()
        .any(|e| e.kind == p::EventKind::ModelCallFinished));
    core_stop.send(()).unwrap();
    core.await.unwrap().unwrap();
    drop(runtime);
}
