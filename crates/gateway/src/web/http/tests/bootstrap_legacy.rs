//! Public HTTP, real auth DB/KDF and pinned signatures; Host replies controlled.
use super::*;
use axum::{extract::State, http::HeaderMap, Json};
use std::sync::atomic::{AtomicBool, Ordering};

const OPERATION: &str = "claim:http-bootstrap";
const TOKEN: &str = "fixture-bootstrap-service-token-123456789";
type RecordedCalls = Arc<Mutex<Vec<(String, Value)>>>;

struct SetupHost {
    client: Arc<crate::web::bootstrap::LocalBootstrapClient>,
    calls: RecordedCalls,
    bad_signature: Arc<AtomicBool>,
    selected: Arc<AtomicBool>,
    wrong_selection: Arc<AtomicBool>,
    new_selected: Arc<AtomicBool>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    _root: tempfile::TempDir,
}

fn candidate() -> p::BootstrapCandidateViewV2 {
    let owner = p::OwnerPrincipalRef("owner:historical-http".into());
    let hash = p::sha256_content_digest(b"http-bootstrap-fixture");
    let mut value = p::BootstrapCandidateViewV2 {
        schema_version: p::SchemaVersion(2),
        candidate_ref: "candidate:historical-http".into(),
        kind: p::BootstrapCandidateKind::ExistingBrain,
        status: p::BootstrapCandidateStatus::Available,
        candidate_digest: hash.clone(),
        verification_epoch: 1,
        owner_source: p::BootstrapOwnerSource::VerifiedFromCandidate,
        owner_binding_digest: Some(
            p::canonical_digest(&("forme-bootstrap-owner-binding-v2", &owner)).unwrap(),
        ),
        owner_ref: Some(owner),
        store_identity_digest: Some(hash.clone()),
        schema_digest: Some(hash.clone()),
        protection_state: p::BootstrapProtectionState::Protected,
        source_observation_digest: hash,
        issue_ref: None,
    };
    value.candidate_digest = value.expected_digest("installation:http-fixture").unwrap();
    value
}

fn new_candidate() -> p::BootstrapCandidateViewV2 {
    let mut value = candidate();
    value.candidate_ref = "candidate:new-http".into();
    value.kind = p::BootstrapCandidateKind::NewBrainTarget;
    value.owner_source = p::BootstrapOwnerSource::GeneratedForNewBrain;
    value.owner_ref = None;
    value.owner_binding_digest = None;
    value.store_identity_digest = None;
    value.schema_digest = None;
    value.candidate_digest = value.expected_digest("installation:http-fixture").unwrap();
    value
}

