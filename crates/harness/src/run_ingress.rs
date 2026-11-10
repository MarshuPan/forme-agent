//! Instance-bound ingress admission, durable command claims, and run dispatch.
use crate::{
    action_budget, append_new_event, delivery_disclosure_is_bound, is_external_action, new_run_id,
    now_ms, request_provenance, scope_within, AgentHarness, HarnessActionIngress, HarnessIngress,
    IngressAuthority, IngressEvent, PreparedSubmission, ReactiveHarness, ResumeInput, RunHandle,
    RunMode, RunRecord,
};
use forme_approval::{ApprovalBroker, ApprovalScope, ApprovalTicket, InMemoryApprovalBroker};
use forme_loop::StopReason;
use forme_protocol as p;
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl ReactiveHarness {
    pub(super) fn submit_ingress_internal(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId> {
        self.submit_ingress_internal_as(request, prelude, None)
    }

    pub(super) fn submit_ingress_internal_as(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
        forced_run: Option<p::RunId>,
    ) -> p::Result<p::RunId> {
        let _data_access = self.data_read_guard()?;
        match self.prepare_ingress_internal_as(request, prelude, forced_run)? {
            PreparedSubmission::Existing(run) => Ok(run),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                self.drive_prepared_run(&run, &handle, &session)?;
                Ok(run)
            }
        }
    }

    pub(super) fn prepare_ingress_internal_as(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
        forced_run: Option<p::RunId>,
    ) -> p::Result<PreparedSubmission> {
        self.prepare_workspace_ingress(request, prelude, forced_run, &self.config.workspace)
    }

    pub(super) fn prepare_workspace_ingress(
        &self,
        mut request: p::RunRequest,
        prelude: Vec<IngressEvent>,
        forced_run: Option<p::RunId>,
        workspace: &p::WorkspaceRef,
    ) -> p::Result<PreparedSubmission> {
        if *workspace != self.config.workspace {
            if let Some(key) = &request.idempotency_key {
                request.idempotency_key = Some(p::IdempotencyKey(format!(
                    "workspace-run:{}",
                    p::canonical_digest(&(workspace, key))?.0
                )));
            }
        }
        if request.source == p::Source::Replay {
            return Err(p::Error(
                "exact replay requires the dedicated read-only replay audit path".into(),
            ));
        }
        if request.schema_version.0 == 0
            || request.session.0.trim().is_empty()
            || request.input.0.trim().is_empty()
            || prelude
                .iter()
                .any(|event| !event.is_authorized_by(&self.ingress_authority))
        {
            return Err(p::Error(
                "run request or ingress prelude is incomplete".into(),
            ));
        }
        // The in-process claim is held for the whole preparation so two
        // concurrent submissions of one command cannot both reach the store.
        let mut idempotency = match &request.idempotency_key {
            Some(_) => Some(
                self.idempotency
                    .lock()
                    .map_err(|_| p::Error("run idempotency state is unavailable".into()))?,
            ),
            None => None,
        };
        let run = forced_run
            .unwrap_or_else(|| new_run_id(self.run_sequence.fetch_add(1, Ordering::SeqCst)));
        let session = p::SessionId(request.session.0.clone());
        let accepted = p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: request.source,
            session_ref: session.clone(),
            input_ref: p::InputRef(request.input.0.clone()),
            idempotency_key: request.idempotency_key.clone(),
        });
        if let (Some(claims), Some(key)) = (idempotency.as_mut(), &request.idempotency_key) {
            // The durable ledger survives restarts; consult it before the
            // first write so a replayed command is answered with its original
            // receipt and a reused key with new semantics writes nothing.
            let probe = p::Event::new(
                p::EventId("probe:run-request".into()),
                run.clone(),
                None,
                accepted.clone(),
                p::SchemaVersion(1),
                now_ms(),
                request_provenance(&request),
            );
            // The ledger decides semantics; the in-process map is only a
            // cache of receipts this process has already bound.
            match self.store.probe_domain_claim(&probe)? {
                forme_store::DomainClaimProbe::Committed(existing) => {
                    claims.insert(key.clone(), existing.clone());
                    return Ok(PreparedSubmission::Existing(existing));
                }
                forme_store::DomainClaimProbe::Conflict(_) => {
                    return Err(p::Error(
                        "run idempotency key is already bound to a different request".into(),
                    ));
                }
                forme_store::DomainClaimProbe::Unclaimed
                | forme_store::DomainClaimProbe::NotIdempotent => {
                    if let Some(existing) = claims.get(key).cloned() {
                        return Ok(PreparedSubmission::Existing(existing));
                    }
                }
            }
        }
        let mut config = self.config.clone();
        config.workspace = workspace.clone();
        let context = (self.context_factory)(&request, &run, &config)?;
        let loop_ctx = forme_loop::RunCtx::new_with_input_provenance(
            session.clone(),
            request.input.clone(),
            context,
            self.config.context_budget,
            self.config.loop_budget.clone(),
            request_provenance(&request),
        )?;
        let record = RunRecord {
            request: request.clone(),
            workspace: workspace.clone(),
            loop_ctx,
            result: None,
            final_output: None,
            resume_state: None,
            pending_action: None,
            recovered_unknown: None,
            approval: InMemoryApprovalBroker::default(),
            mode: RunMode::Loop,
            direct_intent: None,
            owner_discovery_bound: false,
            bound_envelope: None,
            effect_mode: None,
            evolution_snapshot: None,
            evolution_snapshot_full: None,
            federation_snapshot: None,
            m3_binding: None,
            remaining_action_budget: action_budget(request.budget.as_ref())?,
            competence_inputs: self.competence_snapshot.clone(),
            skills_prepared: false,
            evolution_simulation: None,
            long_horizon: None,
            model_selection: None,
            active_model: None,
            fallback_index: 0,
        };
        let handle = Arc::new(RunHandle::new(record));
        for ingress in prelude {
            self.append(run.clone(), None, ingress.provenance, ingress.payload)?;
        }
        // A store-level deduplication here would mean another command won
        // the key after the probe; the new run must then not be driven.
        append_new_event(
            &self.store,
            &self.event_sequence,
            run.clone(),
            None,
            request_provenance(&request),
            accepted,
        )?;
        self.runs
            .lock()
            .map_err(|_| p::Error("run registry is unavailable".into()))?
            .insert(run.clone(), handle.clone());
        if let (Some(claims), Some(key)) = (idempotency.as_mut(), request.idempotency_key) {
            claims.insert(key, run.clone());
        }
        drop(idempotency);
        Ok(PreparedSubmission::New {
            run,
            handle,
            session,
        })
    }

    pub(super) fn drive_prepared_run(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        session: &p::SessionId,
    ) -> p::Result<()> {
        let session_lock = self.session_lock(session)?;
        let _session_guard = session_lock
            .lock()
            .map_err(|_| p::Error("session queue is unavailable".into()))?;
        if let Err(error) = self.bind_and_drive(run, handle) {
            self.fail_prepared_run(run, handle, &error)?;
        }
        Ok(())
    }

    pub(super) fn fail_prepared_run(
        &self,
        run: &p::RunId,
        handle: &Arc<RunHandle>,
        error: &p::Error,
    ) -> p::Result<()> {
        let mut record = handle
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?;
        if record.result.is_none() && !record.loop_ctx.state.is_terminal() {
            self.append_failure(
                run,
                &record,
                p::FailureClass::ExecutionFailure,
                &error.to_string(),
            )?;
            record
                .loop_ctx
                .terminate(p::RunStatus::Failed, StopReason::RetryExhausted);
            self.finish_terminal(run, handle, &mut record)?;
        }
        Ok(())
    }
}

