//! Restart recovery never executes an uncertain action or replays a resolved handoff.
use crate::*;

impl ReactiveHarness {
    pub(super) fn suspend_unknown_action(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        action: p::ActionId,
    ) -> p::Result<()> {
        let pending = PendingKind::ToolInterrupt(action.clone());
        let waiting = self.append_waiting(run, record, "unknown_action_outcome", &pending)?;
        record.loop_ctx.suspend(pending.clone());
        record.resume_state = Some(ResumeState {
            schema_version: p::SchemaVersion(1),
            run: run.clone(),
            at: LoopState::Suspended(pending.clone()),
            pending,
            snapshot_ref: waiting,
        });
        record.recovered_unknown = Some(action);
        handle.changed.notify_all();
        Ok(())
    }

    pub(crate) fn require_manual_outcome_allowed(record: &RunRecord) -> p::Result<()> {
        // Discovery success requires the original Host receipt and protected
        // catalog. The public marker only denies here; it grants no authority.
        if record.owner_discovery_bound
            || (record.request.source == p::Source::OwnerControl
                && record.request.input.0.starts_with("managed-mcp-discovery:"))
        {
            return Err(p::Error(
                "managed discovery requires verification of the original worker job".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn resume_unknown(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        action: p::ActionId,
        outcome: ResolvedOutcome,
    ) -> p::Result<()> {
        if record.recovered_unknown.as_ref() != Some(&action) {
            return Err(p::Error("tool adjudication targets another action".into()));
        }
        Self::require_manual_outcome_allowed(record)?;
        self.append(
            run.clone(),
            None,
            internal_provenance(p::Source::Internal),
            p::EventPayload::RunResumed(p::RunResumedPayload {
                wait_reason: p::WaitReason("unknown_action_outcome".into()),
                resume_ref: p::ResumeRef(format!("manual-outcome:{}", action.0)),
            }),
        )?;
        let (payload, status, reason) = match outcome {
            ResolvedOutcome::Completed(result_ref, _) => (
                p::EventPayload::ActionCompleted(p::ActionCompletedPayload {
                    intent_id: action,
                    result_ref,
                    receipt: None,
                    remote_receipt: None,
                }),
                p::RunStatus::Complete,
                StopReason::FinalOutput,
            ),
            ResolvedOutcome::Failed(failure_ref, _) => (
                p::EventPayload::ActionFailed(p::ActionFailedPayload {
                    intent_id: action,
                    failure_ref,
                    remote_lease: None,
                }),
                p::RunStatus::Failed,
                StopReason::RetryExhausted,
            ),
            ResolvedOutcome::Cancelled(reason_ref) => (
                p::EventPayload::ActionCancelled(p::ActionCancelledPayload {
                    intent_id: action,
                    reason: reason_ref,
                }),
                p::RunStatus::Aborted,
                StopReason::UserCancel,
            ),
        };
        self.append(
            run.clone(),
            None,
            internal_provenance(p::Source::Internal),
            payload,
        )?;
        record.loop_ctx.terminate(status, reason);
        record.resume_state = None;
        record.recovered_unknown = None;
        self.finish_terminal(run, handle, record)
    }

    pub fn recover_unknown_outcomes(&self) -> p::Result<Vec<p::RunId>> {
        let mut recovered = Vec::new();
        for run in self.store.run_ids()? {
            if self.store.require_current_execution(&run).is_err() {
                continue;
            }
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            if !events.iter().any(|e| e.kind == p::EventKind::RunAccepted) {
                continue;
            }
            let lock = self.session_lock(&run_session(&events)?)?;
            let _guard = lock
                .lock()
                .map_err(|_| p::Error("recovery session is unavailable".into()))?;
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            if run_summary_from_events(run.clone(), &events)?
                .result
                .is_some()
            {
                continue;
            }
            if let Ok(handle) = self.run_handle(&run) {
                let mut record = handle
                    .record
                    .lock()
                    .map_err(|_| p::Error("recovery state is unavailable".into()))?;
                if matches!(record.loop_ctx.state, LoopState::Terminal(_))
                    && record.result.is_none()
                {
                    self.finish_terminal(&run, &handle, &mut record)?;
                    recovered.push(run);
                }
                continue;
            }
            let started = events.iter().find(|e| match &e.payload {
                p::EventPayload::ActionStarted(p) => !events
                    .iter()
                    .any(|e| action_terminal(e) == Some(&p.intent_id)),
                _ => false,
            });
            if let Some(started) = started {
                self.restore_unknown(&run, &events, started)?;
                recovered.push(run);
                continue;
            }
            if let Some(started) = unfinished_model_call(&events) {
                self.append(
                    run.clone(),
                    started.turn_id.clone(),
                    internal_provenance(p::Source::Internal),
                    p::EventPayload::RunFailed(p::RunFailedPayload {
                        stop_reason: p::StopReason(
                            "model_call_outcome_unknown_after_restart".into(),
                        ),
                        result_ref: None,
                    }),
                )?;
                recovered.push(run);
                continue;
            }
            // Discovery completion still needs verified catalog publication.
            // Its public marker can only restrict generic recovery, never grant it.
            if events.iter().any(|e| {
                matches!(&e.payload, p::EventPayload::RunAccepted(v)
                if v.source == p::Source::OwnerControl
                    && v.input_ref.0.starts_with("managed-mcp-discovery:"))
            }) {
                continue;
            }
            // Finish only a previously uncertain action, never an ordinary
            // completed tool whose loop still needed a model continuation.
            let adjudicated = events.iter().rev().find_map(|e| match &e.payload {
                p::EventPayload::ActionOutcomeUnknown(p) => events.iter().find(|later| {
                    later.stream_seq > e.stream_seq && action_terminal(later) == Some(&p.intent_id)
                }),
                _ => None,
            });
            if let Some(terminal) = adjudicated {
                let (status, reason) = match &terminal.payload {
                    p::EventPayload::ActionCompleted(_) => {
                        (p::RunStatus::Complete, StopReason::FinalOutput)
                    }
                    p::EventPayload::ActionFailed(_) => {
                        (p::RunStatus::Failed, StopReason::RetryExhausted)
                    }
                    _ => (p::RunStatus::Aborted, StopReason::UserCancel),
                };
                let mut record = self.recovered_observation_record(
                    &run,
                    &events,
                    PendingKind::ToolInterrupt(
                        action_terminal(terminal)
                            .ok_or_else(|| p::Error("recovery action is missing".into()))?
                            .clone(),
                    ),
                    terminal.event_id.clone(),
                )?;
                record.loop_ctx.terminate(status, reason);
                let handle = Arc::new(RunHandle::new(record));
                self.finish_terminal(
                    &run,
                    &handle,
                    &mut *handle
                        .record
                        .lock()
                        .map_err(|_| p::Error("recovery state is unavailable".into()))?,
                )?;
                self.insert_recovered(&run, handle)?;
                recovered.push(run);
                continue;
            }
            if let Some(checkpoint) = events
                .iter()
                .rev()
                .find(|e| e.kind == p::EventKind::HandoffRequested)
            {
                let p::EventPayload::HandoffRequested(handoff) = &checkpoint.payload else {
                    continue;
                };
                let resolved = events.iter().any(|e| e.stream_seq > checkpoint.stream_seq && matches!(&e.payload, p::EventPayload::HandoffResolved(p) if p.target == handoff.target));
                if resolved && !events.iter().any(|e| e.stream_seq > checkpoint.stream_seq && matches!(&e.payload, p::EventPayload::RunWaiting(p) if p.wait_reason.0 == "handoff_continuation_unknown")) {
                    self.append(run.clone(), checkpoint.turn_id.clone(), internal_provenance(p::Source::Internal), p::EventPayload::RunWaiting(p::RunWaitingPayload {
                        wait_reason: p::WaitReason("handoff_continuation_unknown".into()), resume_ref: p::ResumeRef(checkpoint.event_id.0.clone()),
                    }))?;
                }
                if !self.default_recovery_binding_matches(&events) {
                    continue;
                }
                let mut record = self.recovered_record(
                    &run,
                    &events,
                    PendingKind::Handoff(handoff.target.clone()),
                    checkpoint.event_id.clone(),
                )?;
                // A resolved continuation can be inspected or cancelled, but
                // cannot re-enter resume and repeat a model/tool operation.
                if resolved {
                    record.resume_state = None;
                }
                self.insert_recovered(&run, Arc::new(RunHandle::new(record)))?;
                recovered.push(run);
            }
        }
        Ok(recovered)
    }

    fn restore_unknown(
        &self,
        run: &p::RunId,
        events: &[p::Event],
        started: &p::Event,
    ) -> p::Result<()> {
        let p::EventPayload::ActionStarted(action) = &started.payload else {
            return Ok(());
        };
        let existing = events.iter().find(|e| matches!(&e.payload, p::EventPayload::ActionOutcomeUnknown(p) if p.intent_id == action.intent_id));
        let snapshot = match existing {
            Some(event) => event.event_id.clone(),
            None => self.append(
                run.clone(),
                started.turn_id.clone(),
                internal_provenance(p::Source::Internal),
                p::EventPayload::ActionOutcomeUnknown(p::ActionOutcomeUnknownPayload {
                    intent_id: action.intent_id.clone(),
                    probe_hint: p::ProbeHintRef(
                        "manual adjudication required; action was not retried".into(),
                    ),
                    remote_lease: None,
                }),
            )?,
        };
        if existing.is_none() || !events.iter().any(|e| matches!(&e.payload, p::EventPayload::RunWaiting(p) if p.wait_reason.0 == "unknown_action_outcome")) {
            self.append(run.clone(), started.turn_id.clone(), internal_provenance(p::Source::Internal), p::EventPayload::RunWaiting(p::RunWaitingPayload {
                wait_reason: p::WaitReason("unknown_action_outcome".into()), resume_ref: p::ResumeRef(format!("resume:{}", snapshot.0)),
            }))?;
        }
        let mut record = self.recovered_observation_record(
            run,
            events,
            PendingKind::ToolInterrupt(action.intent_id.clone()),
            snapshot,
        )?;
        record.recovered_unknown = Some(action.intent_id.clone());
        self.insert_recovered(run, Arc::new(RunHandle::new(record)))
    }

    pub(crate) fn insert_recovered(&self, run: &p::RunId, handle: Arc<RunHandle>) -> p::Result<()> {
        self.runs
            .lock()
            .map_err(|_| p::Error("recovery registry is unavailable".into()))?
            .insert(run.clone(), handle);
        Ok(())
    }

    pub(super) fn default_recovery_binding_matches(&self, events: &[p::Event]) -> bool {
        let accepted = events
            .iter()
            .filter_map(|e| match &e.payload {
                p::EventPayload::RunAccepted(p) => Some(p),
                _ => None,
            })
            .collect::<Vec<_>>();
        let bound = events
            .iter()
            .filter_map(|e| match &e.payload {
                p::EventPayload::SessionBound(p) => Some(p),
                _ => None,
            })
            .collect::<Vec<_>>();
        if accepted.len() != 1 || bound.len() != 1 {
            return false;
        }
        let b = bound[0];
        accepted[0].source == p::Source::UserTurn
            && b.workspace == self.config.workspace
            && b.policy_profile == self.config.policy_profile
            && b.model_profile == self.config.model_profile
            && events.first().is_some_and(|event| {
                self.validate_tool_binding(&event.run_id, &b.toolset_ref)
                    .is_ok()
            })
            && b.composition_manifest == self.composition_manifest
            && matches!(b.effect_mode, None | Some(p::EffectMode::LiveGoverned))
            && b.evolution_snapshot.is_none()
            && b.federation_snapshot.is_none()
    }

    pub(crate) fn recovered_record(
        &self,
        run: &p::RunId,
        events: &[p::Event],
        pending: PendingKind,
        snapshot_ref: p::EventId,
    ) -> p::Result<RunRecord> {
        self.recovered_record_inner(run, events, pending, snapshot_ref, false)
    }

    fn recovered_observation_record(
        &self,
        run: &p::RunId,
        events: &[p::Event],
        pending: PendingKind,
        snapshot_ref: p::EventId,
    ) -> p::Result<RunRecord> {
        let mut record = self.recovered_record_inner(run, events, pending, snapshot_ref, true)?;
        // Unknown outcomes expose only observation/adjudication. They can never
        // restart a model loop, including legacy runs without a tools original.
        record.mode = RunMode::DirectAction;
        Ok(record)
    }

    fn recovered_record_inner(
        &self,
        run: &p::RunId,
        events: &[p::Event],
        pending: PendingKind,
        snapshot_ref: p::EventId,
        inspection_only: bool,
    ) -> p::Result<RunRecord> {
        let accepted = events
            .iter()
            .find_map(|e| match &e.payload {
                p::EventPayload::RunAccepted(p) => Some(p),
                _ => None,
            })
            .ok_or_else(|| p::Error("recoverable run has no acceptance".into()))?;
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: accepted.source,
            session: p::SessionRef(accepted.session_ref.0.clone()),
            agent_profile: p::AgentProfileRef("agent:recovered".into()),
            input: p::RunInput(accepted.input_ref.0.clone()),
            budget: None,
            idempotency_key: accepted.idempotency_key.clone(),
        };
        let bound = events.iter().find_map(|e| match &e.payload {
            p::EventPayload::SessionBound(p) => Some(p),
            _ => None,
        });
        let workspace = bound
            .map(|b| b.workspace.clone())
            .unwrap_or_else(|| self.config.workspace.clone());
        let mut config = self.config.clone();
        config.workspace = workspace.clone();
        let context = if inspection_only {
            default_context(&request, run, &config)?
        } else {
            (self.context_factory)(&request, run, &config)?
        };
        let mut loop_ctx = forme_loop::RunCtx::new_with_input_provenance(
            accepted.session_ref.clone(),
            request.input.clone(),
            context,
            self.config.context_budget,
            self.config.loop_budget.clone(),
            request_provenance(&request),
        )?;
        loop_ctx.turn_index = events
            .iter()
            .filter_map(|e| match &e.payload {
                p::EventPayload::TurnStarted(p) => Some(p.turn_index),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        loop_ctx.tool_calls = events
            .iter()
            .filter(|e| e.kind == p::EventKind::ActionStarted)
            .count()
            .try_into()
            .unwrap_or(u32::MAX);
        loop_ctx.tokens_used = events
            .iter()
            .filter_map(|e| match &e.payload {
                p::EventPayload::ModelCallFinished(p) => {
                    Some(p.usage.input_tokens.saturating_add(p.usage.output_tokens))
                }
                _ => None,
            })
            .fold(0u64, u64::saturating_add);
        loop_ctx.suspend(pending.clone());
        Ok(RunRecord {
            request,
            workspace,
            loop_ctx,
            result: None,
            final_output: None,
            resume_state: Some(ResumeState {
                schema_version: p::SchemaVersion(1),
                run: run.clone(),
                at: LoopState::Suspended(pending.clone()),
                pending,
                snapshot_ref,
            }),
            pending_action: None,
            recovered_unknown: None,
            approval: InMemoryApprovalBroker::default(),
            mode: RunMode::Loop,
            direct_intent: None,
            owner_discovery_bound: false,
            tools: if inspection_only {
                None
            } else {
                self.run_tool_snapshot(run)?
            },
            bound_envelope: None,
            effect_mode: bound.and_then(|b| b.effect_mode),
            evolution_snapshot: bound.and_then(|b| b.evolution_snapshot.clone()),
            evolution_snapshot_full: None,
            federation_snapshot: bound.and_then(|b| b.federation_snapshot.clone()),
            m3_binding: None,
            remaining_action_budget: None,
            competence_inputs: self.competence_snapshot.clone(),
            skills_prepared: false,
            evolution_simulation: None,
            long_horizon: None,
            model_selection: None,
            active_model: None,
            fallback_index: 0,
        })
    }
}

fn unfinished_model_call(events: &[p::Event]) -> Option<&p::Event> {
    events.iter().rev().find(|event| {
        let p::EventPayload::ModelCallStarted(started) = &event.payload else {
            return false;
        };
        !events.iter().any(|later| {
            later.stream_seq > event.stream_seq
                && matches!(
                    &later.payload,
                    p::EventPayload::ModelCallFinished(finished)
                        if finished.call_id == started.call_id
                )
        })
    })
}

fn action_terminal(event: &p::Event) -> Option<&p::ActionId> {
    match &event.payload {
        p::EventPayload::ActionCompleted(p) => Some(&p.intent_id),
        p::EventPayload::ActionFailed(p) => Some(&p.intent_id),
        p::EventPayload::ActionCancelled(p) => Some(&p.intent_id),
        p::EventPayload::ActionDenied(p) => Some(&p.intent_id),
        _ => None,
    }
}
