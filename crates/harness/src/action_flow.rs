//! Proposal, immutable planning and approval waiting at the Harness boundary.
use crate::*;

impl ReactiveHarness {
    pub(super) fn handle_tool(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        tool: forme_models::ModelToolCall,
    ) -> p::Result<Flow> {
        let turn = Some(current_turn(run, record.loop_ctx.turn_index));
        let provenance = internal_provenance(record.request.source);
        self.append(
            run.clone(),
            turn.clone(),
            provenance.clone(),
            p::EventPayload::ToolCallProposed(p::ToolCallProposedPayload {
                call_id: tool.call_id.clone(),
                tool: tool.tool.clone(),
                args: tool.arguments.clone(),
            }),
        )?;
        let Some(mut intent) = tool
            .intent
            .clone()
            .or_else(|| self.resolve_formal_tool(run, &record.workspace, &tool))
        else {
            self.append(
                run.clone(),
                turn,
                provenance,
                p::EventPayload::ActionDenied(p::ActionDeniedPayload {
                    intent_id: p::ActionId(tool.call_id.0),
                    reason: p::ReasonRef(
                        "tool proposal could not resolve to an action intent".into(),
                    ),
                }),
            )?;
            record
                .loop_ctx
                .terminate(p::RunStatus::Aborted, StopReason::HandoffNoTargetOrLoop);
            self.finish_terminal(run, handle, record)?;
            return Ok(Flow::Terminal);
        };
        intent.source = record.request.source;
        record.competence_inputs =
            self.refresh_competence(&intent.scope, Some(&intent.capability_ref))?;
        let context = self.action_policy_context(run, record, &intent)?;
        self.observe_owner_discovery_competence(run, record, &intent)?;
        let mut evaluation = self.policy.evaluate_detailed(&context, &intent);
        if let Err(error) = self
            .check_project_action(run, &intent)
            .and_then(|_| self.check_workspace_action(&record.workspace, &intent))
        {
            evaluation.decision = p::PolicyDecision::Deny;
            evaluation.reason = p::ReasonRef(error.0);
        }
        if let Some(charter) = self.store.workspace_charter(
            self.charter_workspace_override
                .as_ref()
                .unwrap_or(&record.workspace),
        )? {
            let rules = p::CharterRules::parse(&charter)?;
            if let Err(error) = rules.check_action(&intent) {
                evaluation.decision = p::PolicyDecision::Deny;
                evaluation.reason = p::ReasonRef(error.0);
            } else if rules.require_review && evaluation.decision == p::PolicyDecision::Allow {
                evaluation.decision = p::PolicyDecision::Ask;
                evaluation.reason = p::ReasonRef("workspace charter requires owner review".into());
            }
        }
        if record.effect_mode == Some(p::EffectMode::CounterfactualDeny) {
            self.append(run.clone(), turn, provenance, evaluation.event_payload())?;
            record.loop_ctx.mark_policy(p::PolicyDecision::Deny);
            if record.evolution_simulation.is_some() {
                self.complete_evolution_simulation(run, handle, record, &intent)?;
            } else {
                self.deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    p::ReasonRef("simulation_effect_denied".into()),
                    StopReason::ApprovalDenied,
                )?;
            }
            return Ok(Flow::Terminal);
        }
        let plan = if evaluation.decision == p::PolicyDecision::Deny {
            None
        } else {
            Some(self.backends.backend(intent.backend_hint)?.plan(&intent)?)
        };
        evaluation = apply_policy_floors(
            evaluation,
            &intent,
            plan.as_ref(),
            &context,
            &record.competence_inputs,
            self.competence.as_ref(),
        );
        if intent.backend_hint == p::BackendKind::Computer
            && self.desktop_backend.is_some()
            && evaluation.decision == p::PolicyDecision::Allow
        {
            evaluation.decision = p::PolicyDecision::Ask;
            evaluation.reason = p::ReasonRef(
                "managed desktop action requires session consent and owner approval".into(),
            );
        }
        let effective_decision = evaluation.decision;
        self.append(
            run.clone(),
            turn.clone(),
            provenance.clone(),
            evaluation.event_payload(),
        )?;
        record.loop_ctx.mark_policy(effective_decision);
        match effective_decision {
            p::PolicyDecision::Deny => {
                self.deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    evaluation.reason,
                    StopReason::ApprovalDenied,
                )?;
                Ok(Flow::Terminal)
            }
            p::PolicyDecision::Allow | p::PolicyDecision::Ask => {
                let plan = plan.ok_or_else(|| {
                    p::Error("allowed action is missing its immutable execution plan".into())
                })?;
                if effective_decision == p::PolicyDecision::Ask {
                    let approval_id =
                        p::ApprovalId(format!("approval:{}:{}", run.0, record.loop_ctx.turn_index));
                    let mut request = approval_request(
                        approval_id.clone(),
                        &record.loop_ctx.session,
                        &intent,
                        &plan,
                        &self.config,
                    )?;
                    if intent.backend_hint == p::BackendKind::Computer {
                        if let Some(backend) = &self.desktop_backend {
                            backend.register_desktop_pending(
                                &plan,
                                run.clone(),
                                approval_id.clone(),
                                self.config.policy_version,
                                self.config.tool_schema_version,
                                request.expires_at,
                            )?;
                        }
                    }
                    if let Some(deadline) = self.bound_discovery_deadline(run, record, &intent)? {
                        request.expires_at = request.expires_at.min(deadline);
                    }
                    self.persist_owner_discovery_pending(run, record, &intent, &plan, &request)?;
                    let ticket = record.approval.request(request)?;
                    for payload in record.approval.take_events() {
                        self.append(run.clone(), turn.clone(), provenance.clone(), payload)?;
                    }
                    let pending = PendingKind::ApprovalWait(approval_id);
                    let waiting = self.append_waiting(run, record, "approval", &pending)?;
                    record.loop_ctx.suspend(pending.clone());
                    record.resume_state = Some(ResumeState {
                        schema_version: p::SchemaVersion(1),
                        run: run.clone(),
                        at: LoopState::ApprovalWait,
                        pending,
                        snapshot_ref: waiting,
                    });
                    record.pending_action = Some(PendingAction {
                        intent,
                        plan,
                        ticket,
                    });
                    handle.changed.notify_all();
                    Ok(Flow::Suspended)
                } else {
                    self.execute_action(run, handle, record, intent, plan, None)?;
                    Ok(if record.loop_ctx.state.is_terminal() {
                        Flow::Terminal
                    } else if record.loop_ctx.state.is_suspended() {
                        Flow::Suspended
                    } else {
                        Flow::Continue
                    })
                }
            }
        }
    }
}
