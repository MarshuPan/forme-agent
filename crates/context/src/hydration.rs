//! Event-backed production context hydration.

use std::sync::Arc;

use forme_memory::{
    EventSourcedMemory, GraphQuery, MemoryGraph, MemoryProjection, MemoryScope, MemorySummary,
    TimeSlice,
};
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore};

use crate::{
    AgentWorkspace, ContextBudget, ContextSlice, ContextSources, HistoryEntry, HistoryWindow,
    LoadedSkillBody, Rule, RunCtx, SkillMetadata, ToolSchema,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextHydrationRequest {
    pub schema_version: p::SchemaVersion,
    pub run: p::RunId,
    pub session: p::SessionId,
    pub workspace: p::WorkspaceRef,
    pub scope: p::Scope,
    pub brain_call: bool,
    pub selected_skills: Vec<p::SkillRef>,
    pub model: p::ModelProfileRef,
    pub budget: ContextBudget,
}

impl ContextHydrationRequest {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.run.0.trim().is_empty()
            || self.session.0.trim().is_empty()
            || self.workspace.0.trim().is_empty()
            || self.scope.0.trim().is_empty()
            || self.model.0.trim().is_empty()
            || self.budget.schema_version.0 == 0
            || self.budget.max_tokens == 0
            || self.budget.reserve >= self.budget.max_tokens
            || self
                .selected_skills
                .iter()
                .any(|skill| skill.0.trim().is_empty())
        {
            return Err(p::Error("context hydration request is incomplete".into()));
        }
        Ok(())
    }
}

pub trait ContextHydrator: Send + Sync {
    fn hydrate(&self, request: &ContextHydrationRequest) -> p::Result<RunCtx>;
}

/// Resolves the capability and skill catalog for one hydration request.
///
/// The catalog is deliberately resolved at hydration time rather than frozen
/// in the composition root.  Capability lifecycle changes therefore become
/// visible to the next run without giving `context` ownership of registries or
/// a dependency back to `harness`.
pub trait ContextCatalogSource: Send + Sync {
    fn catalog(&self, request: &ContextHydrationRequest) -> p::Result<ContextCatalogSnapshot>;
}

pub trait ContextSliceSource: Send + Sync {
    fn slices(&self, request: &ContextHydrationRequest) -> p::Result<Vec<ContextSlice>>;
    fn agent_workspace(
        &self,
        request: &ContextHydrationRequest,
    ) -> p::Result<Option<AgentWorkspace>>;
}

#[derive(Debug, Default)]
pub struct EmptyContextSliceSource;

impl ContextSliceSource for EmptyContextSliceSource {
    fn slices(&self, _request: &ContextHydrationRequest) -> p::Result<Vec<ContextSlice>> {
        Ok(Vec::new())
    }

