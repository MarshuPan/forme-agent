//! Bounded loopback peer checks bytes sent through the real configuration client.
use super::*;
use crate::web::configuration::LocalConfigurationClient;
use std::{
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Instant,
};

struct Peer {
    client: Arc<LocalConfigurationClient>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    root: PathBuf,
}
impl Peer {
    fn new(command: p::ManagedMcpProgramCommand, wrong_receipt: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let root = std::env::temp_dir().join(format!(
            "forme-mcp-peer-{}-{}",
            std::process::id(),
            address.port()
        ));
        std::fs::create_dir(&root).unwrap();
        let credential = root.join("configuration.key");
        std::fs::write(&credential, "W".repeat(32)).unwrap();
        let client = Arc::new(LocalConfigurationClient::connect(address, &credential).unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(60);
            while !stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("peer accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let request = read_request(&mut stream);
                assert!(request.contains(&format!("Authorization: Bearer {}\r\n", "W".repeat(32))));
                assert!(!request.contains(PASSWORD));
                let path = request.split_whitespace().nth(1).unwrap();
                let body = if path == "/v1/host/configuration" {
                    serde_json::to_value(snapshot()).unwrap()
                } else if path == "/v1/host/configuration/mcp/programs" {
                    json!({"schema_version":1,"installation_ref":command.installation_ref,
                        "owner_ref":command.owner_ref,"authority_epoch":3,"revision":1,
                        "choices":[],"programs":[]})
                } else {
                    assert!(path == "/v1/host/configuration/mcp/programs/commands"
                        || path == "/v1/host/configuration/mcp/programs/operations/register:http");
                    if path.ends_with("/commands") {
                        let actual: p::ManagedMcpProgramCommand = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
                        assert_eq!(actual, command);
                    }
                    let digest = p::sha256_content_digest(b"registered-http-program");
                    json!({"schema_version":1,"operation_id":command.operation_id,
                        "command_digest":if wrong_receipt {p::sha256_content_digest(b"wrong-command")} else {command.digest().unwrap()},
                        "revision":2,"program":{"schema_version":1,"program_ref":format!("mcp-program:{}", &digest.0[7..]),
                        "registration_digest":digest,"source_ref":"source:http","scope":"workspace:http",
                        "sandbox_profile_ref":p::MANAGED_MCP_PROGRAM_PROFILE,"revoked":false}})
                }.to_string();
                captured.lock().unwrap().push(request);
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
            }
        });
        Self {
            client,
            requests,
            stop,
            thread: Some(thread),
            root,
        }
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let result = self.thread.take().unwrap().join();
        std::fs::remove_dir_all(&self.root).unwrap();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn read_request(stream: &mut TcpStream) -> String {
    let mut wire = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline);
        let mut bytes = [0; 4096];
        let count = stream.read(&mut bytes).unwrap();
        assert!(count > 0);
        wire.extend_from_slice(&bytes[..count]);
        assert!(wire.len() <= 16384);
        if let Some(end) = wire.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let header = std::str::from_utf8(&wire[..end]).unwrap();
            let length: usize = header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .parse()
                .unwrap();
            if wire.len() == end + 4 + length {
                return String::from_utf8(wire).unwrap();
            }
            assert!(wire.len() < end + 4 + length);
        }
    }
}
fn snapshot() -> p::ProductConfigView {
    p::ProductConfigView {
        schema_version: p::SchemaVersion(1),
        install_ref: "installation:http-fixture".into(),
        owner: Some(p::OwnerPrincipalRef("owner:http-fixture".into())),
        active_revision: 1,
        acknowledged_revision: None,
        pending_restart: false,
        configured: false,
        models: vec![],
        brain: p::ProductBrainPreferences {
            schema_version: p::SchemaVersion(1),
            default_workspace: p::WorkspaceRef("workspace:http".into()),
            model_routing: Default::default(),
        },
        brain_source: Default::default(),
        capabilities: Default::default(),
        capabilities_source: Default::default(),
        network: p::ProductNetworkConfig {
            schema_version: p::SchemaVersion(1),
            bind: "127.0.0.1:3847".parse().unwrap(),
            allowed_hosts: vec!["localhost".into()],
            tls: None,
            firewall: None,
        },
        network_active: false,
        network_source: Default::default(),
        availability: None,
        issue_refs: vec![],
    }
}

#[test]
fn mcp_program_http_forwards_exact_command_without_password_and_rejects_substituted_receipt() {
    for wrong_receipt in [false, true] {
        let command = command();
        let peer = Peer::new(command.clone(), wrong_receipt);
        let f = Fixture::with_bridges(true, None, Some(peer.client.clone()));
        let (cookie, csrf) = f.login();
        let headers = [
            ("Origin", f.origin()),
            ("Cookie", cookie),
            ("X-Forme-Csrf", csrf),
            ("Idempotency-Key", command.operation_id.clone()),
        ];
        let root = "/api/v1/product/mcp/programs";
        assert_eq!(f.request("GET", root, &headers, "").status, 200);
        let body = json!({"schema_version":1,"password":PASSWORD,"command":command}).to_string();
        let reply = f.request("POST", &format!("{root}/commands"), &headers, &body);
        assert_eq!(reply.status, if wrong_receipt { 502 } else { 200 });
        if !wrong_receipt {
            let read = f.request(
                "GET",
                &format!("{root}/operations/register:http"),
                &headers,
                "",
            );
            assert_eq!(read.status, 200);
            assert_eq!(read.json(), reply.json());
        }
        let requests = peer.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.starts_with("POST "))
                .count(),
            1
        );
        assert_eq!(requests.len(), if wrong_receipt { 4 } else { 6 });
    }
}
