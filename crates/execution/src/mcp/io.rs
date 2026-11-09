use std::io::{BufRead, BufReader, Read, Write};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::McpExecutionError;

pub(super) const POLL: Duration = Duration::from_millis(5);
const CLEANUP_BOUND: Duration = Duration::from_secs(2);
const MAX_FRAME_BYTES: usize = 8 * 1_024 * 1_024;
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
    pub(super) fn acquire() -> Result<Self, McpExecutionError> {
        let mut state = supervisors()
            .lock()
            .map_err(|_| McpExecutionError::server("MCP cleanup supervision is unavailable"))?;
        let mut index = 0;
        while index < state.pending.len() {
            if state.pending[index].finished() {
                let worker = state.pending.swap_remove(index);
                state.failed |= !worker.join();
                state.active -= 1;
            } else {
                index += 1;
            }
        }
        if state.failed || !state.pending.is_empty() || state.active >= MAX_ACTIVE {
            return Err(McpExecutionError::server(
                "MCP admission is blocked by capacity or original cleanup",
            ));
        }
        state.active += 1;
        Ok(Self)
    }

    fn retain(self, cleanup: CleanupHandle) {
        // The bounded supervisor keeps ownership even when OS I/O or creation
        // of a cleanup thread fails. A new action must not replace this work.
        match supervisors().lock() {
            Ok(mut state) => state.pending.push(cleanup),
            Err(poisoned) => poisoned.into_inner().pending.push(cleanup),
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

pub(super) enum ReadEvent {
    Frame(Vec<u8>),
    Oversized,
    Failed,
}

struct WriteRequest {
    bytes: Vec<u8>,
    complete: SyncSender<std::io::Result<()>>,
    deadline: Instant,
    token: crate::CancelToken,
    tool_dispatch: Option<Arc<AtomicBool>>,
}

pub(super) struct ProcessIo {
    child: Option<Child>,
    output: Option<Receiver<ReadEvent>>,
    input: Option<SyncSender<WriteRequest>>,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
    admission: Option<Admission>,
    tool_dispatch: Arc<AtomicBool>,
}

impl ProcessIo {
    pub(super) fn start(child: Child, admission: Admission) -> Result<Self, McpExecutionError> {
        let mut io = Self {
            child: Some(child),
            output: None,
            input: None,
            stop: Arc::new(AtomicBool::new(false)),
            workers: Vec::new(),
            admission: Some(admission),
            tool_dispatch: Arc::new(AtomicBool::new(false)),
        };
        if let Err(error) = io.start_pipes() {
            return Err(if io.shutdown() {
                error
            } else {
                McpExecutionError::cleanup_unknown()
            });
        }
        Ok(io)
    }

    fn start_pipes(&mut self) -> Result<(), McpExecutionError> {
        let child = self.child.as_mut().expect("original MCP child is owned");
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpExecutionError::server("MCP process did not expose stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpExecutionError::server("MCP process did not expose stdout"))?;
        let (sender, output) = mpsc::sync_channel(2);
        self.output = Some(output);
        let read_stop = self.stop.clone();
        self.workers.push(
            std::thread::Builder::new()
                .name("forme-mcp-read".into())
                .spawn(move || read_frames(stdout, sender, read_stop))
                .map_err(|_| McpExecutionError::server("MCP reader could not start"))?,
        );
        let (input, requests) = mpsc::sync_channel(1);
        self.input = Some(input);
        let write_stop = self.stop.clone();
        self.workers.push(
            std::thread::Builder::new()
                .name("forme-mcp-write".into())
                .spawn(move || write_requests(stdin, requests, write_stop))
                .map_err(|_| McpExecutionError::server("MCP writer could not start"))?,
        );
        Ok(())
    }

    pub(super) fn send(
        &self,
        value: &serde_json::Value,
        deadline: Instant,
        token: &crate::CancelToken,
    ) -> Result<Receiver<std::io::Result<()>>, McpExecutionError> {
        let mut frame = LimitedFrame(Vec::new());
        serde_json::to_writer(&mut frame, value)
            .and_then(|()| frame.write_all(b"\n").map_err(serde_json::Error::io))
            .map_err(|_| McpExecutionError::schema("MCP request exceeds its wire frame bound"))?;
        if token.is_cancelled() {
            return Err(McpExecutionError::cancelled());
        }
        if Instant::now() >= deadline {
            return Err(McpExecutionError::timeout("MCP call deadline elapsed"));
        }
        let (complete, receipt) = mpsc::sync_channel(1);
        self.input
            .as_ref()
            .ok_or_else(|| McpExecutionError::server("MCP input is closed"))?
            .try_send(WriteRequest {
                bytes: frame.0,
                complete,
                deadline,
                token: token.clone(),
                tool_dispatch: (value.get("method").and_then(serde_json::Value::as_str)
                    == Some("tools/call"))
                .then(|| self.tool_dispatch.clone()),
            })
            .map_err(|_| McpExecutionError::server("MCP input is unavailable or occupied"))?;
        Ok(receipt)
    }

    pub(super) fn output(&self) -> &Receiver<ReadEvent> {
        self.output.as_ref().expect("MCP reader is initialized")
    }

    pub(super) fn tool_dispatch_started(&self) -> bool {
        self.tool_dispatch.load(Ordering::SeqCst)
    }

    pub(super) fn shutdown(&mut self) -> bool {
        let Some(child) = self.child.take() else {
            return true;
        };
        self.stop.store(true, Ordering::SeqCst);
        drop(self.input.take());
        // Unblock a reader waiting to deliver a frame before joining it.
        drop(self.output.take());
        let cleanup = CleanupJob {
            child,
            workers: std::mem::take(&mut self.workers),
        };
        let handle = CleanupHandle::start(cleanup);
        let deadline = Instant::now() + CLEANUP_BOUND;
        while !handle.finished() && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
        let admission = self.admission.take().expect("MCP admission is owned");
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

impl Drop for ProcessIo {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct LimitedFrame(Vec<u8>);

impl Write for LimitedFrame {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_FRAME_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("MCP wire frame bound exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn read_frames(reader: impl Read, sender: SyncSender<ReadEvent>, stop: Arc<AtomicBool>) {
    let mut reader = BufReader::new(reader);
    while !stop.load(Ordering::SeqCst) {
        let mut frame = Vec::new();
        let result = (&mut reader)
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut frame);
        let event = match result {
            Ok(0) => break,
            Ok(_) if frame.len() > MAX_FRAME_BYTES => ReadEvent::Oversized,
            Ok(_) => ReadEvent::Frame(frame),
            Err(_) => ReadEvent::Failed,
        };
        let terminal = !matches!(event, ReadEvent::Frame(_));
        if sender.send(event).is_err() || terminal {
            break;
        }
    }
}

fn write_requests(mut writer: impl Write, requests: Receiver<WriteRequest>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        let request = match requests.recv_timeout(POLL) {
            Ok(request) => request,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if request.token.is_cancelled() || Instant::now() >= request.deadline {
            let _ = request
                .complete
                .send(Err(std::io::Error::other("MCP write expired")));
            break;
        }
        // An attempted pipe write may have delivered a complete request even
        // when its receipt is lost. Encoding and enqueue alone do not dispatch.
        if let Some(dispatch) = &request.tool_dispatch {
            dispatch.store(true, Ordering::SeqCst);
        }
        let result = writer
            .write_all(&request.bytes)
            .and_then(|()| writer.flush());
        let failed = result.is_err();
        if request.complete.send(result).is_err() || failed {
            break;
        }
    }
}

struct CleanupJob {
    child: Child,
    workers: Vec<JoinHandle<()>>,
}

impl CleanupJob {
    fn run(mut self) -> bool {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            // Exit may race kill. The subsequent wait is the authority; a
            // failed kill alone does not disprove an observed original exit.
            let _ = self.child.kill();
        }
        let exited = self.child.wait().is_ok();
        let mut joined = true;
        for worker in self.workers {
            joined = worker.join().is_ok() && joined;
        }
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
            .name("forme-mcp-cleanup".into())
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
        // Retain the original job when the cleanup thread could not be created.
        let released = self.custody.lock().is_ok_and(|job| job.is_none());
        let joined = self
            .thread
            .is_some_and(|thread| thread.join().unwrap_or(false));
        released && joined
    }
}
