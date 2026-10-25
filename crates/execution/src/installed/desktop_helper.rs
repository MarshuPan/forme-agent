use forme_protocol as p;

/// The desktop helper is an execution placement only. It receives an exact,
/// already-authorized action and cannot create a grant or choose a session.
pub trait DesktopHelperAdapter: Send + Sync {
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