impl SetupHost {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let credential = root.path().join("bootstrap-web.token");
        std::fs::write(&credential, TOKEN).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&credential, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let bad_signature = Arc::new(AtomicBool::new(false));
        let forged = bad_signature.clone();
        let selected = Arc::new(AtomicBool::new(false));
        let selection = selected.clone();
        let wrong_selection = Arc::new(AtomicBool::new(false));
        let swapped = wrong_selection.clone();
        let new_selected = Arc::new(AtomicBool::new(false));
        let chosen_new = new_selected.clone();
        let (send, receive) = mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let app = axum::Router::new().route("/v1/setup/{action}", post(
                    move |axum::extract::Path(action): axum::extract::Path<String>, headers: HeaderMap,
                          State(calls): State<RecordedCalls>, Json(body): Json<Value>| {
                        assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                        assert_eq!(headers["idempotency-key"], OPERATION);
                        calls.lock().unwrap().push((action.clone(), body.clone()));
                        let bad = forged.load(Ordering::SeqCst);
                        let selected = selection.load(Ordering::SeqCst);
                        let wrong_selection = swapped.load(Ordering::SeqCst);
                        let new_selected = chosen_new.load(Ordering::SeqCst);
                        if action == "select-v2" { selection.store(true, Ordering::SeqCst); }
                        async move {
                            let value = match action.as_str() {
                                "reserve" => json!({"schema_version":1,"operation_id":body["operation_id"],"expires_at":now()+60_000}),
                                "candidates-v2" => {
                                    let mut catalog = p::BootstrapCandidateCatalogV2 { schema_version:p::SchemaVersion(2),
                                        install_ref:"installation:http-fixture".into(), verification_epoch:1,
                                        candidates:vec![new_candidate(),candidate()], catalog_digest:p::sha256_content_digest(b"pending") };
                                    catalog.catalog_digest = catalog.expected_digest().unwrap();
                                    serde_json::to_value(catalog).unwrap()
                                },
                                "state-v2" => {
                                    let candidate = candidate();
                                    serde_json::to_value(p::BootstrapClaimViewV2 {
                                        schema_version:p::SchemaVersion(2), install_ref:"installation:http-fixture".into(),
                                        operation_id:OPERATION.into(),
                                        phase:if selected { p::BootstrapClaimPhaseV2::Selected } else { p::BootstrapClaimPhaseV2::Observed },
                                        candidate_ref:selected.then_some(candidate.candidate_ref),
                                        candidate_digest:selected.then_some(if wrong_selection { p::sha256_content_digest(b"swapped selection") } else { candidate.candidate_digest }),
                                        verification_epoch:selected.then_some(candidate.verification_epoch),
                                        receipt:None, activation:None, active_data_binding_digest:None, issue_ref:None,
                                    }).unwrap()
                                },
                                "data" => json!({"schema_version":1,"candidate_ref":"candidate:new-http","selected":new_selected,"available":true,"issue_code":null}),
                                "select-v2" => serde_json::to_value(candidate()).unwrap(),
                                "adopt-v2" => {
                                    let request: p::BootstrapClaimRequestV2 = serde_json::from_value(body).unwrap();
                                    request.validate().unwrap();
                                    let candidate = candidate();
                                    let receipt = p::BootstrapClaimReceiptV2 { schema_version:p::SchemaVersion(2),
                                        install_ref:"installation:http-fixture".into(), operation_id:request.session.operation_id,
                                        candidate_ref:request.candidate_ref, candidate_kind:candidate.kind,
                                        candidate_digest:request.expected_candidate_digest, owner_source:candidate.owner_source,
                                        owner_ref:candidate.owner_ref.unwrap(), owner_binding_digest:candidate.owner_binding_digest.unwrap(),
                                        account_commitment:request.account_commitment, target_binding_digest:p::sha256_content_digest(b"target"),
                                        migration_outcome:p::BootstrapMigrationOutcome::Recovered, expires_at:now()+60_000 };
                                    let activation = p::BootstrapClaimActivationV2 { schema_version:p::SchemaVersion(2),
                                        operation_id:receipt.operation_id.clone(), receipt_digest:receipt.digest().unwrap(),
                                        active_data_binding_digest:receipt.target_binding_digest.clone() };
                                    let digest = p::SignedBootstrapClaimActivationV2::signing_digest(&receipt, &activation).unwrap();
                                    let key = SigningKey::from_bytes(&[if bad { 126 } else { 127 };32]);
                                    serde_json::to_value(p::SignedBootstrapClaimActivationV2 { schema_version:p::SchemaVersion(2), receipt,
                                        activation, signature:key.sign(digest.0.as_bytes()).to_bytes().to_vec() }).unwrap()
                                },
                                _ => panic!("unexpected setup request: {action}"),
                            };
                            Json(value)
                        }
                    })).with_state(recorded);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                send.send(listener.local_addr().unwrap()).unwrap();
                axum::serve(listener, app).with_graceful_shutdown(async move { let _ = stopped.await; }).await.unwrap();
            });
        });
        let client = Arc::new(
            crate::web::bootstrap::LocalBootstrapClient::connect(
                receive.recv_timeout(Duration::from_secs(5)).unwrap(),
                &credential,
            )
            .unwrap(),
        );
        Self {
            client,
            calls,
            bad_signature,
            selected,
            wrong_selection,
            new_selected,
            stop: Some(stop),
            thread: Some(thread),
            _root: root,
        }
    }
}
impl Drop for SetupHost {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.thread.take().unwrap().join().unwrap();
    }
}

