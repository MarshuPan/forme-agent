//! Owner control serialization and durable resolution claims (prd/03 §5.2.1).
use crate::*;

impl ReactiveHarness {
    pub(super) fn apply_run_control(&self, run: p::RunId, control: p::RunControl) -> p::Result<()> {
        control.validate()?;
        match &control {
            p::RunControl::ResolveApproval(d) => self.resume_run(
                run,
                ResumeInput::Approval(ApprovalGrant {
                    schema_version: d.schema_version,
                    approval_id: d.approval_id.clone(),
                    outcome: d.outcome,
                    granted_scope: GrantScope::OneShot,
                    approver: d.approver.clone(),
                    bound_plan_digest: d.bound_plan_digest.clone(),
                    normalized_action_digest: d.normalized_action_digest.clone(),
                    affected_resources: d.affected_resources.clone(),
                    policy_version: d.policy_version,
                    tool_schema_version: d.tool_schema_version,
                    nonce: d.nonce.clone(),
                    use_by: d.use_by,
                }),
                None,
            ),
            p::RunControl::Cancel(_) => AgentHarness::cancel(self, run),
            p::RunControl::ResolveToolOutcome { resolution: r, .. } => {
                let outcome = match r.outcome {
                    p::ToolOutcomeKind::Completed => ResolvedOutcome::Completed(
                        r.result_ref.clone().ok_or_else(invalid_resolution)?,
                        r.summary.clone(),
                    ),
                    p::ToolOutcomeKind::Failed => ResolvedOutcome::Failed(
                        r.failure_ref.clone().ok_or_else(invalid_resolution)?,
                        r.summary.clone(),
                    ),
                    p::ToolOutcomeKind::Cancelled => {
                        ResolvedOutcome::Cancelled(r.reason.clone().ok_or_else(invalid_resolution)?)
                    }
                };
                self.resume_run(
                    run,
                    ResumeInput::ToolOutcome(r.action.clone(), outcome),
                    Some(control),
                )
            }
            p::RunControl::ResolveHandoff { resolution: r, .. } => self.resume_run(
                run,
                ResumeInput::Handoff(HandoffResolution {
                    schema_version: r.schema_version,
                    target: r.target.clone(),
                    accepted: r.accepted,
                    result: r.result.clone(),
                }),
                Some(control),
            ),
        }
    }

