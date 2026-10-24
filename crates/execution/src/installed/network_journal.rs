use forme_protocol as p;

use super::{invalid_journal, JournalFact, OperationState, ProductJournal};

pub(super) fn validate_stage_receipt(
    state: &OperationState,
    stage: p::MaintenanceStage,
    network_result: Option<&p::ProductNetworkResult>,
) -> p::Result<()> {
    if stage == p::MaintenanceStage::Checking && state.network_contract && network_result.is_none()
    {
        return Err(invalid_journal());
    }
    if let Some(result) = network_result {
        result.validate()?;
    }
    Ok(())
}

pub(super) fn apply_progress(
    state: &mut OperationState,
    progress: &p::ProductNetworkProgress,
    revision: u64,
) -> p::Result<()> {
    progress.validate()?;
    if state.view.pending_stage != Some(p::MaintenanceStage::Checking) {
        return Err(invalid_journal());
    }
    let valid_transition = matches!(
        (
            state.network_progress.as_ref().map(|value| value.phase),
            progress.phase,
        ),
        (None, p::ProductNetworkProgressPhase::TargetIntent)
            | (
                Some(p::ProductNetworkProgressPhase::TargetIntent),
                p::ProductNetworkProgressPhase::TargetVerified,
            )
            | (
                Some(p::ProductNetworkProgressPhase::TargetIntent),
                p::ProductNetworkProgressPhase::FallbackIntent,
            )
            | (
                Some(p::ProductNetworkProgressPhase::FallbackIntent),
                p::ProductNetworkProgressPhase::FallbackVerified,
            )
    );
    if !valid_transition
        || (state.network_progress.is_some()
            && state.network_progress.as_ref().is_some_and(|previous| {
                previous.requested_listener_digest != progress.requested_listener_digest
            }))
    {
        return Err(invalid_journal());
    }
    state.network_progress = Some(progress.clone());
    state.view.network_result = progress.result();
    state.view.revision = revision;
    Ok(())
}

impl ProductJournal {
    pub fn record_network_progress(
        &self,
        id: &str,
        expected: u64,
        progress: p::ProductNetworkProgress,
        now: p::Timestamp,
    ) -> p::Result<()> {
        self.append(
            expected,
            JournalFact::NetworkProgress {
                operation_id: id.into(),
                progress,
                at: now,
            },
        )
    }
}