fn setup(f: &Fixture) -> Vec<(&'static str, String)> {
    let reply = f.request(
        "POST",
        "/api/v1/bootstrap/prove",
        &[
            ("Origin", f.origin()),
            ("Idempotency-Key", OPERATION.into()),
        ],
        &json!({"schema_version":1,"proof":"local-fixture-proof"}).to_string(),
    );
    assert_eq!(reply.status, 200);
    vec![
        ("Origin", f.origin()),
        ("Cookie", reply.cookie(security::SETUP_COOKIE)),
        (
            "X-Forme-Csrf",
            reply.json()["csrf"].as_str().unwrap().into(),
        ),
        ("Idempotency-Key", OPERATION.into()),
    ]
}

#[test]
fn bootstrap_v2_http_rejects_unbound_cookie_csrf_version_and_paths_before_host_io() {
    let host = SetupHost::new();
    let f = Fixture::with_setup(false, None, None, Some(host.client.clone()));
    assert_eq!(
        f.request("GET", "/api/v1/bootstrap/candidates", &[], "")
            .status,
        401
    );
    assert!(host.calls.lock().unwrap().is_empty());
    let headers = setup(&f);
    let body = json!({"schema_version":2,"candidate_ref":candidate().candidate_ref,"expected_candidate_digest":candidate().candidate_digest});
    let selection = "/api/v1/bootstrap/select-v2";
    let no_csrf = vec![headers[0].clone(), headers[1].clone(), headers[3].clone()];
    assert_eq!(
        f.request("POST", selection, &no_csrf, &body.to_string())
            .status,
        403
    );
    for key in ["owner", "path", "verified", "account_commitment"] {
        let mut forged = body.clone();
        forged[key] = "injected".into();
        assert_eq!(
            f.request("POST", selection, &headers, &forged.to_string())
                .status,
            400
        );
    }
    let mut wrong = body.clone();
    wrong["schema_version"] = 1.into();
    assert_eq!(
        f.request("POST", selection, &headers, &wrong.to_string())
            .status,
        400
    );
    let mut mismatch = headers.clone();
    mismatch[3].1 = "claim:other".into();
    assert_eq!(
        f.request("POST", selection, &mismatch, &body.to_string())
            .status,
        409
    );
    assert_eq!(host.calls.lock().unwrap().len(), 1);
    assert_eq!(
        f.request("GET", "/api/v1/bootstrap/candidates", &headers, "")
            .status,
        200
    );
    assert_eq!(
        f.request("POST", selection, &headers, &body.to_string())
            .status,
        200
    );
    let refreshed = f.request("GET", "/api/v1/bootstrap/candidates", &headers, "");
    assert_eq!(refreshed.status, 200);
    assert_eq!(
        refreshed.json()["selected"]["candidate_ref"],
        candidate().candidate_ref
    );
    assert_eq!(host.calls.lock().unwrap().len(), 5); // Refresh is local, no rediscovery.
}

#[test]
fn bootstrap_v2_http_recovers_only_the_exact_host_selection_after_a_lost_response() {
    let host = SetupHost::new();
    let f = Fixture::with_setup(false, None, None, Some(host.client.clone()));
    let headers = setup(&f);
    // Host selected, but Web never received/recorded that response.
    host.selected.store(true, Ordering::SeqCst);
    host.wrong_selection.store(true, Ordering::SeqCst);
    assert_eq!(
        f.request("GET", "/api/v1/bootstrap/candidates", &headers, "")
            .status,
        409
    );
    host.wrong_selection.store(false, Ordering::SeqCst);
    let recovered = f.request("GET", "/api/v1/bootstrap/candidates", &headers, "");
    assert_eq!(recovered.status, 200);
    assert_eq!(
        recovered.json()["selected"],
        serde_json::to_value(candidate()).unwrap()
    );
    let once = host.calls.lock().unwrap().len();
    let refreshed = f.request("GET", "/api/v1/bootstrap/candidates", &headers, "");
    assert_eq!(refreshed.json(), recovered.json());
    let calls = host.calls.lock().unwrap();
    assert_eq!(calls.len(), once);
    assert!(calls
        .iter()
        .all(|(action, _)| matches!(action.as_str(), "reserve" | "candidates-v2" | "state-v2")));
}

