use std::io::{BufRead, BufReader, Read, Write};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{failure, p, McpErrorClass, McpFailure, StdioMcpServer};

const MAX_FRAME_BYTES: usize = 8 * 1_024 * 1_024;
const CLEANUP_BOUND: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(5);
const MAX_ACTIVE: usize = 8;

#[derive(Default)]
struct Supervisors {
    active: usize,
    pending: Vec<CleanupHandle>,
    failed: bool,
}

fn supervisors() -> &'static Mutex<Supervisors> {
    static STATE: OnceLock<Mutex<Supervisors>> = OnceLock::new();
    STATE.get_or_init(Mutex::default)
}

pub(super) struct Admission;

impl Admission {
    pub(super) fn acquire(server: &p::ProviderId) -> Result<Self, McpFailure> {
        let mut state = supervisors().lock().map_err(|_| {
            failure(
                server.clone(),
                McpErrorClass::ServerError,
                "MCP discovery supervision is unavailable",
            )
        })?;
        let mut index = 0;
        while index < state.pending.len() {
            if state.pending[index].finished() {
                let pending = state.pending.swap_remove(index);
                state.failed |= !pending.join();
                state.active -= 1;
            } else {
                index += 1;
            }
        }
        if state.failed || !state.pending.is_empty() || state.active >= MAX_ACTIVE {
            return Err(failure(
                server.clone(),
                McpErrorClass::ServerError,
                "MCP discovery admission is blocked by capacity or original cleanup",
            ));
        }
        state.active += 1;
        Ok(Self)
    }

    fn retain(self, handle: CleanupHandle) {
        match supervisors().lock() {
            Ok(mut state) => state.pending.push(handle),
            Err(poisoned) => poisoned.into_inner().pending.push(handle),
        }
        std::mem::forget(self);
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        if let Ok(mut state) = supervisors().lock() {
            state.active = state.active.saturating_sub(1);
        }
    }
}

struct Request {
    message: Value,
    id: Option<u64>,
    complete: SyncSender<Result<Value, WireError>>,
}

pub(super) struct RpcSession {
    child: Option<Child>,
    requests: Option<SyncSender<Request>>,
    worker: Option<JoinHandle<()>>,
    admission: Option<Admission>,
    deadline: Instant,
    server: p::ProviderId,
    next_id: u64,
}

impl RpcSession {
    pub(super) fn new(
        child: Child,
        server: &StdioMcpServer,
        admission: Admission,
    ) -> Result<Self, McpFailure> {
        let mut session = Self {
            child: Some(child),
            requests: None,
            worker: None,
            admission: Some(admission),
            deadline: Instant::now(),
            server: server.provider_id.clone(),
            next_id: 0,
        };
        if let Err(error) = session.start(server.timeout) {
            return Err(if session.shutdown() {
                error
            } else {
                session.cleanup_error()
            });
        }
        Ok(session)
    }

    fn start(&mut self, timeout: p::DurationMs) -> Result<(), McpFailure> {
        self.deadline = Instant::now()
            .checked_add(Duration::from_millis(timeout.0))
            .ok_or_else(|| {
                failure(
                    self.server.clone(),
                    McpErrorClass::SchemaMismatch,
                    "MCP discovery deadline is out of range",
                )
            })?;
        let server = self.server.clone();
        let child = self
            .child
            .as_mut()
            .expect("original discovery child is owned");
        let stdin = child.stdin.take().ok_or_else(|| {
            failure(
                server.clone(),
                McpErrorClass::ServerError,
                "MCP process did not expose stdin",
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            failure(
                server.clone(),
                McpErrorClass::ServerError,
                "MCP process did not expose stdout",
            )
        })?;
        let (sender, requests) = mpsc::sync_channel(1);
        self.requests = Some(sender);
        let deadline = self.deadline;
        self.worker = Some(
            std::thread::Builder::new()
                .name("forme-mcp-discovery".into())
                .spawn(move || exchange_requests(stdin, stdout, requests, deadline))
                .map_err(|_| {
                    failure(
                        server,
                        McpErrorClass::ServerError,
                        "MCP discovery I/O could not start",
                    )
                })?,
        );
        Ok(())
    }

    pub(super) fn request(&mut self, method: &str, params: Value) -> Result<Value, McpFailure> {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        self.dispatch(
            serde_json::json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}),
            Some(id),
        )
    }

    pub(super) fn notify(&mut self, method: &str, params: Value) -> Result<(), McpFailure> {
        self.dispatch(
            serde_json::json!({"jsonrpc":"2.0", "method":method, "params":params}),
            None,
        )
        .map(|_| ())
    }

    fn dispatch(&self, message: Value, id: Option<u64>) -> Result<Value, McpFailure> {
        let (complete, result) = mpsc::sync_channel(1);
        let remaining = self.remaining()?;
        self.requests
            .as_ref()
            .expect("discovery I/O is initialized")
            .try_send(Request {
                message,
                id,
                complete,
            })
            .map_err(|_| {
                failure(
                    self.server.clone(),
                    McpErrorClass::ServerError,
                    "MCP discovery I/O is unavailable or occupied",
                )
            })?;
        let received = result.recv_timeout(remaining);
        self.remaining()?;
        match received {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(failure(self.server.clone(), error.class, error.detail)),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(self.timeout_error()),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(failure(
                self.server.clone(),
                McpErrorClass::ServerError,
                "MCP discovery I/O ended before responding",
            )),
        }
    }

    fn remaining(&self) -> Result<Duration, McpFailure> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(self.timeout_error())
        } else {
            Ok(remaining)
        }
    }

    fn timeout_error(&self) -> McpFailure {
        failure(
            self.server.clone(),
            McpErrorClass::Timeout,
            "MCP discovery deadline elapsed",
        )
    }

    fn cleanup_error(&self) -> McpFailure {
        failure(
            self.server.clone(),
            McpErrorClass::ServerError,
            "MCP original discovery process or I/O cleanup is unverified",
        )
    }

    pub(super) fn finish<T>(&mut self, result: Result<T, McpFailure>) -> Result<T, McpFailure> {
        let result = match result {
            Ok(value) => self.remaining().map(|_| value),
            Err(error) => Err(error),
        };
        if self.shutdown() {
            result
        } else {
            Err(self.cleanup_error())
        }
    }

    fn shutdown(&mut self) -> bool {
        let Some(child) = self.child.take() else {
            return true;
        };
        drop(self.requests.take());
        let handle = CleanupHandle::start(CleanupJob {
            child,
            worker: self.worker.take(),
        });
        let deadline = Instant::now() + CLEANUP_BOUND;
        while !handle.finished() && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
        let admission = self.admission.take().expect("discovery admission is owned");
        if handle.finished() {
            let success = handle.join();
            if !success {
                match supervisors().lock() {
                    Ok(mut state) => state.failed = true,
                    Err(poisoned) => poisoned.into_inner().failed = true,
                }
            }
            success
        } else {
            admission.retain(handle);
            false
        }
    }
}

