//! An explicit authenticated read of an original job, with no new execution.
use super::*;
use forme_store::VersionedEventStore;

impl ReactiveHarness {
    pub fn verify_managed_mcp(&self, owner: &p::VerifiedPrincipal, run: p::RunId) -> p::Result<()> {
        {
            let _access = self.data_read_guard()?;
            if self.configured_owner.as_ref() != Some(owner) {
                return Err(invalid());
            }
            self.store.require_current_execution(&run)?;
            let pending: PendingDiscovery = read(self, &run, "pending")?.ok_or_else(invalid)?;
            let session = self.session_lock(&pending.approval.session)?;
            let _session = session.lock().map_err(|_| invalid())?;
            self.check_discovery_read(&run, owner, &pending)?;
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            // Already completed originals only need the same publication path.
            if !events
                .iter()
                .any(|e| e.kind == p::EventKind::ActionCompleted)
            {
                let request: p::WorkerDispatchRequest =
                    read(self, &run, "dispatch")?.ok_or_else(invalid)?;
                if !original_job::matches_retained(&self.store, owner, &run, &request)?
                    || run_summary_from_events(run.clone(), &events)?
                        .result
                        .is_some()
                    || events
                        .iter()
                        .filter(|e| {
                            matches!(&e.payload, p::EventPayload::ActionStarted(v)
                        if v.intent_id == request.job.action_ref)
                        })
                        .count()
                        != 1
                {
                    return Err(invalid());
                }
                let version = self.store.aggregate_version(run.clone())?;
                if events.last().map(|v| v.stream_seq) != Some(version.value) {
                    return Err(invalid());
                }
                let runtime = self.owner_discovery.as_ref().ok_or_else(invalid)?;
                let mut plan = pending.plan.restore()?;
                plan.approval_ref = Some(pending.approval.approval_id.clone());
                let observations = runtime.backend.read_discovery_original(&plan, &request)?;
                let payload = observations
                    .into_iter()
                    .find(|v| v.kind() == p::EventKind::ActionCompleted)
                    .ok_or_else(invalid)?;
                let mut completion = p::Event::new(
                    p::EventId(format!(
                        "event:mcp-original-completion:{}",
                        p::canonical_digest(&(&run, &request))?.0
                    )),
                    run.clone(),
                    None,
                    stamp_external_payload(payload),
                    p::SchemaVersion(1),
                    now_ms(),
                    untrusted_action_provenance(),
                );
                completion.stream_seq = version.value.checked_add(1).ok_or_else(invalid)?;
                crate::production_artifact_store::mcp::publication::validate_recovery(
                    &self.store,
                    owner,
                    runtime.authority.as_ref(),
                    &request,
                    &events,
                    &completion,
                )?;
                // Host reads may block. Recheck Core identity and the lifecycle
                // fence after them; append_expected rejects any changed history.
                self.check_discovery_read(&run, owner, &pending)?;
                match self.store.append_expected(completion, version)?.status {
                    p::ExpectedAppendStatus::Applied | p::ExpectedAppendStatus::Duplicate => (),
                    p::ExpectedAppendStatus::Conflict => return Err(invalid()),
                }
            }
        }
        // Release session/data guards before entering the publication guard.
        self.publish_managed_mcp(owner, run)
    }

    pub(super) fn check_discovery_read(
        &self,
        run: &p::RunId,
        owner: &p::VerifiedPrincipal,
        pending: &PendingDiscovery,
    ) -> p::Result<()> {
        self.store.require_current_execution(run)?;
        let runtime = self.owner_discovery.as_ref().ok_or_else(invalid)?;
        let original = &pending.original;
        let p::ActionParameters::McpDiscovery(spec) = &original.intent.parameters else {
            return Err(invalid());
        };
        runtime.settings(spec)?;
        if original.owner != *owner
            || original.run != *run
            || self.composition_manifest.as_ref() != Some(&original.composition)
            || runtime.worker_digest != original.worker
            || pending.approval.policy_version != self.config.policy_version
            || pending.approval.tool_schema_version != self.config.tool_schema_version
            || pending.session_binding.workspace != self.config.workspace
            || pending.session_binding.policy_profile != self.config.policy_profile
            || pending.session_binding.toolset_ref != self.config.toolset_ref
            || read::<OwnerRequest>(self, run, "request")?.as_ref() != Some(original)
            || read::<PendingDiscovery>(self, run, "pending")?.as_ref() != Some(pending)
            || self
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
        // Evaluate current rules against the original finite authorization.
        // The old timebox stays untouched and no execution uses this context.
        let context = self.governance.policy_context(
            pending.approval.session.clone(),
            self.config.toolset_ref.clone(),
            &self.policy,
            Some(&original.envelope),
        );
        let context = self.discovery_policy_context(context, original.clone(), &original.intent)?;
        if self
            .policy
            .evaluate_detailed(&context, &original.intent)
            .decision
            == p::PolicyDecision::Deny
        {
            return Err(invalid());
        }
        Ok(())
    }
}