impl HarnessIngress for ReactiveHarness {
    fn ingress_authority(&self) -> IngressAuthority {
        self.ingress_authority.clone()
    }

    fn submit_ingress(
        &self,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId> {
        self.submit_ingress_internal(request, prelude)
    }

    fn resolve_approval(
        &self,
        ticket: ApprovalTicket,
        grant: forme_approval::ApprovalGrant,
    ) -> p::Result<()> {
        self.validate_configured_owner(&grant.approver)?;
        if ticket.0 != grant.approval_id {
            return Err(p::Error("approval ticket and grant do not match".into()));
        }
        let runs = self
            .runs
            .lock()
            .map_err(|_| p::Error("run registry is unavailable".into()))?
            .iter()
            .map(|(run, handle)| (run.clone(), handle.clone()))
            .collect::<Vec<_>>();
        for (run, handle) in runs {
            let is_pending = handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?
                .approval
                .pending(ApprovalScope::All)
                .iter()
                .any(|request| request.approval_id == ticket.0);
            if is_pending {
                return AgentHarness::resume(self, run, ResumeInput::Approval(grant));
            }
        }
        Err(p::Error(
            "approval ticket is not pending in this harness".into(),
        ))
    }

    fn append_ingress_events(
        &self,
        run: p::RunId,
        events: Vec<IngressEvent>,
    ) -> p::Result<Vec<p::EventId>> {
        events
            .into_iter()
            .map(|event| {
                if !event.is_authorized_by(&self.ingress_authority) {
                    return Err(p::Error("ingress event is not versioned".into()));
                }
                self.append(run.clone(), None, event.provenance, event.payload)
            })
            .collect()
    }

    fn result_text(&self, run: p::RunId) -> p::Result<Option<String>> {
        self.output_text(run)
    }
}

impl HarnessActionIngress for ReactiveHarness {
    fn submit_action(
        &self,
        request: p::RunRequest,
        intent: p::ActionIntent,
        envelope: p::AutonomyEnvelope,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId> {
        let _data_access = self.data_read_guard()?;
        if request.source != intent.source
            || request.idempotency_key.is_none()
            || intent.schema_version.0 == 0
            || envelope.schema_version.0 == 0
            || !is_external_action(&intent)
            || !scope_within(&intent.scope, &envelope.scope)
        {
            return Err(p::Error(
                "direct external action submission is incomplete or unbound".into(),
            ));
        }
        if intent.action_type == p::ActionType::Deliver
            && !delivery_disclosure_is_bound(&request, &intent, &prelude, &self.ingress_authority)
        {
            return Err(p::Error(
                "external delivery requires its exact allowed disclosure decision before submission"
                    .into(),
            ));
        }
        match self.prepare_ingress_internal_as(request, prelude, None)? {
            PreparedSubmission::Existing(run) => Ok(run),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                {
                    let mut record = handle
                        .record
                        .lock()
                        .map_err(|_| p::Error("run state is unavailable".into()))?;
                    record.mode = RunMode::DirectAction;
                    record.direct_intent = Some(intent);
                    record.bound_envelope = Some(envelope.clone());
                    record.remaining_action_budget = action_budget(Some(&envelope.budget))?;
                }
                self.drive_prepared_run(&run, &handle, &session)?;
                Ok(run)
            }
        }
    }
}
