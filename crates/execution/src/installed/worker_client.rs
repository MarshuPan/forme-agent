//! Typed core-to-AgentExecution transport.
//!
//! This client is deliberately narrower than the maintenance clients: it can
//! only reach a loopback AgentExecution listener with its dedicated bearer and
//! only exposes dispatch, probe, and cancel. It never creates a permit or
//! retries an action after an uncertain side effect.

use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use forme_protocol as p;
use serde::Serialize;
use zeroize::Zeroizing;

use super::reject_linked_ancestors;

const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_WIRE_BYTES: usize = 4 * 1024 * 1024;
const MAX_HEADERS: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The only operations a core driver may ask the AgentExecution broker to do.
pub trait WorkerBrokerTransport: Send + Sync {
    fn dispatch(&self, request: p::WorkerDispatchRequest) -> p::Result<p::WorkerObservation>;
    fn probe(&self, job_id: &str) -> p::Result<p::WorkerObservation>;
    fn cancel(&self, job_id: &str) -> p::Result<p::WorkerCancelReceipt>;
    fn result_manifest(&self, job_id: &str) -> p::Result<p::WorkerResultManifest>;
    fn read_result(&self, request: p::WorkerResultRead) -> p::Result<p::WorkerResultChunk>;
}

/// Loopback HTTP client for the AgentExecution domain.
///
/// The endpoint and credential are installation facts. This type does not
/// accept arbitrary paths, headers, credentials, or caller-selected domains.
pub struct WorkerBrokerClient {
    endpoint: AgentExecutionEndpoint,
    credential: Zeroizing<String>,
}

enum AgentExecutionEndpoint {
    Tcp(SocketAddr),
    #[cfg(windows)]
    NamedPipe(String),
}

enum AgentExecutionStream {
    Tcp(TcpStream),
    #[cfg(windows)]
    NamedPipe(std::fs::File),
}

impl Read for AgentExecutionStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buffer),
            #[cfg(windows)]
            Self::NamedPipe(stream) => stream.read(buffer),
        }
    }
}

impl Write for AgentExecutionStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buffer),
            #[cfg(windows)]
            Self::NamedPipe(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(windows)]
            Self::NamedPipe(stream) => stream.flush(),
        }
    }
}

impl WorkerBrokerClient {
    /// Loads the dedicated AgentExecution bearer from a protected file.
    pub fn connect(address: SocketAddr, credential_path: &Path) -> p::Result<Self> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(p::Error(
                "agent execution broker address must be loopback".into(),
            ));
        }
        let credential = read_credential(credential_path)?;
        Ok(Self {
            endpoint: AgentExecutionEndpoint::Tcp(address),
            credential,
        })
    }

    /// Connects through the managed Windows named pipe. The server verifies
    /// the calling process's Brain Service SID and installed image; the
    /// bearer remains a separate application-layer proof.
    #[cfg(windows)]
    pub fn connect_named_pipe(pipe_name: &str, credential_path: &Path) -> p::Result<Self> {
        if !pipe_name.starts_with(r"\\.\pipe\forme-agent-execution-v1-")
            || pipe_name.len() > 256
            || pipe_name.chars().any(char::is_control)
        {
            return Err(p::Error("agent execution pipe name is invalid".into()));
        }
        let credential = read_credential(credential_path)?;
        Ok(Self {
            endpoint: AgentExecutionEndpoint::NamedPipe(pipe_name.to_owned()),
            credential,
        })
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        operation: Operation,
    ) -> p::Result<Vec<u8>> {
        if !matches!(method, "GET" | "POST")
            || !path.starts_with("/v1/agent/workers/")
            || path.contains(['?', '#', '%', '\r', '\n'])
            || body.len() > MAX_BODY_BYTES
        {
            return Err(p::Error("agent execution request is invalid".into()));
        }

        let (mut stream, host) = match &self.endpoint {
            AgentExecutionEndpoint::Tcp(address) => {
                let stream = TcpStream::connect_timeout(address, REQUEST_TIMEOUT)
                    .map_err(|_| operation.transport_error())?;
                stream
                    .set_read_timeout(Some(REQUEST_TIMEOUT))
                    .and_then(|_| stream.set_write_timeout(Some(REQUEST_TIMEOUT)))
                    .map_err(|_| operation.transport_error())?;
                (AgentExecutionStream::Tcp(stream), address.to_string())
            }
            #[cfg(windows)]
            AgentExecutionEndpoint::NamedPipe(name) => (
                AgentExecutionStream::NamedPipe(
                    super::windows_agent_execution_native::open_agent_execution_pipe(name)?,
                ),
                "forme-agent-execution".to_owned(),
            ),
        };
        let header = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            host,
            self.credential.as_str(),
            body.len()
        );
        stream
            .write_all(header.as_bytes())
            .and_then(|_| stream.write_all(body))
            .and_then(|_| stream.flush())
            .map_err(|_| operation.transport_error())?;

        let mut wire = Vec::new();
        stream
            .take((MAX_WIRE_BYTES + 1) as u64)
            .read_to_end(&mut wire)
            .map_err(|_| operation.transport_error())?;
        if wire.len() > MAX_WIRE_BYTES {
            return Err(operation.transport_error());
        }
        let (status, response_body) =
            parse_response(&wire).map_err(|_| operation.transport_error())?;
        if status != 200 {
            return Err(status_error(status, operation));
        }
        Ok(response_body.to_vec())
    }

    fn json<T: Serialize>(
        &self,
        method: &str,
        path: &str,
        value: Option<&T>,
        operation: Operation,
    ) -> p::Result<Vec<u8>> {
        let body = value
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| operation.transport_error())?
            .unwrap_or_default();
        self.request(method, path, &body, operation)
    }
}

