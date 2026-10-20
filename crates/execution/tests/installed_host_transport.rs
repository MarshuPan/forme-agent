//! Real loopback transport tests; no service registration or service mutation.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forme_protocol as p;

const TOKEN: &str = "repository-owned-host-transport-token-0123456789";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Daemon {
    child: Child,
    root: PathBuf,
    address: String,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
impl Daemon {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-host-http-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_forme-hostd"))
            .env("FORME_HOST_BIND", "127.0.0.1:0")
            .env("FORME_HOST_JOURNAL_PATH", root.join("host.jsonl"))
            .env("FORME_INSTALL_ID", "http-contract-install")
            .env("FORME_ACTIVE_RELEASE", "0.1.0")
            .env(
                "FORME_PRODUCT_TARGET",
                if cfg!(windows) {
                    "windows_x86_64"
                } else if cfg!(target_os = "macos") {
                    "macos_arm64"
                } else {
                    "ubuntu_2404_x86_64"
                },
            )
            .env("FORME_HOST_TOKEN", TOKEN)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(output).read_line(&mut line);
            let _ = send.send(line);
        });
        let mut daemon = Self {
            child,
            root,
            address: String::new(),
        };
        let line = receive
            .recv_timeout(Duration::from_secs(15))
            .expect("broker did not start");
        reader.join().unwrap();
        daemon.address = line
            .trim()
            .strip_prefix("FORME_HOST_URL=http://")
            .expect("broker not ready")
            .into();
        daemon
    }
    fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: &str,
    ) -> (u16, serde_json::Value) {
        let mut stream = TcpStream::connect(&self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let auth = token
            .map(|value| format!("Authorization: Bearer {value}\r\n"))
            .unwrap_or_default();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", self.address, body.len()).unwrap();
        let mut bytes = String::new();
        stream.read_to_string(&mut bytes).unwrap();
        let (head, body) = bytes.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        assert!(head.contains("Cache-Control: no-store"));
        (status, serde_json::from_str(body).unwrap())
    }
}

#[test]
fn http_broker_authenticates_and_reuses_the_original_read_only_operation() {
    let mut daemon = Daemon::start();
    assert_eq!(daemon.request("GET", "/healthz", None, "").0, 401);
    assert_eq!(
        daemon
            .request("GET", "/v1/host/status", Some("wrong-token"), "")
            .0,
        401
    );
    assert_eq!(daemon.request("GET", "/healthz", Some(TOKEN), "").0, 200);
    let deadline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
        + 30_000;
    let request = p::HostManagementRequest {
        schema_version: p::SchemaVersion(1),
        command: p::MaintenanceCommand {
            schema_version: p::SchemaVersion(1),
            operation_id: "http-inspect".into(),
            idempotency_key: "http-inspect-key".into(),
            expected_revision: 1,
            actor_proof: p::MaintenanceProofRef {
                schema_version: p::SchemaVersion(1),
                proof_ref: p::canonical_digest(&TOKEN).unwrap().0,
            },
            kind: p::MaintenanceKind::Inspect,
            requested_deadline: deadline,
        },
        references: p::MaintenanceReferences {
            schema_version: p::SchemaVersion(1),
            original_release_ref: "0.1.0".into(),
            target_release_ref: "0.1.0".into(),
            backup_ref: None,
            data_candidate_ref: None,
        },
    };
    assert_eq!(
        daemon
            .request("POST", "/v1/host/plan", Some(TOKEN), "{broken")
            .0,
        400
    );
    let body = serde_json::to_string(&request).unwrap();
    let planned = daemon.request("POST", "/v1/host/plan", Some(TOKEN), &body);
    assert_eq!(planned.0, 200);
    assert_eq!(
        planned,
        daemon.request("POST", "/v1/host/plan", Some(TOKEN), &body)
    );
    let committed = daemon.request(
        "POST",
        "/v1/host/operations/http-inspect/execute",
        Some(TOKEN),
        "",
    );
    assert_eq!(committed.0, 200);
    assert_eq!(committed.1["stage"], "committed");
    assert_eq!(
        committed,
        daemon.request(
            "POST",
            "/v1/host/operations/http-inspect/execute",
            Some(TOKEN),
            ""
        )
    );
    // Windows file locks intentionally prevent raw reads while the writer lives.
    daemon.child.kill().unwrap();
    daemon.child.wait().unwrap();
    assert!(!std::fs::read_to_string(daemon.root.join("host.jsonl"))
        .unwrap()
        .contains(TOKEN));
}
