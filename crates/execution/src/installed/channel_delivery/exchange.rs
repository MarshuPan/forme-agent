//! Complete the finite isolated request before invoking the single Host proxy.
use super::super::{
    managed_mcp_stdio::controller::ExchangeResult,
    windows_appcontainer_native::NativeCancellation,
    worker_stdio::{ReadEvent, Wire},
};
use super::*;
use std::sync::atomic::AtomicBool;
pub(super) fn run(
    wire: &mut Wire,
    channels: &InstalledChannelDelivery,
    admission: &Admission,
    cancel: &crate::CancelToken,
    handles: &NativeCancellation,
    attempted: &AtomicBool,
) -> p::Result<ExchangeResult> {
    wire.start()?;
    let request = p::ManagedChannelWorkerRequestV1 {
        schema_version: p::SchemaVersion(1),
        grant_ref: admission.grant.grant_ref.clone(),
    };
    request.validate()?;
    let mut bytes = serde_json::to_vec(&request).map_err(|_| invalid())?;
    bytes.push(b'\n');
    wire.send(&bytes, |write| {
        live(admission.deadline, cancel)?;
        write()
    })?;
    wire.close_input();
    let ReadEvent::Frame(frame) = wire.next(admission.deadline, cancel)? else {
        return Err(invalid());
    };
    let observed: p::ManagedChannelWorkerRequestV1 =
        serde_json::from_slice(&frame).map_err(|_| invalid())?;
    observed.validate()?;
    if observed != request || !matches!(wire.next(admission.deadline, cancel)?, ReadEvent::Eof) {
        return Err(invalid());
    }
    loop {
        live(admission.deadline, cancel)?;
        wire.healthy()?;
        if wire.join_finished() && handles.terminal_state()? == (true, 0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if handles.exit_code()? != 0 || wire.stderr_bytes() != 0 {
        return Err(invalid());
    }
    let bytes = channels.deliver(admission, cancel, attempted)?;
    Ok(ExchangeResult {
        bytes,
        outcome: p::WorkerOutcome::Completed,
    })
}
