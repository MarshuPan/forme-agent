//! Gateway-facing run control and read-only run observations.
use crate::*;

impl AgentHarness for ReactiveHarness {
    fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId> {
        self.submit_ingress_internal(request, Vec::new())
    }

    fn stream_events(&self, run: p::RunId) -> EventStream {
        let events = self
            .store
            .read_run(run)
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        EventStream::new(events)
    }

    fn wait(&self, run: p::RunId) -> p::Result<p::RunResult> {
        let handle = match self.run_handle(&run) {
            Ok(handle) => handle,
            // A run committed by an earlier process (for example the original
            // receipt of a replayed command) is answered from its durable
            // terminal fact; a non-terminal one needs explicit recovery.
            Err(error) => {
                let events = self
                    .store
                    .read_run(run.clone())
                    .collect::<p::Result<Vec<_>>>()?;
                if events.is_empty() {
                    return Err(error);
                }
                return run_summary_from_events(run, &events)?
                    .result
                    .ok_or_else(|| {
                        p::Error(
                            "run is not active in this runtime and has no terminal result".into(),
                        )
                    });
            }
        };
        let mut record = handle
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?;
        loop {
            if let Some(result) = &record.result {
                return Ok(result.clone());
            }
            if record.loop_ctx.state.is_suspended() {
                return Err(p::Error("run is suspended and requires ResumeInput".into()));
            }
            record = handle
                .changed
                .wait(record)
                .map_err(|_| p::Error("run wait state is unavailable".into()))?;
        }
    }

    fn cancel(&self, run: p::RunId) -> p::Result<()> {
        if let Some(child) = self.child_executor(&run)? {
            return AgentHarness::cancel(child.as_ref(), run);
        }
        let handle = self.run_handle(&run)?;
        handle.cancelled.store(true, Ordering::SeqCst);
        handle.cancel_token.cancel();
        self.cancel_child_runs(&run)?;
        if let Ok(mut record) = handle.record.try_lock() {
            if record.loop_ctx.state.is_suspended()
                && record.result.is_none()
                && record.recovered_unknown.is_none()
            {
                record
                    .loop_ctx
                    .terminate(p::RunStatus::Aborted, StopReason::UserCancel);
                self.finish_terminal(&run, &handle, &mut record)?;
            }
        }
        Ok(())
    }

    fn resume(&self, run: p::RunId, input: ResumeInput) -> p::Result<()> {
        self.resume_run(run, input, None)
    }

    fn drain(&self, session: p::SessionId) -> p::Result<()> {
        let session_lock = self.session_lock(&session)?;
        let _guard = session_lock
            .lock()
            .map_err(|_| p::Error("session queue is unavailable".into()))?;
        Ok(())
    }
}

impl GatewayControl for ReactiveHarness {
    fn start_run(self: Arc<Self>, request: p::RunRequest) -> p::Result<p::RunId> {
        self.start_ingress(request, Vec::new())
    }

    fn gateway_profile(&self, surface: p::SurfaceRef) -> p::Result<p::GatewayProfile> {
        let profile = p::GatewayProfile {
            schema_version: p::SchemaVersion(1),
            surface,
            policy: self.config.policy_profile.clone(),
            model: self.config.model_profile.clone(),
            toolset: self.config.toolset_ref.clone(),
            workspace: self.config.workspace.clone(),
        };
        profile.validate()?;
        Ok(profile)
    }

