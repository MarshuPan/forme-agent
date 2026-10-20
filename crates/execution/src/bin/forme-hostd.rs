#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use forme_execution::installed::{
    HostCredentials, HostManagement, IsolatedExecutorHost, ProductHostBroker, WorkerRuntimeConfig,
};
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
    #[cfg(windows)]
    {
        match windows_service::service_dispatcher::start(
            p::BuiltinServiceId::Host.service_name(),
            ffi_service_main,
        ) {
            Ok(()) => return Ok(()),
            Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1063) => {}
            Err(error) => {
                return Err(p::Error(format!(
                    "Windows service dispatcher failed: {error}"
                )))
            }
        }
    }
    run_host(None, true, false, || Ok(()))
}

fn run_host(
    shutdown: Option<Receiver<()>>,
    announce: bool,
    managed_credentials: bool,
    on_ready: impl FnOnce() -> p::Result<()>,
) -> p::Result<()> {
    let development_workers =
        development_process_workers(std::env::args().skip(1), managed_credentials)?;
    let journal = required_path("FORME_HOST_JOURNAL_PATH")?;
    let install_id = required("FORME_INSTALL_ID")?;
    let credentials = if managed_credentials {
        load_managed_credentials()?
    } else {
        let maintenance_token = load_or_create_token()?
            .ok_or_else(|| p::Error("host management token is unavailable".into()))?;
        HostCredentials::new(
            maintenance_token,
            std::env::var("FORME_AGENT_EXECUTION_TOKEN").ok(),
        )?
    };
    let target = target_from_environment()?;
    let active_release =
        std::env::var("FORME_ACTIVE_RELEASE").unwrap_or_else(|_| "unconfigured".into());
    let agent_digest = credentials
        .agent_execution()
        .map(|value| p::canonical_digest(&value))
        .transpose()?;
    let broker = Arc::new(ProductHostBroker::open_with_service_manager(
        &journal,
        &install_id,
        credentials,
        target,
        active_release,
        forme_execution::installed::SystemServiceManager,
    )?);
    let has_agent = agent_digest.is_some();
    let worker = load_worker_host(&journal, agent_digest, development_workers)?;
    let mut endpoints = vec![HostEndpoint::bind(
        "FORME_HOST_BIND",
        "127.0.0.1:8766",
        EndpointDomain::Maintenance,
    )?];
    if has_agent {
        endpoints.push(HostEndpoint::bind(
            "FORME_AGENT_EXECUTION_BIND",
            "127.0.0.1:8767",
            EndpointDomain::AgentExecution,
        )?);
    }
    if announce {
        for endpoint in &endpoints {
            let name = match endpoint.domain {
                EndpointDomain::Maintenance => "FORME_HOST_URL",
                EndpointDomain::AgentExecution => "FORME_AGENT_EXECUTION_URL",
            };
            println!(
                "{name}=http://{}",
                endpoint
                    .listener
                    .local_addr()
                    .map_err(|_| p::Error("host broker endpoint is unavailable".into()))?
            );
        }
        if let Some(path) = token_path() {
            println!("FORME_HOST_TOKEN_PATH={}", path.display());
        }
        let _ = std::io::stdout().flush();
    }
    on_ready()?;
    loop {
        if let Some(receiver) = shutdown.as_ref() {
            match receiver.try_recv() {
                Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut accepted = false;
        for endpoint in &endpoints {
            match endpoint.listener.accept() {
                Ok((stream, _)) => {
                    accepted = true;
                    if endpoint.connections.fetch_add(1, Ordering::AcqRel) >= 16 {
                        endpoint.connections.fetch_sub(1, Ordering::AcqRel);
                        drop(stream);
                        continue;
                    }
                    let broker = broker.clone();
                    let worker = worker.clone();
                    let connections = endpoint.connections.clone();
                    let domain = endpoint.domain;
                    std::thread::spawn(move || {
                        struct ConnectionGuard(Arc<AtomicUsize>);
                        impl Drop for ConnectionGuard {
                            fn drop(&mut self) {
                                self.0.fetch_sub(1, Ordering::AcqRel);
                            }
                        }
                        let _guard = ConnectionGuard(connections);
                        let _ = handle_connection(stream, broker, worker, domain);
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err(p::Error("host broker accept failed".into())),
            }
        }
        if !accepted {
            // A single supervisor owns both listeners and shutdown. Domain
            // connection budgets remain independent.
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum EndpointDomain {
    Maintenance,
    AgentExecution,
}

struct HostEndpoint {
    listener: TcpListener,
    domain: EndpointDomain,
    connections: Arc<AtomicUsize>,
}

impl HostEndpoint {
    fn bind(key: &str, default: &str, domain: EndpointDomain) -> p::Result<Self> {
        let address: SocketAddr = std::env::var(key)
            .unwrap_or_else(|_| default.into())
            .parse()
            .map_err(|_| p::Error("host broker requires a numeric loopback address".into()))?;
        if !address.ip().is_loopback() {
            return Err(p::Error("host broker must remain loopback-only".into()));
        }
        let listener = TcpListener::bind(address)
            .map_err(|_| p::Error("host broker could not bind its private endpoint".into()))?;
        listener
            .set_nonblocking(true)
            .map_err(|_| p::Error("host broker nonblocking mode is unavailable".into()))?;
        Ok(Self {
            listener,
            domain,
            connections: Arc::new(AtomicUsize::new(0)),
        })
    }
}

fn development_process_workers(
    args: impl IntoIterator<Item = String>,
    managed: bool,
) -> p::Result<bool> {
    let args: Vec<_> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(false),
        [flag] if flag == "--development-process-workers" && !managed => Ok(true),
        _ => Err(p::Error(
            "host arguments are invalid for this execution mode".into(),
        )),
    }
}

#[cfg(windows)]
windows_service::define_windows_service!(ffi_service_main, windows_service_main);

#[cfg(windows)]
fn windows_service_main(_arguments: Vec<std::ffi::OsString>) {
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    let service_name = p::BuiltinServiceId::Host.service_name();
    let handler =
        move |event| -> windows_service::service_control_handler::ServiceControlHandlerResult {
            use windows_service::service::ServiceControl;
            use windows_service::service_control_handler::ServiceControlHandlerResult;
            match event {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    let _ = shutdown_tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };
    let status = match windows_service::service_control_handler::register(service_name, handler) {
        Ok(status) => status,
        Err(_) => return,
    };
    use windows_service::service::{
        ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
    };
    let _ = status.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::StartPending,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 1,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    });
    let result = run_host(Some(shutdown_rx), false, true, || {
        status
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })
            .map_err(|_| p::Error("host service could not report readiness".into()))
    });
    let _ = status.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(if result.is_ok() { 0 } else { 1 }),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    });
}

fn handle_connection(
    mut stream: TcpStream,
    broker: Arc<ProductHostBroker>,
    worker: Arc<dyn IsolatedExecutorHost>,
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
    let auth = match authenticate_request(broker.as_ref(), &request, domain) {
        Ok(auth) => auth,
        Err(error) => {
            write_error(&mut stream, 401, error)?;
            return Ok(());
        }
    };
    match route(&request, broker.as_ref(), worker.as_ref(), &auth) {
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
    if is_agent_path(&request.path) != matches!(domain, EndpointDomain::AgentExecution) {
        return Err(p::Error(
            "host request is outside this endpoint's authorization domain".into(),
        ));
    }
    let bearer = request
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| p::Error("host broker authorization is required".into()))?;
    if is_agent_path(&request.path) {
        broker.authenticate_agent(bearer)
    } else {
        broker.authenticate(bearer)
    }
}

fn is_agent_path(path: &str) -> bool {
    path == "/v1/agent/workers/dispatch" || path.starts_with("/v1/agent/workers/")
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

fn load_worker_host(
    host_journal: &Path,
    agent_digest: Option<p::SchemaDigest>,
    development_workers: bool,
) -> p::Result<Arc<dyn IsolatedExecutorHost>> {
    if let Some(config_path) = std::env::var_os("FORME_WORKER_CONFIG_PATH") {
        let digest = agent_digest.ok_or_else(|| {
            p::Error("worker configuration requires an agent execution credential".into())
        })?;
        let config = WorkerRuntimeConfig::load(&PathBuf::from(config_path))?;
        let mut protected = vec![host_journal.to_path_buf()];
        for key in [
            "FORME_HOST_MAINTENANCE_TOKEN_PATH",
            "FORME_HOST_AGENT_TOKEN_PATH",
        ] {
            if let Some(path) = std::env::var_os(key) {
                protected.push(PathBuf::from(path));
            }
        }
        if let Some(path) = token_path() {
            protected.push(path);
        }
        config.validate_protected_paths(
            &protected.iter().map(PathBuf::as_path).collect::<Vec<_>>(),
        )?;
        return if development_workers {
            Ok(Arc::new(config.open_development_process_host(digest)?))
        } else {
            Ok(Arc::new(config.open_host(digest)?))
        };
    }

    // Development and maintenance-only hosts still expose the typed route,
    // but it remains fail-closed until a managed worker config is installed.
    let digest = agent_digest
        .or_else(|| p::canonical_digest(&"unconfigured-agent-execution").ok())
        .ok_or_else(|| p::Error("worker execution digest is unavailable".into()))?;
    let worker_journal = host_journal.with_extension("worker.jsonl");
    Ok(Arc::new(
        forme_execution::installed::WorkerExecutorHost::open(
            &worker_journal,
            digest,
            p::AuthorityEpoch(1),
            [p::Scope("workspace:unconfigured".into())]
                .into_iter()
                .collect::<BTreeSet<_>>(),
        )?,
    ))
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

fn load_managed_credentials() -> p::Result<HostCredentials> {
    let maintenance_path = required_path("FORME_HOST_MAINTENANCE_TOKEN_PATH")?;
    let agent_path = required_path("FORME_HOST_AGENT_TOKEN_PATH")?;
    if same_path(&maintenance_path, &agent_path) {
        return Err(p::Error(
            "maintenance and agent credential paths must be distinct".into(),
        ));
    }
    HostCredentials::new(
        read_managed_credential(&maintenance_path, "maintenance")?,
        Some(read_managed_credential(&agent_path, "agent execution")?),
    )
}

fn read_managed_credential(path: &Path, domain: &str) -> p::Result<String> {
    forme_execution::installed::reject_linked_ancestors(path)?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| p::Error(format!("{domain} credential file is unavailable")))?;
    if !metadata.is_file() {
        return Err(p::Error(format!(
            "{domain} credential path is not a regular file"
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(p::Error(format!(
                "{domain} credential file permissions are too broad"
            )));
        }
    }
    let value = std::fs::read_to_string(path)
        .map_err(|_| p::Error(format!("{domain} credential file cannot be read")))?;
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err(p::Error(format!("{domain} credential file is malformed")));
    }
    Ok(value)
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.to_string_lossy().replace('\\', "/");
    let right = right.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    {
        left.eq_ignore_ascii_case(&right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
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
    fn service_mode_never_accepts_the_development_process_switch() {
        let flag = vec!["--development-process-workers".into()];
        assert!(development_process_workers(flag.clone(), true).is_err());
        assert!(development_process_workers(flag, false).unwrap());
        assert!(!development_process_workers(Vec::new(), true).unwrap());
        assert!(!development_process_workers(Vec::new(), false).unwrap());
        assert!(development_process_workers(vec!["--unknown".into()], false).is_err());
    }

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
