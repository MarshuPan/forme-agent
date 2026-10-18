//! D-wave ingress acceptance against the real daemon and a fresh SQLite store.
//! No model response or backend is stubbed. A listening, non-serving socket
//! detects accidental model calls. The CLI case also needs `cargo build -p forme-cli`.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_protocol as p;
use serde_json::{json, Value};

const TOKEN: &str = "data-controls-owner-token-0123456789abcdef0123456789";
const OWNER: &str = "owner:data-controls-acceptance";
const WORKSPACE: &str = "workspace:data-controls";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Response {
    status: u16,
    body: String,
}

impl Response {
    fn expect(self, status: u16) -> Self {
        assert_eq!(
            self.status, status,
            "unexpected HTTP response: {}",
            self.body
        );
        self
    }

    fn json(self) -> Value {
        serde_json::from_str(&self.body).expect("JSON response")
    }
}

fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: Option<&Value>,
) -> std::io::Result<Response> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let body = body.map(Value::to_string).unwrap_or_default();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n{body}")?;
    stream.flush()?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let (head, body) = response.split_once("\r\n\r\n").expect("HTTP response");
    Ok(Response {
        status: head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        body: body.to_owned(),
    })
}

fn auth() -> (&'static str, String) {
    ("Authorization", format!("Bearer {TOKEN}"))
}

fn headers(address: SocketAddr) -> Vec<(&'static str, String)> {
    vec![
        auth(),
        ("Content-Type", "application/json".into()),
        ("Origin", format!("http://{address}")),
        ("x-forme-csrf", "1".into()),
    ]
}

struct Daemon {
    root: PathBuf,
    address: SocketAddr,
    child: Option<Child>,
    model_probe: TcpListener,
}

impl Daemon {
    fn new() -> Self {
        let tag = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "forme-v1-data-controls-{}-{tag}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let model_probe = TcpListener::bind("127.0.0.1:0").unwrap();
        model_probe.set_nonblocking(true).unwrap();
        let mut fixture = Self {
            root: root.canonicalize().unwrap(),
            address,
            child: None,
            model_probe,
        };
        fixture.start();
        fixture
    }

    fn start(&mut self) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forme-gatewayd"));
        clear_forme_environment(&mut command);
        self.child = Some(
            command
                .current_dir(&self.root)
                .env("FORME_STORE_PATH", self.root.join("authority.db"))
                .env("FORME_ARTIFACT_ROOT", self.root.join("artifacts"))
                .env("FORME_OWNER_ID", OWNER)
                .env("FORME_WORKSPACE", WORKSPACE)
                .env("FORME_GATEWAY_BIND", "127.0.0.1")
                .env("FORME_GATEWAY_PORT", self.address.port().to_string())
                .env("FORME_GATEWAY_TOKEN", TOKEN)
                .env("FORME_GATEWAY_MAX_BODY_BYTES", "65536")
                .env("FORME_GATEWAY_MAX_REQUESTS_PER_MINUTE", "10000")
                .env(
                    "FORME_MODEL_BASE_URL",
                    format!("http://{}/v1", self.model_probe.local_addr().unwrap()),
                )
                .env("FORME_MODEL_NAME", "unused-data-controls-model")
                .env("FORME_MODEL_API_KEY", "unused-data-controls-fixture-key")
                .env("FORME_MODEL_TIMEOUT_MS", "100")
                .env("FORME_BRAIN_LEASE_MS", "200")
                .env("FORME_SCHEDULER_ENABLED", "false")
                .env("FORME_COGNITIVE_TICK_ENABLED", "false")
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let child = self.child.as_mut().unwrap();
            if let Some(status) = child.try_wait().unwrap() {
                let mut stderr = String::new();
                child
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut stderr)
                    .unwrap();
                panic!("daemon exited before readiness ({status}): {stderr}");
            }
            if http(self.address, "GET", "/healthz", &[], None)
                .is_ok_and(|response| response.status == 200)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("data controls daemon did not become ready");
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn restart(&mut self) {
        self.assert_no_model_calls();
        self.stop();
        // Kill cannot release the durable writer lease; allow its configured TTL.
        std::thread::sleep(Duration::from_millis(250));
        self.start();
    }

    fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Response {
        http(self.address, method, path, &headers(self.address), body).unwrap()
    }

    fn get(&self, path: &str) -> Value {
        self.request("GET", path, None).expect(200).json()
    }

    fn post(&self, operation: &str, body: &Value) -> Response {
        self.request("POST", &format!("/v1/data/{operation}"), Some(body))
    }

    fn backup(&self, name: &str) -> Value {
        self.post("backup", &json!({"schema_version":1,"target":name}))
            .expect(200)
            .json()
    }

    fn restore_plan(&self, source: &str, target: &str) -> Value {
        self.post(
            "restore-plan",
            &json!({"schema_version":1,"source":source,"target":target}),
        )
        .expect(200)
        .json()
    }

    fn lifecycle_plan(&self, subject: &Value, operation: &str, version: u64) -> Value {
        self.post("lifecycle", &lifecycle_prepare(subject, operation, version))
            .expect(200)
            .json()
    }

    fn data_file(&self, name: &str) -> PathBuf {
        self.root.join("data-operations").join(name)
    }

