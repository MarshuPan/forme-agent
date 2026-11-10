//! Request-specific admission preserves effective Deny and actual competence.
use super::*;
use forme_policy::{
    ActionMatcher, ArgMatcher, DelegationGrant, DelegationSubject, PolicyLayer, PolicyLayerSource,
    PolicyRule,
};

impl ReactiveHarness {
    pub(crate) fn action_policy_context(
        &self,
        run: &p::RunId,
        record: &RunRecord,
        intent: &p::ActionIntent,
    ) -> p::Result<PolicyContext> {
        let mut context = self.governance.policy_context(
            record.loop_ctx.session.clone(),
            self.config.toolset_ref.clone(),
            &self.policy,
            record.bound_envelope.as_ref(),
        );
        let Some(original) = self.bound_discovery(run, record, intent)? else {
            return Ok(context);
        };
        let p::ActionParameters::McpDiscovery(spec) = &intent.parameters else {
            return Err(invalid());
        };
        if self
            .governance
            .envelope
            .as_ref()
            .is_some_and(|v| v.approval_rule == p::ApprovalRule::Deny)
            || self
                .governance
                .delegation
                .as_ref()
                .is_some_and(|v| v.envelope.approval_rule == p::ApprovalRule::Deny)
        {
            return Err(invalid());
        }
        let mut layers = context.policy.layers().to_vec();
        layers.push(PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::Session,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                effect: p::PolicyDecision::Ask,
                scope: intent.scope.clone(),
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::Mcp),
                    capability: Some(intent.capability_ref.clone()),
                    action_type: Some(p::ActionType::Execute),
                    parameters: ArgMatcher::McpDiscoveryTarget {
                        server: spec.server.clone(),
                        program_ref: spec.program_ref.clone(),
                        action_digest: p::normalized_action_digest(intent)?,
                    },
                },
            }],
        });
        context.policy = self.policy.merge(&layers);
        // This is an owner configuration grant for exactly one request, never
        // a change to the model's toolset or the shared Agent delegation.
        context.visible_capabilities = vec![intent.capability_ref.clone()];
        context.granted_permissions = original.envelope.capability.permissions.clone();
        context.allowed_scopes = vec![intent.scope.clone()];
        context.sandbox_available = true;
        context.delegation = Some(DelegationGrant {
            schema_version: p::SchemaVersion(1),
            subject: DelegationSubject::Owner,
            envelope: original.envelope.clone(),
            granted_by: p::Actor::Owner,
            audit_ref: self
                .store
                .read_run(run.clone())
                .next()
                .ok_or_else(invalid)??
                .event_id,
        });
        context.envelope = Some(original.envelope);
        Ok(context)
    }

    pub(crate) fn observe_owner_discovery_competence(
        &self,
        run: &p::RunId,
        record: &RunRecord,
        intent: &p::ActionIntent,
    ) -> p::Result<()> {
        if self.bound_discovery(run, record, intent)?.is_none() {
            return Ok(());
        }
        let ceiling = self.competence.ceiling(
            intent.scope.clone(),
            intent.risk_hint,
            &record.competence_inputs,
        );
        self.append(
            run.clone(),
            Some(current_turn(run, record.loop_ctx.turn_index)),
            internal_provenance(record.request.source),
            p::EventPayload::CompetenceGateEvaluated(p::CompetenceGateEvaluatedPayload {
                scope: intent.scope.clone(),
                risk: intent.risk_hint,
                max_level: protocol_intervention(ceiling),
                reads: record.competence_inputs.protocol_reads(),
            }),
        )?;
        Ok(())
    }

    pub(crate) fn owner_discovery_admitted(
        &self,
        run: &p::RunId,
        record: &RunRecord,
        intent: &p::ActionIntent,
        plan: &ExecutionPlan,
        grant: Option<&forme_approval::ApprovalGrant>,
    ) -> p::Result<bool> {
        let Some(original) = self.bound_discovery(run, record, intent)? else {
            return Ok(false);
        };
        let grant = grant.ok_or_else(invalid)?;
        let pending: PendingDiscovery = read(self, run, "pending")?.ok_or_else(invalid)?;
        if pending.schema_version != p::SchemaVersion(1)
            || pending.original != original
            || pending.plan.restore()? != *plan
            || grant.approver != original.owner
            || grant.outcome != p::ApprovalOutcome::Granted
            || grant.granted_scope != GrantScope::OneShot
            || grant.approval_id != pending.approval.approval_id
            || grant.bound_plan_digest != plan.digest
            || pending.approval.expires_at <= now_ms()
        {
            return Err(invalid());
        }
        Ok(true)
    }

    pub(crate) fn bound_discovery_deadline(
        &self,
        run: &p::RunId,
        record: &RunRecord,
        intent: &p::ActionIntent,
    ) -> p::Result<Option<i64>> {
        Ok(self
            .bound_discovery(run, record, intent)?
            .map(|v| v.envelope.timebox.expires_at))
    }
}
