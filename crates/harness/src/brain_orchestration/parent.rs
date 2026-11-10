use crate::production::ProductionRuntime;
use forme_protocol as p;
use forme_store::EventStore;

impl ProductionRuntime {
    pub(super) fn ensure_parent_handle(&self, route: &p::RouteDeclaration) -> p::Result<()> {
        let h = self.harness();
        if h.run_handle(&route.parent).is_ok() {
            return Ok(());
        }
        // A parent run represents only the owner's coordination checkpoint;
        // concrete tasks remain bounded children, sharing this same authority.
        let req = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::Internal,
            session: p::SessionRef(format!("project-parent:{}", route.route.0)),
            agent_profile: self.config().agent_profile.clone(),
            input: p::RunInput(
                "A project execution segment is ready. Preserve its declared scope and contract."
                    .into(),
            ),
            budget: Some(p::Budget("units:1".into())),
            idempotency_key: Some(p::IdempotencyKey(format!("parent:{}", route.route.0))),
        };
        let workspace = self.orchestration_snapshot()?.projects[&route.project]
            .workspace
            .clone();
        match h.prepare_workspace_ingress(
            req.clone(),
            Vec::new(),
            Some(route.parent.clone()),
            &workspace,
        )? {
            crate::PreparedSubmission::New { .. } => {}
            crate::PreparedSubmission::Existing(_) => {
                // Recreate only the coordination container. Children with
                // incomplete effects are reconciled separately, never driven.
                let mut config = h.config.clone();
                config.workspace = workspace.clone();
                let context = (h.context_factory)(&req, &route.parent, &config)?;
                let loop_ctx = forme_loop::RunCtx::new_with_input_provenance(
                    p::SessionId(req.session.0.clone()),
                    req.input.clone(),
                    context,
                    h.config.context_budget,
                    h.config.loop_budget.clone(),
                    crate::request_provenance(&req),
                )?;
                let record = crate::RunRecord {
                    request: req,
                    workspace,
                    loop_ctx,
                    result: None,
                    final_output: None,
                    resume_state: None,
                    pending_action: None,
                    recovered_unknown: None,
                    approval: forme_approval::InMemoryApprovalBroker::default(),
                    mode: crate::RunMode::Loop,
                    direct_intent: None,
                    owner_discovery_bound: false,
                    bound_envelope: None,
                    effect_mode: None,
                    evolution_snapshot: None,
                    evolution_snapshot_full: None,
                    federation_snapshot: None,
                    m3_binding: None,
                    remaining_action_budget: Some(0),
                    competence_inputs: h.competence_snapshot.clone(),
                    skills_prepared: false,
                    evolution_simulation: None,
                    long_horizon: None,
                    model_selection: None,
                    active_model: None,
                    fallback_index: 0,
                };
                h.runs
                    .lock()
                    .map_err(|_| p::Error("parent registry is unavailable".into()))?
                    .insert(
                        route.parent.clone(),
                        std::sync::Arc::new(crate::RunHandle::new(record)),
                    );
            }
        }
        Ok(())
    }

    pub(super) fn finish_project_parent(&self, record: &p::WorkRouteRecord) -> p::Result<()> {
        if record.declaration.nodes.is_empty() {
            return Ok(());
        }
        let route = &record.declaration;
        let events = self
            .services()
            .store
            .read_run(route.parent.clone())
            .collect::<p::Result<Vec<_>>>()?;
        if events.iter().any(|e| {
            matches!(
                e.kind,
                p::EventKind::RunComplete
                    | p::EventKind::RunFailed
                    | p::EventKind::RunAborted
                    | p::EventKind::RunLimited
            )
        }) {
            return Ok(());
        }
        if record.has_inflight() {
            self.services().store.append(p::Event::new(
                p::EventId(format!("route-parent-wait:{}", route.route.0)),
                route.parent.clone(),
                None,
                p::EventPayload::RunWaiting(p::RunWaitingPayload {
                    wait_reason: p::WaitReason("route_child_outcome_unknown".into()),
                    resume_ref: p::ResumeRef(route.route.0.clone()),
                }),
                p::SchemaVersion(1),
                route.created_at,
                crate::internal_provenance(p::Source::Internal),
            ))?;
            return Ok(());
        }
        self.ensure_parent_handle(route)?;
        let h = self.harness();
        let handle = h.run_handle(&route.parent)?;
        let mut parent = handle
            .record
            .lock()
            .map_err(|_| p::Error("parent run is unavailable".into()))?;
        let (status, reason) = match record.status {
            p::WorkRouteStatus::Completed => {
                (p::RunStatus::Complete, crate::StopReason::FinalOutput)
            }
            p::WorkRouteStatus::Cancelled => (p::RunStatus::Aborted, crate::StopReason::UserCancel),
            _ => (p::RunStatus::Failed, crate::StopReason::VerifyUnfixable),
        };
        parent.loop_ctx.terminate(status, reason);
        h.finish_terminal(&route.parent, &handle, &mut parent)
    }
}