#[test]
fn bootstrap_v2_http_recovers_the_original_schema_one_new_choice_without_selecting_again() {
    let host = SetupHost::new();
    let f = Fixture::with_setup(false, None, None, Some(host.client.clone()));
    let headers = setup(&f);
    host.new_selected.store(true, Ordering::SeqCst);
    let recovered = f.request("GET", "/api/v1/bootstrap/candidates", &headers, "");
    assert_eq!(recovered.status, 200);
    assert_eq!(
        recovered.json()["selected"],
        serde_json::to_value(new_candidate()).unwrap()
    );
    let refreshed = f.request("GET", "/api/v1/bootstrap/candidates", &headers, "");
    assert_eq!(refreshed.json(), recovered.json());
    assert!(host
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(action, _)| matches!(
            action.as_str(),
            "reserve" | "candidates-v2" | "state-v2" | "data"
        )));
}

#[test]
fn bootstrap_v2_http_creates_owner_session_only_after_pinned_activation_without_starting_a_core() {
    let host = SetupHost::new();
    let f = Fixture::with_setup(false, None, None, Some(host.client.clone()));
    let headers = setup(&f);
    let selection = json!({"schema_version":2,"candidate_ref":candidate().candidate_ref,"expected_candidate_digest":candidate().candidate_digest});
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/bootstrap/select-v2",
            &headers,
            &selection.to_string()
        )
        .status,
        200
    );
    let body = json!({"schema_version":2,"candidate_ref":candidate().candidate_ref,"operation_id":OPERATION,"login_name":"fixture","password":PASSWORD});
    let mut wrong = body.clone();
    wrong["operation_id"] = "claim:substituted".into();
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/bootstrap/claim-v2",
            &headers,
            &wrong.to_string()
        )
        .status,
        400
    );
    let mut wrong = body.clone();
    wrong["candidate_ref"] = "candidate:substituted".into();
    assert_eq!(
        f.request(
            "POST",
            "/api/v1/bootstrap/claim-v2",
            &headers,
            &wrong.to_string()
        )
        .status,
        409
    );
    host.bad_signature.store(true, Ordering::SeqCst);
    let rejected = f.request(
        "POST",
        "/api/v1/bootstrap/claim-v2",
        &headers,
        &body.to_string(),
    );
    assert_eq!(rejected.status, 401);
    assert_eq!(
        f.request("GET", "/api/v1/bootstrap/status", &[], "").json()["claimed"],
        false
    );
    host.bad_signature.store(false, Ordering::SeqCst);
    let accepted = f.request(
        "POST",
        "/api/v1/bootstrap/claim-v2",
        &headers,
        &body.to_string(),
    );
    assert_eq!(accepted.status, 200);
    assert_eq!(accepted.json()["session"]["owner"], "owner:historical-http");
    assert!(!accepted.body.contains(PASSWORD));
    assert_eq!(
        f.request("GET", "/api/v1/bootstrap/status", &[], "").json()["claimed"],
        true
    );
    let cookie = accepted.cookie(security::SESSION_COOKIE);
    assert_eq!(
        f.request("GET", "/api/v1/brain/runs", &[("Cookie", cookie)], "")
            .status,
        503
    );
    let calls = host.calls.lock().unwrap();
    let adoptions: Vec<_> = calls
        .iter()
        .filter(|(action, _)| action == "adopt-v2")
        .collect();
    assert_eq!(adoptions.len(), 2);
    assert_eq!(adoptions[0].1, adoptions[1].1);
    for (_, value) in adoptions {
        let wire = value.to_string();
        assert!(
            !wire.contains(PASSWORD) && !wire.contains("login_name") && !wire.contains("password")
        );
    }
}
