use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn cli_question_reaches_http_model_and_prints_the_answer() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let request = read_request(&mut stream);
        assert!(request.starts_with("POST /v1/chat/completions HTTP/"));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer local-e2e-secret"));
        assert!(request.contains("\"model\":\"local-e2e-model\""));
        let body = r#"{"choices":[{"message":{"content":"reactive e2e answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        stream.flush().unwrap();
        let _ = stream.shutdown(Shutdown::Both);
    });

    let tag = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let store = std::env::temp_dir().join(format!("forme-cli-e2e-{tag}.db"));
    let output = Command::new(env!("CARGO_BIN_EXE_forme-cli"))
        .arg("Does the reactive path work?")
        .env("FORME_MODEL_BASE_URL", format!("http://{address}/v1"))
        .env("FORME_MODEL_NAME", "local-e2e-model")
        .env("FORME_MODEL_API_KEY", "local-e2e-secret")
        .env("FORME_MODEL_TIMEOUT_MS", "5000")
        .env("FORME_STORE_PATH", &store)
        .output()
        .unwrap();
    server.join().unwrap();
    let _ = std::fs::remove_file(store);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "reactive e2e answer"
    );
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4_096];
    let mut expected = None;
    loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "client closed before sending a complete request");
        bytes.extend_from_slice(&buffer[..read]);
        if expected.is_none() {
            if let Some(header_end) = find_bytes(&bytes, b"\r\n\r\n") {
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

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
