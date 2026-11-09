use forme_protocol as p;

#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use super::windows_desktop_helper_runtime::WindowsDesktopHelperPeer;
#[cfg(windows)]
use super::windows_desktop_session_native::WindowsDesktopSessionHost;

/// The desktop helper is an execution placement only. It receives an exact,
/// already-authorized action and cannot create a grant or choose a session.
pub trait DesktopHelperAdapter: Send + Sync {
    fn supports_output(&self) -> bool {
        false
    }
    fn execute_with_output(
        &self,
        request: &p::DesktopActionRequest,
        intent: &p::ActionIntent,
        _sink: &mut dyn super::DesktopOutputSink,
    ) -> p::Result<p::DesktopActionReceipt> {
        if request.capability == p::DesktopCapability::Screenshot {
            return Err(p::Error("desktop output is unavailable".into()));
        }
        self.execute(request, intent)
    }
    fn observe(
        &self,
        binding: &p::DesktopSessionBinding,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopSessionObservation>;

    fn execute(
        &self,
        request: &p::DesktopActionRequest,
        intent: &p::ActionIntent,
    ) -> p::Result<p::DesktopActionReceipt>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableDesktopHelper;

impl DesktopHelperAdapter for UnavailableDesktopHelper {
    fn observe(
        &self,
        binding: &p::DesktopSessionBinding,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopSessionObservation> {
        Ok(p::DesktopSessionObservation {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            helper_ref: binding.helper_ref.clone(),
            grant_ref: None,
            state: p::DesktopSessionState::Unavailable,
            liveness: p::DesktopSessionLiveness::Lost,
            os_user_ref: None,
            os_session_ref: None,
            session_generation: None,
            platform: binding.platform,
            desktop_ref: None,
            peer_ref: None,
            receipt_ref: None,
            reason_ref: Some("reason:desktop-helper-unavailable".into()),
            observed_at: now,
        })
    }

    fn execute(
        &self,
        _request: &p::DesktopActionRequest,
        _intent: &p::ActionIntent,
    ) -> p::Result<p::DesktopActionReceipt> {
        Err(p::Error(
            "desktop helper is unavailable; interactive execution is blocked".into(),
        ))
    }
}

/// The production Windows adapter keeps all native handles in the two audited
/// modules. This safe wrapper owns only the current grant's peer connection;
/// a connection is consumed by one exact action and its Job closes with it.
#[cfg(windows)]
pub struct WindowsDesktopHelperAdapter {
    session: WindowsDesktopSessionHost,
    peer: Mutex<Option<WindowsDesktopHelperPeer>>,
}

#[cfg(windows)]
impl WindowsDesktopHelperAdapter {
    pub fn new(helper_image: impl AsRef<Path>) -> p::Result<Self> {
        Ok(Self {
            session: WindowsDesktopSessionHost::new(helper_image.as_ref())?,
            peer: Mutex::new(None),
        })
    }
}

#[cfg(windows)]
impl DesktopHelperAdapter for WindowsDesktopHelperAdapter {
    fn observe(
        &self,
        binding: &p::DesktopSessionBinding,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopSessionObservation> {
        binding.validate_at(now)?;
        if binding.platform != p::DesktopPlatform::Windows {
            return Err(p::Error(
                "Windows desktop helper received another platform".into(),
            ));
        }
        let snapshot = self.session.inspect(binding)?;
        let mut peer = self
            .peer
            .lock()
            .map_err(|_| p::Error("desktop helper peer state is unavailable".into()))?;
        if peer
            .as_ref()
            .is_some_and(|current| current.binding_key() != binding.grant_ref)
        {
            *peer = None;
        }
        if peer.is_none() {
            *peer = Some(WindowsDesktopHelperPeer::connect(
                &self.session,
                binding,
                &snapshot,
            )?);
        }
        let ping_deadline = Instant::now()
            + Duration::from_millis(binding.expires_at.saturating_sub(now).clamp(1, 2_000) as u64);
        peer.as_mut()
            .ok_or_else(|| p::Error("desktop helper peer is unavailable".into()))?
            .ping(ping_deadline)?;
        Ok(p::DesktopSessionObservation {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            helper_ref: binding.helper_ref.clone(),
            grant_ref: Some(binding.grant_ref.clone()),
            state: p::DesktopSessionState::Ready,
            liveness: p::DesktopSessionLiveness::Live,
            os_user_ref: Some(snapshot.os_user_ref),
            os_session_ref: Some(snapshot.os_session_ref),
            session_generation: Some(snapshot.session_generation),
            platform: p::DesktopPlatform::Windows,
            desktop_ref: Some(snapshot.desktop_ref),
            peer_ref: Some("peer:desktop-helper".into()),
            receipt_ref: None,
            reason_ref: None,
            observed_at: now,
        })
    }

    fn execute(
        &self,
        request: &p::DesktopActionRequest,
        intent: &p::ActionIntent,
    ) -> p::Result<p::DesktopActionReceipt> {
        let now = now_ms()?;
        request.binding.validate_at(now)?;
        let capability = required_capability(intent)?;
        if capability != request.capability
            || p::normalized_action_digest(intent)? != request.normalized_action_digest
        {
            return Err(p::Error(
                "desktop helper request is not bound to the action".into(),
            ));
        }
        let mut peer = self
            .peer
            .lock()
            .map_err(|_| p::Error("desktop helper peer state is unavailable".into()))?;
        let peer = peer
            .take()
            .filter(|current| current.binding_key() == request.binding.grant_ref)
            .ok_or_else(|| p::Error("desktop helper peer is not bound to the grant".into()))?;
        let receipt = peer.execute(request, intent)?;
        receipt.validate_for(request)?;
        Ok(receipt)
    }
}

#[cfg(windows)]
impl DesktopHelperAdapter for Option<WindowsDesktopHelperAdapter> {
    fn observe(
        &self,
        binding: &p::DesktopSessionBinding,
        now: p::Timestamp,
    ) -> p::Result<p::DesktopSessionObservation> {
        match self {
            Some(adapter) => adapter.observe(binding, now),
            None => UnavailableDesktopHelper.observe(binding, now),
        }
    }

    fn execute(
        &self,
        request: &p::DesktopActionRequest,
        intent: &p::ActionIntent,
    ) -> p::Result<p::DesktopActionReceipt> {
        match self {
            Some(adapter) => adapter.execute(request, intent),
            None => UnavailableDesktopHelper.execute(request, intent),
        }
    }
}

pub(super) fn now_ms() -> p::Result<p::Timestamp> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| p::Error("desktop helper clock is unavailable".into()))
        .and_then(|value| {
            i64::try_from(value.as_millis())
                .map_err(|_| p::Error("desktop helper clock overflowed".into()))
        })
}

pub(super) fn validate_live_observation(
    binding: &p::DesktopSessionBinding,
    observation: &p::DesktopSessionObservation,
    now: p::Timestamp,
) -> p::Result<()> {
    observation.validate_for(binding)?;
    if observation.observed_at > now || observation.observed_at >= binding.expires_at {
        return Err(p::Error("desktop session observation is stale".into()));
    }
    Ok(())
}

pub(super) fn required_capability(intent: &p::ActionIntent) -> p::Result<p::DesktopCapability> {
    match &intent.parameters {
        p::ActionParameters::Computer(spec) => Ok(match spec.operation {
            p::ComputerOperation::Move { .. } => p::DesktopCapability::PointerMove,
            p::ComputerOperation::Click { .. } => p::DesktopCapability::PointerClick,
            p::ComputerOperation::Type { .. } => p::DesktopCapability::KeyboardType,
            p::ComputerOperation::Key { .. } => p::DesktopCapability::KeyboardKey,
            p::ComputerOperation::Scroll { .. } => p::DesktopCapability::Scroll,
            p::ComputerOperation::Screenshot => p::DesktopCapability::Screenshot,
        }),
        p::ActionParameters::Pty(_) => Ok(p::DesktopCapability::PtyInput),
        _ => Err(p::Error(
            "desktop session binding requires a computer or PTY action".into(),
        )),
    }
}

pub(super) fn worker_receipt(
    request: &p::DesktopActionRequest,
    receipt: p::DesktopActionReceipt,
    started_at: p::Timestamp,
) -> p::Result<p::WorkerReceipt> {
    receipt.validate_for(request)?;
    if receipt.observed_at < started_at || receipt.observed_at > request.deadline {
        return Err(p::Error(
            "desktop receipt timestamp is outside the action".into(),
        ));
    }
    let observed_outcome = match receipt.outcome {
        p::DesktopActionOutcome::Committed => p::WorkerOutcome::Completed,
        p::DesktopActionOutcome::Failed => p::WorkerOutcome::Failed,
        p::DesktopActionOutcome::Cancelled => p::WorkerOutcome::Cancelled,
        p::DesktopActionOutcome::Unknown => p::WorkerOutcome::Unknown,
    };
    let receipt_ref = p::canonical_digest(&(
        "desktop-action-receipt",
        &receipt.job_id,
        &receipt.permit_ref,
        &receipt.plan_digest,
        &receipt.nonce,
        &receipt.normalized_action_digest,
        receipt.observed_at,
    ))?
    .0;
    Ok(p::WorkerReceipt {
        schema_version: p::SchemaVersion(1),
        receipt_ref,
        job_id: receipt.job_id,
        plan_digest: receipt.plan_digest,
        authority_epoch: request.binding.authority_epoch,
        nonce: receipt.nonce,
        observed_outcome,
        safe_result_ref: receipt.result_ref,
        ground_truth_refs: Vec::new(),
        started_at,
        finished_at: Some(receipt.observed_at),
    })
}