    fn events(&self, run: &str) -> Vec<p::Event> {
        let response = self
            .request("GET", &format!("/v1/runs/{run}/events"), None)
            .expect(200);
        let events: Vec<p::Event> = response
            .body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for (index, event) in events.iter().enumerate() {
            assert_eq!(event.stream_seq, index as u64 + 1);
            assert_eq!(event.provenance.actor, p::Actor::Owner);
            assert_eq!(event.provenance.source, p::Source::OwnerControl);
            assert_eq!(event.provenance.trust_tier, p::TrustTier::OwnerInput);
            assert_ne!(event.kind, p::EventKind::ModelCallStarted);
            assert_ne!(event.kind, p::EventKind::ActionStarted);
        }
        events
    }

    fn assert_no_model_calls(&self) {
        match self.model_probe.accept() {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            _ => panic!("data/dashboard controls connected to the model endpoint"),
        }
    }

    fn snapshot(&self) -> Snapshot {
        self.assert_no_model_calls();
        let dashboard = self.get("/v1/brain/dashboard");
        assert_eq!(dashboard["metrics"]["model_attempts"]["denominator"], 0);
        let mut files = BTreeMap::new();
        collect_files(&self.root, &self.root, &mut files);
        Snapshot { dashboard, files }
    }

    fn reject(&self, operation: &str, request: &Value, status: u16) {
        let before = self.snapshot();
        self.post(operation, request).expect(status);
        assert_eq!(
            before,
            self.snapshot(),
            "rejected {operation} changed state"
        );
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop();
        // This directory was exclusively created by this fixture, never supplied
        // by an environment variable. Validate the exact cleanup boundary.
        let temp = std::env::temp_dir().canonicalize().unwrap();
        if self.root.parent() == Some(temp.as_path())
            && self
                .root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("forme-v1-data-controls-")
        {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn clear_forme_environment(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("FORME_")
        {
            command.env_remove(key);
        }
    }
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    dashboard: Value,
    // Paths and hashes only: never print raw key bytes. The live SQLite files
    // change with lease heartbeats; event digest/count covers logical writes.
    files: BTreeMap<PathBuf, Option<p::SchemaDigest>>,
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<PathBuf, Option<p::SchemaDigest>>,
) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap().to_owned();
        if ["authority.db", "authority.db-wal", "authority.db-shm"]
            .iter()
            .any(|name| relative == Path::new(name))
        {
            continue;
        }
        let kind = entry.file_type().unwrap();
        assert!(!kind.is_symlink(), "unexpected symlink in fixture");
        if kind.is_dir() {
            files.insert(relative, None);
            collect_files(root, &path, files);
        } else {
            assert!(kind.is_file());
            files.insert(
                relative,
                Some(p::sha256_content_digest(&std::fs::read(path).unwrap())),
            );
        }
    }
}

fn control_run(plan: &Value) -> String {
    format!("data-control:{}", plan["digest"].as_str().unwrap())
}

fn confirmation(plan: &Value) -> Value {
    json!({"schema_version":1,"plan_digest":plan["digest"],"nonce":plan["nonce"],
        "use_by":plan["expires_at"],"approved":true})
}

fn apply_request(plan: &Value, lifecycle: bool) -> Value {
    let mut body = json!({"schema_version":1,"plan":plan,"confirmation":confirmation(plan)});
    if lifecycle {
        body["action"] = json!("Apply");
    }
    body
}

fn lifecycle_prepare(subject: &Value, operation: &str, version: u64) -> Value {
    json!({"action":"Prepare","schema_version":1,"scope":WORKSPACE,
        "subject_digest":p::sha256_content_digest(control_run(subject).as_bytes()),
        "operation":operation,"expected_version":version})
}

fn refresh_digest(plan: &mut Value) {
    plan["digest"] = json!("");
    plan["digest"] = json!(p::canonical_digest(plan).unwrap());
}

fn assert_audit(daemon: &Daemon, plan: &Value, complete: bool) {
    let events = daemon.events(&control_run(plan));
    let expected = if complete {
        vec![
            p::EventKind::ApprovalRequested,
            p::EventKind::ApprovalResolved,
            p::EventKind::RunComplete,
        ]
    } else {
        vec![p::EventKind::ApprovalRequested]
    };
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        expected
    );
    let p::EventPayload::ApprovalRequested(request) = &events[0].payload else {
        panic!("approval request")
    };
    assert_eq!(request.plan_digest.as_ref().unwrap().0, plan["digest"]);
    assert_eq!(
        request.normalized_action_digest.as_ref().unwrap().0,
        plan["digest"]
    );
    assert_eq!(request.expires_at, plan["expires_at"]);
    assert_eq!(request.risk, p::Risk::High);
    assert_eq!(request.affected_resources.len(), 1);
    assert_eq!(
        request.affected_resources[0].0,
        format!("data-plan:{}", plan["digest"].as_str().unwrap())
    );
    if complete {
        let p::EventPayload::ApprovalResolved(resolved) = &events[1].payload else {
            panic!("approval resolution")
        };
        assert_eq!(resolved.approval_id, request.approval_id);
        assert_eq!(resolved.outcome, p::ApprovalOutcome::Granted);
    }
}

fn assert_verified(value: &Value) {
    for field in [
        "digest_ok",
        "integrity_ok",
        "event_continuity_ok",
        "projection_replay_ok",
    ] {
        assert_eq!(value[field], true, "backup verification {field}");
    }
}

