//! Private host HTTP framing, domain authentication and typed routes.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use super::{now_ms, EndpointDomain};
use forme_execution::installed::{HostManagement, IsolatedExecutorHost, ProductHostBroker};
use forme_protocol as p;

const MAX_REQUEST: usize = 1_048_576;
const MAX_HEADERS: usize = 16_384;

pub(super) fn handle_connection(
    mut stream: TcpStream,
    broker: Arc<ProductHostBroker>,
    worker: Arc<dyn IsolatedExecutorHost>,
    bootstrap: Option<Arc<forme_execution::installed::bootstrap::BootstrapAuthority>>,
    domain: EndpointDomain,
) -> p::Result<()> {
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
    let expected_host = stream
        .local_addr()
        .map_err(|_| p::Error("host broker endpoint is unavailable".into()))?
        .to_string();
    if request.header("host") != Some(expected_host.as_str()) {
        write_error(
            &mut stream,
            400,
            p::Error("host request authority is invalid".into()),
        )?;
        return Ok(());
    }
    if super::bootstrap::handles(&request.path) {
        let (status, body) =
            super::bootstrap::route(&request, broker.as_ref(), bootstrap.as_deref(), domain);
        return write_response(&mut stream, status, &body);
    }
    let auth = match authenticate_request(broker.as_ref(), &request, domain) {
        Ok(auth) => auth,
        Err(error) => {
            write_error(&mut stream, 401, error)?;
            return Ok(());
        }
    };
    match route(&request, broker.as_ref(), worker.as_ref(), &auth, domain) {
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
    worker: &dyn IsolatedExecutorHost,
    auth: &forme_execution::installed::HostAuth,
    domain: EndpointDomain,
) -> p::Result<(u16, Vec<u8>)> {
    let body = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/healthz") => serde_json::to_vec(&serde_json::json!({"status":"ok"}))
            .map_err(|_| p::Error("host health response failed".into()))?,
        ("GET", "/v1/host/status") => serde_json::to_vec(&broker.status(auth)?)
            .map_err(|_| p::Error("host status response failed".into()))?,
        ("GET", "/v1/host/configuration") if matches!(domain, EndpointDomain::Configuration) => {
            serde_json::to_vec(&broker.configuration(auth)?)
                .map_err(|_| p::Error("host configuration response failed".into()))?
        }
        ("POST", "/v1/host/configuration") if matches!(domain, EndpointDomain::Configuration) => {
            let patch: p::ProductConfigPatch = serde_json::from_slice(&request.body)
                .map_err(|_| p::Error("host configuration body is invalid".into()))?;
            serde_json::to_vec(&broker.apply_configuration(auth, patch)?)
                .map_err(|_| p::Error("host configuration response failed".into()))?
        }
        ("POST", "/v1/host/configuration/owner")
            if matches!(domain, EndpointDomain::Configuration) =>
        {
            let binding: p::ProductConfigOwnerBinding = serde_json::from_slice(&request.body)
                .map_err(|_| p::Error("host owner binding body is invalid".into()))?;
            serde_json::to_vec(&broker.bind_configuration_owner(auth, binding)?)
                .map_err(|_| p::Error("host configuration response failed".into()))?
        }
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
        ("POST", "/v1/agent/workers/dispatch") => {
            let request: p::WorkerDispatchRequest = serde_json::from_slice(&request.body)
                .map_err(|_| p::Error("worker dispatch body is invalid".into()))?;
            let now = now_ms();
            request.validate_at(now)?;
            serde_json::to_vec(&worker.dispatch(auth, request.job, request.permit, now)?)
                .map_err(|_| p::Error("worker dispatch response failed".into()))?
        }
        ("GET", path) if path.starts_with("/v1/agent/workers/") => {
            let job_id = worker_id(path, "/v1/agent/workers/", "")?;
            serde_json::to_vec(&worker.probe(auth, job_id, now_ms())?)
                .map_err(|_| p::Error("worker probe response failed".into()))?
        }
        ("POST", path) if path.starts_with("/v1/agent/workers/") && path.ends_with("/cancel") => {
            let job_id = worker_id(path, "/v1/agent/workers/", "/cancel")?;
            serde_json::to_vec(&worker.cancel(auth, job_id, now_ms())?)
                .map_err(|_| p::Error("worker cancel response failed".into()))?
        }
        _ => return Ok((404, b"{}".to_vec())),
    };
    Ok((200, body))
}

fn authenticate_request(
    broker: &ProductHostBroker,
    request: &Request,
    domain: EndpointDomain,
) -> p::Result<forme_execution::installed::HostAuth> {
    let valid_route = match domain {
        EndpointDomain::Maintenance => {
            !is_agent_path(&request.path) && !is_configuration_path(&request.path)
        }
        EndpointDomain::AgentExecution => is_agent_path(&request.path),
        EndpointDomain::Configuration => is_configuration_path(&request.path),
    };
    if !valid_route {
        return Err(p::Error(
            "host request is outside this endpoint's authorization domain".into(),
        ));
    }
    let bearer = request
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| p::Error("host broker authorization is required".into()))?;
    match domain {
        EndpointDomain::Maintenance => broker.authenticate(bearer),
        EndpointDomain::AgentExecution => broker.authenticate_agent(bearer),
        EndpointDomain::Configuration => broker.authenticate_web_configuration(bearer),
    }
}

fn is_agent_path(path: &str) -> bool {
    path == "/v1/agent/workers/dispatch" || path.starts_with("/v1/agent/workers/")
}

fn is_configuration_path(path: &str) -> bool {
    matches!(
        path,
        "/v1/host/configuration" | "/v1/host/configuration/owner"
    )
}

fn worker_id<'a>(path: &'a str, prefix: &str, suffix: &str) -> p::Result<&'a str> {
    let value = path
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .ok_or_else(|| p::Error("worker route job id is invalid".into()))?;
    p::validate_product_id(value).map_err(|_| p::Error("worker route job id is invalid".into()))?;
    Ok(value)
}
pub(crate) struct Request {
    pub(super) method: String,
    pub(crate) path: String,
    headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
}

impl Request {
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

pub(crate) fn read_request(stream: &mut impl Read) -> p::Result<Request> {
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
