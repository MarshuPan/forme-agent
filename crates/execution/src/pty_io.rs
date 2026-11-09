use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use forme_protocol as p;
use portable_pty::{Child, MasterPty};

const CLEANUP_BOUND: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(5);
const MAX_ACTIVE: usize = 8;

#[derive(Default)]
struct Supervisors {
    active: usize,
    pending: Vec<JoinHandle<p::Result<()>>>,
    cleanup_failed: bool,
}

fn supervisors() -> &'static Mutex<Supervisors> {
    static SUPERVISORS: OnceLock<Mutex<Supervisors>> = OnceLock::new();
    SUPERVISORS.get_or_init(Mutex::default)
}

pub(super) struct Admission;

impl Admission {
    pub(super) fn acquire() -> p::Result<Self> {
        let mut state = supervisors()
            .lock()
            .map_err(|_| p::Error("PTY cleanup supervision is unavailable".into()))?;
        let mut index = 0;
        while index < state.pending.len() {
            if state.pending[index].is_finished() {
                let worker = state.pending.swap_remove(index);
                state.cleanup_failed |= !matches!(worker.join(), Ok(Ok(())));
                state.active -= 1;
            } else {
                index += 1;
            }
        }
        if state.cleanup_failed || !state.pending.is_empty() || state.active >= MAX_ACTIVE {
            return Err(p::Error(
                "PTY execution is blocked by capacity or original cleanup".into(),
            ));
        }
        state.active += 1;
        Ok(Self)
    }

    fn retain(self, worker: JoinHandle<p::Result<()>>) {
        if let Ok(mut state) = supervisors().lock() {
            state.pending.push(worker);
            std::mem::forget(self);
        }
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
    Bytes(Vec<u8>),
    Failed,
}

pub(super) struct TerminalIo {
    pub(super) output: Receiver<ReadEvent>,
    pub(super) input_complete: Arc<AtomicBool>,
    pub(super) write_failed: Arc<AtomicBool>,
    controls: SyncSender<Vec<u8>>,
    stop: Arc<AtomicBool>,
    discard: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl TerminalIo {
    pub(super) fn start(
        mut reader: Box<dyn Read + Send>,
        mut writer: Box<dyn Write + Send>,
        input: Option<Vec<u8>>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let discard = Arc::new(AtomicBool::new(false));
        let input_complete = Arc::new(AtomicBool::new(input.as_ref().is_none_or(Vec::is_empty)));
        let write_failed = Arc::new(AtomicBool::new(false));
        let (sender, output) = mpsc::sync_channel(16);
        let read_stop = discard.clone();
        let read_thread = std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) if !read_stop.load(Ordering::SeqCst) => {
                        send_read(
                            &sender,
                            ReadEvent::Bytes(buffer[..count].to_vec()),
                            &read_stop,
                        );
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => break,
                    Err(_) => {
                        send_read(&sender, ReadEvent::Failed, &read_stop);
                        break;
                    }
                }
            }
        });
        let (controls, commands) = mpsc::sync_channel::<Vec<u8>>(4);
        let write_stop = stop.clone();
        let write_complete = input_complete.clone();
        let write_failure = write_failed.clone();
        let write_thread = std::thread::spawn(move || {
            let input = input.unwrap_or_default();
            let mut offset = 0;
            while !write_stop.load(Ordering::SeqCst) {
                let chunk = match commands.try_recv() {
                    Ok(control) => control,
                    Err(_) if offset < input.len() => {
                        let next = (offset + 256).min(input.len());
                        let chunk = input[offset..next].to_vec();
                        offset = next;
                        chunk
                    }
                    Err(_) => match commands.recv_timeout(POLL) {
                        Ok(control) => control,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    },
                };
                if writer
                    .write_all(&chunk)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    write_failure.store(true, Ordering::SeqCst);
                    break;
                }
                if offset == input.len() {
                    write_complete.store(true, Ordering::SeqCst);
                }
            }
        });
        Self {
            output,
            input_complete,
            write_failed,
            controls,
            stop,
            discard,
            threads: vec![read_thread, write_thread],
        }
    }

    pub(super) fn reply(&self, bytes: &[u8]) -> p::Result<()> {
        self.controls
            .try_send(bytes.to_vec())
            .map_err(|_| p::Error("PTY control input is unavailable or exceeds its bound".into()))
    }

    pub(super) fn shutdown(
        self,
        mut child: Box<dyn Child + Send + Sync>,
        master: Box<dyn MasterPty + Send>,
        admission: Admission,
        capture: impl FnMut(&[u8]) -> p::Result<()>,
    ) -> p::Result<()> {
        self.stop.store(true, Ordering::SeqCst);
        let Self {
            output,
            controls,
            threads,
            discard,
            ..
        } = self;
        drop(controls);
        let cleanup = std::thread::spawn(move || {
            let terminated = match child.try_wait() {
                Ok(Some(_)) => true,
                _ => child.kill().is_ok(),
            };
            // ClosePseudoConsole can wait for its output to drain. Keep the
            // reader alive and consume its bounded queue during close.
            drop(master);
            let exited = child.wait().is_ok();
            let mut joined = true;
            for worker in threads {
                // Join every worker even when an earlier worker panicked.
                joined = worker.join().is_ok() && joined;
            }
            if terminated && exited && joined {
                Ok(())
            } else {
                Err(p::Error(
                    "PTY original process or I/O cleanup is unverified".into(),
                ))
            }
        });
        finish_cleanup(output, discard, cleanup, admission, capture)
    }
}