#[test]
fn c21_c23_every_data_route_requires_owner_bearer_origin_and_csrf_without_mutation() {
    let daemon = Daemon::new();
    daemon.backup("source.db");
    let restore = daemon.restore_plan("source.db", "restored.db");
    let subject = daemon.restore_plan("source.db", "subject.db");
    let lifecycle = daemon.lifecycle_plan(&subject, "CryptoShred", 0);
    let cases = [
        (
            "backup",
            json!({"schema_version":1,"target":"unauthorized.db"}),
        ),
        (
            "verify-backup",
            json!({"schema_version":1,"source":"source.db"}),
        ),
        (
            "restore-plan",
            json!({"schema_version":1,"source":"source.db","target":"unapproved.db"}),
        ),
        ("restore", apply_request(&restore, false)),
        ("lifecycle", lifecycle_prepare(&subject, "Delete", 0)),
        ("lifecycle", apply_request(&lifecycle, true)),
    ];
    for (operation, body) in cases {
        for (field, replacement, expected) in [
            ("Authorization", None, 401),
            (
                "Authorization",
                Some("Bearer wrong-owner-token-0123456789abcdef"),
                401,
            ),
            ("Origin", None, 403),
            ("Origin", Some("http://127.0.0.1:1"), 403),
            ("x-forme-csrf", None, 403),
            ("x-forme-csrf", Some("0"), 403),
        ] {
            let before = daemon.snapshot();
            let mut request_headers = headers(daemon.address);
            request_headers.retain(|(name, _)| *name != field);
            if let Some(value) = replacement {
                request_headers.push((field, value.to_owned()));
            }
            http(
                daemon.address,
                "POST",
                &format!("/v1/data/{operation}"),
                &request_headers,
                Some(&body),
            )
            .unwrap()
            .expect(expected);
            assert_eq!(
                before,
                daemon.snapshot(),
                "{operation} accepted invalid {field}"
            );
        }
    }
    assert_audit(&daemon, &restore, false);
    assert_audit(&daemon, &lifecycle, false);
    // Denied attempts must not burn the legitimate one-shot confirmations.
    daemon
        .post("restore", &apply_request(&restore, false))
        .expect(200);
    daemon
        .post("lifecycle", &apply_request(&lifecycle, true))
        .expect(200);
    daemon.assert_no_model_calls();
}

#[test]
fn c21_exact_restore_verifies_real_files_audit_and_durable_one_shot_without_authority_switch() {
    let mut daemon = Daemon::new();
    let initial = daemon.backup("empty.db");
    daemon.restore_plan("empty.db", "pending.db");
    let manifest = daemon.backup("snapshot.db");
    // Production startup records its ConfigDoctorReport before accepting HTTP.
    assert_eq!(
        manifest["event_count"],
        initial["event_count"].as_u64().unwrap() + 1
    );
    assert_eq!(
        manifest["owner_digest"],
        json!(p::sha256_content_digest(OWNER.as_bytes()))
    );
    let before_verify = daemon.snapshot();
    let verified = daemon
        .post(
            "verify-backup",
            &json!({"schema_version":1,"source":"snapshot.db"}),
        )
        .expect(200)
        .json();
    assert_eq!(verified["manifest"], manifest);
    assert_verified(&verified);
    assert_eq!(before_verify, daemon.snapshot());
    let authority = daemon.get("/v1/runtime/manifest");
    let plan = daemon.restore_plan("snapshot.db", "restored.db");
    assert_audit(&daemon, &plan, false);
    assert!(!daemon.data_file("restored.db").exists());
    let body = apply_request(&plan, false);
    let restored = daemon.post("restore", &body).expect(200).json();
    assert_verified(&restored["Restored"]["verification"]);
    for field in [
        "owner_digest",
        "schema_digest",
        "event_count",
        "last_stream_seq",
    ] {
        assert_eq!(restored["Restored"]["manifest"][field], manifest[field]);
    }
    assert!(daemon.data_file("restored.db").is_file());
    assert!(daemon.data_file("restored.db.manifest.json").is_file());
    assert_audit(&daemon, &plan, true);
    assert_eq!(authority, daemon.get("/v1/runtime/manifest"));
    assert_verified(
        &daemon
            .post(
                "verify-backup",
                &json!({"schema_version":1,"source":"restored.db"}),
            )
            .expect(200)
            .json(),
    );
    daemon.reject("restore", &body, 400);
    // Remove target occupancy as an alternative rejection cause. The original
    // nonce must still be consumed after restarting the actual authority.
    std::fs::rename(
        daemon.data_file("restored.db"),
        daemon.root.join("kept-restored.db"),
    )
    .unwrap();
    std::fs::rename(
        daemon.data_file("restored.db.manifest.json"),
        daemon.root.join("kept-restored.manifest.json"),
    )
    .unwrap();
    let before_restart = daemon.snapshot();
    daemon.restart();
    assert_eq!(before_restart, daemon.snapshot());
    assert_eq!(authority, daemon.get("/v1/runtime/manifest"));
    let rejected = daemon.post("restore", &body).expect(400);
    assert!(rejected.body.contains("already used"), "{}", rejected.body);
    assert_eq!(before_restart, daemon.snapshot());
    assert!(!daemon.data_file("restored.db").exists());
    assert_audit(&daemon, &plan, true);
}

