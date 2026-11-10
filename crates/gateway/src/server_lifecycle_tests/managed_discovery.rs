//! Exercise real HTTP admission against the production runtime.
use super::*;

fn discovery() -> serde_json::Value {
    serde_json::json!({
        "schema_version":1,"session":"session:discovery","command":"command:discovery",
        "spec":{"schema_version":1,"server":"fixture","program_ref":"program:fixture",
            "registration_digest":p::sha256_content_digest(b"registration"),
            "settings_digest":p::sha256_content_digest(b"settings"),
            "timeout":1000,"output_budget_bytes":8192}
    })
}

async fn post(address: SocketAddr, path: &str, body: &str, auth: u8) -> u16 {
    let path = path.to_owned();
    let body = body.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let headers = match auth {
            0 => String::new(),
            1 => "Authorization: Bearer wrong-token\r\n".to_owned(),
            2 => format!("Authorization: Bearer {TOKEN}\r\nOrigin: http://{address}\r\nx-forme-csrf: wrong\r\n"),
            3 => format!("Authorization: Bearer {TOKEN}\r\nOrigin: http://external.invalid\r\nx-forme-csrf: 1\r\n"),
            _ => format!("Authorization: Bearer {TOKEN}\r\nOrigin: http://{address}\r\nx-forme-csrf: 1\r\n"),
        };
        write!(stream, "POST {path} HTTP/1.1\r\nHost: {address}\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response.split_whitespace().nth(1).unwrap().parse().unwrap()
    }).await.unwrap()
}

#[tokio::test]
async fn discovery_http_rejects_untrusted_requests_without_runs_or_model_calls() {
    let fixture = Fixture::new();
    let runtime = fixture.build();
    let before = GatewayControl::list_runs(runtime.harness().as_ref()).unwrap();
    let (server, stop, ready) = start(&fixture, runtime.clone(), Duration::from_secs(3));
    let address = ready.await.unwrap();
    let paths = [
        ("/v1/managed/mcp/discovery", discovery()),
        (
            "/v1/managed/mcp/discovery/run:original/publish",
            serde_json::json!({"schema_version":1,"command":"command:publish"}),
        ),
    ];
    for (path, body) in paths {
        let wire = body.to_string();
        for (auth, expected) in [(0, 401), (1, 401), (2, 403), (3, 403), (4, 400)] {
            assert_eq!(post(address, path, &wire, auth).await, expected);
        }
        let mut forged = body.clone();
        forged["owner"] = "owner:forged".into();
        assert_eq!(post(address, path, &forged.to_string(), 4).await, 422);
        let mut version = body;
        version["schema_version"] = 99.into();
        assert_eq!(post(address, path, &version.to_string(), 4).await, 400);
        assert_eq!(post(address, path, "{", 4).await, 400);
        assert_eq!(post(address, path, &"x".repeat(8193), 4).await, 413);
    }
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        GatewayControl::list_runs(runtime.harness().as_ref()).unwrap(),
        before
    );
    stop.send(()).unwrap();
    server.await.unwrap().unwrap();
}
