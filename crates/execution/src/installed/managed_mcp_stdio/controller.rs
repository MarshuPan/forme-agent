//! A controller owns all I/O; its independent supervisor owns the original Job.
use super::super::{
    managed_mcp_program::DiscoveryAdmission,
    windows_appcontainer_native::{NativeCancellation, NativeWorker},
    InstalledManagedMcpPrograms,
};
use super::{
    exchange::ManagedMcpDiscoveryExchange,
    invalid,
    wire::{ReadEvent, Wire},
};
use forme_protocol as p;
use std::{
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) struct Controller {
    thread: Option<JoinHandle<Completion>>,
    supervisor: Option<JoinHandle<p::Result<()>>>,
    completion: Option<Completion>,
    pub(super) cancelled: crate::CancelToken,
    state: Arc<AtomicU8>,
    supervision_confirmed: bool,
}
struct Completion {
    wire: Wire,
    result: p::Result<Vec<u8>>,
    finished_at: p::Timestamp,
}
impl Controller {
    pub(super) fn prepare() -> Self {
        Self {
            thread: None,
            supervisor: None,
            completion: None,
            cancelled: crate::CancelToken::default(),
            state: Arc::new(AtomicU8::new(0)),
            supervision_confirmed: false,
        }
    }
    /// On partial failure the caller retains this Controller and original tree.
    pub(super) fn start(
        &mut self,
        native: &NativeWorker,
        wire: Wire,
        programs: Arc<InstalledManagedMcpPrograms>,
        admission: DiscoveryAdmission,
    ) -> p::Result<()> {
        if self.thread.is_some() || self.supervisor.is_some() {
            return Err(invalid());
        }
        let handles = Arc::new(native.cancellation()?);
        let supervisor_handles = handles.clone();
        let cancelled = self.cancelled.clone();
        let state = self.state.clone();
        let deadline = admission.deadline;
        self.supervisor = Some(
            std::thread::Builder::new()
                .name("forme-mcp-deadline".into())
                .spawn(move || supervise(supervisor_handles, cancelled, state, deadline))
                .map_err(|_| invalid())?,
        );
        let cancelled = self.cancelled.clone();
        let state = self.state.clone();
        self.thread = match std::thread::Builder::new()
            .name("forme-mcp-controller".into())
            .spawn(move || {
                let mut wire = wire;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    exchange(&mut wire, &programs, &admission, &cancelled, &handles)
                }))
                .unwrap_or_else(|_| Err(invalid()));
                if result.is_err() {
                    wire.abandon();
                }
                state.store(if result.is_ok() { 1 } else { 2 }, Ordering::SeqCst);
                Completion {
                    wire,
                    result,
                    finished_at: super::super::windows_appcontainer_support::recovery_timestamp(),
                }
            }) {
            Ok(thread) => Some(thread),
            Err(_) => {
                self.cancelled.cancel();
                return Err(invalid());
            }
        };
        Ok(())
    }
    /// No joins while a thread is live. Failure to observe cleanup retains the
    /// controller, original handles and source custody in its worker record.
    pub(super) fn poll(&mut self) -> p::Result<bool> {
        if self.thread.as_ref().is_some_and(JoinHandle::is_finished) {
            self.completion = Some(
                self.thread
                    .take()
                    .ok_or_else(invalid)?
                    .join()
                    .map_err(|_| invalid())?,
            );
        }
        if self
            .supervisor
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
        {
            self.supervision_confirmed = self
                .supervisor
                .take()
                .ok_or_else(invalid)?
                .join()
                .map_err(|_| invalid())?
                .is_ok();
        }
        if self.supervisor.is_none()
            && self.thread.is_none()
            && self.completion.is_none()
            && self.state.load(Ordering::SeqCst) == 0
        {
            return Ok(true);
        }
        if !self.supervision_confirmed || self.thread.is_some() || self.supervisor.is_some() {
            return Ok(false);
        }
        // A controller spawn failure started no readers, but still needs the
        // independent supervisor's confirmation before setup can be cleaned.
        Ok(self
            .completion
            .as_mut()
            .is_none_or(|done| done.wire.join_finished()))
    }
    pub(super) fn result(&self) -> p::Result<(&[u8], p::Timestamp)> {
        let done = self.completion.as_ref().ok_or_else(invalid)?;
        done.wire.healthy()?;
        let bytes = done.result.as_ref().map_err(|_| invalid())?;
        Ok((bytes, done.finished_at))
    }
    pub(super) fn cancel(&self) {
        self.cancelled.cancel();
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        // The worker owner retains live controllers on Drop; this also stops a
        // partially constructed supervisor when setup never reached admission.
        self.cancelled.cancel();
        self.state.store(2, Ordering::SeqCst);
    }
}
fn supervise(
    handles: Arc<NativeCancellation>,
    cancelled: crate::CancelToken,
    state: Arc<AtomicU8>,
    deadline: Instant,
) -> p::Result<()> {
    loop {
        let phase = state.load(Ordering::SeqCst);
        if cancelled.is_cancelled() || phase == 2 || Instant::now() >= deadline {
            return handles.cancel();
        }
        if phase == 1 {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn exchange(
    wire: &mut Wire,
    programs: &InstalledManagedMcpPrograms,
    admission: &DiscoveryAdmission,
    cancelled: &crate::CancelToken,
    handles: &NativeCancellation,
) -> p::Result<Vec<u8>> {
    wire.start()?;
    let mut exchange =
        ManagedMcpDiscoveryExchange::new(admission.spec.clone(), admission.deadline)?;
    while let Some(frame) = exchange.next_frame()? {
        wire.send(&frame, |write| {
            programs.with_discovery_egress(admission, cancelled, write)
        })?;
        if exchange.awaiting_reply() {
            match wire.next(admission.deadline, cancelled)? {
                ReadEvent::Frame(frame) => exchange.accept_frame(&frame)?,
                ReadEvent::Eof => return Err(invalid()),
            }
        }
    }
    wire.close_input();
    if !matches!(wire.next(admission.deadline, cancelled)?, ReadEvent::Eof) {
        return Err(invalid());
    }
    loop {
        if cancelled.is_cancelled() || Instant::now() >= admission.deadline {
            return Err(invalid());
        }
        wire.healthy()?;
        if wire.join_finished() && handles.terminal_state()? == (true, 0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if handles.exit_code()? != 0 {
        return Err(invalid());
    }
    exchange.account_stderr(usize::try_from(wire.stderr_bytes()).map_err(|_| invalid())?)?;
    let catalog = exchange.finish()?;
    programs.with_discovery_egress(admission, cancelled, || {
        serde_json::to_vec(&catalog).map_err(|_| invalid())
    })
}