#[test]
fn c23_exact_lifecycle_shreds_real_subject_keys_preserves_other_subject_and_replays_receipt() {
    let mut daemon = Daemon::new();
    daemon.backup("source.db");
    let subject = daemon.restore_plan("source.db", "subject.db");
    let other = daemon.restore_plan("source.db", "other.db");
    let other_events = daemon.events(&control_run(&other));
    let plan = daemon.lifecycle_plan(&subject, "CryptoShred", 0);
    let cleanup = &plan["operation"]["Lifecycle"]["plan"];
    assert!(!cleanup["objects"].as_array().unwrap().is_empty());
    let before_keys = daemon.snapshot().files;
    let body = apply_request(&plan, true);
    let result = daemon.post("lifecycle", &body).expect(200).json();
    let receipt = &result["Lifecycle"];
    assert_eq!(receipt["operation"], "CryptoShred");
    assert_eq!(receipt["scope"], WORKSPACE);
    assert_eq!(receipt["subject_digest"], cleanup["subject_digest"]);
    assert_eq!(receipt["remote_disposition"], "Unknown");
    assert!(receipt["cleaned_projections"]
        .as_array()
        .unwrap()
        .contains(&json!("content")));
    let destroyed = receipt["destroyed_key_digests"].as_array().unwrap();
    assert!(!destroyed.is_empty());
    let after_keys = daemon.snapshot().files;
    for digest in destroyed {
        let digest: p::SchemaDigest = serde_json::from_value(digest.clone()).unwrap();
        assert!(before_keys
            .iter()
            .any(|(path, hash)| path.extension().is_some_and(|s| s == "key")
                && hash.as_ref() == Some(&digest)));
        assert!(!after_keys
            .iter()
            .any(|(path, hash)| path.extension().is_some_and(|s| s == "key")
                && hash.as_ref() == Some(&digest)));
    }
    daemon
        .request(
            "GET",
            &format!("/v1/runs/{}/events", control_run(&subject)),
            None,
        )
        .expect(400);
    assert_eq!(other_events, daemon.events(&control_run(&other)));
    assert_audit(&daemon, &plan, true);
    let aggregate = receipt["aggregate"].as_str().unwrap();
    let events = daemon.events(aggregate);
    assert_eq!(events.len(), 1);
    let p::EventPayload::DataLifecycleApplied(applied) = &events[0].payload else {
        panic!("lifecycle tombstone")
    };
    assert_eq!(applied.expected_version, 0);
    assert_eq!(applied.committed_version, 1);
    assert_eq!(json!(applied.receipt), *receipt);
    let before_replay = daemon.snapshot();
    assert_eq!(daemon.post("lifecycle", &body).expect(200).json(), result);
    assert_eq!(before_replay, daemon.snapshot());
    daemon.restart();
    assert_eq!(before_replay, daemon.snapshot());
    assert_eq!(daemon.post("lifecycle", &body).expect(200).json(), result);
    assert_eq!(before_replay, daemon.snapshot());
    assert_eq!(other_events, daemon.events(&control_run(&other)));
    assert_eq!(events, daemon.events(aggregate));
    daemon.reject("restore", &apply_request(&subject, false), 400);
}

#[test]
fn c21_restore_rejects_forged_nonce_digest_owner_expiry_and_recomputed_unissued_plan() {
    let daemon = Daemon::new();
    daemon.backup("source.db");
    let plan = daemon.restore_plan("source.db", "restored.db");
    let body = apply_request(&plan, false);
    for (pointer, value) in [
        ("/confirmation/nonce", json!("forged-nonce")),
        (
            "/confirmation/plan_digest",
            json!(p::sha256_content_digest(b"forged")),
        ),
        ("/confirmation/approved", json!(false)),
        ("/confirmation/schema_version", json!(0)),
        ("/confirmation/use_by", json!(0)),
        (
            "/confirmation/use_by",
            json!(plan["expires_at"].as_i64().unwrap() + 1),
        ),
        ("/plan/nonce", json!("substituted-plan-nonce")),
        (
            "/plan/digest",
            json!(p::sha256_content_digest(b"forged-plan")),
        ),
        (
            "/plan/owner_digest",
            json!(p::sha256_content_digest(b"owner:other")),
        ),
        ("/plan/schema_version", json!(0)),
        ("/plan/expires_at", json!(0)),
        ("/plan/operation/Restore/target", json!("changed.db")),
        ("/plan/operation/Restore/source", json!("changed-source.db")),
        (
            "/plan/operation/Restore/backup_digest",
            json!(p::sha256_content_digest(b"different-backup")),
        ),
    ] {
        let mut changed = body.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        daemon.reject("restore", &changed, 400);
    }
    // Public digest algorithms are not authentication. Even a correctly
    // recomputed plan plus matching confirmation needs a persisted request.
    for (pointer, value) in [
        ("/nonce", json!("unissued-nonce")),
        (
            "/owner_digest",
            json!(p::sha256_content_digest(b"owner:other")),
        ),
        ("/expires_at", json!(0)),
        ("/operation/Restore/target", json!("changed.db")),
    ] {
        let mut changed = plan.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        refresh_digest(&mut changed);
        daemon.reject("restore", &apply_request(&changed, false), 400);
    }
    assert_audit(&daemon, &plan, false);
    daemon.post("restore", &body).expect(200);
    assert_audit(&daemon, &plan, true);
    daemon.assert_no_model_calls();
}

