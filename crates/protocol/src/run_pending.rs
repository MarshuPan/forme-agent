//! Read-only pending projection; does not extend the frozen RunSummary or log.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunPendingView {
    pub schema_version: SchemaVersion,
    pub run: RunId,
    pub snapshot_upper_bound: u64,
    pub pending: Option<RunPendingKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum RunPendingKind {
    ApprovalWait {
        approval: ApprovalId,
        checkpoint: EventId,
    },
    ToolInterrupt {
        action: ActionId,
        checkpoint: EventId,
        probe_hint: ProbeHintRef,
    },
    Handoff {
        target: HandoffTargetRef,
        checkpoint: EventId,
        reason: ReasonRef,
    },
    ResolutionUnknown {
        checkpoint: EventId,
        command_digest: Option<SchemaDigest>,
        reason: ResolutionUncertainty,
        replay_command: Option<RunControl>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionUncertainty {
    ClaimWithoutResult,
    ActionWithoutRunTerminal,
    HandoffContinuation,
    ReusedHandoffTarget,
}

/// Folds one complete, ordered run snapshot. Claims alone never prove success.
pub fn project_run_pending(run: RunId, events: &[Event]) -> Result<RunPendingView> {
    let mut pending = None;
    let mut terminal = false;
    let mut bound = 0u64;
    let mut targets = std::collections::BTreeSet::new();
    for event in events {
        if event.run_id != run || event.stream_seq != bound.saturating_add(1) {
            return Err(Error(
                "pending projection requires one complete ordered run".into(),
            ));
        }
        bound = event.stream_seq;
        if terminal {
            continue;
        }
        match &event.payload {
            EventPayload::RunComplete(_)
            | EventPayload::RunAborted(_)
            | EventPayload::RunFailed(_)
            | EventPayload::RunLimited(_) => {
                terminal = true;
                pending = None;
            }
            EventPayload::ApprovalRequested(p) => {
                pending = Some(RunPendingKind::ApprovalWait {
                    approval: p.approval_id.clone(),
                    checkpoint: event.event_id.clone(),
                })
            }
            EventPayload::ApprovalResolved(_) => pending = None,
            EventPayload::ActionOutcomeUnknown(p) => {
                pending = Some(RunPendingKind::ToolInterrupt {
                    action: p.intent_id.clone(),
                    checkpoint: event.event_id.clone(),
                    probe_hint: p.probe_hint.clone(),
                })
            }
            EventPayload::HandoffRequested(p) => {
                pending = Some(if targets.insert(p.target.clone()) {
                    RunPendingKind::Handoff {
                        target: p.target.clone(),
                        checkpoint: event.event_id.clone(),
                        reason: p.reason.clone(),
                    }
                } else {
                    uncertain(event, None, ResolutionUncertainty::ReusedHandoffTarget)
                });
            }
            EventPayload::RunResumed(_) => {
                pending = match ControlResolutionBinding::from_event(event)? {
                    Some(binding) => Some(uncertain(
                        event,
                        Some(binding.command_digest),
                        ResolutionUncertainty::ClaimWithoutResult,
                    )),
                    None if matches!(pending, Some(RunPendingKind::ToolInterrupt { .. })) => Some(
                        uncertain(event, None, ResolutionUncertainty::ClaimWithoutResult),
                    ),
                    None => None,
                };
            }
            EventPayload::ActionCompleted(_)
            | EventPayload::ActionFailed(_)
            | EventPayload::ActionCancelled(_) => {
                if matches!(
                    pending,
                    Some(
                        RunPendingKind::ToolInterrupt { .. }
                            | RunPendingKind::ResolutionUnknown { .. }
                    )
                ) {
                    let command_digest = match &pending {
                        Some(RunPendingKind::ResolutionUnknown { command_digest, .. }) => {
                            command_digest.clone()
                        }
                        _ => None,
                    };
                    pending = Some(uncertain(
                        event,
                        command_digest,
                        ResolutionUncertainty::ActionWithoutRunTerminal,
                    ));
                }
            }
            EventPayload::HandoffResolved(_) => {
                pending = Some(uncertain(
                    event,
                    None,
                    ResolutionUncertainty::HandoffContinuation,
                ))
            }
            EventPayload::TurnStarted(_) => {
                if matches!(
                    pending,
                    Some(RunPendingKind::ResolutionUnknown {
                        reason: ResolutionUncertainty::HandoffContinuation,
                        ..
                    })
                ) {
                    pending = None;
                }
            }
            EventPayload::RunWaiting(p) if p.wait_reason.0 == "handoff_continuation_unknown" => {
                pending = Some(uncertain(
                    event,
                    None,
                    ResolutionUncertainty::HandoffContinuation,
                ))
            }
            _ => {}
        }
    }
    if events.is_empty() || !events.iter().any(|e| e.kind == EventKind::RunAccepted) {
        return Err(Error("pending projection has no accepted run".into()));
    }
    Ok(RunPendingView {
        schema_version: SchemaVersion(1),
        run,
        snapshot_upper_bound: bound,
        pending,
    })
}

fn uncertain(
    event: &Event,
    command_digest: Option<SchemaDigest>,
    reason: ResolutionUncertainty,
) -> RunPendingKind {
    RunPendingKind::ResolutionUnknown {
        checkpoint: event.event_id.clone(),
        command_digest,
        reason,
        replay_command: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(run: &RunId, sequence: u64, payload: EventPayload) -> Event {
        let mut value = Event::new(
            EventId(format!("event:{sequence}")),
            run.clone(),
            None,
            payload,
            SchemaVersion(1),
            sequence as i64,
            Provenance {
                source: Source::Internal,
                actor: Actor::System,
                trust_tier: TrustTier::VerifiedProcess,
                caused_by: None,
            },
        );
        value.stream_seq = sequence;
        value
    }

    fn accepted(run: &RunId) -> Event {
        event(
            run,
            1,
            EventPayload::RunAccepted(RunAcceptedPayload {
                source: Source::UserTurn,
                session_ref: SessionId("session:projection".into()),
                input_ref: InputRef("work".into()),
                idempotency_key: None,
            }),
        )
    }

    #[test]
    fn preserves_waiting_claims_and_handoff_uncertainty_until_new_work_or_terminal() {
        let run = RunId("run:projection".into());
        let action = ActionId("action:uncertain".into());
        let unknown = event(
            &run,
            2,
            EventPayload::ActionOutcomeUnknown(ActionOutcomeUnknownPayload {
                intent_id: action.clone(),
                probe_hint: ProbeHintRef("inspect".into()),
                remote_lease: None,
            }),
        );
        assert!(matches!(
            project_run_pending(run.clone(), &[accepted(&run), unknown.clone()])
                .unwrap()
                .pending,
            Some(RunPendingKind::ToolInterrupt { .. })
        ));
        let claim = event(
            &run,
            3,
            EventPayload::RunResumed(RunResumedPayload {
                wait_reason: WaitReason("owner_resolution_claim".into()),
                resume_ref: ControlResolutionBinding {
                    schema_version: SchemaVersion(1),
                    command_digest: sha256_content_digest(b"command"),
                    resolution_digest: sha256_content_digest(b"resolution"),
                    target_digest: sha256_content_digest(b"target"),
                }
                .as_ref(),
            }),
        );
        let unrelated = event(
            &run,
            4,
            EventPayload::ObservationRecorded(ObservationRecordedPayload {
                learning_reply: None,
                source: Source::Internal,
                scope: Scope("workspace:test".into()),
                grant_ref: None,
            }),
        );
        assert!(matches!(
            project_run_pending(
                run.clone(),
                &[
                    accepted(&run),
                    unknown.clone(),
                    claim.clone(),
                    unrelated.clone()
                ]
            )
            .unwrap()
            .pending,
            Some(RunPendingKind::ResolutionUnknown {
                reason: ResolutionUncertainty::ClaimWithoutResult,
                ..
            })
        ));
        let action_done = event(
            &run,
            5,
            EventPayload::ActionCompleted(ActionCompletedPayload {
                intent_id: action,
                result_ref: ActionResultRef("owner-result".into()),
                receipt: None,
                remote_receipt: None,
            }),
        );
        assert!(matches!(
            project_run_pending(
                run.clone(),
                &[accepted(&run), unknown, claim, unrelated, action_done]
            )
            .unwrap()
            .pending,
            Some(RunPendingKind::ResolutionUnknown {
                reason: ResolutionUncertainty::ActionWithoutRunTerminal,
                ..
            })
        ));
    }

    #[test]
    fn distinct_pending_projection_never_changes_frozen_summary_or_event_taxonomy() {
        let run = RunId("run:handoff-projection".into());
        let first = accepted(&run);
        let handoff = event(
            &run,
            2,
            EventPayload::HandoffRequested(HandoffRequestedPayload {
                target: HandoffTargetRef("target:one".into()),
                reason: ReasonRef("bounded step".into()),
            }),
        );
        let resolved = event(
            &run,
            3,
            EventPayload::HandoffResolved(HandoffResolvedPayload {
                target: HandoffTargetRef("target:one".into()),
                reason: ReasonRef("handoff accepted".into()),
            }),
        );
        assert!(matches!(
            project_run_pending(
                run.clone(),
                &[first.clone(), handoff.clone(), resolved.clone()]
            )
            .unwrap()
            .pending,
            Some(RunPendingKind::ResolutionUnknown {
                reason: ResolutionUncertainty::HandoffContinuation,
                ..
            })
        ));
        let reused = event(
            &run,
            4,
            EventPayload::HandoffRequested(HandoffRequestedPayload {
                target: HandoffTargetRef("target:one".into()),
                reason: ReasonRef("new checkpoint".into()),
            }),
        );
        assert!(matches!(
            project_run_pending(run.clone(), &[first, handoff, resolved, reused])
                .unwrap()
                .pending,
            Some(RunPendingKind::ResolutionUnknown {
                reason: ResolutionUncertainty::ReusedHandoffTarget,
                ..
            })
        ));
        assert_eq!(EventKind::ALL.len(), 99);
    }
}
