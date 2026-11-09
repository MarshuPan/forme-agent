use std::io::{ErrorKind, Read};
use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use forme_protocol as p;

use crate::support::BoundedOutput;
use crate::{CancelToken, EventSink, ExecutionPlan};

const POLL: Duration = Duration::from_millis(5);
const CLEANUP_BOUND: Duration = Duration::from_secs(2);
const MAX_ACTIVE: usize = 8;

#[derive(Default)]
struct Supervision {
    active: usize,
    pending: Vec<CleanupHandle>,
    failed: bool,
}

fn supervision() -> &'static Mutex<Supervision> {
    static STATE: OnceLock<Mutex<Supervision>> = OnceLock::new();
    STATE.get_or_init(Mutex::default)
}

pub(super) struct Admission;

impl Admission {
    pub(super) fn acquire() -> p::Result<Self> {
        let mut state = supervision()
            .lock()
            .map_err(|_| p::Error("shell cleanup supervision is unavailable".into()))?;
        let mut index = 0;
        while index < state.pending.len() {
            if state.pending[index].finished() {
                let original = state.pending.swap_remove(index);
                state.failed |= !original.join();
                state.active -= 1;
            } else {
                index += 1;
            }
        }
        if state.failed || !state.pending.is_empty() || state.active >= MAX_ACTIVE {
            return Err(p::Error(
                "shell admission is blocked by capacity or original cleanup".into(),
            ));
        }
        state.active += 1;
        Ok(Self)
    }

    fn retain(self, handle: CleanupHandle) {
        // Keep the exact child/readers even if the OS could not create a
        // cleanup thread. A subsequent action cannot replace this custody.
        match supervision().lock() {
            Ok(mut state) => state.pending.push(handle),
            Err(poisoned) => poisoned.into_inner().pending.push(handle),
        }
        std::mem::forget(self);
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        if let Ok(mut state) = supervision().lock() {
            state.active = state.active.saturating_sub(1);
        }
    }
}

pub(super) enum ProcessOutcome {
    Exited(ExitStatus),
    Cancelled,
    TimedOut,
}

enum ReadEvent {
    Data(Vec<u8>),
    Failed,
}

pub(super) struct ProcessIo {
    child: Option<Child>,
    readers: Vec<JoinHandle<()>>,
    output: Option<Receiver<ReadEvent>>,
    stop: Arc<AtomicBool>,
    admission: Option<Admission>,
}

impl ProcessIo {
    pub(super) fn new(child: Child, admission: Admission) -> Self {
        Self {
            child: Some(child),
            readers: Vec::new(),
            output: None,
            stop: Arc::new(AtomicBool::new(false)),
            admission: Some(admission),
        }
    }

