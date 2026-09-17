use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use forme_capabilities::{
    rank_prefiltered, PrefilteredSelectionSet, SelectionCandidate, SelectionEvidenceSet,
    SelectionPolicyRegistry,
};
use forme_coordination::{CoordinationRegistry, ExecutionRoute};
use forme_loop::{Budget as LoopBudget, LoopRegistry};
use forme_models::{
    adapt_model_scaffold, ModelAdaptationRegistry, ModelOutcomeEvidence, ModelProfile,
    ModelScaffoldPlan,
};
use forme_protocol as p;
use forme_store::EventStore;

use crate::{AgentHarness, HarnessActionIngress, IngressEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3RuntimeCompatibility {
    pub schema_version: p::SchemaVersion,
    pub event_schema: p::SchemaVersion,
    pub tool_schema: p::SchemaDigest,
    pub backend_schema: p::SchemaDigest,
}

impl M3RuntimeCompatibility {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.event_schema.0 == 0
            || self.tool_schema.0.trim().is_empty()
            || self.backend_schema.0.trim().is_empty()
        {
            return Err(p::Error(
                "M3 domain runtime compatibility is incomplete".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundM3Domains {
    pub schema_version: p::SchemaVersion,
    pub snapshot: p::EvolutionSnapshotRef,
    pub loop_spec: Option<p::LoopStrategySpec>,
    pub coordination: Option<p::CoordinationStrategySpec>,
    pub selection: BTreeMap<p::SelectionTarget, p::SelectionStrategySpec>,
    pub model_adaptation: Option<p::ModelAdaptationSpec>,
    pub model_scaffold: Option<ModelScaffoldPlan>,
}

impl BoundM3Domains {
    pub fn selection_for(&self, target: p::SelectionTarget) -> Option<&p::SelectionStrategySpec> {
        self.selection.get(&target)
    }

    pub fn active_versions(&self) -> Vec<p::StrategyVersionRef> {
        let mut versions = Vec::new();
        if let Some(spec) = &self.loop_spec {
            versions.push(spec.version.clone());
        }
        if let Some(spec) = &self.coordination {
            versions.push(spec.version.clone());
        }
        versions.extend(self.selection.values().map(|spec| spec.version.clone()));
        if let Some(spec) = &self.model_adaptation {
            versions.push(spec.version.clone());
        }
        versions
    }
}

pub struct M3DomainRuntime {
    compatibility: M3RuntimeCompatibility,
    loop_registry: Arc<dyn LoopRegistry + Send + Sync>,
    coordination_registry: Arc<dyn CoordinationRegistry + Send + Sync>,
    selection_registry: Arc<dyn SelectionPolicyRegistry + Send + Sync>,
    model_registry: Arc<dyn ModelAdaptationRegistry + Send + Sync>,
    model_evidence: BTreeMap<p::ModelProfileRef, ModelOutcomeEvidence>,
    selection_evidence: SelectionEvidenceSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongHorizonProject {
    pub schema_version: p::SchemaVersion,
    pub project: p::GoalFrameRef,
    pub goal: p::LongTermGoal,
    pub intention: p::IntentionId,
    pub route: ExecutionRoute,
    pub maximum_checkpoints: u16,
    pub remaining_checkpoints: u16,
    pub next_checkpoint: u16,
    pub deferred_for_foreground: bool,
    pub cancelled: bool,
    pub revoked: bool,
    pub checkpoints: Vec<LongHorizonCheckpointRecord>,
}

impl LongHorizonProject {
    pub fn validate(&self) -> p::Result<()> {
        self.goal.validate()?;
        if self.schema_version.0 == 0
            || self.project.0.trim().is_empty()
            || self.project != self.goal.goal_frame
            || self.intention.0.trim().is_empty()
            || self.route.schema_version.0 == 0
            || self.route.reference.0.trim().is_empty()
            || self.route.nodes.is_empty()
            || self.maximum_checkpoints == 0
            || self.remaining_checkpoints > self.maximum_checkpoints
            || self.next_checkpoint > self.maximum_checkpoints
        {
            return Err(p::Error(
                "long-horizon project is incomplete or unbounded".into(),
            ));
        }
        Ok(())
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongHorizonCheckpointRecord {
    pub schema_version: p::SchemaVersion,
    pub index: u16,
    pub run: p::RunId,
    pub checkpoint: p::GoalCheckpointRef,
    pub snapshot: p::EvolutionSnapshotRef,
    pub strategy_versions: Vec<p::StrategyVersionRef>,
    pub status: p::RunStatus,
    pub outward_run: Option<p::RunId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongHorizonDisposition {
    Deferred,
    Advanced,
    Limited,
    Cancelled,
    Revoked,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongHorizonAdvance {
    pub schema_version: p::SchemaVersion,
    pub disposition: LongHorizonDisposition,
    pub checkpoint_run: Option<p::RunId>,
    pub outward_run: Option<p::RunId>,
    pub checkpoint: Option<p::GoalCheckpointRef>,
    pub snapshot: Option<p::EvolutionSnapshotRef>,
    pub remaining_checkpoints: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LongHorizonOutwardStep {
    pub schema_version: p::SchemaVersion,
    pub request: p::RunRequest,
    pub intent: p::ActionIntent,
    pub envelope: p::AutonomyEnvelope,
    pub prelude: Vec<IngressEvent>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingLongHorizonCheckpoint {
    pub goal: p::LongTermGoal,
    pub intention: p::IntentionId,
    pub route: ExecutionRoute,
    pub index: u16,
    pub checkpoint: p::GoalCheckpointRef,
    pub artifact: p::ContentRef,
    pub deferred_for_foreground: bool,
}

impl M3DomainRuntime {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        compatibility: M3RuntimeCompatibility,
        loop_registry: Arc<dyn LoopRegistry + Send + Sync>,
        coordination_registry: Arc<dyn CoordinationRegistry + Send + Sync>,
        selection_registry: Arc<dyn SelectionPolicyRegistry + Send + Sync>,
        model_registry: Arc<dyn ModelAdaptationRegistry + Send + Sync>,
        model_evidence: Vec<ModelOutcomeEvidence>,
        selection_evidence: SelectionEvidenceSet,
    ) -> p::Result<Self> {
        compatibility.validate()?;
        if selection_evidence.schema_version.0 == 0 {
            return Err(p::Error("selection evidence set is unversioned".into()));
        }
        let mut evidence_by_model = BTreeMap::new();
        for evidence in model_evidence {
            if evidence.schema_version.0 == 0
                || evidence.profile.0.trim().is_empty()
                || evidence_by_model
                    .insert(evidence.profile.clone(), evidence)
                    .is_some()
            {
                return Err(p::Error(
                    "model outcome evidence is invalid or duplicated".into(),
                ));
            }
        }
        Ok(Self {
            compatibility,
            loop_registry,
            coordination_registry,
            selection_registry,
            model_registry,
            model_evidence: evidence_by_model,
            selection_evidence,
        })
    }

    pub fn bind(
        &self,
        snapshot: &p::EvolutionSnapshot,
        scope: &p::Scope,
        model: &ModelProfile,
    ) -> p::Result<BoundM3Domains> {
        snapshot.validate()?;
        self.compatibility.validate()?;
        let loop_spec = self
            .resolve_active(snapshot, scope, p::StrategyDomain::Loop)?
            .map(|active| {
                let spec = self.loop_registry.resolve(&active.version)?;
                validate_loop_identity(active, &spec)?;
                self.validate_compatibility(&spec.compatibility, model)?;
                Ok(spec)
            })
            .transpose()?;
        let coordination = self
            .resolve_active(snapshot, scope, p::StrategyDomain::Coordination)?
            .map(|active| {
                let spec = self.coordination_registry.resolve(&active.version)?;
                validate_coordination_identity(active, &spec)?;
                self.validate_compatibility(&spec.compatibility, model)?;
                Ok(spec)
            })
            .transpose()?;
        let mut selection = BTreeMap::new();
        for (domain, target) in [
            (
                p::StrategyDomain::CapabilitySelection,
                p::SelectionTarget::Capability,
            ),
            (p::StrategyDomain::ModelSelection, p::SelectionTarget::Model),
            (
                p::StrategyDomain::BackendSelection,
                p::SelectionTarget::Backend,
            ),
        ] {
            if let Some(active) = self.resolve_active(snapshot, scope, domain)? {
                let spec = self.selection_registry.resolve(&active.version)?;
                validate_selection_identity(active, &spec, target)?;
                self.validate_compatibility(&spec.compatibility, model)?;
                selection.insert(target, spec);
            }
        }
        let model_adaptation = self
            .resolve_active(snapshot, scope, p::StrategyDomain::ModelAdaptation)?
            .map(|active| {
                let spec = self.model_registry.resolve(&active.version)?;
                validate_model_identity(active, &spec)?;
                self.validate_compatibility(&spec.compatibility, model)?;
                Ok(spec)
            })
            .transpose()?;
        let model_scaffold = model_adaptation
            .as_ref()
            .map(|spec| {
                let evidence = self
                    .model_evidence
                    .get(&model.profile_ref())
                    .ok_or_else(|| {
                        p::Error("active model adaptation has no measured outcome evidence".into())
                    })?;
                adapt_model_scaffold(spec, model, evidence, true)
            })
            .transpose()?;
        Ok(BoundM3Domains {
            schema_version: p::SchemaVersion(1),
            snapshot: snapshot.snapshot.clone(),
            loop_spec,
            coordination,
            selection,
            model_adaptation,
            model_scaffold,
        })
    }

    pub fn apply_loop(
        &self,
        bound: &BoundM3Domains,
        hard_budget: &LoopBudget,
    ) -> p::Result<Option<LoopBudget>> {
        bound
            .loop_spec
            .as_ref()
            .map(|spec| forme_loop::apply_loop_strategy(spec, hard_budget))
            .transpose()
    }

    pub fn rank_capabilities(
        &self,
        bound: &BoundM3Domains,
        capabilities: Vec<p::CapabilityRef>,
    ) -> p::Result<Vec<p::CapabilityRef>> {
        let Some(policy) = bound.selection_for(p::SelectionTarget::Capability) else {
            return Ok(capabilities);
        };
        let authorized = capabilities
            .iter()
            .map(|capability| p::ResourceRef(capability.0.clone()))
            .collect::<BTreeSet<_>>();
        let candidates = capabilities
            .iter()
            .map(|capability| SelectionCandidate {
                schema_version: p::SchemaVersion(1),
                reference: p::ResourceRef(capability.0.clone()),
                evidence_key: capability.clone(),
                cost_microunits: 0,
                latency_ms: 0,
                compatible: true,
                lifecycle_active: true,
                managed_allowed: true,
                permission_allowed: true,
                scope_allowed: true,
            })
            .collect();
        let prefiltered = PrefilteredSelectionSet::from_authorized(
            p::SelectionTarget::Capability,
            candidates,
            &authorized,
        )?;
        let ranked = rank_prefiltered(policy, &prefiltered, &self.selection_evidence)?;
        Ok(ranked
            .ordered
            .into_iter()
            .map(|resource| p::CapabilityRef(resource.0))
            .collect())
    }

    fn resolve_active<'a>(
        &self,
        snapshot: &'a p::EvolutionSnapshot,
        scope: &p::Scope,
        domain: p::StrategyDomain,
    ) -> p::Result<Option<&'a p::ActiveStrategyRef>> {
        let mut matches = snapshot
            .strategies
            .iter()
            .filter(|active| active.domain == domain && super::scope_within(scope, &active.scope))
            .collect::<Vec<_>>();
        matches.sort_by_key(|candidate| std::cmp::Reverse(candidate.scope.0.len()));
        if matches.len() > 1 && matches[0].scope.0.len() == matches[1].scope.0.len() {
            return Err(p::Error(format!(
                "active {domain:?} strategy is ambiguous for scope {}",
                scope.0
            )));
        }
        Ok(matches.into_iter().next())
    }

    fn validate_compatibility(
        &self,
        required: &p::StrategyRuntimeCompatibility,
        model: &ModelProfile,
    ) -> p::Result<()> {
        required.validate()?;
        if required.minimum_runtime_schema.0 > self.compatibility.schema_version.0
            || required.event_schema != self.compatibility.event_schema
            || required
                .model_profile
                .as_ref()
                .is_some_and(|profile| profile != &model.profile_ref())
            || required
                .tool_schema
                .as_ref()
                .is_some_and(|digest| digest != &self.compatibility.tool_schema)
            || required
                .backend_schema
                .as_ref()
                .is_some_and(|digest| digest != &self.compatibility.backend_schema)
        {
            return Err(p::Error(
                "active M3 strategy is incompatible with the bound runtime".into(),
            ));
        }
        Ok(())
    }
}

impl crate::ReactiveHarness {
    pub fn advance_long_horizon(
        &self,
        project: &mut LongHorizonProject,
        request: p::RunRequest,
        foreground_active: bool,
        outward: Option<LongHorizonOutwardStep>,
    ) -> p::Result<LongHorizonAdvance> {
        project.validate()?;
        if project.cancelled {
            return Ok(stopped_advance(
                LongHorizonDisposition::Cancelled,
                project.remaining_checkpoints,
            ));
        }
        if project.revoked {
            return Ok(stopped_advance(
                LongHorizonDisposition::Revoked,
                project.remaining_checkpoints,
            ));
        }
        if foreground_active {
            project.deferred_for_foreground = true;
            return Ok(stopped_advance(
                LongHorizonDisposition::Deferred,
                project.remaining_checkpoints,
            ));
        }
        if project.remaining_checkpoints == 0
            || project.next_checkpoint >= project.maximum_checkpoints
            || project.goal.expires_at <= super::now_ms()
        {
            return Ok(stopped_advance(LongHorizonDisposition::Limited, 0));
        }
        if request.source != p::Source::Schedule || request.idempotency_key.is_none() {
            return Err(p::Error(
                "long-horizon checkpoint requires scheduled idempotent ingress".into(),
            ));
        }
        if outward.as_ref().is_some_and(|step| {
            step.schema_version.0 == 0
                || step.request.source != step.intent.source
                || step.request.idempotency_key.is_none()
        }) {
            return Err(p::Error(
                "long-horizon outward step is incomplete or source-mismatched".into(),
            ));
        }

        let index = project.next_checkpoint;
        let checkpoint = p::GoalCheckpointRef(format!("checkpoint:{}:{index}", project.project.0));
        let artifact = p::ContentRef(format!("artifact:checkpoint:{}:{index}", project.project.0));
        let prepared = self.prepare_ingress_internal_as(request, Vec::new(), None)?;
        let (run, handle, session) = match prepared {
            super::PreparedSubmission::Existing(run) => {
                return Err(p::Error(format!(
                    "long-horizon checkpoint request already belongs to run {}",
                    run.0
                )));
            }
            super::PreparedSubmission::New {
                run,
                handle,
                session,
            } => (run, handle, session),
        };
        {
            let mut record = handle
                .record
                .lock()
                .map_err(|_| p::Error("run state is unavailable".into()))?;
            record.long_horizon = Some(PendingLongHorizonCheckpoint {
                goal: project.goal.clone(),
                intention: project.intention.clone(),
                route: project.route.clone(),
                index,
                checkpoint: checkpoint.clone(),
                artifact,
                deferred_for_foreground: project.deferred_for_foreground,
            });
        }
        self.drive_prepared_run(&run, &handle, &session)?;
        let result = AgentHarness::wait(self, run.clone())?;
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let snapshot = events
            .iter()
            .find_map(|event| match &event.payload {
                p::EventPayload::SessionBound(payload) => payload.evolution_snapshot.clone(),
                _ => None,
            })
            .ok_or_else(|| p::Error("checkpoint run did not bind an evolution snapshot".into()))?;
        let strategy_versions = handle
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?
            .m3_binding
            .as_ref()
            .map(BoundM3Domains::active_versions)
            .unwrap_or_default();
        let outward_run = if result.status == p::RunStatus::Complete {
            outward
                .map(|step| {
                    HarnessActionIngress::submit_action(
                        self,
                        step.request,
                        step.intent,
                        step.envelope,
                        step.prelude,
                    )
                })
                .transpose()?
        } else {
            None
        };
        project.remaining_checkpoints -= 1;
        project.next_checkpoint += 1;
        project.deferred_for_foreground = false;
        project.checkpoints.push(LongHorizonCheckpointRecord {
            schema_version: p::SchemaVersion(1),
            index,
            run: run.clone(),
            checkpoint: checkpoint.clone(),
            snapshot: snapshot.clone(),
            strategy_versions,
            status: result.status,
            outward_run: outward_run.clone(),
        });
        Ok(LongHorizonAdvance {
            schema_version: p::SchemaVersion(1),
            disposition: if result.status == p::RunStatus::Complete {
                LongHorizonDisposition::Advanced
            } else {
                LongHorizonDisposition::Failed
            },
            checkpoint_run: Some(run),
            outward_run,
            checkpoint: Some(checkpoint),
            snapshot: Some(snapshot),
            remaining_checkpoints: project.remaining_checkpoints,
        })
    }
}

fn stopped_advance(
    disposition: LongHorizonDisposition,
    remaining_checkpoints: u16,
) -> LongHorizonAdvance {
    LongHorizonAdvance {
        schema_version: p::SchemaVersion(1),
        disposition,
        checkpoint_run: None,
        outward_run: None,
        checkpoint: None,
        snapshot: None,
        remaining_checkpoints,
    }
}

fn validate_loop_identity(
    active: &p::ActiveStrategyRef,
    spec: &p::LoopStrategySpec,
) -> p::Result<()> {
    spec.validate()?;
    validate_identity(
        active,
        &spec.version,
        &spec.scope,
        &spec.content_ref,
        &spec.content_digest,
    )
}

fn validate_coordination_identity(
    active: &p::ActiveStrategyRef,
    spec: &p::CoordinationStrategySpec,
) -> p::Result<()> {
    spec.validate()?;
    validate_identity(
        active,
        &spec.version,
        &spec.scope,
        &spec.content_ref,
        &spec.content_digest,
    )
}

fn validate_selection_identity(
    active: &p::ActiveStrategyRef,
    spec: &p::SelectionStrategySpec,
    target: p::SelectionTarget,
) -> p::Result<()> {
    spec.validate()?;
    if spec.target != target {
        return Err(p::Error(
            "active selection domain does not match its strategy target".into(),
        ));
    }
    validate_identity(
        active,
        &spec.version,
        &spec.scope,
        &spec.content_ref,
        &spec.content_digest,
    )
}

fn validate_model_identity(
    active: &p::ActiveStrategyRef,
    spec: &p::ModelAdaptationSpec,
) -> p::Result<()> {
    spec.validate()?;
    validate_identity(
        active,
        &spec.version,
        &spec.scope,
        &spec.content_ref,
        &spec.content_digest,
    )
}

fn validate_identity(
    active: &p::ActiveStrategyRef,
    version: &p::StrategyVersionRef,
    scope: &p::Scope,
    content_ref: &p::ContentRef,
    digest: &p::SchemaDigest,
) -> p::Result<()> {
    active.validate()?;
    if &active.version != version
        || &active.scope != scope
        || &active.spec_ref != content_ref
        || &active.spec_digest != digest
    {
        return Err(p::Error(
            "active strategy reference does not match immutable registry content".into(),
        ));
    }
    Ok(())
}
