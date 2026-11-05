//! Real Host HTTP handler over sockets; deliberately no native collector.
use super::super::{http::handle_connection, EndpointDomain};
use forme_execution::installed::{
    HostCredentials, ProductHostBroker, SystemServiceManager, WorkerExecutorHost,
};
use forme_protocol as p;
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    time::Duration,
};

const ADMIN: &str = "legacy-route-admin-fixture-0123456789012345678";
const WEB: &str = "legacy-route-web-fixture-012345678901234567890";
const AGENT: &str = "legacy-route-agent-fixture-0123456789012345678";
struct Fixture {
    root: tempfile::TempDir,
    host: Arc<ProductHostBroker>,
    workers: Arc<WorkerExecutorHost>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let credentials = HostCredentials::new(ADMIN.into(), Some(AGENT.into()))
            .unwrap()
            .with_web_configuration(Some(WEB.into()))
            .unwrap();
        let host = Arc::new(
            ProductHostBroker::open_with_service_manager(
                &root.path().join("host.jsonl"),
                "install:legacy-http",
                credentials,
                p::ProductTarget::WindowsX86_64,
                "1.0.0".into(),
                SystemServiceManager,
            )
            .unwrap(),
        );
        let workers = Arc::new(
            WorkerExecutorHost::open(
                &root.path().join("workers.jsonl"),
                p::canonical_digest(&AGENT).unwrap(),
                p::AuthorityEpoch(1),
                BTreeSet::from([p::Scope("workspace:legacy-route".into())]),
            )
            .unwrap(),
        );
        Self {
            root,
            host,
            workers,
        }
    }
    fn bytes(&self) -> (u64, u64) {
        (
            std::fs::metadata(self.root.path().join("host.jsonl"))
                .unwrap()
                .len(),
            std::fs::metadata(self.root.path().join("workers.jsonl"))
                .unwrap()
                .len(),
        )
    }
    fn request(
        &self,
        domain: EndpointDomain,
        token: &str,
        method: &str,
        suffix: &str,
        body: &str,
    ) -> (u16, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = self.host.clone();
        let worker = self.workers.clone();
        let thread = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_connection(stream, host, worker, None, domain).unwrap();
        });
        let mut client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(client, "{method} /v1/host/configuration/legacy-release-recovery{suffix} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        thread.join().unwrap();
        let (head, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(head.contains("Cache-Control: no-store"));
        (
            head.split_whitespace().nth(1).unwrap().parse().unwrap(),
            body.into(),
        )
    }
}

#[test]
fn legacy_host_http_domains_reject_cross_credentials_and_unavailable_dependencies_without_facts() {
    let f = Fixture::new();
    let before = f.bytes();
    let hash = p::sha256_content_digest(b"not-a-native-report");
    let request = serde_json::json!({"schema_version":1,"operation_id":"legacy:http","idempotency_key":"legacy:http",
        "expected_revision":1,"owner":"owner:http","review_id":"review:http","review_digest":hash,"requested_deadline":1});
    let verification = serde_json::json!({"schema_version":1,"operation_id":"legacy:http","request_digest":hash,
        "expected_revision":1,"owner":"owner:http","requested_deadline":1});
    for (method, suffix, body) in [
        ("GET", "/review/upgrade:original", String::new()),
        ("GET", "/operations", String::new()),
        ("GET", "/operations/legacy:http", String::new()),
        ("POST", "", request.to_string()),
        ("POST", "/verify", verification.to_string()),
    ] {
        for (domain, credential) in [
            (EndpointDomain::Maintenance, ADMIN),
            (EndpointDomain::AgentExecution, AGENT),
            (EndpointDomain::Configuration, ADMIN),
            (EndpointDomain::Configuration, AGENT),
        ] {
            assert_eq!(f.request(domain, credential, method, suffix, &body).0, 401);
            assert_eq!(f.bytes(), before);
        }
        let (status, body) = f.request(EndpointDomain::Configuration, WEB, method, suffix, &body);
        assert!(
            [400, 409].contains(&status),
            "must reach the typed unavailable facade, not 404/200: {status}"
        );
        assert!(!body.contains(WEB));
        assert!(!body.contains(ADMIN));
        assert_eq!(f.bytes(), before);
    }
}