#[test]
fn c23_lifecycle_rejects_scope_subject_object_version_and_confirmation_forgery() {
    let daemon = Daemon::new();
    daemon.backup("source.db");
    let subject = daemon.restore_plan("source.db", "subject.db");
    let plan = daemon.lifecycle_plan(&subject, "CryptoShred", 0);
    let body = apply_request(&plan, true);
    for (pointer, value) in [
        ("/confirmation/nonce", json!("forged-nonce")),
        (
            "/confirmation/plan_digest",
            json!(p::sha256_content_digest(b"forged")),
        ),
        ("/confirmation/approved", json!(false)),
        ("/confirmation/use_by", json!(0)),
        (
            "/confirmation/use_by",
            json!(plan["expires_at"].as_i64().unwrap() + 1),
        ),
        (
            "/plan/owner_digest",
            json!(p::sha256_content_digest(b"owner:other")),
        ),
        (
            "/plan/operation/Lifecycle/plan/scope",
            json!("workspace:other"),
        ),
        (
            "/plan/operation/Lifecycle/plan/subject_digest",
            json!(p::sha256_content_digest(b"different-subject")),
        ),
        ("/plan/operation/Lifecycle/plan/expected_version", json!(1)),
        ("/plan/operation/Lifecycle/plan/operation", json!("Retain")),
        (
            "/plan/operation/Lifecycle/plan/objects/0/row_key",
            json!("event:other"),
        ),
        (
            "/plan/operation/Lifecycle/plan/objects/0/key_digest",
            json!(p::sha256_content_digest(b"wrong-key")),
        ),
    ] {
        let mut changed = body.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        daemon.reject("lifecycle", &changed, 400);
    }
    for (field, value) in [
        ("scope", json!("workspace:other")),
        (
            "subject_digest",
            json!(p::sha256_content_digest(b"other-subject")),
        ),
        ("expected_version", json!(1)),
    ] {
        let mut changed = plan.clone();
        let cleanup = &mut changed["operation"]["Lifecycle"]["plan"];
        cleanup[field] = value;
        refresh_digest(cleanup);
        refresh_digest(&mut changed);
        daemon.reject("lifecycle", &apply_request(&changed, true), 400);
    }
    let mut outside = lifecycle_prepare(&subject, "CryptoShred", 0);
    outside["scope"] = json!("workspace:other");
    daemon.reject("lifecycle", &outside, 400);
    let mut absent = lifecycle_prepare(&subject, "CryptoShred", 0);
    absent["subject_digest"] = json!(p::sha256_content_digest(b"absent"));
    daemon.reject("lifecycle", &absent, 400);
    assert_audit(&daemon, &plan, false);
    daemon.post("lifecycle", &body).expect(200);
    assert_audit(&daemon, &plan, true);
    daemon.assert_no_model_calls();
}

#[test]
fn c23_stale_lifecycle_version_is_zero_write_and_fresh_exact_plan_can_proceed() {
    let daemon = Daemon::new();
    daemon.backup("source.db");
    let subject = daemon.restore_plan("source.db", "subject.db");
    let first = daemon.lifecycle_plan(&subject, "Retain", 0);
    let stale = daemon.lifecycle_plan(&subject, "Delete", 0);
    daemon
        .post("lifecycle", &apply_request(&first, true))
        .expect(200);
    daemon.reject("lifecycle", &apply_request(&stale, true), 400);
    daemon.reject(
        "lifecycle",
        &lifecycle_prepare(&subject, "CryptoShred", 0),
        400,
    );
    assert_audit(&daemon, &stale, false);
    let fresh = daemon.lifecycle_plan(&subject, "Delete", 1);
    let receipt = daemon
        .post("lifecycle", &apply_request(&fresh, true))
        .expect(200)
        .json();
    let events = daemon.events(receipt["Lifecycle"]["aggregate"].as_str().unwrap());
    assert_eq!(events.len(), 2);
    for (index, event) in events.iter().enumerate() {
        let p::EventPayload::DataLifecycleApplied(payload) = &event.payload else {
            panic!("lifecycle event")
        };
        assert_eq!(payload.expected_version, index as u64);
        assert_eq!(payload.committed_version, index as u64 + 1);
    }
    assert_eq!(receipt["Lifecycle"]["remote_disposition"], "Unknown");
    daemon.assert_no_model_calls();
}

#[test]
fn c23_subject_growth_after_preview_requires_new_cleanup_before_consuming_approval() {
    let daemon = Daemon::new();
    daemon.backup("source.db");
    let subject = daemon.restore_plan("source.db", "subject.db");
    let stale = daemon.lifecycle_plan(&subject, "CryptoShred", 0);
    // Completing the still-pending restore adds two real protected audit rows
    // to this subject. The preview can no longer describe its exact cleanup.
    daemon
        .post("restore", &apply_request(&subject, false))
        .expect(200);
    daemon.reject("lifecycle", &apply_request(&stale, true), 400);
    assert_audit(&daemon, &stale, false);
    let fresh = daemon.lifecycle_plan(&subject, "CryptoShred", 0);
    assert!(
        fresh["operation"]["Lifecycle"]["plan"]["objects"]
            .as_array()
            .unwrap()
            .len()
            > stale["operation"]["Lifecycle"]["plan"]["objects"]
                .as_array()
                .unwrap()
                .len()
    );
    daemon
        .post("lifecycle", &apply_request(&fresh, true))
        .expect(200);
    assert_audit(&daemon, &fresh, true);
    daemon.assert_no_model_calls();
}