    fn start_readers(&mut self) -> p::Result<()> {
        let child = self
            .child
            .as_mut()
            .ok_or_else(|| p::Error("original shell process is unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| p::Error("shell process stdout is unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| p::Error("shell process stderr is unavailable".into()))?;
        let (sender, receiver) = mpsc::sync_channel(16);
        self.output = Some(receiver);
        for (name, pipe) in [
            (
                "forme-shell-stdout",
                Box::new(stdout) as Box<dyn Read + Send>,
            ),
            (
                "forme-shell-stderr",
                Box::new(stderr) as Box<dyn Read + Send>,
            ),
        ] {
            let sender = sender.clone();
            let stop = self.stop.clone();
            self.readers.push(
                std::thread::Builder::new()
                    .name(name.into())
                    .spawn(move || read_output(pipe, sender, stop))
                    .map_err(|_| p::Error("shell output reader could not start".into()))?,
            );
        }
        Ok(())
    }

    pub(super) fn monitor(
        &mut self,
        plan: &ExecutionPlan,
        sink: &EventSink,
        token: &CancelToken,
        output: &mut BoundedOutput,
        started: Instant,
    ) -> p::Result<ProcessOutcome> {
        self.start_readers()?;
        let mut status = None;
        let mut output_closed = false;
        loop {
            if token.is_cancelled() {
                return Ok(ProcessOutcome::Cancelled);
            }
            if started.elapsed() >= Duration::from_millis(plan.timeout.0) {
                return Ok(ProcessOutcome::TimedOut);
            }
            let receiver = self
                .output
                .as_ref()
                .ok_or_else(|| p::Error("shell output receiver is unavailable".into()))?;
            match receiver.recv_timeout(POLL) {
                Ok(ReadEvent::Data(bytes)) => output.push(&bytes, plan, sink)?,
                Ok(ReadEvent::Failed) => {
                    return Err(p::Error("shell output could not be read".into()));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => output_closed = true,
            }
            if status.is_none() {
                status = self
                    .child
                    .as_mut()
                    .ok_or_else(|| p::Error("original shell process is unavailable".into()))?
                    .try_wait()
                    .map_err(|_| p::Error("shell process exit could not be observed".into()))?;
            }
            if let Some(status) = status {
                if output_closed {
                    return Ok(ProcessOutcome::Exited(status));
                }
            } else if output_closed {
                std::thread::sleep(POLL);
            }
        }
    }

    pub(super) fn shutdown(&mut self) -> bool {
        let Some(child) = self.child.take() else {
            return true;
        };
        self.stop.store(true, Ordering::SeqCst);
        // Wake a reader blocked in send; a reader blocked in OS read remains
        // owned by the cleanup supervisor until the original pipe closes.
        drop(self.output.take());
        let handle = CleanupHandle::start(CleanupJob {
            child,
            readers: std::mem::take(&mut self.readers),
        });
        let started = Instant::now();
        while !handle.finished() && started.elapsed() < CLEANUP_BOUND {
            std::thread::sleep(POLL);
        }
        if handle.finished() {
            let confirmed = handle.join();
            if !confirmed {
                match supervision().lock() {
                    Ok(mut state) => state.failed = true,
                    Err(poisoned) => poisoned.into_inner().failed = true,
                }
            }
            drop(self.admission.take());
            confirmed
        } else {
            if let Some(admission) = self.admission.take() {
                admission.retain(handle);
            }
            false
        }
    }
}

impl Drop for ProcessIo {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn read_output(mut pipe: impl Read, sender: SyncSender<ReadEvent>, stop: Arc<AtomicBool>) {
    let mut buffer = [0_u8; 4_096];
    while !stop.load(Ordering::SeqCst) {
        let event = match pipe.read(&mut buffer) {
            Ok(0) => return,
            Ok(count) => ReadEvent::Data(buffer[..count].to_vec()),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => {
                let _ = sender.send(ReadEvent::Failed);
                return;
            }
        };
        if sender.send(event).is_err() {
            return;
        }
    }
}

struct CleanupJob {
    child: Child,
    readers: Vec<JoinHandle<()>>,
}

impl CleanupJob {
    fn run(mut self) -> bool {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        // A kill/exit race is resolved by wait, not the kill return value.
        let exited = self.child.wait().is_ok();
        let mut joined = true;
        for reader in self.readers {
            joined = reader.join().is_ok() && joined;
        }
        exited && joined
    }
}

struct CleanupHandle {
    worker: Option<JoinHandle<bool>>,
    custody: Arc<Mutex<Option<CleanupJob>>>,
}

impl CleanupHandle {
    fn start(job: CleanupJob) -> Self {
        let custody = Arc::new(Mutex::new(Some(job)));
        let owned = custody.clone();
        let worker = std::thread::Builder::new()
            .name("forme-shell-cleanup".into())
            .spawn(move || {
                owned
                    .lock()
                    .ok()
                    .and_then(|mut job| job.take())
                    .is_some_and(CleanupJob::run)
            })
            .ok();
        Self { worker, custody }
    }

    fn finished(&self) -> bool {
        self.worker.as_ref().is_some_and(JoinHandle::is_finished)
    }

    fn join(self) -> bool {
        let released = self.custody.lock().is_ok_and(|job| job.is_none());
        let joined = self
            .worker
            .is_some_and(|worker| worker.join().unwrap_or(false));
        released && joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_read_failure_is_reported_instead_of_eof() {
        struct BrokenPipe;
        impl Read for BrokenPipe {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("fixture read failure"))
            }
        }
        let (sender, receiver) = mpsc::sync_channel(16);
        read_output(BrokenPipe, sender, Arc::new(AtomicBool::new(false)));
        assert!(matches!(receiver.recv().unwrap(), ReadEvent::Failed));
        assert!(receiver.recv().is_err());
    }
}