impl WorkerBrokerTransport for WorkerBrokerClient {
    fn dispatch(&self, request: p::WorkerDispatchRequest) -> p::Result<p::WorkerObservation> {
        request.validate_at(now_ms())?;
        let job_id = request.job.job_id.clone();
        let job = request.job.clone();
        let body = self.json(
            "POST",
            "/v1/agent/workers/dispatch",
            Some(&request),
            Operation::Dispatch,
        )?;
        let observation: p::WorkerObservation = decode(&body, Operation::Dispatch)?;
        observation.validate()?;
        validate_observation_for_job(&observation, &job)?;
        if observation.job_id != job_id {
            return Err(p::Error(
                "agent execution response is bound to another job".into(),
            ));
        }
        Ok(observation)
    }

    fn probe(&self, job_id: &str) -> p::Result<p::WorkerObservation> {
        validate_job_id(job_id)?;
        let body = self.json(
            "GET",
            &format!("/v1/agent/workers/{job_id}"),
            None::<&()>,
            Operation::Probe,
        )?;
        let observation: p::WorkerObservation = decode(&body, Operation::Probe)?;
        observation.validate()?;
        if observation.job_id != job_id {
            return Err(p::Error(
                "agent execution response is bound to another job".into(),
            ));
        }
        Ok(observation)
    }

    fn cancel(&self, job_id: &str) -> p::Result<p::WorkerCancelReceipt> {
        validate_job_id(job_id)?;
        let body = self.json(
            "POST",
            &format!("/v1/agent/workers/{job_id}/cancel"),
            None::<&()>,
            Operation::Cancel,
        )?;
        let receipt: p::WorkerCancelReceipt = decode(&body, Operation::Cancel)?;
        if receipt.job_id != job_id {
            return Err(p::Error(
                "agent execution response is bound to another job".into(),
            ));
        }
        Ok(receipt)
    }

    fn result_manifest(&self, job_id: &str) -> p::Result<p::WorkerResultManifest> {
        validate_job_id(job_id)?;
        let body = self.json(
            "GET",
            &format!("/v1/agent/workers/{job_id}/result"),
            None::<&()>,
            Operation::ResultManifest,
        )?;
        let manifest: p::WorkerResultManifest = decode(&body, Operation::ResultManifest)?;
        manifest.validate()?;
        if manifest.binding.job_id != job_id {
            return Err(p::Error(
                "agent execution result is bound to another job".into(),
            ));
        }
        Ok(manifest)
    }