    fn agent_workspace(
        &self,
        _request: &ContextHydrationRequest,
    ) -> p::Result<Option<AgentWorkspace>> {
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCatalogSnapshot {
    pub schema_version: p::SchemaVersion,
    pub rules: Vec<Rule>,
    pub skills_metadata: Vec<SkillMetadata>,
    pub loaded_skill_bodies: Vec<LoadedSkillBody>,
    pub tool_schema: ToolSchema,
}

impl ContextCatalogSnapshot {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0 || self.tool_schema.schema_version.0 == 0 {
            return Err(p::Error("context catalog snapshot is not versioned".into()));
        }
        for rule in &self.rules {
            if rule.schema_version.0 == 0
                || rule.id.trim().is_empty()
                || rule.content.trim().is_empty()
                || !matches!(rule.provenance.actor, p::Actor::Owner | p::Actor::System)
                || !matches!(
                    rule.provenance.trust_tier,
                    p::TrustTier::OwnerInput | p::TrustTier::VerifiedProcess
                )
            {
                return Err(p::Error(
                    "context rule is not a trusted owner or fixed-kernel instruction".into(),
                ));
            }
        }
        if self.skills_metadata.iter().any(|skill| {
            skill.schema_version.0 == 0
                || skill.id.0.trim().is_empty()
                || skill.scope.0.trim().is_empty()
                || skill.version.0 == 0
        }) || self.loaded_skill_bodies.iter().any(|body| {
            body.schema_version.0 == 0
                || body.id.0.trim().is_empty()
                || body.body.trim().is_empty()
                || body.trigger.0.trim().is_empty()
        }) || self.tool_schema.entries.iter().any(|entry| {
            entry.schema_version.0 == 0
                || entry.tool.0.trim().is_empty()
                || entry.schema.trim().is_empty()
        }) {
            return Err(p::Error("context catalog contains an invalid entry".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct StaticContextCatalogSource {
    catalog: ContextCatalogSnapshot,
}

impl StaticContextCatalogSource {
    pub fn new(catalog: ContextCatalogSnapshot) -> p::Result<Self> {
        catalog.validate()?;
        Ok(Self { catalog })
    }
}

impl ContextCatalogSource for StaticContextCatalogSource {
    fn catalog(&self, _request: &ContextHydrationRequest) -> p::Result<ContextCatalogSnapshot> {
        Ok(self.catalog.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextHydrationLimits {
    pub max_session_runs: usize,
    pub max_history_entries: usize,
    pub max_slices: usize,
    pub max_workspace_items: usize,
    /// Upper bound on memory nodes recalled into one brain-level call.
    pub max_recall_items: usize,
}

impl Default for ContextHydrationLimits {
    fn default() -> Self {
        Self {
            max_session_runs: 32,
            max_history_entries: 64,
            max_slices: 32,
            max_workspace_items: 10,
            max_recall_items: 8,
        }
    }
}

pub struct EventBackedContextHydrator {
    store: Arc<SqliteEventStore>,
    memory: Arc<EventSourcedMemory<SqliteEventStore>>,
    slices: Arc<dyn ContextSliceSource>,
    catalog_source: Arc<dyn ContextCatalogSource>,
    limits: ContextHydrationLimits,
}

impl EventBackedContextHydrator {
    pub fn new(
        store: Arc<SqliteEventStore>,
        memory: Arc<EventSourcedMemory<SqliteEventStore>>,
        slices: Arc<dyn ContextSliceSource>,
        catalog: ContextCatalogSnapshot,
        limits: ContextHydrationLimits,
    ) -> p::Result<Self> {
        let catalog_source = Arc::new(StaticContextCatalogSource::new(catalog)?);
        Self::new_with_catalog_source(store, memory, slices, catalog_source, limits)
    }

    pub fn new_with_catalog_source(
        store: Arc<SqliteEventStore>,
        memory: Arc<EventSourcedMemory<SqliteEventStore>>,
        slices: Arc<dyn ContextSliceSource>,
        catalog_source: Arc<dyn ContextCatalogSource>,
        limits: ContextHydrationLimits,
    ) -> p::Result<Self> {
        if limits.max_session_runs == 0
            || limits.max_history_entries == 0
            || limits.max_slices == 0
            || limits.max_workspace_items == 0
            || limits.max_recall_items == 0
        {
            return Err(p::Error("context hydration limits are invalid".into()));
        }
        Ok(Self {
            store,
            memory,
            slices,
            catalog_source,
            limits,
        })
    }

    fn history(&self, request: &ContextHydrationRequest) -> p::Result<HistoryWindow> {
        let mut matching_runs = Vec::new();
        for run in self.store.run_ids()? {
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            let session_matches = events.iter().any(|event| {
                matches!(
                    &event.payload,
                    p::EventPayload::RunAccepted(payload) if payload.session_ref == request.session
                )
            });
            let workspace_matches = events.iter().any(|event| {
                matches!(
                    &event.payload,
                    p::EventPayload::SessionBound(payload) if payload.workspace == request.workspace
                )
            });
            if session_matches && (workspace_matches || run == request.run) {
                let last_time = events.last().map(|event| event.ts_unix_ms).unwrap_or(0);
                matching_runs.push((last_time, run, events));
            }
        }
        matching_runs
            .sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        matching_runs.truncate(self.limits.max_session_runs);

        let mut entries = matching_runs
            .into_iter()
            .flat_map(|(_, _, events)| events)
            .filter_map(|event| {
                let content = match &event.payload {
                    p::EventPayload::RunAccepted(payload) => Some(payload.input_ref.0.clone()),
                    p::EventPayload::ModelCallDelta(payload) => Some(payload.delta.clone()),
                    p::EventPayload::ActionOutputDelta(payload) => Some(payload.delta.clone()),
                    _ => None,
                }?;
                Some((
                    event.ts_unix_ms,
                    event.run_id.clone(),
                    event.stream_seq,
                    HistoryEntry {
                        schema_version: p::SchemaVersion(1),
                        event_ref: event.event_id,
                        content,
                        provenance: event.provenance,
                    },
                ))
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });
        if entries.len() > self.limits.max_history_entries {
            entries.drain(..entries.len() - self.limits.max_history_entries);
        }
        Ok(HistoryWindow {
            schema_version: p::SchemaVersion(1),
            entries: entries.into_iter().map(|(_, _, _, entry)| entry).collect(),
        })
    }

    /// Bounded recall of the owner's memory for a brain-level call. Nodes
    /// keep their original provenance, so memory that came from external
    /// content remains data rather than instruction. Until activation-based
    /// relevance replaces it, the newest nodes in the request's scopes win.
    fn memory_recall(&self, request: &ContextHydrationRequest) -> Vec<ContextSlice> {
        if !request.brain_call {
            return Vec::new();
        }
        let mut nodes = [TimeSlice::Session, TimeSlice::Daily, TimeSlice::LongTerm]
            .into_iter()
            .flat_map(|slice| {
                self.memory.query(GraphQuery {
                    schema_version: p::SchemaVersion(1),
                    scope: Some(MemoryScope {
                        schema_version: p::SchemaVersion(1),
                        slice,
                        workspace: Some(request.workspace.clone()),
                        scope: request.scope.clone(),
                    }),
                    kind: None,
                    content_contains: None,
                })
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| {
            right
                .recency
                .cmp(&left.recency)
                .then_with(|| left.id.cmp(&right.id))
        });
        nodes.truncate(self.limits.max_recall_items);
        nodes
            .into_iter()
            .map(|node| ContextSlice {
                schema_version: p::SchemaVersion(1),
                id: p::ContextSliceRef(format!("memory:recall:{}", node.id.0)),
                content: format!(
                    "recalled {:?} memory {} at tier {:?}: {}",
                    node.kind, node.id.0, node.tier, node.content_ref.0
                ),
                provenance: node.provenance,
            })
            .collect()
    }

    fn memory_summary(&self, request: &ContextHydrationRequest) -> MemorySummary {
        let scopes = [TimeSlice::Session, TimeSlice::Daily, TimeSlice::LongTerm];
        let summaries = scopes.map(|slice| {
            self.memory.summary(MemoryScope {
                schema_version: p::SchemaVersion(1),
                slice,
                workspace: Some(request.workspace.clone()),
                scope: request.scope.clone(),
            })
        });
        let raw_event_count = summaries
            .iter()
            .map(|summary| summary.raw_event_count)
            .sum::<u64>();
        let mut source_refs = summaries
            .iter()
            .flat_map(|summary| summary.source_refs.clone())
            .collect::<Vec<_>>();
        source_refs.sort();
        source_refs.dedup();
        let text = if raw_event_count == 0 {
            String::new()
        } else {
            format!(
                "{raw_event_count} bounded memory records are available across session, daily, and long-term tiers"
            )
        };
        MemorySummary {
            schema_version: p::SchemaVersion(1),
            scope: request.scope.clone(),
            text,
            source_refs,
            raw_event_count,
            is_raw_dump: false,
            provenance: internal_provenance(),
        }
    }
}

impl ContextHydrator for EventBackedContextHydrator {
    fn hydrate(&self, request: &ContextHydrationRequest) -> p::Result<RunCtx> {
        request.validate()?;
        let catalog = self.catalog_source.catalog(request)?;
        catalog.validate()?;
        let mut slices = self.slices.slices(request)?;
        slices.extend(self.memory_recall(request));
        if slices.iter().any(|slice| {
            slice.schema_version.0 == 0
                || slice.id.0.trim().is_empty()
                || slice.content.trim().is_empty()
        }) {
            return Err(p::Error(
                "context slice source returned invalid data".into(),
            ));
        }
        slices.sort_by(|left, right| left.id.cmp(&right.id));
        let mut unique_slices: Vec<ContextSlice> = Vec::with_capacity(slices.len());
        for slice in slices {
            if let Some(previous) = unique_slices.last() {
                if previous.id == slice.id {
                    if previous == &slice {
                        continue;
                    }
                    return Err(p::Error(format!(
                        "context slice {} has conflicting content or provenance",
                        slice.id.0
                    )));
                }
            }
            unique_slices.push(slice);
        }
        let slices = unique_slices;

        let mut agent_workspace = if request.brain_call {
            self.slices.agent_workspace(request)?
        } else {
            None
        };
        if let Some(workspace) = &mut agent_workspace {
            if workspace.schema_version.0 == 0
                || workspace.snapshot_ref.0.trim().is_empty()
                || workspace.items.iter().any(|item| {
                    item.schema_version.0 == 0
                        || item.id.trim().is_empty()
                        || item.content.trim().is_empty()
                })
            {
                return Err(p::Error(
                    "context slice source returned an invalid AgentWorkspace".into(),
                ));
            }
            workspace.items.sort_by(|left, right| {
                right
                    .value
                    .saturating_mul(right.urgency)
                    .cmp(&left.value.saturating_mul(left.urgency))
                    .then_with(|| right.urgency.cmp(&left.urgency))
                    .then_with(|| left.id.cmp(&right.id))
            });
        }

        let selected = request
            .selected_skills
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let loaded_skill_bodies = catalog
            .loaded_skill_bodies
            .iter()
            .filter(|body| selected.contains(&body.id))
            .cloned()
            .collect::<Vec<_>>();
        let sources = ContextSources {
            schema_version: p::SchemaVersion(1),
            rules: catalog.rules,
            history: self.history(request)?,
            memory_summary: self.memory_summary(request),
            skills_metadata: catalog.skills_metadata,
            loaded_skill_bodies,
            tool_schema: catalog.tool_schema,
            slices,
            agent_workspace,
        };
        let has_context = !sources.rules.is_empty()
            || !sources.history.entries.is_empty()
            || !sources.memory_summary.text.trim().is_empty()
            || !sources.skills_metadata.is_empty()
            || !sources.tool_schema.entries.is_empty()
            || !sources.slices.is_empty()
            || sources.agent_workspace.is_some();
        if !has_context {
            return Err(p::Error(
                "formal model context hydration produced no usable source".into(),
            ));
        }
        Ok(RunCtx {
            schema_version: p::SchemaVersion(1),
            run: request.run.clone(),
            session: request.session.clone(),
            scope: request.scope.clone(),
            selected_skills: request.selected_skills.clone(),
            brain_call: request.brain_call,
            sources,
        })
    }
}

fn internal_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextBuilder;
    use forme_store::StoreOptions;

    #[derive(Clone, Default)]
    struct StaticSliceSource {
        slices: Vec<ContextSlice>,
        workspace: Option<AgentWorkspace>,
    }

    impl ContextSliceSource for StaticSliceSource {
        fn slices(&self, _request: &ContextHydrationRequest) -> p::Result<Vec<ContextSlice>> {
            Ok(self.slices.clone())
        }

        fn agent_workspace(
            &self,
            _request: &ContextHydrationRequest,
        ) -> p::Result<Option<AgentWorkspace>> {
            Ok(self.workspace.clone())
        }
    }

    fn provenance(source: p::Source, actor: p::Actor, trust: p::TrustTier) -> p::Provenance {
        p::Provenance {
            source,
            actor,
            trust_tier: trust,
            caused_by: None,
        }
    }

    fn event(
        id: &str,
        run: &p::RunId,
        timestamp: i64,
        payload: p::EventPayload,
        provenance: p::Provenance,
    ) -> p::Event {
        p::Event::new(
            p::EventId(id.into()),
            run.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            timestamp,
            provenance,
        )
    }

    fn append_history_run(
        store: &SqliteEventStore,
        run_name: &str,
        session: &str,
        workspace: &str,
        marker: &str,
        timestamp: i64,
    ) {
        let run = p::RunId(run_name.into());
        store
            .append(event(
                &format!("{run_name}:accepted"),
                &run,
                timestamp,
                p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                    source: p::Source::UserTurn,
                    session_ref: p::SessionId(session.into()),
                    input_ref: p::InputRef(format!("owner:{marker}")),
                    idempotency_key: None,
                }),
                provenance(
                    p::Source::UserTurn,
                    p::Actor::Owner,
                    p::TrustTier::OwnerInput,
                ),
            ))
            .unwrap();
        store
            .append(event(
                &format!("{run_name}:bound"),
                &run,
                timestamp + 1,
                p::EventPayload::SessionBound(p::SessionBoundPayload {
                    policy_profile: p::PolicyProfileRef("policy:test".into()),
                    model_profile: p::ModelProfileRef("model:test".into()),
                    toolset_ref: p::ToolsetRef("tools:test".into()),
                    workspace: p::WorkspaceRef(workspace.into()),
                    effect_mode: None,
                    evolution_snapshot: None,
                    federation_snapshot: None,
                    composition_manifest: None,
                }),
                internal_provenance(),
            ))
            .unwrap();
        store
            .append(event(
                &format!("{run_name}:model"),
                &run,
                timestamp + 2,
                p::EventPayload::ModelCallDelta(p::ModelCallDeltaPayload {
                    call_id: p::ModelCallId(format!("call:{run_name}")),
                    delta: format!("model:{marker}"),
                }),
                provenance(
                    p::Source::Internal,
                    p::Actor::Agent,
                    p::TrustTier::Untrusted,
                ),
            ))
            .unwrap();
        store
            .append(event(
                &format!("{run_name}:tool"),
                &run,
                timestamp + 3,
                p::EventPayload::ActionOutputDelta(p::ActionOutputDeltaPayload {
                    intent_id: p::ActionId(format!("action:{run_name}")),
                    backend: p::BackendKind::Shell,
                    scope: p::Scope(workspace.into()),
                    delta: format!("tool:{marker}"),
                    truncated: false,
                    trust: p::TrustTier::Untrusted,
                    content_ref: None,
                    remote_lease: None,
                }),
                provenance(
                    p::Source::Internal,
                    p::Actor::System,
                    p::TrustTier::Untrusted,
                ),
            ))
            .unwrap();
    }

    fn catalog() -> ContextCatalogSnapshot {
        let scope = p::Scope("workspace:alpha".into());
        ContextCatalogSnapshot {
            schema_version: p::SchemaVersion(1),
            rules: vec![Rule {
                schema_version: p::SchemaVersion(1),
                id: "fixed:owner-loyalty".into(),
                content: "Serve the authenticated owner.".into(),
                provenance: internal_provenance(),
            }],
            skills_metadata: vec![
                SkillMetadata {
                    schema_version: p::SchemaVersion(1),
                    id: p::SkillRef("skill:selected".into()),
                    summary: "Selected skill".into(),
                    scope: scope.clone(),
                    version: p::Version(1),
                    trust: p::TrustTier::ApprovedSource,
                },
                SkillMetadata {
                    schema_version: p::SchemaVersion(1),
                    id: p::SkillRef("skill:other".into()),
                    summary: "Other skill".into(),
                    scope,
                    version: p::Version(1),
                    trust: p::TrustTier::ApprovedSource,
                },
            ],
            loaded_skill_bodies: vec![
                LoadedSkillBody {
                    schema_version: p::SchemaVersion(1),
                    id: p::SkillRef("skill:selected".into()),
                    body: "selected body".into(),
                    trigger: p::SkillTriggerRef("selected".into()),
                    provenance: provenance(
                        p::Source::Internal,
                        p::Actor::System,
                        p::TrustTier::ApprovedSource,
                    ),
                },
                LoadedSkillBody {
                    schema_version: p::SchemaVersion(1),
                    id: p::SkillRef("skill:other".into()),
                    body: "other body must stay out".into(),
                    trigger: p::SkillTriggerRef("other".into()),
                    provenance: provenance(
                        p::Source::Internal,
                        p::Actor::System,
                        p::TrustTier::ApprovedSource,
                    ),
                },
            ],
            tool_schema: ToolSchema {
                schema_version: p::SchemaVersion(1),
                version: p::Version(1),
                entries: vec![crate::ToolSchemaEntry {
                    schema_version: p::SchemaVersion(1),
                    tool: p::ToolRef("tool:read".into()),
                    schema: "{path:string}".into(),
                }],
                provenance: internal_provenance(),
            },
        }
    }

    fn empty_catalog() -> ContextCatalogSnapshot {
        ContextCatalogSnapshot {
            schema_version: p::SchemaVersion(1),
            rules: Vec::new(),
            skills_metadata: Vec::new(),
            loaded_skill_bodies: Vec::new(),
            tool_schema: ToolSchema {
                schema_version: p::SchemaVersion(1),
                version: p::Version(1),
                entries: Vec::new(),
                provenance: internal_provenance(),
            },
        }
    }

    fn request() -> ContextHydrationRequest {
        ContextHydrationRequest {
            schema_version: p::SchemaVersion(1),
            run: p::RunId("run:current".into()),
            session: p::SessionId("session:owner".into()),
            workspace: p::WorkspaceRef("workspace:alpha".into()),
            scope: p::Scope("workspace:alpha".into()),
            brain_call: true,
            selected_skills: vec![p::SkillRef("skill:selected".into())],
            model: p::ModelProfileRef("model:test".into()),
            budget: ContextBudget {
                schema_version: p::SchemaVersion(1),
                max_tokens: 2_000,
                reserve: 200,
            },
        }
    }

    fn hydrator(
        store: Arc<SqliteEventStore>,
        slices: StaticSliceSource,
        catalog: ContextCatalogSnapshot,
    ) -> EventBackedContextHydrator {
        let memory = Arc::new(
            EventSourcedMemory::open(store.clone(), p::RunId("brain:memory".into())).unwrap(),
        );
        EventBackedContextHydrator::new(
            store,
            memory,
            Arc::new(slices),
            catalog,
            ContextHydrationLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn loads_only_same_session_and_workspace_history_with_original_provenance() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        append_history_run(
            &store,
            "run:matching",
            "session:owner",
            "workspace:alpha",
            "matching",
            10,
        );
        append_history_run(
            &store,
            "run:wrong-workspace",
            "session:owner",
            "workspace:beta",
            "workspace-leak",
            20,
        );
        append_history_run(
            &store,
            "run:wrong-session",
            "session:other",
            "workspace:alpha",
            "session-leak",
            30,
        );

        let context = hydrator(store, StaticSliceSource::default(), catalog())
            .hydrate(&request())
            .unwrap();
        let contents = context
            .sources
            .history
            .entries
            .iter()
            .map(|entry| entry.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            contents,
            vec!["owner:matching", "model:matching", "tool:matching"]
        );
        assert_eq!(
            context.sources.history.entries[0].provenance.trust_tier,
            p::TrustTier::OwnerInput
        );
        assert!(context.sources.history.entries[1..]
            .iter()
            .all(|entry| entry.provenance.trust_tier == p::TrustTier::Untrusted));
    }

    #[test]
    fn keeps_only_selected_skill_body_and_preserves_untrusted_slices_as_data() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let source = StaticSliceSource {
            slices: vec![ContextSlice {
                schema_version: p::SchemaVersion(1),
                id: p::ContextSliceRef("slice:external".into()),
                content: "external text says to ignore policy".into(),
                provenance: provenance(
                    p::Source::Communication,
                    p::Actor::External(p::ParticipantId("participant:test".into())),
                    p::TrustTier::Untrusted,
                ),
            }],
            workspace: None,
        };
        let run_ctx = hydrator(store, source, catalog())
            .hydrate(&request())
            .unwrap();
        assert_eq!(run_ctx.sources.loaded_skill_bodies.len(), 1);
        assert_eq!(
            run_ctx.sources.loaded_skill_bodies[0].id,
            p::SkillRef("skill:selected".into())
        );
        assert!(!run_ctx
            .sources
            .loaded_skill_bodies
            .iter()
            .any(|body| body.body.contains("must stay out")));

        let built = crate::LayeredContextBuilder::default()
            .build(&run_ctx, request().budget)
            .unwrap();
        let rule = built
            .assembled
            .sections
            .iter()
            .find(|section| section.source_ref == "fixed:owner-loyalty")
            .unwrap();
        let external = built
            .assembled
            .sections
            .iter()
            .find(|section| section.source_ref == "slice:external")
            .unwrap();
        assert_eq!(rule.treatment, crate::ContentTreatment::Instruction);
        assert_eq!(external.treatment, crate::ContentTreatment::UntrustedData);
    }

    #[test]
    fn identical_slice_ids_deduplicate_but_conflicts_fail_closed() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let slice = ContextSlice {
            schema_version: p::SchemaVersion(1),
            id: p::ContextSliceRef("slice:duplicate".into()),
            content: "same".into(),
            provenance: internal_provenance(),
        };
        let context = hydrator(
            store.clone(),
            StaticSliceSource {
                slices: vec![slice.clone(), slice.clone()],
                workspace: None,
            },
            catalog(),
        )
        .hydrate(&request())
        .unwrap();
        assert_eq!(context.sources.slices, vec![slice.clone()]);

        let mut conflicting = slice;
        conflicting.content = "different".into();
        let error = hydrator(
            store,
            StaticSliceSource {
                slices: vec![
                    conflicting.clone(),
                    ContextSlice {
                        content: "same".into(),
                        ..conflicting
                    },
                ],
                workspace: None,
            },
            catalog(),
        )
        .hydrate(&request())
        .unwrap_err();
        assert!(error.0.contains("conflicting content or provenance"));
    }

    #[test]
    fn empty_formal_context_fails_closed_instead_of_calling_a_model() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let error = hydrator(store, StaticSliceSource::default(), empty_catalog())
            .hydrate(&request())
            .unwrap_err();
        assert!(error.0.contains("no usable source"));
    }

    #[test]
    fn brain_recall_is_bounded_newest_first_scoped_and_keeps_untrusted_data() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let memory = Arc::new(
            EventSourcedMemory::open(store.clone(), p::RunId("brain:memory".into())).unwrap(),
        );
        let scope = |workspace: &str| MemoryScope {
            schema_version: p::SchemaVersion(1),
            slice: TimeSlice::LongTerm,
            workspace: Some(p::WorkspaceRef(workspace.into())),
            scope: p::Scope(workspace.into()),
        };
        for (id, workspace, recency, trust) in [
            (
                "node:oldest",
                "workspace:alpha",
                10,
                p::TrustTier::OwnerInput,
            ),
            (
                "node:external",
                "workspace:alpha",
                30,
                p::TrustTier::Untrusted,
            ),
            (
                "node:newest",
                "workspace:alpha",
                40,
                p::TrustTier::OwnerInput,
            ),
            (
                "node:elsewhere",
                "workspace:beta",
                50,
                p::TrustTier::OwnerInput,
            ),
        ] {
            memory
                .add_node(forme_memory::MemoryNode {
                    schema_version: p::SchemaVersion(1),
                    id: p::NodeId(id.into()),
                    kind: forme_memory::NodeKind::Fact,
                    content_ref: p::ContentRef(format!("content:{id}")),
                    tier: p::StabilityTier::Working,
                    confidence: p::Confidence(0.7),
                    scope: scope(workspace),
                    resting_activation: p::RestingActivation(0.1),
                    recency,
                    provenance: provenance(p::Source::Internal, p::Actor::System, trust),
                })
                .unwrap();
        }
        let limits = ContextHydrationLimits {
            max_recall_items: 2,
            ..ContextHydrationLimits::default()
        };
        let hydrator = EventBackedContextHydrator::new(
            store,
            memory,
            Arc::new(StaticSliceSource::default()),
            catalog(),
            limits,
        )
        .unwrap();

        let recalled = hydrator
            .hydrate(&request())
            .unwrap()
            .sources
            .slices
            .into_iter()
            .filter(|slice| slice.id.0.starts_with("memory:recall:"))
            .collect::<Vec<_>>();
        assert_eq!(
            recalled
                .iter()
                .map(|slice| slice.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["memory:recall:node:external", "memory:recall:node:newest"],
            "the two newest in-scope nodes, in stable slice order"
        );
        let external = recalled
            .iter()
            .find(|slice| slice.id.0.ends_with("node:external"))
            .unwrap();
        assert_eq!(external.provenance.trust_tier, p::TrustTier::Untrusted);

        let mut execution_node = request();
        execution_node.brain_call = false;
        assert!(hydrator
            .hydrate(&execution_node)
            .unwrap()
            .sources
            .slices
            .iter()
            .all(|slice| !slice.id.0.starts_with("memory:recall:")));
    }
}
