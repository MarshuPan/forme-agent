//! V1 closure A (C1/C5): the CLI's default path is the running gateway
//! daemon, and an embedded CLI composes the identical runtime manifest.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_protocol as p;

const TOKEN: &str = "cli-gateway-entry-token-0123456789abcdef0123456789";
const ANSWER: &str = "gateway entry answer";

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct ModelServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<usize>>,
}

impl ModelServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let handle = std::thread::spawn(move || {
            let mut calls = 0;
            while !stop_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let request = read_message(&mut stream);
                        assert!(request.starts_with("POST /v1/chat/completions HTTP/"));
                        calls += 1;
                        let body = format!(
                            r#"{{"choices":[{{"message":{{"content":"{ANSWER}"}},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":7,"completion_tokens":3}}}}"#
                        );
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        )
                        .unwrap();
                        stream.flush().unwrap();
                        let _ = stream.shutdown(Shutdown::Both);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("model fixture accept failed: {error}"),
                }
            }
            calls
        });
        Self {
            address,
            stop,
            handle: Some(handle),
        }
    }

    fn finish(mut self) -> usize {
        self.stop.store(true, Ordering::SeqCst);
        self.handle.take().unwrap().join().unwrap()
    }
}

fn runtime_environment(command: &mut Command, model: SocketAddr, store: &Path) {
    command
        .env("FORME_MODEL_BASE_URL", format!("http://{model}/v1"))
        .env("FORME_MODEL_NAME", "cli-gateway-model")
        .env("FORME_MODEL_API_KEY", "cli-gateway-model-secret")
        .env("FORME_MODEL_TIMEOUT_MS", "5000")
        .env("FORME_STORE_PATH", store)
        .env("FORME_OWNER_ID", "owner:cli-gateway")
        .env("FORME_SCHEDULER_ENABLED", "false")
        .env("FORME_COGNITIVE_TICK_ENABLED", "false")
        // A short lease lets the test observe expiry after the daemon is
        // killed; the runtime heartbeat keeps it alive while the daemon runs.
        .env("FORME_BRAIN_LEASE_MS", "1500")
        .env_remove("FORME_GATEWAY_URL")
        .env_remove("FORME_CAPABILITY_CONFIG")
        .env_remove("FORME_MODEL_PROFILES_JSON");
}

fn gatewayd_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("FORME_GATEWAYD_BIN") {
        return PathBuf::from(path);
    }
    Path::new(env!("CARGO_BIN_EXE_forme-cli"))
        .with_file_name(format!("forme-gatewayd{}", std::env::consts::EXE_SUFFIX))
}

fn free_address() -> SocketAddr {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    address
}

fn temp_path(label: &str) -> PathBuf {
    let tag = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("forme-cli-gateway-{label}-{tag}.db"))
}

