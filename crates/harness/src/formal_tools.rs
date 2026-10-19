//! Model arguments supply data; composition bindings supply authority.
use crate::ReactiveHarness;
use forme_models::ModelToolCall;
use forme_protocol as p;
use forme_store::EventStore;

#[derive(Clone)]
pub(crate) struct FormalToolBinding {
    pub capability: p::CapabilityRef,
    pub backend: p::BackendKind,
    pub scope: p::Scope,
    pub permissions: Vec<p::PermissionRef>,
    pub risk: p::Risk,
}

impl ReactiveHarness {
    /// A direct owner-brain run still obeys its narrower project declaration.
    /// Re-read the event-derived reservation at both policy and dispatch time.
    pub(crate) fn check_project_action(
        &self,
        run: &p::RunId,
        intent: &p::ActionIntent,
    ) -> p::Result<()> {
        let Some(services) = &self.brain_services else {
            return Ok(());
        };
        let ledger = self
            .store
            .orchestration_ledger(&services.aggregates.charter)?;
        for route in ledger.routes.values() {
            for (id, node) in &route.nodes {
                if node.attempts == 0 || route.declaration.node_run_id(id, node.attempts) != *run {
                    continue;
                }
                let spec = route.node_spec(id).expect("recorded node");
                if route.closed
                    || route.status != p::WorkRouteStatus::Running
                    || !matches!(
                        node.status,
                        p::WorkNodeStatus::Running | p::WorkNodeStatus::Waiting
                    )
                    || !spec.capabilities.contains(&intent.capability_ref)
                    || spec.scope != intent.scope
                {
                    return Err(p::Error(
                        "project reservation does not permit this action".into(),
                    ));
                }
            }
        }
        Ok(())
    }
    pub(crate) fn subagent_result(&self, run: &p::RunId) -> p::Result<p::RunResult> {
        use crate::AgentHarness;
        if self.state(run.clone())?.is_suspended() {
            return Ok(p::RunResult {
                schema_version: p::SchemaVersion(1),
                status: p::RunStatus::Waiting,
                stop_reason: p::StopReason("awaiting_resume".into()),
                outputs: Vec::new(),
                evidence_refs: Vec::new(),
            });
        }
        self.wait(run.clone())
    }
    pub(crate) fn resolve_formal_tool(
        &self,
        run: &p::RunId,
        tool: &ModelToolCall,
    ) -> Option<p::ActionIntent> {
        let binding = self.formal_tool_bindings.get(&tool.tool)?;
        if !self
            .governance
            .visible_capabilities
            .contains(&binding.capability)
        {
            return None;
        }
        let mut parameters: p::ActionParameters =
            serde_json::from_value(tool.arguments.clone()).ok()?;
        let (backend, action_type) = match &mut parameters {
            p::ActionParameters::File {
                operation, path, ..
            } => {
                if let Some(root) = &self.project_directory {
                    let relative = std::path::Path::new(path);
                    if !relative.is_absolute() {
                        if relative
                            .components()
                            .any(|c| !matches!(c, std::path::Component::Normal(_)))
                        {
                            return None;
                        }
                        *path = root.join(relative).to_str()?.to_owned();
                    }
                }
                (
                    p::BackendKind::File,
                    if *operation == p::FileOperation::Read {
                        p::ActionType::Observe
                    } else {
                        p::ActionType::Write
                    },
                )
            }
            p::ActionParameters::Shell { .. } => (p::BackendKind::Shell, p::ActionType::Execute),
            p::ActionParameters::Mcp { .. } => (p::BackendKind::Mcp, p::ActionType::Execute),
            p::ActionParameters::Notification { .. } => {
                (p::BackendKind::Notification, p::ActionType::Deliver)
            }
            p::ActionParameters::Browser(_) => (p::BackendKind::Browser, p::ActionType::Execute),
            p::ActionParameters::Computer(_) => (p::BackendKind::Computer, p::ActionType::Execute),
            p::ActionParameters::Pty(_) => (p::BackendKind::Pty, p::ActionType::Execute),
            p::ActionParameters::AppApi(_) => (p::BackendKind::AppApi, p::ActionType::Execute),
            p::ActionParameters::Remote(_) => return None,
        };
        if backend != binding.backend {
            return None;
        }
        Some(p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId(format!(
                "formal-tool:{}",
                p::canonical_digest(&(run, &tool.call_id)).ok()?.0
            )),
            source: p::Source::Internal,
            goal: p::GoalRef(format!("goal:run:{}", run.0)),
            backend_hint: backend,
            capability_ref: binding.capability.clone(),
            action_type,
            scope: binding.scope.clone(),
            risk_hint: binding.risk,
            expected_effect: if backend == p::BackendKind::Notification {
                p::ExpectedEffect::Outward
            } else {
                p::ExpectedEffect::Internal
            },
            rollback_expectation: p::RollbackBoundary("none".into()),
            parameters,
            requested_permissions: binding.permissions.clone(),
            requested_at: self
                .store
                .read_run(run.clone())
                .filter_map(Result::ok)
                .filter_map(|e| match e.payload {
                    p::EventPayload::ToolCallProposed(p) if p.call_id == tool.call_id => {
                        Some(e.ts_unix_ms)
                    }
                    _ => None,
                })
                .last()
                .unwrap_or(0),
            estimated_output_bytes: 4096,
            estimated_duration: p::DurationMs(5000),
        })
    }

    pub(crate) fn child_executor(
        &self,
        run: &p::RunId,
    ) -> p::Result<Option<std::sync::Arc<ReactiveHarness>>> {
        Ok(self
            .child_harnesses
            .lock()
            .map_err(|_| p::Error("child registry is unavailable".into()))?
            .get(run)
            .map(|(_, h)| h.clone()))
    }
    pub(crate) fn child_executors(&self) -> p::Result<Vec<std::sync::Arc<ReactiveHarness>>> {
        Ok(self
            .child_harnesses
            .lock()
            .map_err(|_| p::Error("child registry is unavailable".into()))?
            .values()
            .map(|(_, h)| h.clone())
            .collect())
    }
    /// Restore an unconsumed formal approval from its immutable proposal and
    /// binding. Replanning must reproduce the exact old digests; no model or
    /// driver is called, and changed configuration fails closed.
    pub(crate) fn restore_formal_approval(&self, run: &p::RunId, max_turns: u64) -> p::Result<()> {
        use crate::*;
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        if self.run_handle(run).is_ok() {
            return Ok(());
        }
        let accepted = events
            .iter()
            .find_map(|e| match &e.payload {
                p::EventPayload::RunAccepted(p) => Some(p),
                _ => None,
            })
            .ok_or_else(|| p::Error("approval recovery has no accepted run".into()))?;
        let (approval_event, old) = events
            .iter()
            .rev()
            .find_map(|e| match &e.payload {
                p::EventPayload::ApprovalRequested(p) => Some((e, p)),
                _ => None,
            })
            .ok_or_else(|| p::Error("approval recovery has no bound request".into()))?;
        if events.iter().any(|e| {
            matches!(
                e.kind,
                p::EventKind::ActionStarted | p::EventKind::ApprovalResolved
            )
        }) {
            return Err(p::Error(
                "consumed or uncertain approval cannot be restored".into(),
            ));
        }
        let proposal = events
            .iter()
            .rev()
            .find_map(|e| match &e.payload {
                p::EventPayload::ToolCallProposed(p) => Some(p),
                _ => None,
            })
            .ok_or_else(|| p::Error("approval recovery has no typed tool proposal".into()))?;
        let tool = forme_models::ModelToolCall {
            schema_version: p::SchemaVersion(1),
            call_id: proposal.call_id.clone(),
            tool: proposal.tool.clone(),
            arguments: proposal.args.clone(),
            intent: None,
        };
        let mut intent = self
            .resolve_formal_tool(run, &tool)
            .ok_or_else(|| p::Error("approval tool binding is no longer available".into()))?;
        intent.source = accepted.source;
        if self
            .governance
            .envelope
            .as_ref()
            .is_none_or(|e| crate::now_ms() > e.timebox.expires_at)
        {
            return Err(p::Error(
                "delegation expired before approval recovery".into(),
            ));
        }
        let plan = self.backends.backend(intent.backend_hint)?.plan(&intent)?;
        if old.plan_digest.as_ref() != Some(&plan.digest)
            || old.normalized_action_digest.as_ref() != Some(&plan.normalized_action_digest)
            || old.policy_version != Some(self.config.policy_version)
            || old.tool_schema_version != Some(self.config.tool_schema_version)
        {
            return Err(p::Error(
                "approval recovery configuration differs from the approved plan".into(),
            ));
        }
        let request = p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: accepted.source,
            session: p::SessionRef(accepted.session_ref.0.clone()),
            agent_profile: p::AgentProfileRef("agent:recovered-owner-project".into()),
            input: p::RunInput(accepted.input_ref.0.clone()),
            budget: Some(p::Budget(format!("units:{max_turns}"))),
            idempotency_key: accepted.idempotency_key.clone(),
        };
        let context = (self.context_factory)(&request, run, &self.config)?;
        let mut loop_ctx = forme_loop::RunCtx::new_with_input_provenance(
            accepted.session_ref.clone(),
            request.input.clone(),
            context,
            self.config.context_budget,
            self.config.loop_budget.clone(),
            request_provenance(&request),
        )?;
        // ToolCallProposed stores the registered catalog reference. Restore
        // its deterministic provider alias without repeating the model/action.
        let mut native_call = tool.clone();
        native_call.tool = p::ToolRef(forme_models::ToolDefinition::catalog_alias(&tool.tool.0)?);
        loop_ctx.record_model_tool_call(&native_call)?;
        loop_ctx.budget.max_turns = loop_ctx
            .budget
            .max_turns
            .min(u32::try_from(max_turns).unwrap_or(u32::MAX));
        loop_ctx.turn_index = events
            .iter()
            .filter(|e| e.kind == p::EventKind::ModelCallStarted)
            .count()
            .saturating_sub(1) as u32;
        let pending = forme_loop::PendingKind::ApprovalWait(old.approval_id.clone());
        loop_ctx.suspend(pending.clone());
        let broker = forme_approval::InMemoryApprovalBroker::default();
        let ticket = broker.request(forme_approval::ApprovalRequest {
            schema_version: p::SchemaVersion(1),
            approval_id: old.approval_id.clone(),
            session: accepted.session_ref.clone(),
            action_summary: old.action_summary.0.clone(),
            risk_level: old.risk,
            scope: old.scope.clone(),
            requested_permissions: old.requested_permissions.clone(),
            normalized_action_digest: plan.normalized_action_digest.clone(),
            affected_resources: plan.affected_resources.clone(),
            rollback_boundary: old.rollback_boundary.clone(),
            expires_at: old.expires_at,
            choices: old.choices.clone(),
            plan_digest: plan.digest.clone(),
            policy_version: self.config.policy_version,
            tool_schema_version: self.config.tool_schema_version,
        })?;
        broker.take_events(); // Rehydration is a read, not a new approval fact.
        let record = RunRecord {
            request,
            loop_ctx,
            result: None,
            final_output: None,
            resume_state: Some(ResumeState {
                schema_version: p::SchemaVersion(1),
                run: run.clone(),
                at: LoopState::ApprovalWait,
                pending,
                snapshot_ref: approval_event.event_id.clone(),
            }),
            pending_action: Some(PendingAction {
                intent,
                plan,
                ticket,
            }),
            recovered_unknown: None,
            approval: broker,
            mode: RunMode::Loop,
            direct_intent: None,
            bound_envelope: None,
            effect_mode: None,
            evolution_snapshot: None,
            evolution_snapshot_full: None,
            federation_snapshot: None,
            m3_binding: None,
            remaining_action_budget: None,
            competence_inputs: self.competence_snapshot.clone(),
            skills_prepared: true,
            evolution_simulation: None,
            long_horizon: None,
            model_selection: None,
            active_model: None,
            fallback_index: 0,
        };
        self.runs
            .lock()
            .map_err(|_| p::Error("approval run registry unavailable".into()))?
            .insert(run.clone(), std::sync::Arc::new(RunHandle::new(record)));
        Ok(())
    }
}
