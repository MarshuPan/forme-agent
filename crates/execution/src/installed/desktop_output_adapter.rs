//! One authenticated output peer per original grant; late observation cannot relaunch it.
use super::*;
use crate::installed::{
    windows_desktop_output_runtime::WindowsDesktopOutputPeer,
    windows_desktop_session_native::WindowsDesktopSessionSnapshot, DesktopOutputSink,
};

pub(super) enum OutputPeerState {
    Ready(WindowsDesktopOutputPeer),
    Executed { grant: String, verified: bool },
}
impl OutputPeerState {
    fn grant(&self) -> &str {
        match self {
            Self::Ready(peer) => peer.binding_key(),
            Self::Executed { grant, .. } => grant,
        }
    }
}
impl WindowsDesktopHelperAdapter {
    pub(super) fn observe_output(
        &self,
        binding: &p::DesktopSessionBinding,
        snapshot: &WindowsDesktopSessionSnapshot,
        now: p::Timestamp,
    ) -> p::Result<()> {
        if !self.session.supports_output() {
            return Err(unavailable());
        }
        let mut state = self.output_peer.lock().map_err(|_| unavailable())?;
        if state
            .as_ref()
            .is_some_and(|value| value.grant() != binding.grant_ref)
        {
            *state = None;
        }
        if state.is_none() {
            *state = Some(OutputPeerState::Ready(WindowsDesktopOutputPeer::connect(
                &self.session,
                binding,
                snapshot,
            )?));
        }
        match state.as_mut().ok_or_else(unavailable)? {
            OutputPeerState::Ready(peer) => {
                let remaining = binding
                    .expires_at
                    .checked_sub(now)
                    .filter(|time| *time > 0)
                    .ok_or_else(unavailable)?;
                peer.ping(Instant::now() + Duration::from_millis((remaining as u64).min(2000)))
            }
            // `observe` already inspected the current WTS user/generation and
            // desktop before arriving here. The original exchange authenticated
            // the peer; its one-shot helper exits after returning Finish.
            OutputPeerState::Executed { verified: true, .. } => Ok(()),
            OutputPeerState::Executed {
                verified: false, ..
            } => Err(unavailable()),
        }
    }
    pub(super) fn execute_output(
        &self,
        request: &p::DesktopActionRequest,
        intent: &p::ActionIntent,
        sink: &mut dyn DesktopOutputSink,
    ) -> p::Result<p::DesktopActionReceipt> {
        if !self.session.supports_output() {
            return Err(unavailable());
        }
        request.binding.validate_at(now_ms()?)?;
        sink.transfer().binding.validate_request(request)?;
        sink.transfer().validate_intent(intent)?;
        self.session.inspect(&request.binding)?;
        let mut state = self.output_peer.lock().map_err(|_| unavailable())?;
        let peer = match state.take() {
            Some(OutputPeerState::Ready(peer))
                if peer.binding_key() == request.binding.grant_ref =>
            {
                peer
            }
            previous => {
                *state = previous;
                return Err(unavailable());
            }
        };
        *state = Some(OutputPeerState::Executed {
            grant: request.binding.grant_ref.clone(),
            verified: false,
        });
        let receipt = peer.execute(request, intent, sink)?;
        self.session.inspect(&request.binding)?;
        receipt.validate_for(request)?;
        *state = Some(OutputPeerState::Executed {
            grant: request.binding.grant_ref.clone(),
            verified: true,
        });
        Ok(receipt)
    }
}
fn unavailable() -> p::Error {
    p::Error("desktop output peer is unavailable for the original grant".into())
}
