//! Approval waiting and desktop preflight preserve the original one-shot flow.
use crate::*;

#[cfg(test)]
mod tests;

impl ReactiveHarness {
    pub(super) fn resume_approval(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
        grant: forme_approval::ApprovalGrant,
    ) -> p::Result<()> {
        self.validate_configured_owner(&grant.approver)?;
        let Some(pending) = record.pending_action.clone() else {
            return Err(p::Error("run has no pending approval action".into()));
        };
        if pending.ticket.0 != grant.approval_id {
            return Err(p::Error("approval grant targets another ticket".into()));
        }
        if requires_one_shot_approval(&pending.intent, &pending.plan.rollback_boundary)
            && grant.granted_scope != GrantScope::OneShot
        {
            record.resume_state = None;
            record.pending_action = None;
            self.deny_action(
                run,
                handle,
                record,
                &pending.intent,
                p::ReasonRef(
                    "high-impact external action requires a one-shot owner approval".into(),
                ),
                StopReason::ApprovalDenied,
            )?;
            return Ok(());
        }
        let prepared_desktop = if pending.intent.backend_hint == p::BackendKind::Computer
            && grant.outcome == p::ApprovalOutcome::Granted
        {
            if let Some(backend) = &self.desktop_backend {
                if grant.granted_scope != GrantScope::OneShot {
                    return Err(p::Error(
                        "managed desktop requires one-shot approval".into(),
                    ));
                }
                backend.require_desktop_consent(
                    &pending
                        .plan
                        .clone()
                        .with_approval(grant.approval_id.clone()),
                )?;
                let Some(plan) = self.prepare_action(
                    run,
                    handle,
                    record,
                    pending.intent.clone(),
                    pending.plan.clone(),
                    Some(grant.clone()),
                )?
                else {
                    return Ok(());
                };
                Some(backend.prepare_desktop(plan)?)
            } else {
                None
            }
        } else {
            None
        };
        record
            .approval
            .resolve(pending.ticket.clone(), grant.clone())?;
        let turn = Some(current_turn(run, record.loop_ctx.turn_index));
        let provenance = internal_provenance(record.request.source);
        for payload in record.approval.take_events() {
            self.append(run.clone(), turn.clone(), provenance.clone(), payload)?;
        }
        self.append(
            run.clone(),
            turn,
            provenance,
            p::EventPayload::RunResumed(p::RunResumedPayload {
                wait_reason: p::WaitReason("approval".into()),
                resume_ref: p::ResumeRef(format!("approval:{}", grant.approval_id.0)),
            }),
        )?;
        record.loop_ctx.mark_approval(grant.outcome);
        if grant.outcome != p::ApprovalOutcome::Granted {
            record.resume_state = None;
            record.pending_action = None;
            self.deny_action(
                run,
                handle,
                record,
                &pending.intent,
                p::ReasonRef("approval was denied or expired".into()),
                StopReason::ApprovalDenied,
            )?;
            return Ok(());
        }
        let authorization = record.approval.authorize(&ApprovalAuthorization {
            approval_id: grant.approval_id.clone(),
            session: record.loop_ctx.session.clone(),
            scope: pending.intent.scope.clone(),
            plan_digest: pending.plan.digest.clone(),
            normalized_action_digest: pending.plan.normalized_action_digest.clone(),
            affected_resources: pending.plan.affected_resources.clone(),
            policy_version: self.config.policy_version,
            tool_schema_version: self.config.tool_schema_version,
            now: now_ms(),
            intent: pending.intent.clone(),
        });
        if let Err(error) = authorization {
            record.resume_state = None;
            record.pending_action = None;
            self.deny_action(
                run,
                handle,
                record,
                &pending.intent,
                p::ReasonRef(error.to_string()),
                StopReason::ApprovalDenied,
            )?;
            return Ok(());
        }
        record.resume_state = None;
        record.pending_action = None;
        let execution = match prepared_desktop {
            Some(plan) => self.execute_prepared_action(
                run,
                handle,
                record,
                pending.intent.clone(),
                plan,
                Some(grant.clone()),
            ),
            None => self.execute_action(
                run,
                handle,
                record,
                pending.intent.clone(),
                pending.plan,
                Some(grant.clone()),
            ),
        };
        if let Err(error) = execution {
            if !record.loop_ctx.state.is_terminal() {
                self.deny_action(
                    run,
                    handle,
                    record,
                    &pending.intent,
                    p::ReasonRef(error.to_string()),
                    StopReason::ApprovalDenied,
                )?;
            }
        }
        Ok(())
    }
}

pub(super) fn approval_request(
    approval_id: p::ApprovalId,
    session: &p::SessionId,
    intent: &p::ActionIntent,
    plan: &ExecutionPlan,
    config: &HarnessConfig,
) -> p::Result<ApprovalRequest> {
    let now = now_ms();
    let ttl = i64::try_from(config.approval_ttl_ms)
        .map_err(|_| p::Error("approval TTL exceeds timestamp range".into()))?;
    Ok(ApprovalRequest {
        schema_version: p::SchemaVersion(1),
        approval_id,
        session: session.clone(),
        action_summary: format!("{:?} action in {}", intent.backend_hint, intent.scope.0),
        risk_level: intent.risk_hint,
        scope: intent.scope.clone(),
        requested_permissions: intent.requested_permissions.clone(),
        normalized_action_digest: plan.normalized_action_digest.clone(),
        affected_resources: plan.affected_resources.clone(),
        rollback_boundary: plan.rollback_boundary.clone(),
        expires_at: now.saturating_add(ttl),
        choices: vec![
            p::ApprovalChoice("grant-once".into()),
            p::ApprovalChoice("deny".into()),
        ],
        plan_digest: plan.digest.clone(),
        policy_version: config.policy_version,
        tool_schema_version: config.tool_schema_version,
    })
}

pub(super) fn pending_approval(run: p::RunId, request: ApprovalRequest) -> p::PendingApproval {
    p::PendingApproval {
        schema_version: request.schema_version,
        run,
        session: request.session,
        approval_id: request.approval_id,
        action_summary: request.action_summary,
        risk_level: request.risk_level,
        scope: request.scope,
        requested_permissions: request.requested_permissions,
        normalized_action_digest: request.normalized_action_digest,
        affected_resources: request.affected_resources,
        rollback_boundary: request.rollback_boundary,
        expires_at: request.expires_at,
        choices: request.choices,
        plan_digest: request.plan_digest,
        policy_version: request.policy_version,
        tool_schema_version: request.tool_schema_version,
    }
}
