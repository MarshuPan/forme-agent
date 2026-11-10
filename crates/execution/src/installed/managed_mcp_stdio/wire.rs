//! Bounded native pipe ownership and shared stdout/stderr accounting.
use forme_protocol as p;
use std::{
    io::{PipeReader, PipeWriter, Read, Write},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) struct ChildPipes {
    pub(super) input: PipeReader,
    pub(super) output: PipeWriter,
    pub(super) error: PipeWriter,
}
pub(super) enum ReadEvent {
    Frame(Vec<u8>),
    Eof,
}

pub(super) struct Wire {
    input: Option<PipeWriter>,
    output: Option<PipeReader>,
    error: Option<PipeReader>,
    sender: Option<SyncSender<ReadEvent>>,
    receiver: Option<Receiver<ReadEvent>>,
    readers: Vec<JoinHandle<()>>,
    meter: Arc<Meter>,
    output_eof: bool,
}
struct Meter {
    limit: u64,
    bytes: AtomicU64,
    stderr: AtomicU64,
    failed: AtomicBool,
}
impl Meter {
    fn account(&self, size: usize) -> bool {
        let Ok(size) = u64::try_from(size) else {
            self.fail();
            return false;
        };
        if self
            .bytes
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
                old.checked_add(size).filter(|next| *next <= self.limit)
            })
            .is_err()
        {
            self.fail();
            return false;
        }
        true
    }
    fn fail(&self) {
        self.failed.store(true, Ordering::SeqCst);
    }
}
impl Wire {
    pub(super) fn prepare(limit: u64) -> p::Result<(Self, ChildPipes)> {
        if limit == 0 || limit > 1_048_576 {
            return Err(invalid());
        }
        let (input, to_child) = std::io::pipe().map_err(|_| invalid())?;
        let (from_child, output) = std::io::pipe().map_err(|_| invalid())?;
        let (from_error, error) = std::io::pipe().map_err(|_| invalid())?;
        let (sender, receiver) = mpsc::sync_channel(2);
        Ok((
            Self {
                input: Some(to_child),
                output: Some(from_child),
                error: Some(from_error),
                sender: Some(sender),
                receiver: Some(receiver),
                readers: vec![],
                meter: Arc::new(Meter {
                    limit,
                    bytes: AtomicU64::new(0),
                    stderr: AtomicU64::new(0),
                    failed: AtomicBool::new(false),
                }),
                output_eof: false,
            },
            ChildPipes {
                input,
                output,
                error,
            },
        ))
    }
    /// Keep this Wire in the original worker owner on any partial-start error;
    /// a successfully started reader must not be detached and forgotten.
    pub(super) fn start(&mut self) -> p::Result<()> {
        if !self.readers.is_empty() {
            return Err(invalid());
        }
        let output = self.output.take().ok_or_else(invalid)?;
        let sender = self.sender.take().ok_or_else(invalid)?;
        let meter = self.meter.clone();
        self.readers.push(
            std::thread::Builder::new()
                .name("forme-mcp-stdout".into())
                .spawn(move || read_stdout(output, sender, meter))
                .map_err(|_| invalid())?,
        );
        let error = self.error.take().ok_or_else(invalid)?;
        let meter = self.meter.clone();
        self.readers.push(
            std::thread::Builder::new()
                .name("forme-mcp-stderr".into())
                .spawn(move || read_stderr(error, meter))
                .map_err(|_| invalid())?,
        );
        Ok(())
    }
    /// Gate must check live configuration and registration, then invoke write
    /// within that same exclusion. The independent original-tree supervisor
    /// must already be active, since a synchronous pipe write can block.
    pub(super) fn send(
        &mut self,
        bytes: &[u8],
        gate: impl FnOnce(&mut dyn FnMut() -> p::Result<()>) -> p::Result<()>,
    ) -> p::Result<()> {
        self.healthy()?;
        if bytes.is_empty() || bytes.len() > 1_048_576 || bytes.last() != Some(&b'\n') {
            return Err(invalid());
        }
        let writer = self.input.as_mut().ok_or_else(invalid)?;
        let mut attempted = false;
        gate(&mut || {
            if attempted {
                return Err(invalid());
            }
            attempted = true;
            writer.write_all(bytes).map_err(|_| invalid())
        })?;
        if !attempted {
            return Err(invalid());
        }
        self.healthy()
    }
    pub(super) fn next(
        &mut self,
        deadline: Instant,
        cancelled: &crate::CancelToken,
    ) -> p::Result<ReadEvent> {
        if self.output_eof {
            return Err(invalid());
        }
        loop {
            self.healthy()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if cancelled.is_cancelled() || remaining.is_zero() {
                return Err(invalid());
            }
            match self
                .receiver
                .as_ref()
                .ok_or_else(invalid)?
                .recv_timeout(remaining.min(Duration::from_millis(10)))
            {
                Ok(event) => {
                    self.healthy()?;
                    if matches!(event, ReadEvent::Eof) {
                        self.output_eof = true;
                    }
                    return Ok(event);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(invalid()),
            }
        }
    }
    pub(super) fn close_input(&mut self) {
        self.input.take();
    }
    pub(super) fn abandon(&mut self) {
        self.close_input();
        self.receiver.take();
        self.sender.take();
    }
    /// Nonblocking observation; never join a live pipe reader in the Host
    /// request thread. A false result retains all original worker resources.
    pub(super) fn join_finished(&mut self) -> bool {
        if self.readers.iter().any(|thread| !thread.is_finished()) {
            return false;
        }
        for thread in self.readers.drain(..) {
            if thread.join().is_err() {
                self.meter.fail();
            }
        }
        true
    }
    pub(super) fn stderr_bytes(&self) -> u64 {
        self.meter.stderr.load(Ordering::SeqCst)
    }
    pub(super) fn healthy(&self) -> p::Result<()> {
        if self.meter.failed.load(Ordering::SeqCst) {
            return Err(invalid());
        }
        Ok(())
    }
}
fn read_stdout(mut reader: PipeReader, sender: SyncSender<ReadEvent>, meter: Arc<Meter>) {
    let mut line = Vec::new();
    let mut bytes = [0; 8192];
    loop {
        let length = match reader.read(&mut bytes) {
            Ok(n) => n,
            Err(_) => {
                meter.fail();
                return;
            }
        };
        if length == 0 {
            if !line.is_empty() {
                meter.fail();
                return;
            }
            let _ = sender.send(ReadEvent::Eof);
            return;
        }
        if !meter.account(length) {
            return;
        }
        for byte in &bytes[..length] {
            line.push(*byte);
            if *byte == b'\n'
                && sender
                    .send(ReadEvent::Frame(std::mem::take(&mut line)))
                    .is_err()
            {
                return;
            }
        }
    }
}
fn read_stderr(mut reader: PipeReader, meter: Arc<Meter>) {
    let mut bytes = [0; 8192];
    loop {
        let length = match reader.read(&mut bytes) {
            Ok(0) => return,
            Ok(n) => n,
            Err(_) => {
                meter.fail();
                return;
            }
        };
        if !meter.account(length) {
            return;
        }
        meter.stderr.fetch_add(length as u64, Ordering::SeqCst);
    }
}
fn invalid() -> p::Error {
    p::Error("managed MCP pipe exchange is unavailable or unconfirmed".into())
}
