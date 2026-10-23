//! Provider selection, fallback and context preparation before a model turn.
use super::*;

impl ReactiveHarness {
    pub(super) fn model_for_record(
        &self,
        run: &p::RunId,
        record: &mut RunRecord,
    ) -> p::Result<Arc<dyn ModelProvider>> {
        if record.model_selection.is_none() {
            let Some(registry) = &self.model_registry else {
                record.active_model = Some(self.model.profile().profile_ref());
                return Ok(self.model.clone());
            };
            // A visible catalog is not itself a requirement to call tools:
            // optional capabilities may be present in the formal context
            // while the selected model is intentionally text-only.  The
            // loop will expose tools only when the bound provider declares
            // tool use, so routing must not reject an otherwise valid
            // text-only production profile here.
            let require_tool_use = self.model.profile().capability.tool_use
                && !record
                    .loop_ctx
                    .context
                    .sources
                    .tool_schema
                    .entries
                    .is_empty();
            let minimum_strength = match record
                .direct_intent
                .as_ref()
                .map(|intent| intent.risk_hint)
                .unwrap_or(p::Risk::Medium)
            {
                p::Risk::Low => ModelStrength::Basic,
                p::Risk::Medium => ModelStrength::Standard,
                p::Risk::High => ModelStrength::Strong,
            };
            let selection = registry.select(&ModelSelectionRequest {
                schema_version: p::SchemaVersion(1),
                task: None,
                minimum_context_window: u32::try_from(record.loop_ctx.context_budget.max_tokens)
                    .map_err(|_| {
                        p::Error("model context window requirement is too large".into())
                    })?,
                require_tool_use,
                tool_schema_version: require_tool_use
                    .then_some(record.loop_ctx.context.sources.tool_schema.version),
                minimum_strength,
                risk: record
                    .direct_intent
                    .as_ref()
                    .map(|intent| intent.risk_hint)
                    .unwrap_or(p::Risk::Medium),
                evidence: Vec::new(),
            })?;
            let snapshot = p::canonical_digest(&(
                &selection.selected,
                &selection.fallback_chain,
                &selection.policy,
                &selection.rationale,
            ))?;
            self.append(
                run.clone(),
                None,
                internal_provenance(p::Source::Internal),
                p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                    trace_ref: p::DecisionTraceRef(format!("model-selection:{}", run.0)),
                    refs: p::DecisionRefs {
                        map: None,
                        user: None,
                        agent_self: None,
                        trust: None,
                        failure: Vec::new(),
                    },
                    rationale: p::Rationale(format!(
                        "model selection {} selected={} fallback_count={} snapshot={}",
                        selection.policy.0,
                        selection.selected.0,
                        selection.fallback_chain.len(),
                        snapshot.0
                    )),
                    workspace_snapshot: p::AgentWorkspaceSnapshotRef(format!(
                        "model-selection:{}",
                        snapshot.0
                    )),
                    resource_graph_snapshot: None,
                    evolution_snapshot: None,
                    federation_snapshot: None,
                    orchestration: None,
                }),
            )?;
            record.active_model = Some(selection.selected.clone());
            record.model_selection = Some(selection);
        }
        let reference = record
            .active_model
            .as_ref()
            .ok_or_else(|| p::Error("model attempt has no bound profile".into()))?;
        let provider = self
            .model_registry
            .as_ref()
            .map(|registry| registry.provider(reference))
            .unwrap_or_else(|| Ok(self.model.clone()))?;
        // Pinned strategy compatibility and measured adaptation evidence are
        // profile-specific. A selected/fallback provider cannot inherit the
        // default model's scaffold merely because both support tool use.
        if let (Some(runtime), Some(snapshot)) =
            (&self.m3_domains, record.evolution_snapshot_full.as_ref())
        {
            let scope = p::Scope(record.workspace.0.clone());
            let strategy_scope = self.m3_scope_override.as_ref().unwrap_or(&scope);
            let binding = runtime.bind(snapshot, strategy_scope, &provider.profile())?;
            if binding.model_scaffold.as_ref().map(|plan| &plan.scaffold)
                != record
                    .m3_binding
                    .as_ref()
                    .and_then(|bound| bound.model_scaffold.as_ref())
                    .map(|plan| &plan.scaffold)
            {
                return Err(p::Error(
                    "selected model cannot change the pinned run scaffold".into(),
                ));
            }
            record.m3_binding = Some(binding);
        }
        Ok(provider)
    }

    pub(super) fn advance_model_fallback(
        &self,
        run: &p::RunId,
        record: &mut RunRecord,
    ) -> p::Result<bool> {
        let Some(selection) = &record.model_selection else {
            return Ok(false);
        };
        // A fallback is a pre-effect availability recovery, never a way to
        // silently change the model behind a high-impact action.
        if record
            .direct_intent
            .as_ref()
            .is_some_and(|intent| intent.risk_hint == p::Risk::High)
        {
            return Ok(false);
        }
        if record.fallback_index >= 2 {
            return Ok(false);
        }
        // This also covers a model-produced L5 action after its direct intent
        // has left the pending slot, and remains true across a restart.
        for event in self.store.read_run(run.clone()) {
            if matches!(
                event?.payload,
                p::EventPayload::ApprovalRequested(p::ApprovalRequestedPayload {
                    risk: p::Risk::High,
                    ..
                }) | p::EventPayload::ActionOutcomeUnknown(_)
            ) {
                return Ok(false);
            }
        }
        let next = selection.fallback_chain.get(record.fallback_index).cloned();
        let Some(next) = next else {
            return Ok(false);
        };
        record.fallback_index = record.fallback_index.saturating_add(1);
        record.active_model = Some(next);
        Ok(true)
    }

    pub(super) fn compact_context_if_needed(
        &self,
        run: &p::RunId,
        record: &mut RunRecord,
    ) -> p::Result<()> {
        let compactor = AutomaticCompactor::new(self.store.clone(), self.config.compaction)?;
        let report = compactor.compact_if_needed(
            &mut record.loop_ctx.context,
            record.loop_ctx.context_budget,
            p::SummaryRef(format!("context-summary:{}", run.0)),
        )?;
        if report.is_none() {
            return Ok(());
        }
        for payload in compactor.take_events() {
            self.append(
                run.clone(),
                Some(current_turn(run, record.loop_ctx.turn_index)),
                internal_provenance(p::Source::Internal),
                payload,
            )?;
        }
        Ok(())
    }

    pub(super) fn prepare_skill_context(
        &self,
        run: &p::RunId,
        record: &mut RunRecord,
    ) -> p::Result<()> {
        let Some(registry) = &self.skill_registry else {
            return Ok(());
        };
        if record.skills_prepared {
            return Ok(());
        }
        let _guard = self
            .skill_runtime_lock
            .lock()
            .map_err(|_| p::Error("skill runtime selection is unavailable".into()))?;
        let hits = registry.search(SkillSearchQuery {
            schema_version: p::SchemaVersion(1),
            text: record.request.input.0.clone(),
            scope: record.loop_ctx.context.scope.clone(),
            limit: 8,
        })?;
        let mut events = registry.take_events();
        record.loop_ctx.context.sources.skills_metadata = hits
            .iter()
            .map(|hit| SkillMetadata {
                schema_version: hit.metadata.schema_version,
                id: hit.metadata.id.clone(),
                summary: hit.metadata.summary.clone(),
                scope: hit.metadata.scope.clone(),
                version: hit.metadata.version,
                trust: hit.metadata.trust,
            })
            .collect();
        if let Some(selected) = hits.first() {
            let body = registry.load_body(selected.metadata.id.clone(), LoadTrigger::Selected)?;
            events.extend(registry.take_events());
            record.loop_ctx.context.selected_skills = vec![selected.metadata.id.clone()];
            record
                .loop_ctx
                .context
                .sources
                .loaded_skill_bodies
                .push(LoadedSkillBody {
                    schema_version: p::SchemaVersion(1),
                    id: selected.metadata.id.clone(),
                    body: body.0,
                    trigger: p::SkillTriggerRef("selected".into()),
                    provenance: p::Provenance {
                        source: p::Source::Internal,
                        actor: p::Actor::System,
                        trust_tier: selected.metadata.trust,
                        caused_by: None,
                    },
                });
        }
        for payload in events {
            self.append(
                run.clone(),
                Some(current_turn(run, record.loop_ctx.turn_index)),
                internal_provenance(p::Source::Internal),
                payload,
            )?;
        }
        record.skills_prepared = true;
        Ok(())
    }
}
