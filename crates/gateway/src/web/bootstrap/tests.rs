//! Protocol-boundary tests, with synthetic credentials and local sockets only.
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forme-bootstrap-client-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn credential(&self) -> PathBuf {
        let path = self.0.join("credential");
        std::fs::write(&path, b"fixture-bootstrap-transport-credential-1").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let actual = self.0.canonicalize().unwrap();
        assert_eq!(
            actual.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("forme-bootstrap-client-"));
        assert!(!std::fs::symlink_metadata(&self.0)
            .unwrap()
            .file_type()
            .is_symlink());
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn private_setup_transport_rejects_lan_and_header_injection_before_connecting() {
    let directory = Directory::new();
    let path = directory.credential();
    for address in ["0.0.0.0:8766", "192.0.2.1:8766", "127.0.0.1:0"] {
        assert_eq!(
            LocalBootstrapClient::connect(address.parse().unwrap(), &path)
                .err()
                .unwrap()
                .0,
            Code::InvalidInput
        );
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = LocalBootstrapClient::connect(listener.local_addr().unwrap(), &path).unwrap();
    assert_eq!(
        client
            .issue_proof("proof:one\r\nInjected: value")
            .err()
            .unwrap()
            .0,
        Code::InvalidInput
    );
    let mut request = p::BootstrapReserveRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: "claim:one\r\nInjected: value".into(),
        proof: "0".repeat(64),
        setup_digest: p::canonical_digest(&"fixture").unwrap(),
    };
    assert_eq!(
        client.reserve(&request).err().unwrap().0,
        Code::InvalidInput
    );
    request.operation_id = "claim:one".into();
    assert_eq!(
        listener.accept().err().unwrap().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn setup_http_framing_rejects_ambiguity_truncation_and_chunking() {
    for bytes in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}".as_slice(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\n\r\n{}",
        b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n{}",
        b"HTTP/1.1 200 OK\r\n\r\n{}",
        b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\n{}",
    ] {
        assert!(response(bytes).is_err());
    }
    assert_eq!(
        response(b"HTTP/1.1 409 Conflict\r\nContent-Length: 2\r\n\r\n{}").unwrap(),
        (409, b"{}".as_slice())
    );
}

#[test]
fn setup_transport_preserves_typed_errors_and_rejects_unknown_reply_versions() {
    let directory = Directory::new();
    let path = directory.credential();
    for (status, body, expected) in [
        (
            409,
            r#"{"schema_version":1,"code":"conflict"}"#,
            Code::Conflict,
        ),
        (
            200,
            r#"{"schema_version":2,"proof":"fixture","expires_at":42}"#,
            Code::InvalidInput,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = LocalBootstrapClient::connect(listener.local_addr().unwrap(), &path).unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 8192];
            assert!(stream.read(&mut request).unwrap() > 0);
            write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        assert_eq!(
            client.issue_proof("proof:fixture").err().unwrap().0,
            expected
        );
        worker.join().unwrap();
    }
}
