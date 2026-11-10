//! Restore only an unchanged pending plan; resolved or ambiguous actions stay closed.
use super::*;

impl ReactiveHarness {
    pub(crate) fn recover_owner_discoveries(&self) -> p::Result<()> {
        for run in self.store.run_ids()? {
            if self.run_handle(&run).is_ok() || self.store.require_current_execution(&run).is_err()
            {
                continue;
            }
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            let ours = events.iter().any(|e| matches!(&e.payload, p::EventPayload::RunAccepted(v)
                if v.source == p::Source::OwnerControl && v.input_ref.0.starts_with("managed-mcp-discovery:")));
            if !ours
                || run_summary_from_events(run.clone(), &events)?
                    .result
                    .is_some()
            {
                continue;
            }
            let session = run_session(&events)?;
            let lock = self.session_lock(&session)?;
            let _lock = lock.lock().map_err(|_| invalid())?;
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            // Generic recovery preserves Unknown; manual completion cannot
            // substitute for a verified discovery original. Never turn a
            // started/completed/resolved action into another approval wait.
            if events.iter().any(|e| {
                matches!(
                    e.kind,
                    p::EventKind::ActionStarted
                        | p::EventKind::ActionCompleted
                        | p::EventKind::ActionOutcomeUnknown
                )
            }) {
                continue;
            }
            if self.restore_discovery_pending(&run, &events).is_err() {
                self.append(
                    run,
                    None,
                    internal_provenance(p::Source::OwnerControl),
                    p::EventPayload::RunFailed(p::RunFailedPayload {
                        stop_reason: p::StopReason("owner_discovery_original_not_resumable".into()),
                        result_ref: None,
                    }),
                )?;
            }
        }
        Ok(())
    }

    fn restore_discovery_pending(&self, run: &p::RunId, events: &[p::Event]) -> p::Result<()> {
        if events.iter().any(|e| {
            matches!(
                e.kind,
                p::EventKind::ApprovalResolved
                    | p::EventKind::ActionPlanned
                    | p::EventKind::ActionDenied
                    | p::EventKind::ActionFailed
                    | p::EventKind::ActionCancelled
                    | p::EventKind::ModelCallStarted
            )
        }) {
            return Err(invalid());
        }
        let original: OwnerRequest = read(self, run, "request")?.ok_or_else(invalid)?;
        self.check_discovery_request(&original)?;
        let pending: PendingDiscovery = read(self, run, "pending")?.ok_or_else(invalid)?;
        if pending.schema_version != p::SchemaVersion(1)
            || pending.original != original
            || pending.approval.run != *run
            || pending.approval.expires_at <= now_ms()
            || pending.verification_passes == 0
        {
            return Err(invalid());
        }
        let plan = pending.plan.restore()?;
        if plan.intent != original.intent || plan.digest != pending.approval.plan_digest {
            return Err(invalid());
        }
        let accepted: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.payload {
                p::EventPayload::RunAccepted(v) => Some(v),
                _ => None,
            })
            .collect();
        let bindings: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.payload {
                p::EventPayload::SessionBound(v) => Some(v),
                _ => None,
            })
            .collect();
        if accepted.len() != 1
            || bindings != vec![&pending.session_binding]
            || accepted[0].source != original.request.source
            || accepted[0].session_ref.0 != original.request.session.0
            || accepted[0].input_ref.0 != original.request.input.0
            || accepted[0].idempotency_key != original.request.idempotency_key
            || pending.session_binding.composition_manifest.as_ref() != Some(&original.composition)
            || pending.session_binding.workspace != self.config.workspace
            || pending.session_binding.policy_profile != self.config.policy_profile
            || pending.session_binding.toolset_ref != self.config.toolset_ref
            || pending.approval.policy_version != self.config.policy_version
            || pending.approval.tool_schema_version != self.config.tool_schema_version
        {
            return Err(invalid());
        }
        let approval = &pending.approval;
        let request = ApprovalRequest {
            schema_version: approval.schema_version,
            approval_id: approval.approval_id.clone(),
            session: approval.session.clone(),
            action_summary: approval.action_summary.clone(),
            risk_level: approval.risk_level,
            scope: approval.scope.clone(),
            requested_permissions: approval.requested_permissions.clone(),
            normalized_action_digest: approval.normalized_action_digest.clone(),
            affected_resources: approval.affected_resources.clone(),
            rollback_boundary: approval.rollback_boundary.clone(),
            expires_at: approval.expires_at,
            choices: approval.choices.clone(),
            plan_digest: approval.plan_digest.clone(),
            policy_version: approval.policy_version,
            tool_schema_version: approval.tool_schema_version,
        };
        let broker = InMemoryApprovalBroker::default();
        let ticket = broker.request(request)?;
        let expected = broker.take_events();
        let actual = events
            .iter()
            .filter(|e| e.kind == p::EventKind::ApprovalRequested)
            .map(|e| e.payload.clone())
            .collect::<Vec<_>>();
        if expected != actual {
            return Err(invalid());
        }
        let waiting = events.iter().rev().find(|e| matches!(&e.payload, p::EventPayload::RunWaiting(v) if v.wait_reason.0 == "approval")).ok_or_else(invalid)?;
        let mut record = self.recovered_record(
            run,
            events,
            PendingKind::ApprovalWait(ticket.0.clone()),
            waiting.event_id.clone(),
        )?;
        record.request = original.request.clone();
        record.mode = RunMode::DirectAction;
        record.owner_discovery_bound = true;
        record.direct_intent = Some(original.intent.clone());
        record.bound_envelope = Some(original.envelope.clone());
        record.remaining_action_budget = Some(1);
        record.approval = broker;
        // Preserve the original version/evolution snapshot and verification
        // passes via the encrypted companion, without re-running adaptation.
        record.pending_action = Some(PendingAction {
            intent: original.intent.clone(),
            plan,
            ticket,
        });
        let context = self.action_policy_context(run, &record, &original.intent)?;
        if self
            .policy
            .evaluate_detailed(&context, &original.intent)
            .decision
            != p::PolicyDecision::Ask
        {
            return Err(invalid());
        }
        self.insert_recovered(run, Arc::new(RunHandle::new(record)))
    }
}
