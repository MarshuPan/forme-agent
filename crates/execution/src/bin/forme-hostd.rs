#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use forme_execution::installed::{HostManagement, ProductHostBroker};
use forme_protocol as p;

const MAX_REQUEST: usize = 1_048_576;
const MAX_HEADERS: usize = 16_384;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-hostd: {}", error.0);
        std::process::exit(1);
    }
}

fn run() -> p::Result<()> {
    let bind = std::env::var("FORME_HOST_BIND").unwrap_or_else(|_| "127.0.0.1:8766".into());
    let address: SocketAddr = bind
        .parse()
        .map_err(|_| p::Error("host broker requires a numeric loopback address".into()))?;
    if !address.ip().is_loopback() {
        return Err(p::Error("host broker must remain loopback-only".into()));
    }
    let listener = TcpListener::bind(address)
        .map_err(|_| p::Error("host broker could not bind its private endpoint".into()))?;
    let journal = required_path("FORME_HOST_JOURNAL_PATH")?;
    let install_id = required("FORME_INSTALL_ID")?;
    let token = load_or_create_token()?
        .ok_or_else(|| p::Error("host management token is unavailable".into()))?;
    let target = target_from_environment()?;
    let active_release =
        std::env::var("FORME_ACTIVE_RELEASE").unwrap_or_else(|_| "unconfigured".into());
    let broker = Arc::new(ProductHostBroker::open(
        &journal,
        &install_id,
        token,
        target,
        active_release,
    )?);
    println!(
        "FORME_HOST_URL=http://{}",
        listener
            .local_addr()
            .map_err(|_| p::Error("host broker endpoint is unavailable".into()))?
    );
    if let Some(path) = token_path() {
        println!("FORME_HOST_TOKEN_PATH={}", path.display());
    }
    let _ = std::io::stdout().flush();
    let connections = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if connections.fetch_add(1, Ordering::AcqRel) >= 16 {
                    connections.fetch_sub(1, Ordering::AcqRel);
                    drop(stream);
                    continue;
                }
                let broker = broker.clone();
                let connections = connections.clone();
                std::thread::spawn(move || {
                    struct ConnectionGuard(Arc<AtomicUsize>);
                    impl Drop for ConnectionGuard {
                        fn drop(&mut self) {
                            self.0.fetch_sub(1, Ordering::AcqRel);
                        }
                    }
                    let _guard = ConnectionGuard(connections);
                    let _ = handle_connection(stream, broker);
                });
            }
            Err(_) => return Err(p::Error("host broker accept failed".into())),
        }
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, broker: Arc<ProductHostBroker>) -> p::Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .and_then(|_| stream.set_write_timeout(Some(Duration::from_secs(10))))
        .map_err(|_| p::Error("host broker socket limits could not be applied".into()))?;
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            write_error(&mut stream, 400, error)?;
            return Ok(());
        }
    };
    let bearer = match request
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        Some(value) => value,
        None => {
            write_error(
                &mut stream,
                401,
                p::Error("host broker authorization is required".into()),
            )?;
            return Ok(());
        }
    };
    let auth = match broker.authenticate(bearer) {
        Ok(auth) => auth,
        Err(error) => {
            write_error(&mut stream, 401, error)?;
            return Ok(());
        }
    };
    match route(&request, broker.as_ref(), &auth) {
        Ok((status, body)) => write_response(&mut stream, status, &body),
        Err(error) => {
            write_error(&mut stream, 400, error)?;
            Ok(())
        }
    }
}

