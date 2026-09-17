#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use forme_protocol as p;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme-m5-registryd: {}", error.0);
        std::process::exit(1);
    }
}

fn run() -> p::Result<()> {
    let bind = required("FORME_M5_REGISTRY_BIND")?;
    let package_path = PathBuf::from(required("FORME_M5_REGISTRY_PACKAGE_PATH")?);
    let count_path = PathBuf::from(required("FORME_M5_REGISTRY_COUNT_PATH")?);
    let package = std::fs::read(&package_path)
        .map_err(|_| p::Error("registry package file cannot be read".into()))?;
    let decoded: p::SignedCapabilityPackage = serde_json::from_slice(&package)
        .map_err(|_| p::Error("registry package file is malformed".into()))?;
    decoded.validate()?;
    let listener = TcpListener::bind(&bind)
        .map_err(|_| p::Error("registry loopback listener cannot be bound".into()))?;
    if !listener
        .local_addr()
        .map_err(|_| p::Error("registry listener address is unavailable".into()))?
        .ip()
        .is_loopback()
    {
        return Err(p::Error("registry listener must be loopback-only".into()));
    }
    write_count(&count_path, 0)?;
    if let Ok(path) = std::env::var("FORME_M5_REGISTRY_READY_PATH") {
        std::fs::write(path, b"ready")
            .map_err(|_| p::Error("registry ready marker cannot be written".into()))?;
    }
    let requests = AtomicU64::new(0);
    for stream in listener.incoming() {
        let mut stream =
            stream.map_err(|_| p::Error("registry connection could not be accepted".into()))?;
        serve(&mut stream, &package, &count_path, &requests)?;
    }
    Ok(())
}

fn serve(
    stream: &mut TcpStream,
    package: &[u8],
    count_path: &Path,
    requests: &AtomicU64,
) -> p::Result<()> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .map_err(|_| p::Error("registry connection timeout cannot be set".into()))?;
    let mut request = [0_u8; 8_192];
    let read = stream
        .read(&mut request)
        .map_err(|_| p::Error("registry request cannot be read".into()))?;
    let head = std::str::from_utf8(&request[..read])
        .map_err(|_| p::Error("registry request is not UTF-8".into()))?;
    let target = head
        .lines()
        .next()
        .and_then(|line| {
            let mut parts = line.split_whitespace();
            (parts.next() == Some("GET"))
                .then(|| parts.next())
                .flatten()
        })
        .unwrap_or("");
    let (status, body) = if target == "/package" {
        let count = requests.fetch_add(1, Ordering::SeqCst).saturating_add(1);
        write_count(count_path, count)?;
        ("200 OK", package)
    } else {
        ("404 Not Found", b"not found".as_slice())
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .and_then(|_| stream.write_all(body))
        .and_then(|_| stream.flush())
        .map_err(|_| p::Error("registry response cannot be written".into()))
}

fn write_count(path: &Path, count: u64) -> p::Result<()> {
    std::fs::write(path, count.to_string())
        .map_err(|_| p::Error("registry request count cannot be persisted".into()))
}

fn required(name: &str) -> p::Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| p::Error(format!("required configuration {name} is missing")))
}
