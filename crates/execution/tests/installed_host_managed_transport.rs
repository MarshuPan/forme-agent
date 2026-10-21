//! Managed daemon is exercised without registering services or owner data.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const MAINTENANCE: &str = "repository-managed-maintenance-credential-0001";
const AGENT: &str = "repository-managed-agent-credential-000000002";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    child: Option<Child>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forme-managed-host-http-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self { root, child: None }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_forme-hostd"));
        command
            .args([
                std::ffi::OsStr::new("--config"),
                self.root.join("host.json").as_os_str(),
            ])
            // Poison legacy inputs: managed startup must not use any of them.
            .env("FORME_HOST_BIND", "not-a-socket")
            .env("FORME_AGENT_EXECUTION_BIND", "not-a-socket")
            .env("FORME_HOST_TOKEN", "wrong-environment-credential")
            .env("FORME_AGENT_EXECUTION_TOKEN", "wrong-agent-credential")
            .env(
                "FORME_HOST_JOURNAL_PATH",
                self.root.join("environment-must-not-exist.jsonl"),
            )
            .env(
                "FORME_WORKER_CONFIG_PATH",
                self.root.join("missing-worker.json"),
            )
            .env(
                "FORME_BOOTSTRAP_CONFIG_PATH",
                self.root.join("missing-bootstrap.json"),
            )
            .env("FORME_INSTALL_ID", "environment-install")
            .env("FORME_PRODUCT_TARGET", "unsupported-environment-target")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command
    }
    fn configure(&self) {
        std::fs::create_dir_all(self.root.join("host-state")).unwrap();
        for (name, value) in [
            ("keys/host-maintenance.token", MAINTENANCE),
            ("keys/agent-execution.token", AGENT),
        ] {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let path = self.root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            options
                .open(path)
                .unwrap()
                .write_all(value.as_bytes())
                .unwrap();
        }
        // Keep both reservations alive until their ports have been selected.
        let one = TcpListener::bind("127.0.0.1:0").unwrap();
        let two = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = if cfg!(windows) {
            "windows_x86_64"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "macos_arm64"
            } else {
                "macos_x86_64"
            }
        } else if cfg!(target_arch = "aarch64") {
            "ubuntu_2404_arm64"
        } else {
            "ubuntu_2404_x86_64"
        };
        let doc = serde_json::json!({
            "schema_version": 1, "install_ref": "managed-process-test", "active_release": "0.0.1",
            "product_target": target,
            "installation_manifest_path": self.root.join("config/installation.json"),
            "journal_path": self.root.join("host-state/host.journal"),
            "maintenance_credential_path": self.root.join("keys/host-maintenance.token"),
            "agent_execution_credential_path": self.root.join("keys/agent-execution.token"),
            "host_bind": one.local_addr().unwrap(), "agent_execution_bind": two.local_addr().unwrap(),
        });
        let manifest = serde_json::json!({
            "schema_version": 1,
            "install_id": "managed-process-test",
            "target": target,
            "release_id": "0.0.1",
            "release_digest": format!("sha256:{}", "b".repeat(64)),
            "paths": {
                "schema_version": 1,
                "releases": self.root.join("releases"),
                "config": self.root.join("config"),
                "brain": self.root.join("brain"),
                "web_auth": self.root.join("web-auth"),
                "host_state": self.root.join("host-state"),
                "keys": self.root.join("keys"),
                "logs": self.root.join("logs"),
                "backups": self.root.join("backups"),
                "workspaces": self.root.join("workspaces"),
                "ipc": self.root.join("ipc")
            },
            "services": [
                {"schema_version": 1, "service": "host", "account": if cfg!(windows) {"LocalSystem"} else {"root"}},
                {"schema_version": 1, "service": "web", "account": if cfg!(windows) {"NT SERVICE\\forme-web"} else {"forme-web"}},
                {"schema_version": 1, "service": "brain", "account": if cfg!(windows) {"NT SERVICE\\forme-brain"} else {"forme-brain"}}
            ],
            "created_at": 1
        });
        std::fs::create_dir_all(self.root.join("config")).unwrap();
        std::fs::write(
            self.root.join("config/installation.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::write(
            self.root.join("host.json"),
            serde_json::to_vec(&doc).unwrap(),
        )
        .unwrap();
    }

    fn start(&mut self) -> Vec<String> {
        self.child = Some(self.command().spawn().unwrap());
        let output = self.child.as_mut().unwrap().stdout.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let lines: Vec<_> = BufReader::new(output)
                .lines()
                .take(2)
                .collect::<Result<_, _>>()
                .unwrap();
            let _ = send.send(lines);
        });
        receive
            .recv_timeout(Duration::from_secs(15))
            .expect("managed host did not settle")
    }

    fn stop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn status(address: SocketAddr, path: &str, token: &str) -> u16 {
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(socket, "GET {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).unwrap();
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn managed_host_uses_file_config_and_keeps_both_credential_domains_separate() {
    let mut fixture = Fixture::new();
    fixture.configure();
    let lines = fixture.start();
    assert_eq!(lines.len(), 2);
    let host: SocketAddr = lines[0]
        .strip_prefix("FORME_HOST_URL=http://")
        .unwrap()
        .parse()
        .unwrap();
    let agent: SocketAddr = lines[1]
        .strip_prefix("FORME_AGENT_EXECUTION_URL=http://")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(status(host, "/healthz", MAINTENANCE), 200);
    assert_eq!(
        status(host, "/healthz", "wrong-environment-credential"),
        401
    );
    assert_eq!(status(host, "/healthz", AGENT), 401);
    // Health belongs to maintenance, even a correct agent token cannot cross.
    assert_eq!(status(agent, "/healthz", AGENT), 401);
    let probe = "/v1/agent/workers/managed-test-missing";
    assert_eq!(status(agent, probe, AGENT), 400); // authenticated; missing job
    assert_eq!(status(agent, probe, MAINTENANCE), 401);
    assert!(fixture.root.join("host-state/host.journal").is_file());
    assert!(!fixture
        .root
        .join("environment-must-not-exist.jsonl")
        .exists());
}

#[test]
fn managed_host_binds_installation_verification_once_and_rejects_manifest_replacement() {
    let mut fixture = Fixture::new();
    fixture.configure();
    assert_eq!(fixture.start().len(), 2);
    fixture.stop();
    let journal = std::fs::read_to_string(fixture.root.join("host-state/host.journal")).unwrap();
    assert_eq!(
        journal
            .matches("\"record\":\"installation_verified\"")
            .count(),
        1
    );

    assert_eq!(fixture.start().len(), 2);
    fixture.stop();
    let journal = std::fs::read_to_string(fixture.root.join("host-state/host.journal")).unwrap();
    assert_eq!(
        journal
            .matches("\"record\":\"installation_verified\"")
            .count(),
        1
    );

    let manifest_path = fixture.root.join("config/installation.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["release_digest"] = serde_json::json!(format!("sha256:{}", "c".repeat(64)));
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(fixture.start().is_empty());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = fixture.child.as_mut().unwrap().try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "replaced manifest did not fail closed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("host-state/host.journal"))
            .unwrap()
            .matches("\"record\":\"installation_verified\"")
            .count(),
        1
    );
}

#[test]
fn invalid_managed_host_config_fails_without_legacy_fallback_or_journal() {
    let mut fixture = Fixture::new();
    std::fs::write(fixture.root.join("host.json"), br#"{"schema_version":999}"#).unwrap();
    fixture.child = Some(fixture.command().spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = fixture.child.as_mut().unwrap().try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "invalid managed host did not exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!fixture.root.join("host.jsonl").exists());
    assert!(!fixture
        .root
        .join("environment-must-not-exist.jsonl")
        .exists());
}

#[test]
fn managed_host_subconfig_errors_have_zero_management_or_worker_facts() {
    for field in ["worker_config_path", "bootstrap_config_path"] {
        let mut fixture = Fixture::new();
        fixture.configure();
        let path = fixture.root.join("host.json");
        let mut doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        doc[field] = serde_json::json!(fixture.root.join("broken-subconfig.json"));
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        std::fs::write(fixture.root.join("broken-subconfig.json"), b"{}").unwrap();
        fixture.child = Some(fixture.command().spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = fixture.child.as_mut().unwrap().try_wait().unwrap() {
                assert!(!status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "invalid managed reference did not exit"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        for name in [
            "host-state/host.journal",
            "host.worker.jsonl",
            "environment-must-not-exist.jsonl",
        ] {
            assert!(
                !fixture.root.join(name).exists(),
                "created {name} for invalid {field}"
            );
        }
    }
}
