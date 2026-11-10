//! A failed publication is recoverable without creating another execution.
use super::*;

impl ReactiveHarness {
    pub(crate) fn finish_owner_discovery(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        record: &mut RunRecord,
    ) -> p::Result<bool> {
        if !record.owner_discovery_bound {
            return Ok(false);
        }
        let published = (|| {
            let pending: PendingDiscovery = read(self, run, "pending")?.ok_or_else(invalid)?;
            let runtime = self.owner_discovery.as_ref().ok_or_else(invalid)?;
            crate::production_artifact_store::mcp::publication::publish(
                &self.store,
                &pending.original.owner,
                runtime.authority.as_ref(),
                run,
                &pending.original.intent,
            )
        })();
        if published.is_err() {
            let pending_kind =
                PendingKind::Handoff(p::HandoffTargetRef(format!("mcp-publication:{}", run.0)));
            self.append_waiting(run, record, "mcp_catalog_publication", &pending_kind)?;
            record.loop_ctx.suspend(pending_kind);
            record.resume_state = None;
            handle.changed.notify_all();
            return Ok(true);
        }
        let verified = self.verify(run, record)?;
        record.loop_ctx.terminate(
            if verified {
                p::RunStatus::Complete
            } else {
                p::RunStatus::Failed
            },
            if verified {
                StopReason::FinalOutput
            } else {
                StopReason::VerifyUnfixable
            },
        );
        self.finish_terminal(run, handle, record)?;
        Ok(true)
    }

    /// Explicit owner retry of catalog publication only. A completed worker
    /// original is mandatory, so this cannot dispatch or recover an unknown job.
    pub fn publish_managed_mcp(
        &self,
        owner: &p::VerifiedPrincipal,
        run: p::RunId,
    ) -> p::Result<()> {
        let _access = self.data_read_guard()?;
        if self.configured_owner.as_ref() != Some(owner) {
            return Err(invalid());
        }
        self.store.require_current_execution(&run)?;
        let pending: PendingDiscovery = read(self, &run, "pending")?.ok_or_else(invalid)?;
        if pending.original.owner != *owner || pending.original.run != run {
            return Err(invalid());
        }
        let lock = self.session_lock(&pending.approval.session)?;
        let _lock = lock.lock().map_err(|_| invalid())?;
        self.check_discovery_read(&run, owner, &pending)?;
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let terminal = run_summary_from_events(run.clone(), &events)?.result;
        if terminal
            .as_ref()
            .is_some_and(|result| result.status != p::RunStatus::Complete)
            || (terminal.is_some() && !events.iter().any(|e| e.kind == p::EventKind::McpDiscovered))
        {
            return Err(invalid());
        }
        let completed = events.iter().find(|e| matches!(&e.payload, p::EventPayload::ActionCompleted(v) if v.intent_id == pending.original.intent.intent_id)).ok_or_else(invalid)?;
        // Validate the completed material before creating any transient state.
        let runtime = self.owner_discovery.as_ref().ok_or_else(invalid)?;
        crate::production_artifact_store::mcp::publication::publish(
            &self.store,
            owner,
            runtime.authority.as_ref(),
            &run,
            &pending.original.intent,
        )?;
        if terminal.is_some() {
            return Ok(());
        }
        let handle = match self.run_handle(&run) {
            Ok(handle) => handle,
            Err(_) => {
                let mut record = self.recovered_record(
                    &run,
                    &events,
                    PendingKind::Handoff(p::HandoffTargetRef(format!("mcp-publication:{}", run.0))),
                    completed.event_id.clone(),
                )?;
                record.request = pending.original.request.clone();
                record.mode = RunMode::DirectAction;
                record.owner_discovery_bound = true;
                record.bound_envelope = Some(pending.original.envelope.clone());
                record.resume_state = None;
                let handle = Arc::new(RunHandle::new(record));
                self.insert_recovered(&run, handle.clone())?;
                handle
            }
        };
        let mut record = handle.record.lock().map_err(|_| invalid())?;
        record.request = pending.original.request;
        record.mode = RunMode::DirectAction;
        record.owner_discovery_bound = true;
        record.bound_envelope = Some(pending.original.envelope);
        record.resume_state = None;
        record.recovered_unknown = None;
        if !self.finish_owner_discovery(&run, &handle, &mut record)? {
            return Err(invalid());
        }
        Ok(())
    }
}
