//! Real hostd setup RPC, restart and credential-domain checks. No OS mutation.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use forme_execution::installed::bootstrap::BootstrapAuthorityConfig;
use forme_protocol as p;

const ADMIN: &str = "fixture-bootstrap-http-administration-credential-1";
const WEB: &str = "fixture-bootstrap-http-web-service-credential-1";
const AGENT: &str = "fixture-bootstrap-http-agent-execution-credential-1";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forme-claim-http-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        assert_eq!(
            self.0.canonicalize().unwrap().parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("forme-claim-http-"));
        assert!(!std::fs::symlink_metadata(&self.0)
            .unwrap()
            .file_type()
            .is_symlink());
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

struct Daemon {
    child: Child,
    address: String,
    agent: String,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn private_write(path: &std::path::Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn prepare(directory: &Directory) {
    let root = &directory.0;
    private_write(&root.join("issuer.key"), &[117; 32]);
    private_write(&root.join("web.credential"), WEB.as_bytes());
    std::fs::create_dir(root.join("new-brain")).unwrap();
    let config = BootstrapAuthorityConfig {
        schema_version: p::SchemaVersion(1),
        install_ref: "installation:setup-http".into(),
        journal_path: root.join("claim.journal"),
        signing_key_path: root.join("issuer.key"),
        web_credential_path: root.join("web.credential"),
        new_brain_directory: root.join("new-brain"),
        candidate_ref: "data:new-http".into(),
    };
    private_write(
        &root.join("setup.json"),
        &serde_json::to_vec(&config).unwrap(),
    );
}

impl Daemon {
    fn start(directory: &Directory) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forme-hostd"));
        command.env_clear();
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
        let mut child = command
            .env("TEMP", &directory.0)
            .env("TMP", &directory.0)
            .env("FORME_HOST_BIND", "127.0.0.1:0")
            .env("FORME_AGENT_EXECUTION_BIND", "127.0.0.1:0")
            .env("FORME_HOST_JOURNAL_PATH", directory.0.join("host.journal"))
            .env(
                "FORME_BOOTSTRAP_CONFIG_PATH",
                directory.0.join("setup.json"),
            )
            .env("FORME_HOST_TOKEN", ADMIN)
            .env("FORME_AGENT_EXECUTION_TOKEN", AGENT)
            .env("FORME_INSTALL_ID", "installation:setup-http")
            .env("FORME_ACTIVE_RELEASE", "0.1.0")
            .env(
                "FORME_PRODUCT_TARGET",
                if cfg!(windows) {
                    "windows_x86_64"
                } else {
                    "ubuntu_2404_x86_64"
                },
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            let mut lines = Vec::new();
            for _ in 0..2 {
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                lines.push(line);
            }
            let _ = send.send(lines);
        });
        let mut daemon = Self {
            child,
            address: String::new(),
            agent: String::new(),
        };
        let lines = receive
            .recv_timeout(Duration::from_secs(15))
            .expect("setup broker failed to start");
        reader.join().unwrap();
        daemon.address = lines[0]
            .trim()
            .strip_prefix("FORME_HOST_URL=http://")
            .unwrap()
            .into();
        daemon.agent = lines[1]
            .trim()
            .strip_prefix("FORME_AGENT_EXECUTION_URL=http://")
            .unwrap()
            .into();
        daemon
    }