    fn list_runs(&self) -> p::Result<Vec<p::RunSummary>> {
        let mut summaries = self
            .store
            .run_ids()?
            .into_iter()
            .filter_map(|run| {
                let events = self
                    .store
                    .read_run(run.clone())
                    .collect::<p::Result<Vec<_>>>();
                match events {
                    Ok(events)
                        if events
                            .iter()
                            .any(|event| event.kind == p::EventKind::RunAccepted) =>
                    {
                        Some(run_summary_from_events(run, &events))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                }
            })
            .collect::<p::Result<Vec<_>>>()?;
        summaries.sort_by(|left, right| right.run.cmp(&left.run));
        Ok(summaries)
    }

    fn stream_event_page(&self, cursor: p::EventCursor) -> p::Result<p::EventPage> {
        cursor.validate()?;
        let events = self
            .store
            .read_run(cursor.run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let snapshot_upper_bound = events
            .last()
            .map(|event| event.stream_seq)
            .ok_or_else(|| p::Error("run event stream was not found".into()))?;
        if cursor.after_stream_seq > snapshot_upper_bound {
            return Err(p::Error("event cursor is ahead of the run snapshot".into()));
        }
        let page = p::EventPage {
            schema_version: p::SchemaVersion(1),
            run: cursor.run,
            after_stream_seq: cursor.after_stream_seq,
            snapshot_upper_bound,
            events: events
                .into_iter()
                .filter(|event| event.stream_seq > cursor.after_stream_seq)
                .collect(),
        };
        page.validate()?;
        Ok(page)
    }

    fn run_summary(&self, run: p::RunId) -> p::Result<p::RunSummary> {
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        run_summary_from_events(run, &events)
    }

    fn pending_approvals(&self, session: p::SessionId) -> p::Result<Vec<p::PendingApproval>> {
        let pending = self
            .pending_approval_records(&session)?
            .into_iter()
            .map(|(run, request)| pending_approval(run, request))
            .collect::<Vec<_>>();
        for approval in &pending {
            approval.validate()?;
        }
        Ok(pending)
    }

    fn control(&self, run: p::RunId, control: p::RunControl) -> p::Result<()> {
        self.apply_run_control(run, control)
    }

    fn trace_view(&self, run: p::RunId) -> p::Result<p::TraceView> {
        let export = eval::TraceExporter::new(Arc::new(self.store.clone())).export(run.clone())?;
        let snapshot_upper_bound = export
            .events
            .last()
            .map(|event| event.stream_seq)
            .ok_or_else(|| p::Error("run trace was not found".into()))?;
        Ok(p::TraceView {
            schema_version: p::SchemaVersion(1),
            run,
            snapshot_upper_bound,
            events: export.events,
            failure_refs: export
                .failures
                .into_iter()
                .map(|failure| failure.failure_ref)
                .collect(),
            verification_outcomes: export.verification_outcomes,
        })
    }

    fn review_candidate(&self, command: p::CandidateReviewCommand) -> p::Result<()> {
        self.review_candidate_bound(command)
    }
}

impl Observability for ReactiveHarness {
    fn decision_trace(&self, run: p::RunId) -> DecisionTrace {
        let mut event_refs = Vec::new();
        let mut reasons = Vec::new();
        for event in self.store.read_run(run.clone()).filter_map(Result::ok) {
            match &event.payload {
                p::EventPayload::ToolPolicyEvaluated(payload) => {
                    event_refs.push(event.event_id);
                    reasons.push(payload.reason.clone());
                }
                p::EventPayload::DecisionTraceRecorded(payload) => {
                    event_refs.push(event.event_id);
                    reasons.push(p::ReasonRef(payload.rationale.0.clone()));
                }
                p::EventPayload::ActionDenied(payload) => {
                    event_refs.push(event.event_id);
                    reasons.push(payload.reason.clone());
                }
                _ => {}
            }
        }
        DecisionTrace {
            schema_version: p::SchemaVersion(1),
            run,
            event_refs,
            reasons,
        }
    }

    fn state(&self, run: p::RunId) -> RunView {
        let events = self
            .store
            .read_run(run.clone())
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        let session = events.iter().find_map(|event| match &event.payload {
            p::EventPayload::RunAccepted(payload) => Some(payload.session_ref.clone()),
            _ => None,
        });
        let (state, result) = self
            .run_handle(&run)
            .ok()
            .and_then(|handle| {
                handle
                    .record
                    .lock()
                    .ok()
                    .map(|record| (Some(record.loop_ctx.state.clone()), record.result.clone()))
            })
            .unwrap_or((None, None));
        RunView {
            schema_version: p::SchemaVersion(1),
            run,
            session,
            state,
            last_stream_seq: events.last().map(|event| event.stream_seq).unwrap_or(0),
            result,
        }
    }
}

impl ReactiveHarness {
    /// Uses the same accepted-run dispatcher, with authority-stamped ingress facts.
    pub fn start_ingress(
        self: Arc<Self>,
        request: p::RunRequest,
        prelude: Vec<IngressEvent>,
    ) -> p::Result<p::RunId> {
        match self.prepare_ingress_internal_as(request, prelude, None)? {
            PreparedSubmission::Existing(run) => Ok(run),
            PreparedSubmission::New {
                run,
                handle,
                session,
            } => {
                let worker = self.clone();
                let worker_run = run.clone();
                let worker_handle = handle.clone();
                let worker_session = session.clone();
                if let Err(error) = std::thread::Builder::new()
                    .name("forme-gateway-run".into())
                    .spawn(move || {
                        if let Err(drive_error) =
                            worker.drive_prepared_run(&worker_run, &worker_handle, &worker_session)
                        {
                            let _ =
                                worker.fail_prepared_run(&worker_run, &worker_handle, &drive_error);
                        }
                    })
                {
                    let spawn_error = p::Error(format!("failed to start accepted run: {error}"));
                    self.fail_prepared_run(&run, &handle, &spawn_error)?;
                    return Err(spawn_error);
                }
                Ok(run)
            }
        }
    }
}