#[test]
fn c21_c23_data_dtos_reject_unknown_fields_schema_spoofing_and_cross_operation_plans() {
    let daemon = Daemon::new();
    daemon.backup("source.db");
    let restore = daemon.restore_plan("source.db", "restored.db");
    let subject = daemon.restore_plan("source.db", "subject.db");
    let lifecycle = daemon.lifecycle_plan(&subject, "Retain", 0);
    let cases = [
        ("backup", json!({"schema_version":1,"target":"new.db"})),
        (
            "verify-backup",
            json!({"schema_version":1,"source":"source.db"}),
        ),
        (
            "restore-plan",
            json!({"schema_version":1,"source":"source.db","target":"new.db"}),
        ),
        ("restore", apply_request(&restore, false)),
        ("lifecycle", lifecycle_prepare(&subject, "Retain", 0)),
        ("lifecycle", apply_request(&lifecycle, true)),
    ];
    for (operation, body) in &cases {
        for field in ["owner", "principal", "future_field"] {
            let mut changed = body.clone();
            changed[field] = json!("owner:payload-cannot-authorize");
            daemon.reject(operation, &changed, 422);
        }
        let mut changed = body.clone();
        changed["schema_version"] = json!(99);
        daemon.reject(operation, &changed, 400);
    }
    for (operation, plan, is_lifecycle) in [
        ("restore", &restore, false),
        ("lifecycle", &lifecycle, true),
    ] {
        for pointer in ["/plan", "/confirmation"] {
            let mut changed = apply_request(plan, is_lifecycle);
            changed.pointer_mut(pointer).unwrap()["future_field"] = json!(true);
            daemon.reject(operation, &changed, 422);
        }
        let mut missing = apply_request(plan, is_lifecycle);
        missing.as_object_mut().unwrap().remove("confirmation");
        daemon.reject(operation, &missing, 422);
    }
    for pointer in [
        "/plan/operation/Lifecycle/plan",
        "/plan/operation/Lifecycle/plan/objects/0",
    ] {
        let mut changed = apply_request(&lifecycle, true);
        changed.pointer_mut(pointer).unwrap()["future_field"] = json!(true);
        daemon.reject("lifecycle", &changed, 422);
    }
    let mut nested = apply_request(&restore, false);
    nested["plan"]["operation"]["Restore"]["future_field"] = json!(true);
    daemon.reject("restore", &nested, 422);
    daemon.reject("restore", &apply_request(&lifecycle, false), 400);
    daemon.reject("lifecycle", &apply_request(&restore, true), 400);
    daemon
        .post("restore", &apply_request(&restore, false))
        .expect(200);
    daemon
        .post("lifecycle", &apply_request(&lifecycle, true))
        .expect(200);
    daemon.assert_no_model_calls();
}

#[test]
fn c21_opaque_paths_existing_files_and_stale_targets_reject_without_writes() {
    let daemon = Daemon::new();
    daemon.backup("source.db");
    std::fs::write(daemon.root.join("outside.db"), b"outside path sentinel").unwrap();
    let long = "a".repeat(97);
    let absolute = daemon
        .root
        .join("escaped.db")
        .to_string_lossy()
        .into_owned();
    for name in [
        "",
        ".",
        "..",
        "../outside.db",
        "..\\outside.db",
        "/escaped.db",
        "C:\\escaped.db",
        "nested/escape.db",
        "%2e%2e%2foutside.db",
        &long,
        &absolute,
    ] {
        daemon.reject("backup", &json!({"schema_version":1,"target":name}), 400);
        daemon.reject(
            "verify-backup",
            &json!({"schema_version":1,"source":name}),
            400,
        );
        daemon.reject(
            "restore-plan",
            &json!({"schema_version":1,"source":name,"target":"unused.db"}),
            400,
        );
        daemon.reject(
            "restore-plan",
            &json!({"schema_version":1,"source":"source.db","target":name}),
            400,
        );
    }
    daemon.reject(
        "backup",
        &json!({"schema_version":1,"target":"source.db"}),
        400,
    );
    daemon.reject(
        "restore-plan",
        &json!({"schema_version":1,"source":"source.db","target":"source.db"}),
        400,
    );
    std::fs::create_dir(daemon.data_file("directory")).unwrap();
    daemon.reject(
        "backup",
        &json!({"schema_version":1,"target":"directory"}),
        400,
    );
    let plan = daemon.restore_plan("source.db", "restore-race.db");
    std::fs::write(
        daemon.data_file("restore-race.db"),
        b"must preserve newer file",
    )
    .unwrap();
    daemon.reject("restore", &apply_request(&plan, false), 400);
    assert_audit(&daemon, &plan, false);
    std::fs::rename(
        daemon.data_file("restore-race.db"),
        daemon.root.join("saved-race-sentinel"),
    )
    .unwrap();
    // The target collision was rejected before the one-shot nonce was claimed.
    daemon
        .post("restore", &apply_request(&plan, false))
        .expect(200);
    assert_eq!(
        std::fs::read(daemon.root.join("saved-race-sentinel")).unwrap(),
        b"must preserve newer file"
    );
    assert!(!daemon.root.join("escaped.db").exists());
    daemon.assert_no_model_calls();
}

