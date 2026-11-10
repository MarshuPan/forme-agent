//! Final policy, capability and immutable-plan checks shared by resume and execution.
use crate::*;

impl ReactiveHarness {
    pub(super) fn prepare_action(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        intent: p::ActionIntent,
        mut plan: ExecutionPlan,
        approval: Option<forme_approval::ApprovalGrant>,
    ) -> p::Result<Option<ExecutionPlan>> {
        record.competence_inputs =
            self.refresh_competence(&intent.scope, Some(&intent.capability_ref))?;
        let turn = Some(current_turn(run, record.loop_ctx.turn_index));
        let provenance = internal_provenance(record.request.source);
        let policy_context = self.action_policy_context(run, record, &intent)?;
        let owner_discovery =
            self.owner_discovery_admitted(run, record, &intent, &plan, approval.as_ref())?;
        let mut recheck = self.policy.evaluate_detailed(&policy_context, &intent);
        recheck = apply_policy_floors(
            recheck,
            &intent,
            Some(&plan),
            &policy_context,
            &record.competence_inputs,
            self.competence.as_ref(),
        );
        self.append(
            run.clone(),
            turn.clone(),
            provenance.clone(),
            recheck.event_payload(),
        )?;
        if record.effect_mode == Some(p::EffectMode::CounterfactualDeny) {
            return self
                .deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    p::ReasonRef("simulation_effect_denied".into()),
                    StopReason::ApprovalDenied,
                )
                .map(|()| None);
        }
        if recheck.decision == p::PolicyDecision::Deny {
            return self
                .deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    recheck.reason,
                    StopReason::ApprovalDenied,
                )
                .map(|()| None);
        }
        if recheck.decision == p::PolicyDecision::Ask && approval.is_none() {
            return self
                .deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    p::ReasonRef(format!(
                        "final policy recheck requires approval before dispatch: {}",
                        recheck.reason.0
                    )),
                    StopReason::ApprovalDenied,
                )
                .map(|()| None);
        }
        let capability_recheck = if owner_discovery {
            Ok(())
        } else {
            match &self.capability_rechecker {
                Some(rechecker) => rechecker.recheck(&intent, &policy_context),
                None if matches!(
                    intent.backend_hint,
                    p::BackendKind::Mcp | p::BackendKind::AppApi
                ) =>
                {
                    Err(p::Error(
                        "external capability execution has no registry rechecker".into(),
                    ))
                }
                None => Ok(()),
            }
        };
        if let Err(error) = capability_recheck {
            self.append(
                run.clone(),
                turn,
                provenance,
                p::EventPayload::ActionDenied(p::ActionDeniedPayload {
                    intent_id: intent.intent_id.clone(),
                    reason: p::ReasonRef(error.to_string()),
                }),
            )?;
            self.append_failure(
                run,
                record,
                p::FailureClass::ExecutionFailure,
                &error.to_string(),
            )?;
            record
                .loop_ctx
                .terminate(p::RunStatus::Aborted, StopReason::RetryExhausted);
            return self.finish_terminal(run, handle, record).map(|()| None);
        }
        self.enforce_envelope(&intent, approval.is_some(), &policy_context)?;
        if requires_competence_gate(&intent) {
            let ceiling = self.competence.ceiling(
                intent.scope.clone(),
                intent.risk_hint,
                &record.competence_inputs,
            );
            self.append(
                run.clone(),
                turn.clone(),
                provenance.clone(),
                p::EventPayload::CompetenceGateEvaluated(p::CompetenceGateEvaluatedPayload {
                    scope: intent.scope.clone(),
                    risk: intent.risk_hint,
                    max_level: protocol_intervention(ceiling),
                    reads: record.competence_inputs.protocol_reads(),
                }),
            )?;
            if !owner_discovery
                && ceiling < required_intervention_level(&intent, &plan.rollback_boundary)
            {
                return self
                    .deny_action(
                        run,
                        handle,
                        record,
                        &intent,
                        p::ReasonRef(
                            "competence ceiling downgraded the action below execution".into(),
                        ),
                        StopReason::ApprovalDenied,
                    )
                    .map(|()| None);
            }
        }

        // Re-plan at the execution boundary.  A backend may derive
        // driver-affecting fields from live state, so the plan captured before
        // approval cannot be trusted without this final comparison.
        plan.validate_digest()?;
        // A charter may tighten while an approval is waiting. Re-read it at
        // the final driver boundary; an old approval cannot override it.
        if let Err(error) = self
            .check_project_action(run, &intent)
            .and_then(|_| self.check_workspace_action(&record.workspace, &intent))
        {
            return self
                .deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    p::ReasonRef(error.0),
                    StopReason::ApprovalDenied,
                )
                .map(|()| None);
        }
        if let Some(charter) = self.store.workspace_charter(
            self.charter_workspace_override
                .as_ref()
                .unwrap_or(&record.workspace),
        )? {
            let rules = p::CharterRules::parse(&charter)?;
            if rules.check_action(&intent).is_err() || (rules.require_review && approval.is_none())
            {
                return self
                    .deny_action(
                        run,
                        handle,
                        record,
                        &intent,
                        p::ReasonRef("current workspace charter blocks dispatch".into()),
                        StopReason::ApprovalDenied,
                    )
                    .map(|()| None);
            }
        }
        if plan.intent != intent {
            return self
                .deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    p::ReasonRef("execution plan intent changed before dispatch".into()),
                    StopReason::ApprovalDenied,
                )
                .map(|()| None);
        }
        let current_plan = self.backends.backend(intent.backend_hint)?.plan(&intent)?;
        current_plan.validate_digest()?;
        if current_plan.digest != plan.digest
            || current_plan.normalized_action_digest != plan.normalized_action_digest
            || current_plan.affected_resources != plan.affected_resources
        {
            return self
                .deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    p::ReasonRef(
                        "execution plan changed before dispatch; approval is stale".into(),
                    ),
                    StopReason::ApprovalDenied,
                )
                .map(|()| None);
        }

        let mut final_recheck = self.policy.evaluate_detailed(&policy_context, &intent);
        final_recheck = apply_policy_floors(
            final_recheck,
            &intent,
            Some(&current_plan),
            &policy_context,
            &record.competence_inputs,
            self.competence.as_ref(),
        );
        self.append(
            run.clone(),
            turn.clone(),
            provenance.clone(),
            final_recheck.event_payload(),
        )?;
        record.loop_ctx.mark_policy(final_recheck.decision);
        if final_recheck.decision == p::PolicyDecision::Deny {
            return self
                .deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    final_recheck.reason,
                    StopReason::ApprovalDenied,
                )
                .map(|()| None);
        }
        if final_recheck.decision == p::PolicyDecision::Ask && approval.is_none() {
            return self
                .deny_action(
                    run,
                    handle,
                    record,
                    &intent,
                    p::ReasonRef(format!(
                        "final policy recheck requires approval before dispatch: {}",
                        final_recheck.reason.0
                    )),
                    StopReason::ApprovalDenied,
                )
                .map(|()| None);
        }
        plan = current_plan;

        if let Some(grant) = approval.as_ref() {
            if plan
                .approval_ref
                .as_ref()
                .is_some_and(|approval_ref| approval_ref != &grant.approval_id)
            {
                return Err(p::Error(
                    "execution plan approval reference does not match the grant".into(),
                ));
            }
            if grant.bound_plan_digest != plan.digest
                || grant.normalized_action_digest != plan.normalized_action_digest
                || grant.affected_resources != plan.affected_resources
                || grant.policy_version != self.config.policy_version
                || grant.tool_schema_version != self.config.tool_schema_version
            {
                return Err(p::Error(
                    "approval binding does not match the final execution plan".into(),
                ));
            }
            plan = plan.with_approval(grant.approval_id.clone());
        } else if plan.approval_ref.is_some() {
            return Err(p::Error(
                "unapproved execution plan carries an approval reference".into(),
            ));
        }
        plan.validate_digest()?;
        if record.remaining_action_budget == Some(0) {
            record.loop_ctx.terminate(
                p::RunStatus::Limited,
                StopReason::BudgetExhausted(forme_loop::BudgetKind::ToolCalls),
            );
            return self.finish_terminal(run, handle, record).map(|()| None);
        }
        Ok(Some(plan))
    }
}