fn finish_cleanup(
    output: Receiver<ReadEvent>,
    discard: Arc<AtomicBool>,
    cleanup: JoinHandle<p::Result<()>>,
    admission: Admission,
    mut capture: impl FnMut(&[u8]) -> p::Result<()>,
) -> p::Result<()> {
    let deadline = Instant::now() + CLEANUP_BOUND;
    let mut output_error = None;
    while !cleanup.is_finished() && Instant::now() < deadline {
        match output.recv_timeout(POLL) {
            Ok(ReadEvent::Bytes(bytes)) if output_error.is_none() => {
                if let Err(error) = capture(&bytes) {
                    output_error = Some(error);
                    discard.store(true, Ordering::SeqCst);
                }
            }
            Ok(ReadEvent::Failed) => {
                output_error = Some(p::Error("PTY output read failed".into()));
                discard.store(true, Ordering::SeqCst);
            }
            Ok(_) => {}
            Err(_) => std::thread::sleep(POLL),
        }
    }
    if !cleanup.is_finished() {
        discard.store(true, Ordering::SeqCst);
        admission.retain(cleanup);
        return Err(p::Error(
            "PTY original cleanup remains under supervision".into(),
        ));
    }
    match cleanup.join() {
        Ok(Ok(())) => {}
        _ => {
            if let Ok(mut state) = supervisors().lock() {
                state.cleanup_failed = true;
            }
            return Err(p::Error(
                "PTY original cleanup could not be verified".into(),
            ));
        }
    }
    drop(admission);
    for event in output.try_iter() {
        match event {
            ReadEvent::Bytes(bytes) if output_error.is_none() => {
                if let Err(error) = capture(&bytes) {
                    output_error = Some(error);
                }
            }
            ReadEvent::Failed => {
                output_error = Some(p::Error("PTY output read failed".into()));
            }
            _ => {}
        }
    }
    output_error.map_or(Ok(()), Err)
}

fn send_read(sender: &SyncSender<ReadEvent>, mut event: ReadEvent, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        match sender.try_send(event) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => return,
            Err(TrySendError::Full(pending)) => event = pending,
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_cleanup_keeps_its_slot_and_blocks_new_programs_until_original_exit() {
        let admission = Admission::acquire().unwrap();
        let (release, held) = mpsc::channel();
        let cleanup = std::thread::spawn(move || {
            held.recv().unwrap();
            Ok(())
        });
        let (sender, output) = mpsc::sync_channel(1);
        drop(sender);
        let started = Instant::now();
        let outcome = finish_cleanup(
            output,
            Arc::new(AtomicBool::new(false)),
            cleanup,
            admission,
            |_| Ok(()),
        );
        assert!(outcome.is_err());
        assert!(started.elapsed() < CLEANUP_BOUND + Duration::from_secs(1));
        assert!(
            Admission::acquire().is_err(),
            "unknown cleanup must retain admission"
        );
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Ok(recovered) = Admission::acquire() {
                drop(recovered);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "original supervisor never released its slot"
            );
            std::thread::sleep(POLL);
        }
    }
}