    pub(super) fn resume_run(
        &self,
        run: p::RunId,
        input: ResumeInput,
        command: Option<p::RunControl>,
    ) -> p::Result<()> {
        self.store.require_current_execution(&run)?;
        if let Some(child) = self.child_executor(&run)? {
            return child.resume_run(run, input, command);
        }
        if matches!(&input, ResumeInput::Approval(_)) && self.run_handle(&run).is_err() {
            self.restore_default_formal_approval(&run)?;
        }
        let before = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let session_lock = self.session_lock(&run_session(&before)?)?;
        let _session_guard = session_lock
            .lock()
            .map_err(|_| p::Error("session queue is unavailable".into()))?;
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let claim = command
            .as_ref()
            .map(|command| self.resolution_claim(&run, command, &input, &events))
            .transpose()?;
        if let Some(event) = &claim {
            match self.store.probe_domain_claim(event)? {
                forme_store::DomainClaimProbe::Committed(original) if original == run => {
                    if has_resolution_receipt(&events, event, &input)
                        && matches!(input, ResumeInput::Handoff(_))
                    {
                        return Ok(());
                    }
                    if has_resolution_receipt(&events, event, &input) {
                        let current = run_summary_from_events(run.clone(), &events)?;
                        if current.result.is_some() {
                            return Ok(());
                        }
                        drop(_session_guard);
                        self.recover_unknown_outcomes()?;
                        let refreshed = self
                            .store
                            .read_run(run.clone())
                            .collect::<p::Result<Vec<_>>>()?;
                        return if run_summary_from_events(run, &refreshed)?.result.is_some() {
                            Ok(())
                        } else {
                            Err(unknown_resolution())
                        };
                    }
                    // Exact semantic replay may finish the recorded
                    // adjudication; it does not repeat the external action.
                }
                forme_store::DomainClaimProbe::Conflict(_)
                | forme_store::DomainClaimProbe::Committed(_) => {
                    return Err(p::Error(
                        "control command conflicts with its original binding".into(),
                    ));
                }
                forme_store::DomainClaimProbe::Unclaimed => {}
                forme_store::DomainClaimProbe::NotIdempotent => return Err(invalid_resolution()),
            }
        }
        let handle = self.run_handle(&run)?;
        let mut record = handle
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?;
        let resume = record
            .resume_state
            .clone()
            .ok_or_else(|| p::Error("run is not suspended".into()))?;
        if !matches_pending(&resume.pending, &input) {
            return Err(invalid_resolution());
        }
        if !matches!(&input, ResumeInput::Approval(_)) {
            let target = checkpoint_digest(&run, &input, &events)?;
            let replay_command = claim
                .as_ref()
                .map(p::ControlResolutionBinding::from_event)
                .transpose()?
                .flatten()
                .map(|binding| binding.command_digest);
            for event in &events {
                if p::ControlResolutionBinding::from_event(event)?.is_some_and(|binding| {
                    binding.target_digest == target
                        && Some(binding.command_digest) != replay_command
                }) {
                    return Err(unknown_resolution());
                }
            }
            if let ResumeInput::Handoff(resolution) = &input {
                // The frozen handoff wire request has no checkpoint version.
                // Reusing a target cannot safely distinguish an old reply.
                if events.iter().filter(|e| matches!(&e.payload, p::EventPayload::HandoffRequested(h) if h.target == resolution.target)).count() != 1 {
                    return Err(p::Error("handoff target was reused; inspect or cancel the original run".into()));
                }
            }
        }
        if let Some(event) = &claim {
            let original = command.as_ref().ok_or_else(invalid_resolution)?;
            self.store
                .append_control_resolution_claim(event.clone(), original.clone())?;
        }
        match input {
            ResumeInput::Approval(grant) => {
                self.resume_approval(&run, &handle, &mut record, grant)?;
                if !record.loop_ctx.state.is_terminal() && !record.loop_ctx.state.is_suspended() {
                    drop(record);
                    self.drive(&run, &handle)?;
                }
                Ok(())
            }
            ResumeInput::ToolOutcome(action, outcome) => {
                self.resume_unknown(&run, &handle, &mut record, action, outcome)
            }
            ResumeInput::Handoff(resolution) => {
                self.append(
                    run.clone(),
                    Some(current_turn(&run, record.loop_ctx.turn_index)),
                    internal_provenance(record.request.source),
                    p::EventPayload::HandoffResolved(p::HandoffResolvedPayload {
                        target: resolution.target.clone(),
                        reason: p::ReasonRef(
                            if resolution.accepted {
                                "handoff accepted"
                            } else {
                                "handoff rejected"
                            }
                            .into(),
                        ),
                    }),
                )?;
                record.loop_ctx.continue_after_handoff(resolution)?;
                record.resume_state = None;
                self.persist_loop_events(&run, &mut record)?;
                if record.loop_ctx.state.is_terminal() {
                    self.finish_terminal(&run, &handle, &mut record)
                } else {
                    drop(record);
                    self.drive(&run, &handle)
                }
            }
        }
    }

    fn resolution_claim(
        &self,
        run: &p::RunId,
        command: &p::RunControl,
        input: &ResumeInput,
        events: &[p::Event],
    ) -> p::Result<p::Event> {
        let key = match command {
            p::RunControl::ResolveToolOutcome { command, .. }
            | p::RunControl::ResolveHandoff { command, .. } => command,
            _ => return Err(invalid_resolution()),
        };
        let command_digest = p::canonical_digest(&("control-resolution-v1", key))?;
        let mut original_target = None;
        for event in events {
            if let Some(binding) = p::ControlResolutionBinding::from_event(event)? {
                if binding.command_digest == command_digest {
                    original_target = Some(binding.target_digest);
                    break;
                }
            }
        }
        let target_digest = match original_target {
            Some(value) => value,
            None => checkpoint_digest(run, input, events)?,
        };
        let owner_digest = self
            .store
            .content_protection()?
            .ok_or_else(|| p::Error("control resolution protection unavailable".into()))?
            .owner_digest()
            .clone();
        let binding = p::ControlResolutionBinding {
            schema_version: p::SchemaVersion(1),
            command_digest,
            resolution_digest: p::canonical_digest(&(&owner_digest, run, command))?,
            target_digest,
        };
        Ok(p::Event::new(
            p::EventId(format!(
                "control:{}:{}",
                now_nanos(),
                self.event_sequence.fetch_add(1, Ordering::SeqCst)
            )),
            run.clone(),
            None,
            p::EventPayload::RunResumed(p::RunResumedPayload {
                wait_reason: p::WaitReason("owner_resolution_claim".into()),
                resume_ref: binding.as_ref(),
            }),
            p::SchemaVersion(1),
            now_ms(),
            p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
        ))
    }