    fn post(
        &self,
        agent: bool,
        path: &str,
        token: &str,
        key: &str,
        body: &serde_json::Value,
    ) -> (u16, serde_json::Value) {
        let address = if agent { &self.agent } else { &self.address };
        let body = serde_json::to_vec(body).unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(stream, "POST {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nIdempotency-Key: {key}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
        stream.write_all(&body).unwrap();
        let mut response = Vec::new();
        stream
            .take(64 * 1024 + 1)
            .read_to_end(&mut response)
            .unwrap();
        assert!(response.len() <= 64 * 1024);
        let index = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let header = std::str::from_utf8(&response[..index]).unwrap();
        let code = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        assert!(header.contains("Cache-Control: no-store"));
        (
            code,
            serde_json::from_slice(&response[index + 4..]).unwrap(),
        )
    }
}

fn binding() -> p::BootstrapSessionBinding {
    p::BootstrapSessionBinding {
        schema_version: p::SchemaVersion(1),
        operation_id: "claim:http".into(),
        setup_digest: p::canonical_digest(&"fixture setup cookie digest").unwrap(),
    }
}

#[test]
fn broker_setup_rpc_enforces_three_distinct_credential_scopes_and_zero_replay() {
    let directory = Directory::new();
    prepare(&directory);
    let daemon = Daemon::start(&directory);
    let rotation = serde_json::json!({"schema_version":1,"operation_id":"proof:http"});
    for (agent, token) in [(false, WEB), (false, AGENT), (true, ADMIN), (true, WEB)] {
        assert_eq!(
            daemon
                .post(
                    agent,
                    "/v1/host/bootstrap/proof",
                    token,
                    "proof:http",
                    &rotation
                )
                .0,
            401
        );
    }
    let (status, proof) = daemon.post(
        false,
        "/v1/host/bootstrap/proof",
        ADMIN,
        "proof:http",
        &rotation,
    );
    assert_eq!(status, 200);
    let before = std::fs::metadata(directory.0.join("claim.journal"))
        .unwrap()
        .len();
    assert_eq!(
        daemon
            .post(
                false,
                "/v1/host/bootstrap/proof",
                ADMIN,
                "proof:http",
                &rotation
            )
            .0,
        409
    );
    assert_eq!(
        std::fs::metadata(directory.0.join("claim.journal"))
            .unwrap()
            .len(),
        before
    );
    let session = binding();
    let reserve = serde_json::json!({"schema_version":1,"operation_id":session.operation_id,
        "setup_digest":session.setup_digest,"proof":proof["proof"]});
    assert_eq!(
        daemon
            .post(false, "/v1/setup/reserve", ADMIN, "claim:http", &reserve)
            .0,
        401
    );
    assert_eq!(
        daemon
            .post(true, "/v1/setup/reserve", WEB, "claim:http", &reserve)
            .0,
        401
    );
    assert_eq!(
        daemon
            .post(false, "/v1/setup/reserve", WEB, "claim:http", &reserve)
            .0,
        200
    );
    let mut replacement = reserve.clone();
    replacement["operation_id"] = "claim:another".into();
    assert_eq!(
        daemon
            .post(
                false,
                "/v1/setup/reserve",
                WEB,
                "claim:another",
                &replacement
            )
            .0,
        409
    );
    drop(daemon);
    let log = std::fs::read_to_string(directory.0.join("claim.journal")).unwrap();
    assert!(!log.contains(proof["proof"].as_str().unwrap()));
    assert!(!log.contains(ADMIN) && !log.contains(WEB) && !log.contains(AGENT));
}

#[test]
fn killed_broker_recovers_the_original_committed_claim_without_creating_a_brain() {
    let directory = Directory::new();
    prepare(&directory);
    let daemon = Daemon::start(&directory);
    let (_, proof) = daemon.post(
        false,
        "/v1/host/bootstrap/proof",
        ADMIN,
        "proof:restart",
        &serde_json::json!({"schema_version":1,"operation_id":"proof:restart"}),
    );
    let session = binding();
    let reserve = serde_json::json!({"schema_version":1,"operation_id":session.operation_id,
        "setup_digest":session.setup_digest,"proof":proof["proof"]});
    assert_eq!(
        daemon
            .post(false, "/v1/setup/reserve", WEB, "claim:http", &reserve)
            .0,
        200
    );
    let selection =
        serde_json::json!({"schema_version":1,"session":session,"candidate_ref":"data:new-http"});
    assert_eq!(
        daemon
            .post(false, "/v1/setup/select", WEB, "claim:http", &selection)
            .0,
        200
    );
    let request = serde_json::json!({"schema_version":1,"session":session,"candidate_ref":"data:new-http",
        "account_commitment":p::canonical_digest(&"fixture prepared account").unwrap()});
    let (status, original) = daemon.post(false, "/v1/setup/commit", WEB, "claim:http", &request);
    assert_eq!(status, 200);
    let before = std::fs::metadata(directory.0.join("claim.journal"))
        .unwrap()
        .len();
    drop(daemon); // hard termination, deliberately without a graceful shutdown.
    let daemon = Daemon::start(&directory);
    let (status, recovered) = daemon.post(false, "/v1/setup/commit", WEB, "claim:http", &request);
    assert_eq!(status, 200);
    assert_eq!(original["receipt"]["owner"], recovered["receipt"]["owner"]);
    assert_eq!(
        original["receipt"]["account_commitment"],
        recovered["receipt"]["account_commitment"]
    );
    assert_eq!(
        std::fs::metadata(directory.0.join("claim.journal"))
            .unwrap()
            .len(),
        before
    );
    assert_eq!(
        std::fs::read_dir(directory.0.join("new-brain"))
            .unwrap()
            .count(),
        0
    );
    let grant: p::SignedWebClaimCommit = serde_json::from_value(recovered).unwrap();
    let message = p::canonical_digest(&("forme-web-claim-commit-v1", &grant.receipt)).unwrap();
    ed25519_dalek::SigningKey::from_bytes(&[117; 32])
        .verifying_key()
        .verify_strict(
            message.0.as_bytes(),
            &ed25519_dalek::Signature::from_slice(&grant.signature).unwrap(),
        )
        .unwrap();
}