#[test]
fn c21_tampered_backup_bytes_manifest_and_changed_source_block_restore_without_mutation() {
    let daemon = Daemon::new();
    daemon.backup("source.db");
    let plan = daemon.restore_plan("source.db", "restored.db");
    let database = std::fs::read(daemon.data_file("source.db")).unwrap();
    let manifest_path = daemon.data_file("source.db.manifest.json");
    let manifest = std::fs::read(&manifest_path).unwrap();
    let mut tampered = database.clone();
    tampered.push(1);
    std::fs::write(daemon.data_file("source.db"), tampered).unwrap();
    daemon.reject(
        "verify-backup",
        &json!({"schema_version":1,"source":"source.db"}),
        400,
    );
    daemon.reject(
        "restore-plan",
        &json!({"schema_version":1,"source":"source.db","target":"new.db"}),
        400,
    );
    daemon.reject("restore", &apply_request(&plan, false), 400);
    std::fs::write(daemon.data_file("source.db"), database).unwrap();
    let mut altered: Value = serde_json::from_slice(&manifest).unwrap();
    altered["event_count"] = json!(999);
    std::fs::write(&manifest_path, serde_json::to_vec(&altered).unwrap()).unwrap();
    daemon.reject(
        "verify-backup",
        &json!({"schema_version":1,"source":"source.db"}),
        400,
    );
    daemon.reject("restore", &apply_request(&plan, false), 400);
    std::fs::write(&manifest_path, manifest).unwrap();
    assert_audit(&daemon, &plan, false);
    daemon
        .post("restore", &apply_request(&plan, false))
        .expect(200);
    assert_audit(&daemon, &plan, true);
    daemon.assert_no_model_calls();
}

fn seed_charter(daemon: &Daemon) {
    let mut charter = p::WorkspaceCharterRecord {
        schema_version: p::SchemaVersion(1),
        workspace: p::WorkspaceRef(WORKSPACE.into()),
        version: 1,
        goals: vec![],
        constraints: vec![p::Constraint("max-nodes:2".into())],
        prohibitions: vec![],
        done_contract: None,
        review_cadence: None,
        actor: p::Actor::Owner,
        digest: p::SchemaDigest(String::new()),
    };
    charter.refresh_digest().unwrap();
    daemon
        .request(
            "PUT",
            &format!("/v1/workspaces/{WORKSPACE}/charter"),
            Some(&json!({
                "schema_version":1,"command":"data-dashboard-charter",
                "change":{"charter":charter,"expected_version":0,"committed_version":1}
            })),
        )
        .expect(200);
}

#[test]
fn c20_owner_dashboard_filters_workspace_time_and_replays_identically_after_restart() {
    let mut daemon = Daemon::new();
    let initial = daemon.get("/v1/brain/dashboard");
    seed_charter(&daemon);
    daemon.backup("source.db");
    daemon.restore_plan("source.db", "pending.db");
    let dashboard = daemon.get("/v1/brain/dashboard");
    assert_eq!(dashboard["schema_version"], 1);
    assert_eq!(
        dashboard["owner_digest"],
        json!(p::sha256_content_digest(OWNER.as_bytes()))
    );
    assert_eq!(
        dashboard["event_count"],
        initial["event_count"].as_u64().unwrap() + 2
    );
    assert!(dashboard["cursor"].as_u64().unwrap() > 0);
    assert!(dashboard["digest"].as_str().unwrap().starts_with("sha256:"));
    for metric in [
        "proposal_adoption",
        "ask_to_learn_acceptance",
        "candidate_resolution",
        "verification",
        "model_attempts",
        "model_fallback",
        "failures",
    ] {
        for field in ["numerator", "denominator", "unverifiable"] {
            assert!(dashboard["metrics"][metric][field].as_u64().is_some());
        }
    }
    let scoped_path = format!("/v1/brain/dashboard?workspace={WORKSPACE}");
    let scoped = daemon.get(&scoped_path);
    assert_eq!(scoped["workspace"], WORKSPACE);
    assert_eq!(scoped["event_count"], 1);
    assert_eq!(
        daemon.get("/v1/brain/dashboard?workspace=workspace:other")["event_count"],
        0
    );
    let charter_run = daemon.get("/v1/runtime/manifest")["aggregate_refs"]["charter"]
        .as_str()
        .unwrap()
        .to_owned();
    let timestamp = daemon.events(&charter_run)[0].ts_unix_ms;
    let window_path = format!("{scoped_path}&from={timestamp}&to={timestamp}");
    let window = daemon.get(&window_path);
    assert_eq!(window["event_count"], 1);
    assert_eq!(
        daemon.get(&format!("{scoped_path}&from={}", timestamp + 1))["event_count"],
        0
    );
    assert_eq!(
        daemon.get(&format!("{scoped_path}&to={}", timestamp - 1))["event_count"],
        0
    );
    let before = daemon.snapshot();
    http(daemon.address, "GET", "/v1/brain/dashboard", &[], None)
        .unwrap()
        .expect(401);
    http(
        daemon.address,
        "GET",
        "/v1/brain/dashboard",
        &[("Authorization", "Bearer invalid-token".into())],
        None,
    )
    .unwrap()
    .expect(401);
    // Reads need owner authentication, with no mutation CSRF headers.
    assert_eq!(
        http(
            daemon.address,
            "GET",
            "/v1/brain/dashboard",
            &[auth()],
            None
        )
        .unwrap()
        .expect(200)
        .json(),
        dashboard
    );
    for path in [
        "/v1/brain/dashboard?owner=owner:other",
        "/v1/brain/dashboard?unknown=1",
        "/v1/brain/dashboard?from=20&to=10",
        "/v1/brain/dashboard?from=bad",
        "/v1/brain/dashboard?workspace=",
    ] {
        daemon.request("GET", path, None).expect(400);
        assert_eq!(before, daemon.snapshot());
    }
    let encoded = dashboard.to_string();
    assert!(!encoded.contains(TOKEN));
    assert!(!encoded.contains("unused-data-controls-fixture-key"));
    assert!(!encoded.contains(&daemon.root.to_string_lossy().replace('\\', "\\\\")));
    daemon.restart();
    assert_eq!(before, daemon.snapshot());
    assert_eq!(dashboard, daemon.get("/v1/brain/dashboard"));
    assert_eq!(scoped, daemon.get(&scoped_path));
    assert_eq!(window, daemon.get(&window_path));
}

