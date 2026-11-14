//! Real authenticated HTTP with a slow or disconnected controlled Host.
use super::*;
use std::net::TcpListener;

struct SlowHost {
    client: Arc<crate::web::configuration::LocalConfigurationClient>,
    received: mpsc::Receiver<Value>,
    thread: Option<std::thread::JoinHandle<()>>,
    _root: tempfile::TempDir,
}

impl SlowHost {
    fn new(reply_delay: Option<Duration>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let token = root.path().join("configuration-token");
        std::fs::write(&token, "maintenance-transport-fixture-token-123456").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (send, received) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let limit = std::time::Instant::now() + Duration::from_secs(45);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(accepted) => break accepted,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < limit,
                            "Host request did not arrive"
                        );
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("Host accept failed: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut wire = Vec::new();
            let request = loop {
                let mut bytes = [0; 4096];
                let count = stream.read(&mut bytes).unwrap();
                assert!(count > 0 && wire.len() + count < 8192);
                wire.extend_from_slice(&bytes[..count]);
                if let Some(end) = wire.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&wire[..end]).unwrap();
                    assert!(headers.starts_with("POST /v1/host/configuration/maintenance HTTP/1.1"));
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("Content-Length: ")
                                .map(|v| v.parse().unwrap())
                        })
                        .unwrap();
                    if wire.len() == end + 4 + length {
                        break serde_json::from_slice::<Value>(&wire[end + 4..]).unwrap();
                    }
                }
            };
            send.send(request.clone()).unwrap();
            if let Some(delay) = reply_delay {
                std::thread::sleep(delay);
                let request: p::ProductMaintenanceRequest =
                    serde_json::from_value(request).unwrap();
                let response = json!({
                    "schema_version":1, "operation_id":request.operation_id,
                    "command_digest":request.digest().unwrap(), "revision":104,
                    "stage":"validated", "pending_stage":"quiescing",
                    "original_release_ref":"source-fixture", "target_release_ref":"target-fixture",
                    "data_candidate_ref":null, "backup_ref":"backup:fixture", "receipts":[], "failure_ref":null
                }).to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                    response.len()
                );
            }
            drop(stream);
            listener.set_nonblocking(true).unwrap();
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        });
        Self {
            client: Arc::new(
                crate::web::configuration::LocalConfigurationClient::connect(address, &token)
                    .unwrap(),
            ),
            received,
            thread: Some(thread),
            _root: root,
        }
    }
}

impl Drop for SlowHost {
    fn drop(&mut self) {
        self.thread.take().unwrap().join().unwrap();
    }
}

fn submit(fixture: &Fixture, cookie: &str, csrf: &str, command: &Value) -> (u16, Value) {
    let body = command.to_string();
    let mut stream = TcpStream::connect(fixture.address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(45)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream, "POST /api/v1/product/maintenance HTTP/1.1\r\nHost: {}\r\nOrigin: {}\r\nCookie: {cookie}\r\nX-Forme-Csrf: {csrf}\r\nIdempotency-Key: maintenance:slow-fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", fixture.address, fixture.origin(), body.len()).unwrap();
    let mut wire = String::new();
    stream.read_to_string(&mut wire).unwrap();
    let (head, body) = wire.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn command() -> Value {
    json!({"schema_version":1,"operation_id":"maintenance:slow-fixture","expected_revision":99,
        "requested_deadline":now()+120_000,"password":PASSWORD,
        "action":{"kind":"upgrade","release_id":"target-fixture","release_digest":p::sha256_content_digest(b"release"),"backup_ref":"backup:fixture"}})
}

#[test]
fn slow_release_preflight_returns_original_receipt_after_old_http_timeout() {
    let host = SlowHost::new(Some(Duration::from_secs(31)));
    let fixture = Fixture::with_bridges(true, None, Some(host.client.clone()));
    let (cookie, csrf) = fixture.login();
    let body = command();
    let (status, response) = submit(&fixture, &cookie, &csrf, &body);
    let mut expected = body.clone();
    expected.as_object_mut().unwrap().remove("password");
    expected["owner"] = "owner:http-fixture".into();
    assert_eq!(
        host.received.recv_timeout(Duration::from_secs(2)).unwrap(),
        expected
    );
    assert_eq!(status, 200);
    assert_eq!(response["operation_id"], body["operation_id"]);
    assert_eq!(response["stage"], "validated");
    assert_eq!(response["pending_stage"], "quiescing");
}

#[test]
fn disconnected_release_is_unknown_and_bad_password_never_reaches_host() {
    let host = SlowHost::new(None);
    let fixture = Fixture::with_bridges(true, None, Some(host.client.clone()));
    let (cookie, csrf) = fixture.login();
    let body = command();
    let mut wrong = body.clone();
    wrong["password"] = "incorrect fixture password".into();
    assert_eq!(submit(&fixture, &cookie, &csrf, &wrong).0, 401);
    assert!(matches!(
        host.received.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let (status, response) = submit(&fixture, &cookie, &csrf, &body);
    assert_eq!(status, 502);
    assert_eq!(response["code"], "ConfigurationRejected");
    assert_eq!(
        host.received.recv_timeout(Duration::from_secs(2)).unwrap()["operation_id"],
        body["operation_id"]
    );
}