fn remove_sqlite(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

fn start_gateway(model: SocketAddr, store: &Path, address: SocketAddr) -> ChildGuard {
    let binary = gatewayd_binary();
    assert!(
        binary.is_file(),
        "forme-gatewayd must be built alongside the CLI (cargo test --workspace) or named by FORME_GATEWAYD_BIN"
    );
    let mut command = Command::new(binary);
    runtime_environment(&mut command, model, store);
    // The guard owns the child from the moment it exists, so every exit path
    // kills and reaps it.
    let mut child = ChildGuard(
        command
            .env("FORME_GATEWAY_BIND", "127.0.0.1")
            .env("FORME_GATEWAY_PORT", address.port().to_string())
            .env("FORME_GATEWAY_TOKEN", TOKEN)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(status) = child.0.try_wait().unwrap() {
            let mut stderr = String::new();
            if let Some(mut stream) = child.0.stderr.take() {
                let _ = stream.read_to_string(&mut stderr);
            }
            panic!("gateway exited before readiness ({status}): {stderr}");
        }
        if let Ok((200, _)) = get(address, "/healthz") {
            return child;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("gateway did not become ready");
}

fn get(address: SocketAddr, path: &str) -> std::io::Result<(u16, String)> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(200))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {TOKEN}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    Ok((status, body))
}

fn cli(configure: impl FnOnce(&mut Command)) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_forme-cli"));
    configure(&mut command);
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn cli_defaults_to_the_gateway_daemon_and_shares_its_single_runtime() {
    let model = ModelServer::start();
    let store = temp_path("daemon");
    let address = free_address();
    let url = format!("http://{address}");
    let gateway = start_gateway(model.address, &store, address);

    let (status, manifest) = get(address, "/v1/runtime/manifest").unwrap();
    assert_eq!(status, 200);
    let manifest = serde_json::from_str::<serde_json::Value>(&manifest).unwrap();
    let daemon_digest = manifest["digest"].as_str().unwrap().to_owned();

    let unused_store = temp_path("never-opened");
    let via_gateway = |arguments: &[&str]| {
        cli(|command| {
            command
                .args(arguments)
                .env("FORME_GATEWAY_URL", &url)
                .env("FORME_GATEWAY_TOKEN", TOKEN)
                .env("FORME_OWNER_ID", "owner:cli-gateway")
                .env("FORME_STORE_PATH", &unused_store);
        })
    };
    let reported = via_gateway(&["--manifest"]);
    assert!(reported.status.success(), "stderr: {}", stderr(&reported));
    assert_eq!(stdout(&reported), daemon_digest);

    let answered = via_gateway(&["--session", "cli:entry", "Is the daemon path live?"]);
    assert!(answered.status.success(), "stderr: {}", stderr(&answered));
    assert_eq!(stdout(&answered), ANSWER);
    assert!(
        !unused_store.exists(),
        "the default CLI path must never open an authority store"
    );

    // A second process composing the same brain is fenced by the writer
    // lease: the daemon's runtime stays the only authority.
    let competing = cli(|command| {
        runtime_environment(command, model.address, &store);
        command.arg("--embedded").arg("Can I write too?");
    });
    assert!(!competing.status.success());
    assert!(
        stderr(&competing).contains("held by another runtime"),
        "stderr: {}",
        stderr(&competing)
    );

    let (status, runs) = get(address, "/v1/runs").unwrap();
    assert_eq!(status, 200);
    let runs = serde_json::from_str::<Vec<p::RunSummary>>(&runs).unwrap();
    let cli_run = runs
        .iter()
        .find(|summary| summary.session == p::SessionId("cli:entry".into()))
        .expect("the CLI run is visible through the daemon");
    assert_eq!(cli_run.source, p::Source::UserTurn);
    let (status, events) = get(address, &format!("/v1/runs/{}/events", cli_run.run.0)).unwrap();
    assert_eq!(status, 200);
    let bound = events
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|json| serde_json::from_str::<p::Event>(json).ok())
        .find_map(|event| match event.payload {
            p::EventPayload::SessionBound(payload) => Some(payload),
            _ => None,
        })
        .expect("the CLI run is bound by the daemon harness");
    assert_eq!(
        bound.composition_manifest,
        Some(p::SchemaDigest(daemon_digest.clone()))
    );

    drop(gateway);
    assert_eq!(model.finish(), 1, "exactly one model call served the CLI");
    // A killed daemon cannot release its lease; the next runtime may claim
    // the brain only after the lease has expired.
    std::thread::sleep(Duration::from_millis(1_800));

    // With the daemon stopped, the explicit embedded mode composes the same
    // runtime through the same builder and reports the same manifest.
    let embedded = cli(|command| {
        runtime_environment(command, "127.0.0.1:9".parse().unwrap(), &store);
        command.arg("--embedded").arg("--manifest");
    });
    assert!(embedded.status.success(), "stderr: {}", stderr(&embedded));
    let embedded_digest = stdout(&embedded);
    // The model endpoint is not part of the manifest; only its safe profile
    // reference is, so the digests match across processes.
    assert_eq!(embedded_digest, daemon_digest);
    remove_sqlite(&store);
}

#[test]
fn cli_without_a_configured_gateway_fails_closed_with_guidance() {
    let store = temp_path("no-gateway");
    let output = cli(|command| {
        runtime_environment(command, "127.0.0.1:9".parse().unwrap(), &store);
        command.arg("Is anyone there?");
    });
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("--embedded"),
        "stderr: {}",
        stderr(&output)
    );
    assert!(!store.exists(), "failing closed must not create a store");
}

fn read_message(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4_096];
    let mut expected = None;
    loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "client closed before sending a complete request");
        bytes.extend_from_slice(&buffer[..read]);
        if expected.is_none() {
            if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::trim)
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                expected = Some(header_end + 4 + content_length);
            }
        }
        if expected.is_some_and(|length| bytes.len() >= length) {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}