fn cli(daemon: &Daemon, args: &[&str], input: Option<&Value>, token: &str) -> Output {
    let binary = std::env::var_os("FORME_CLI_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_BIN_EXE_forme-gatewayd"))
                .with_file_name(format!("forme-cli{}", std::env::consts::EXE_SUFFIX))
        });
    assert!(
        binary.is_file(),
        "build forme-cli before running the ignored CLI acceptance case"
    );
    let mut command = Command::new(binary);
    clear_forme_environment(&mut command);
    let mut child = command
        .current_dir(&daemon.root)
        .env("FORME_GATEWAY_URL", format!("http://{}", daemon.address))
        .env("FORME_GATEWAY_TOKEN", token)
        .env("FORME_GATEWAY_TIMEOUT_MS", "15000")
        // A data/brain command must stay a thin client even with a store path
        // in the environment. Opening a second runtime here would create it.
        .env("FORME_STORE_PATH", daemon.root.join("forbidden-cli.db"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(!daemon.root.join("forbidden-cli.db").exists());
    daemon.assert_no_model_calls();
    output
}

fn cli_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI JSON")
}

#[test]
#[ignore = "requires cargo build -p forme-cli; run with --include-ignored"]
fn c20_c21_c23_cli_data_and_brain_use_authenticated_real_daemon_without_local_writer() {
    let daemon = Daemon::new();
    let backup = cli_json(cli(
        &daemon,
        &["data", "backup"],
        Some(&json!({"schema_version":1,"target":"cli.db"})),
        TOKEN,
    ));
    let verify_body = json!({"schema_version":1,"source":"cli.db"}).to_string();
    let verified = cli_json(cli(
        &daemon,
        &["data", "verify-backup", &verify_body],
        None,
        TOKEN,
    ));
    assert_eq!(backup, verified["manifest"]);
    assert_verified(&verified);
    let restore_body = json!({"schema_version":1,"source":"cli.db","target":"cli-restored.db"});
    let plan = cli_json(cli(
        &daemon,
        &["data", "restore-plan"],
        Some(&restore_body),
        TOKEN,
    ));
    let prepare = lifecycle_prepare(&plan, "Retain", 0);
    let lifecycle = cli_json(cli(
        &daemon,
        &["data", "lifecycle", &prepare.to_string()],
        None,
        TOKEN,
    ));
    let lifecycle_result = cli_json(cli(
        &daemon,
        &["data", "lifecycle"],
        Some(&apply_request(&lifecycle, true)),
        TOKEN,
    ));
    assert_eq!(lifecycle_result["Lifecycle"]["operation"], "Retain");
    let before = daemon.snapshot();
    let wrong = cli(
        &daemon,
        &["data", "restore"],
        Some(&apply_request(&plan, false)),
        "wrong-owner-token-0123456789abcdef0123456789",
    );
    assert!(!wrong.status.success());
    assert_eq!(before, daemon.snapshot());
    let restored = cli_json(cli(
        &daemon,
        &["data", "restore"],
        Some(&apply_request(&plan, false)),
        TOKEN,
    ));
    assert_verified(&restored["Restored"]["verification"]);
    assert_audit(&daemon, &plan, true);
    assert_audit(&daemon, &lifecycle, true);
    let before = daemon.snapshot();
    assert_eq!(
        cli_json(cli(&daemon, &["brain", "dashboard"], None, TOKEN)),
        before.dashboard
    );
    let scoped = cli_json(cli(
        &daemon,
        &["brain", "dashboard", WORKSPACE],
        None,
        TOKEN,
    ));
    assert_eq!(
        scoped,
        daemon.get(&format!("/v1/brain/dashboard?workspace={WORKSPACE}"))
    );
    for args in [
        vec!["data", "not-an-operation", "{}"],
        vec!["data", "backup", "invalid-json"],
        vec![
            "data",
            "backup",
            "{\"schema_version\":1,\"target\":\"x.db\",\"owner\":\"other\"}",
        ],
    ] {
        assert!(!cli(&daemon, &args, None, TOKEN).status.success());
        assert_eq!(before, daemon.snapshot());
    }
}