    pub(super) fn pending_view(&self, run: p::RunId) -> p::Result<p::RunPendingView> {
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let mut view = p::project_run_pending(run.clone(), &events)?;
        let command_digest = match view.pending.as_ref() {
            Some(p::RunPendingKind::ResolutionUnknown {
                command_digest: Some(command_digest),
                reason:
                    p::ResolutionUncertainty::ClaimWithoutResult
                    | p::ResolutionUncertainty::ActionWithoutRunTerminal,
                ..
            }) => command_digest.clone(),
            _ => return Ok(view),
        };
        let mut claim = None;
        for event in events.iter().rev() {
            if let Some(binding) = p::ControlResolutionBinding::from_event(event)? {
                if binding.command_digest == command_digest {
                    claim = Some((event, binding));
                    break;
                }
            }
        }
        let Some((claim_event, _binding)) = claim else {
            return Ok(view);
        };
        if let Ok(Some(command)) = self.read_durable_control_resolution(&run, claim_event) {
            if let Some(p::RunPendingKind::ResolutionUnknown { replay_command, .. }) =
                view.pending.as_mut()
            {
                *replay_command = Some(command);
            }
        }
        Ok(view)
    }

    fn read_durable_control_resolution(
        &self,
        run: &p::RunId,
        claim_event: &p::Event,
    ) -> p::Result<Option<p::RunControl>> {
        self.store.read_control_resolution_claim(run, claim_event)
    }
}

impl ProductionRuntime {
    pub fn run_pending_view(&self, run: p::RunId) -> p::Result<p::RunPendingView> {
        let _data_access = self.data_read_guard()?;
        self.harness().pending_view(run)
    }
}

fn matches_pending(pending: &PendingKind, input: &ResumeInput) -> bool {
    match (pending, input) {
        (PendingKind::ApprovalWait(a), ResumeInput::Approval(b)) => a == &b.approval_id,
        (PendingKind::ToolInterrupt(a), ResumeInput::ToolOutcome(b, _)) => a == b,
        (PendingKind::Handoff(a), ResumeInput::Handoff(b)) => a == &b.target,
        _ => false,
    }
}

fn checkpoint_digest(
    run: &p::RunId,
    input: &ResumeInput,
    events: &[p::Event],
) -> p::Result<p::SchemaDigest> {
    let checkpoint = events
        .iter()
        .rev()
        .find(|e| match (&e.payload, input) {
            (p::EventPayload::ActionOutcomeUnknown(p), ResumeInput::ToolOutcome(action, _)) => {
                &p.intent_id == action
            }
            (p::EventPayload::HandoffRequested(p), ResumeInput::Handoff(r)) => p.target == r.target,
            _ => false,
        })
        .ok_or_else(invalid_resolution)?;
    p::canonical_digest(&(run, &checkpoint.event_id))
}

fn has_resolution_receipt(events: &[p::Event], claim: &p::Event, input: &ResumeInput) -> bool {
    let Some(original) = events
        .iter()
        .find(|e| e.payload == claim.payload && e.provenance == claim.provenance)
    else {
        return false;
    };
    events
        .iter()
        .filter(|e| e.stream_seq > original.stream_seq)
        .any(|event| match (&event.payload, input) {
            (
                p::EventPayload::ActionCompleted(p),
                ResumeInput::ToolOutcome(action, ResolvedOutcome::Completed(result, _)),
            ) => &p.intent_id == action && &p.result_ref == result,
            (
                p::EventPayload::ActionFailed(p),
                ResumeInput::ToolOutcome(action, ResolvedOutcome::Failed(failure, _)),
            ) => &p.intent_id == action && &p.failure_ref == failure,
            (
                p::EventPayload::ActionCancelled(p),
                ResumeInput::ToolOutcome(action, ResolvedOutcome::Cancelled(reason)),
            ) => &p.intent_id == action && &p.reason == reason,
            (p::EventPayload::HandoffResolved(p), ResumeInput::Handoff(r)) => {
                p.target == r.target
                    && p.reason.0
                        == if r.accepted {
                            "handoff accepted"
                        } else {
                            "handoff rejected"
                        }
            }
            _ => false,
        })
}

fn invalid_resolution() -> p::Error {
    p::Error("ResumeInput does not match the persisted PendingKind".into())
}
fn unknown_resolution() -> p::Error {
    p::Error("control resolution outcome is unknown; inspect the original command".into())
}