impl Drop for RpcSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct WireError {
    class: McpErrorClass,
    detail: String,
}

fn wire(class: McpErrorClass, detail: impl Into<String>) -> WireError {
    WireError {
        class,
        detail: detail.into(),
    }
}

fn exchange_requests(
    mut stdin: impl Write,
    stdout: impl Read,
    requests: Receiver<Request>,
    deadline: Instant,
) {
    let mut reader = BufReader::new(stdout);
    for request in requests {
        let result = exchange(&mut stdin, &mut reader, &request, deadline);
        let failed = result.is_err();
        if request.complete.send(result).is_err() || failed {
            break;
        }
    }
}

fn exchange(
    writer: &mut impl Write,
    reader: &mut impl BufRead,
    request: &Request,
    deadline: Instant,
) -> Result<Value, WireError> {
    let mut frame = LimitedFrame(Vec::new());
    serde_json::to_writer(&mut frame, &request.message)
        .and_then(|()| frame.write_all(b"\n").map_err(serde_json::Error::io))
        .map_err(|_| {
            wire(
                McpErrorClass::SchemaMismatch,
                "MCP discovery request exceeds its wire frame bound",
            )
        })?;
    if Instant::now() >= deadline {
        return Err(wire(
            McpErrorClass::Timeout,
            "MCP discovery deadline elapsed",
        ));
    }
    writer
        .write_all(&frame.0)
        .and_then(|()| writer.flush())
        .map_err(|_| {
            wire(
                McpErrorClass::ServerError,
                "MCP discovery request could not be written",
            )
        })?;
    let Some(id) = request.id else {
        return Ok(Value::Null);
    };
    loop {
        if Instant::now() >= deadline {
            return Err(wire(
                McpErrorClass::Timeout,
                "MCP discovery deadline elapsed",
            ));
        }
        let mut frame = Vec::new();
        let count = reader
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut frame)
            .map_err(|_| {
                wire(
                    McpErrorClass::ServerError,
                    "MCP discovery response could not be read",
                )
            })?;
        if count == 0 {
            return Err(wire(
                McpErrorClass::ServerError,
                "MCP process ended before responding",
            ));
        }
        if frame.len() > MAX_FRAME_BYTES {
            return Err(wire(
                McpErrorClass::SchemaMismatch,
                "MCP discovery response exceeds its wire frame bound",
            ));
        }
        let Ok(response) = serde_json::from_slice::<Value>(&frame) else {
            continue;
        };
        if response.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if let Some(error) = response.get("error") {
            return Err(wire(
                McpErrorClass::ServerError,
                format!("MCP server returned an error: {error}"),
            ));
        }
        return response
            .get("result")
            .cloned()
            .ok_or_else(|| wire(McpErrorClass::SchemaMismatch, "MCP response has no result"));
    }
}

struct LimitedFrame(Vec<u8>);

impl Write for LimitedFrame {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_FRAME_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other(
                "MCP discovery wire frame bound exceeded",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct CleanupJob {
    child: Child,
    worker: Option<JoinHandle<()>>,
}

impl CleanupJob {
    fn run(mut self) -> bool {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let exited = self.child.wait().is_ok();
        let joined = self.worker.is_none_or(|worker| worker.join().is_ok());
        exited && joined
    }
}

struct CleanupHandle {
    thread: Option<JoinHandle<bool>>,
    custody: Arc<Mutex<Option<CleanupJob>>>,
}

impl CleanupHandle {
    fn start(job: CleanupJob) -> Self {
        let custody = Arc::new(Mutex::new(Some(job)));
        let owned = custody.clone();
        let thread = std::thread::Builder::new()
            .name("forme-mcp-discovery-cleanup".into())
            .spawn(move || {
                owned
                    .lock()
                    .ok()
                    .and_then(|mut job| job.take())
                    .is_some_and(CleanupJob::run)
            })
            .ok();
        Self { thread, custody }
    }
    fn finished(&self) -> bool {
        self.thread.as_ref().is_some_and(JoinHandle::is_finished)
    }
    fn join(self) -> bool {
        let released = self.custody.lock().is_ok_and(|job| job.is_none());
        let joined = self
            .thread
            .is_some_and(|thread| thread.join().unwrap_or(false));
        released && joined
    }
}