fn route(
    request: &Request,
    broker: &ProductHostBroker,
    auth: &forme_execution::installed::HostAuth,
) -> p::Result<(u16, Vec<u8>)> {
    let body = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/healthz") => serde_json::to_vec(&serde_json::json!({"status":"ok"}))
            .map_err(|_| p::Error("host health response failed".into()))?,
        ("GET", "/v1/host/status") => serde_json::to_vec(&broker.status(auth)?)
            .map_err(|_| p::Error("host status response failed".into()))?,
        ("POST", "/v1/host/plan") => {
            let request: p::HostManagementRequest = serde_json::from_slice(&request.body)
                .map_err(|_| p::Error("host command body is invalid".into()))?;
            let operation = broker.plan(auth, request, now_ms())?;
            serde_json::to_vec(&p::HostOperationResponse {
                schema_version: p::SchemaVersion(1),
                operation,
                accepted: true,
            })
            .map_err(|_| p::Error("host operation response failed".into()))?
        }
        ("GET", path) if path.starts_with("/v1/host/operations/") => {
            let id = path.trim_start_matches("/v1/host/operations/");
            serde_json::to_vec(&broker.operation(auth, id)?)
                .map_err(|_| p::Error("host operation response failed".into()))?
        }
        ("POST", path)
            if path.starts_with("/v1/host/operations/") && path.ends_with("/execute") =>
        {
            let id = path
                .trim_start_matches("/v1/host/operations/")
                .trim_end_matches("/execute");
            serde_json::to_vec(&broker.execute(auth, id, now_ms())?)
                .map_err(|_| p::Error("host execution response failed".into()))?
        }
        _ => return Ok((404, b"{}".to_vec())),
    };
    Ok((200, body))
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn read_request(stream: &mut impl Read) -> p::Result<Request> {
    let mut bytes = Vec::with_capacity(8192);
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let count = stream
            .read(&mut chunk)
            .map_err(|_| p::Error("host request could not be read".into()))?;
        if count == 0 {
            return Err(p::Error("host request ended before headers".into()));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_REQUEST {
            return Err(p::Error("host request exceeds its size bound".into()));
        }
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if index > MAX_HEADERS {
                return Err(p::Error(
                    "host request headers exceed their size bound".into(),
                ));
            }
            break index;
        }
        if bytes.len() > MAX_HEADERS {
            return Err(p::Error(
                "host request headers exceed their size bound".into(),
            ));
        }
    };
    let mut raw_headers = [httparse::EMPTY_HEADER; 48];
    let mut parsed = httparse::Request::new(&mut raw_headers);
    if !parsed
        .parse(&bytes[..header_end + 4])
        .map_err(|_| p::Error("host request headers are invalid".into()))?
        .is_complete()
    {
        return Err(p::Error("host request headers are incomplete".into()));
    }
    let method = parsed.method.unwrap_or_default().to_owned();
    let path = parsed.path.unwrap_or_default().to_owned();
    if parsed.version != Some(1)
        || !matches!(method.as_str(), "GET" | "POST")
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['?', '#', '%'])
    {
        return Err(p::Error("host request line is not supported".into()));
    }
    let mut headers = Vec::new();
    for header in parsed.headers.iter() {
        let name = header.name.to_ascii_lowercase();
        let value = std::str::from_utf8(header.value)
            .map_err(|_| p::Error("host request header is invalid".into()))?
            .trim();
        if headers.iter().any(|(key, _)| key == &name)
            || matches!(
                name.as_str(),
                "transfer-encoding" | "origin" | "forwarded" | "x-forwarded-host"
            )
        {
            return Err(p::Error(
                "host request contains ambiguous or browser headers".into(),
            ));
        }
        headers.push((name, value.to_owned()));
    }
    let length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.parse::<usize>())
        .transpose()
        .map_err(|_| p::Error("host request content length is invalid".into()))?
        .unwrap_or(0);
    if length > MAX_REQUEST || header_end + 4 + length > MAX_REQUEST {
        return Err(p::Error("host request body exceeds its size bound".into()));
    }
    let body_start = header_end + 4;
    if bytes.len() > body_start + length {
        return Err(p::Error(
            "host endpoint accepts only one exact request".into(),
        ));
    }
    while bytes.len() < body_start + length {
        let mut chunk = [0_u8; 4096];
        let remaining = (body_start + length - bytes.len()).min(chunk.len());
        let count = stream
            .read(&mut chunk[..remaining])
            .map_err(|_| p::Error("host request body could not be read".into()))?;
        if count == 0 {
            return Err(p::Error("host request body ended early".into()));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(Request {
        method,
        path,
        headers,
        body: bytes[body_start..body_start + length].to_vec(),
    })
}

fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) -> p::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|_| p::Error("host response could not be written".into()))
}