    fn read_result(&self, request: p::WorkerResultRead) -> p::Result<p::WorkerResultChunk> {
        request.validate()?;
        let job_id = request.binding.job_id.clone();
        let body = self.json(
            "POST",
            &format!("/v1/agent/workers/{job_id}/result/read"),
            Some(&request),
            Operation::ResultRead,
        )?;
        let chunk: p::WorkerResultChunk = decode(&body, Operation::ResultRead)?;
        chunk.validate()?;
        if chunk.manifest_digest != request.manifest_digest {
            return Err(p::Error(
                "agent execution result chunk is bound to another manifest".into(),
            ));
        }
        Ok(chunk)
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Dispatch,
    Probe,
    Cancel,
    ResultManifest,
    ResultRead,
}

impl Operation {
    fn transport_error(self) -> p::Error {
        match self {
            Self::Dispatch => p::Error(
                "agent execution dispatch outcome is unknown; probe the original job".into(),
            ),
            Self::Probe => p::Error("agent execution probe is unavailable".into()),
            Self::Cancel => p::Error(
                "agent execution cancellation outcome is unknown; probe the original job".into(),
            ),
            Self::ResultManifest => {
                p::Error("agent execution result manifest is unavailable".into())
            }
            Self::ResultRead => p::Error("agent execution result read is unavailable".into()),
        }
    }
}

fn status_error(status: u16, operation: Operation) -> p::Error {
    match status {
        401 => p::Error("agent execution authorization was rejected".into()),
        404 => p::Error("agent execution job was not found".into()),
        400 | 409 => p::Error("agent execution request was rejected before admission".into()),
        _ => operation.transport_error(),
    }
}

fn decode<T: serde::de::DeserializeOwned>(body: &[u8], operation: Operation) -> p::Result<T> {
    serde_json::from_slice(body).map_err(|_| operation.transport_error())
}

fn validate_observation_for_job(
    observation: &p::WorkerObservation,
    job: &p::WorkerJob,
) -> p::Result<()> {
    if observation.job_id != job.job_id
        || observation.plan_digest != job.plan_digest
        || observation.authority_epoch != job.authority_epoch
        || observation.nonce != job.nonce
    {
        return Err(p::Error(
            "agent execution observation is not bound to the dispatched job".into(),
        ));
    }
    Ok(())
}

fn validate_job_id(job_id: &str) -> p::Result<()> {
    p::validate_product_id(job_id).map_err(|_| p::Error("agent execution job id is invalid".into()))
}

fn read_credential(path: &Path) -> p::Result<Zeroizing<String>> {
    if !path.is_absolute() {
        return Err(p::Error(
            "agent execution credential path must be absolute".into(),
        ));
    }
    reject_linked_ancestors(path)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| p::Error("agent execution credential is unavailable".into()))?;
    if !metadata.is_file() || metadata.len() > 258 {
        return Err(p::Error(
            "agent execution credential is not a regular file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o7777;
        if mode != 0o600 && mode != 0o640 {
            return Err(p::Error(
                "agent execution credential permissions are too broad".into(),
            ));
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| p::Error("agent execution credential cannot be read".into()))?
        .take(259)
        .read_to_end(&mut bytes)
        .map_err(|_| p::Error("agent execution credential cannot be read".into()))?;
    let value = String::from_utf8(bytes)
        .map_err(|_| p::Error("agent execution credential is malformed".into()))?;
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    if !(32..=256).contains(&value.len()) || value.chars().any(char::is_whitespace) {
        return Err(p::Error("agent execution credential is malformed".into()));
    }
    Ok(Zeroizing::new(value))
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

fn parse_response(bytes: &[u8]) -> p::Result<(u16, &[u8])> {
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| p::Error("agent execution response headers are incomplete".into()))?;
    if header_end > MAX_HEADERS {
        return Err(p::Error(
            "agent execution response headers are too large".into(),
        ));
    }
    let header = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| p::Error("agent execution response headers are invalid".into()))?;
    let mut lines = header.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| p::Error("agent execution response status is invalid".into()))?;
    let mut content_length = None;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| p::Error("agent execution response header is invalid".into()))?;
        if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && content_length.is_some())
        {
            return Err(p::Error(
                "agent execution response framing is ambiguous".into(),
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| p::Error("agent execution response length is invalid".into()))?,
            );
        }
    }
    let body = &bytes[header_end + 4..];
    if content_length != Some(body.len()) {
        return Err(p::Error(
            "agent execution response length is invalid".into(),
        ));
    }
    Ok((status, body))
}
