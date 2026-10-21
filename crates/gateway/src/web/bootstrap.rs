//! Typed private bootstrap transport. No generic proxy, model or brain access.
#[cfg(test)]
mod tests;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use forme_protocol as p;
use serde::{de::DeserializeOwned, Serialize};
use zeroize::Zeroizing;

const LIMIT: usize = 64 * 1024;
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapClientError(pub p::BootstrapProblemCode);
impl std::fmt::Display for BootstrapClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "installation setup unavailable: {:?}", self.0)
    }
}
impl std::error::Error for BootstrapClientError {}
type Result<T> = std::result::Result<T, BootstrapClientError>;
use p::BootstrapProblemCode as Code;

/// The installer decides which credential file each process may read. The
/// server, not this client type, enforces management versus setup authority.
pub struct LocalBootstrapClient {
    address: SocketAddr,
    credential: Zeroizing<String>,
}

impl LocalBootstrapClient {
    pub fn connect(address: SocketAddr, credential_path: &Path) -> Result<Self> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(BootstrapClientError(Code::InvalidInput));
        }
        super::config::check_path(credential_path)
            .map_err(|_| BootstrapClientError(Code::InvalidInput))?;
        let file = std::fs::File::open(credential_path)
            .map_err(|_| BootstrapClientError(Code::Unavailable))?;
        let metadata = file
            .metadata()
            .map_err(|_| BootstrapClientError(Code::Unavailable))?;
        if !metadata.is_file() {
            return Err(BootstrapClientError(Code::InvalidInput));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(BootstrapClientError(Code::InvalidInput));
            }
        }
        let mut value = Zeroizing::new(String::new());
        file.take(259)
            .read_to_string(&mut value)
            .map_err(|_| BootstrapClientError(Code::Unavailable))?;
        if value.len() > 258 {
            return Err(BootstrapClientError(Code::InvalidInput));
        }
        let value = value.trim_end_matches(['\r', '\n']);
        if !(32..=256).contains(&value.len()) || value.chars().any(char::is_whitespace) {
            return Err(BootstrapClientError(Code::InvalidInput));
        }
        Ok(Self {
            address,
            credential: Zeroizing::new(value.into()),
        })
    }

    /// Explicit local rotation. Reusing this operation never issues a new proof.
    /// Callers must display it only in the local administrator's terminal.
    pub fn issue_proof(&self, operation: &str) -> Result<p::BootstrapProofView> {
        p::validate_product_id(operation).map_err(|_| BootstrapClientError(Code::InvalidInput))?;
        self.post(
            "/v1/host/bootstrap/proof",
            Some(operation),
            &p::BootstrapProofRequest {
                schema_version: p::SchemaVersion(1),
                operation_id: operation.into(),
            },
        )
    }

    pub fn reserve(
        &self,
        request: &p::BootstrapReserveRequest,
    ) -> Result<p::BootstrapReservationView> {
        self.post("/v1/setup/reserve", Some(&request.operation_id), request)
    }
    pub fn candidate(
        &self,
        session: &p::BootstrapSessionBinding,
    ) -> Result<p::BootstrapDataCandidateView> {
        self.post("/v1/setup/data", Some(&session.operation_id), session)
    }
    pub fn select(
        &self,
        request: &p::BootstrapDataChoice,
    ) -> Result<p::BootstrapDataCandidateView> {
        self.post(
            "/v1/setup/select",
            Some(&request.session.operation_id),
            request,
        )
    }
    pub fn commit(&self, request: &p::BootstrapCommitRequest) -> Result<p::SignedWebClaimCommit> {
        self.post(
            "/v1/setup/commit",
            Some(&request.session.operation_id),
            request,
        )
    }

    fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        key: Option<&str>,
        body: &impl Serialize,
    ) -> Result<T> {
        if let Some(key) = key {
            p::validate_product_id(key).map_err(|_| BootstrapClientError(Code::InvalidInput))?;
        }
        let unavailable = || BootstrapClientError(Code::Unavailable);
        let body = Zeroizing::new(serde_json::to_vec(body).map_err(|_| unavailable())?);
        if body.len() > 8192 {
            return Err(BootstrapClientError(Code::InvalidInput));
        }
        let deadline = Instant::now() + DEADLINE;
        let mut stream =
            TcpStream::connect_timeout(&self.address, DEADLINE).map_err(|_| unavailable())?;
        stream
            .set_write_timeout(Some(DEADLINE))
            .map_err(|_| unavailable())?;
        let idempotency = key.map_or_else(String::new, |key| format!("Idempotency-Key: {key}\r\n"));
        let header = Zeroizing::new(format!(
            "POST {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{idempotency}Connection: close\r\n\r\n",
            self.address, self.credential.as_str(), body.len()));
        stream
            .write_all(header.as_bytes())
            .and_then(|_| stream.write_all(&body))
            .map_err(|_| unavailable())?;
        let mut bytes = Zeroizing::new(Vec::new());
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(unavailable)?;
            stream
                .set_read_timeout(Some(remaining))
                .map_err(|_| unavailable())?;
            let mut buffer = [0; 4096];
            let count = match stream.read(&mut buffer) {
                Ok(count) => count,
                // On Windows a peer that closes immediately after a complete
                // response can surface as a reset instead of EOF. The strict
                // Content-Length check below still rejects any incomplete or
                // ambiguous response before it is accepted.
                Err(_) if !bytes.is_empty() => break,
                Err(_) => return Err(unavailable()),
            };
            if count == 0 {
                break;
            }
            if bytes.len().saturating_add(count) > LIMIT {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        let (status, body) = response(&bytes)?;
        if status != 200 {
            let problem: p::BootstrapProblem =
                serde_json::from_slice(body).map_err(|_| unavailable())?;
            if problem.schema_version != p::SchemaVersion(1) {
                return Err(unavailable());
            }
            return Err(BootstrapClientError(problem.code));
        }
        let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| unavailable())?;
        if value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
        {
            return Err(BootstrapClientError(Code::InvalidInput));
        }
        serde_json::from_slice(body).map_err(|_| unavailable())
    }
}

fn response(bytes: &[u8]) -> Result<(u16, &[u8])> {
    let bad = || BootstrapClientError(Code::Unavailable);
    let index = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(bad)?;
    if index > 8192 {
        return Err(bad());
    }
    let header = std::str::from_utf8(&bytes[..index]).map_err(|_| bad())?;
    let mut lines = header.split("\r\n");
    let status_line = lines.next().ok_or_else(bad)?;
    let mut words = status_line.split_whitespace();
    if words.next() != Some("HTTP/1.1") {
        return Err(bad());
    }
    let status = words
        .next()
        .ok_or_else(bad)?
        .parse::<u16>()
        .map_err(|_| bad())?;
    let mut length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(bad)?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(bad());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(bad());
            }
            length = Some(value.trim().parse::<usize>().map_err(|_| bad())?);
        }
    }
    let body = &bytes[index + 4..];
    if length != Some(body.len()) {
        return Err(bad());
    }
    Ok((status, body))
}