fn write_error(stream: &mut TcpStream, status: u16, error: p::Error) -> p::Result<()> {
    let body = serde_json::to_vec(&serde_json::json!({
        "error": "host_request_rejected",
        "message": error.0,
    }))
    .map_err(|_| p::Error("host error response failed".into()))?;
    write_response(stream, status, &body)
}

fn required(key: &str) -> p::Result<String> {
    std::env::var(key).map_err(|_| p::Error(format!("required configuration {key} is missing")))
}

fn required_path(key: &str) -> p::Result<PathBuf> {
    let path = PathBuf::from(required(key)?);
    if !path.is_absolute() {
        return Err(p::Error(format!("{key} must be an absolute path")));
    }
    Ok(path)
}

fn token_path() -> Option<PathBuf> {
    std::env::var_os("FORME_HOST_TOKEN_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("FORME_HOST_STATE_ROOT")
                .map(|root| PathBuf::from(root).join("host.token"))
        })
}

fn load_or_create_token() -> p::Result<Option<String>> {
    if let Ok(token) = std::env::var("FORME_HOST_TOKEN") {
        return Ok(Some(token));
    }
    let Some(path) = token_path() else {
        return Ok(None);
    };
    forme_execution::installed::reject_linked_ancestors(&path)?;
    match std::fs::read_to_string(&path) {
        Ok(token) => return Ok(Some(token.trim().to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            return Err(p::Error(
                "host token cannot be read; refusing to replace it".into(),
            ))
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| p::Error("host state directory cannot be created".into()))?;
    }
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| p::Error("host token entropy is unavailable".into()))?;
    let token = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|_| p::Error("host token could not be stored".into()))?;
    file.write_all(token.as_bytes())
        .map_err(|_| p::Error("host token could not be stored".into()))?;
    file.write_all(b"\n")
        .map_err(|_| p::Error("host token could not be stored".into()))?;
    file.sync_all()
        .map_err(|_| p::Error("host token could not be synced".into()))?;
    Ok(Some(token))
}

fn target_from_environment() -> p::Result<p::ProductTarget> {
    let value = std::env::var("FORME_PRODUCT_TARGET").or_else(|_| {
        if cfg!(all(windows, target_arch = "x86_64")) {
            Ok("windows_x86_64".into())
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Ok("macos_arm64".into())
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Ok("macos_x86_64".into())
        } else {
            Err(p::Error(
                "FORME_PRODUCT_TARGET must identify the verified native target".into(),
            ))
        }
    })?;
    match value.as_str() {
        "windows_x86_64" => Ok(p::ProductTarget::WindowsX86_64),
        "macos_arm64" => Ok(p::ProductTarget::MacosArm64),
        "macos_x86_64" => Ok(p::ProductTarget::MacosX86_64),
        "ubuntu_2404_x86_64" => Ok(p::ProductTarget::Ubuntu2404X86_64),
        "ubuntu_2404_arm64" => Ok(p::ProductTarget::Ubuntu2404Arm64),
        "rocky9_x86_64" => Ok(p::ProductTarget::Rocky9X86_64),
        _ => Err(p::Error("FORME_PRODUCT_TARGET is unsupported".into())),
    }
}

fn now_ms() -> p::Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_http11_is_parsed_and_ambiguous_or_browser_requests_are_rejected() {
        let mut request = std::io::Cursor::new(b"GET /v1/host/status HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer example\r\n\r\n");
        let parsed = read_request(&mut request).unwrap();
        assert_eq!(parsed.path, "/v1/host/status");
        for request in [
            "POST /v1/host/plan HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
            "POST /v1/host/plan HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
            "GET /healthz HTTP/1.1\r\nOrigin: http://example.invalid\r\n\r\n",
            "GET /healthz HTTP/1.1\r\n\r\nextra",
        ] {
            assert!(read_request(&mut std::io::Cursor::new(request)).is_err());
        }
    }
}
