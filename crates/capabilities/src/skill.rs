use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use forme_protocol as p;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMetadata {
    pub schema_version: p::SchemaVersion,
    pub id: p::SkillRef,
    pub summary: String,
    pub scope: p::Scope,
    pub version: p::Version,
    pub trust: p::TrustTier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillBody(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    pub schema_version: p::SchemaVersion,
    pub metadata: SkillMetadata,
    pub body: SkillBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSearchQuery {
    pub schema_version: p::SchemaVersion,
    pub text: String,
    pub scope: p::Scope,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSearchHit {
    pub schema_version: p::SchemaVersion,
    pub metadata: SkillMetadata,
    pub score: u32,
    pub matched_terms: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadTrigger {
    Selected,
    Referenced,
}

impl LoadTrigger {
    fn as_ref(self) -> p::SkillTriggerRef {
        let value = match self {
            Self::Selected => "selected",
            Self::Referenced => "referenced",
        };
        p::SkillTriggerRef(value.into())
    }
}

pub trait SkillRegistry {
    fn metadata(&self, scope: p::Scope) -> Vec<SkillMetadata>;
    fn load_body(&self, id: p::SkillRef, trigger: LoadTrigger) -> p::Result<SkillBody>;
}

pub struct InMemorySkillRegistry {
    state: Mutex<SkillState>,
    search_limit: usize,
}

#[derive(Default)]
struct SkillState {
    definitions: BTreeMap<p::SkillRef, SkillDefinition>,
    disabled: BTreeSet<p::SkillRef>,
    events: Vec<p::EventPayload>,
}

impl InMemorySkillRegistry {
    pub fn new(definitions: Vec<SkillDefinition>) -> p::Result<Self> {
        Self::with_search_limit(definitions, 8)
    }

    pub fn with_search_limit(
        definitions: Vec<SkillDefinition>,
        search_limit: usize,
    ) -> p::Result<Self> {
        if search_limit == 0 {
            return Err(p::Error("skill search limit must be non-zero".into()));
        }
        let mut indexed = BTreeMap::new();
        for definition in definitions {
            if definition.metadata.id.0.trim().is_empty()
                || definition.metadata.summary.trim().is_empty()
                || definition.metadata.scope.0.trim().is_empty()
                || definition.body.0.trim().is_empty()
            {
                return Err(p::Error("skill definition is incomplete".into()));
            }
            if indexed
                .insert(definition.metadata.id.clone(), definition)
                .is_some()
            {
                return Err(p::Error("skill id is duplicated".into()));
            }
        }
        Ok(Self {
            state: Mutex::new(SkillState {
                definitions: indexed,
                disabled: BTreeSet::new(),
                events: Vec::new(),
            }),
            search_limit,
        })
    }

    pub fn set_enabled(&self, id: p::SkillRef, enabled: bool) -> p::Result<()> {
        let mut state = self.lock_state()?;
        if !state.definitions.contains_key(&id) {
            return Err(p::Error("skill is not registered".into()));
        }
        if enabled {
            state.disabled.remove(&id);
        } else {
            state.disabled.insert(id);
        }
        Ok(())
    }

    pub fn search(&self, query: SkillSearchQuery) -> p::Result<Vec<SkillSearchHit>> {
        if query.schema_version.0 == 0
            || query.text.trim().is_empty()
            || query.scope.0.trim().is_empty()
            || query.limit == 0
        {
            return Err(p::Error("skill search query is incomplete".into()));
        }
        let terms = search_terms(&query.text);
        if terms.is_empty() {
            return Err(p::Error(
                "skill search query has no searchable terms".into(),
            ));
        }
        let query_text = query.text.to_lowercase();
        let mut state = self.lock_state()?;
        let mut hits = state
            .definitions
            .values()
            .filter(|definition| {
                definition.metadata.trust != p::TrustTier::Untrusted
                    && !state.disabled.contains(&definition.metadata.id)
                    && scope_contains(&definition.metadata.scope, &query.scope)
            })
            .filter_map(|definition| rank_skill(definition, &query_text, &terms))
            .collect::<Vec<_>>();
        hits.sort_by_key(|hit| (Reverse(hit.score), hit.metadata.id.clone()));
        hits.truncate(query.limit.min(self.search_limit));
        for hit in &hits {
            state.events.push(metadata_event(&hit.metadata));
        }
        Ok(hits)
    }

    pub fn take_events(&self) -> Vec<p::EventPayload> {
        self.lock_state()
            .map(|mut state| std::mem::take(&mut state.events))
            .unwrap_or_default()
    }

    fn lock_state(&self) -> p::Result<MutexGuard<'_, SkillState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("skill registry state is unavailable".into()))
    }
}

impl SkillRegistry for InMemorySkillRegistry {
    fn metadata(&self, scope: p::Scope) -> Vec<SkillMetadata> {
        self.lock_state()
            .map(|mut state| {
                let metadata: Vec<_> = state
                    .definitions
                    .values()
                    .filter(|definition| {
                        definition.metadata.trust != p::TrustTier::Untrusted
                            && !state.disabled.contains(&definition.metadata.id)
                            && scope_contains(&definition.metadata.scope, &scope)
                    })
                    .map(|definition| definition.metadata.clone())
                    .collect();
                for item in &metadata {
                    state.events.push(metadata_event(item));
                }
                metadata
            })
            .unwrap_or_default()
    }

    fn load_body(&self, id: p::SkillRef, trigger: LoadTrigger) -> p::Result<SkillBody> {
        let mut state = self.lock_state()?;
        let definition = state
            .definitions
            .get(&id)
            .cloned()
            .ok_or_else(|| p::Error("skill is not registered".into()))?;
        if state.disabled.contains(&id) {
            return Err(p::Error("disabled skill body is not loadable".into()));
        }
        if definition.metadata.trust == p::TrustTier::Untrusted {
            return Err(p::Error("untrusted skill body is not loadable".into()));
        }
        state.events.push(p::EventPayload::SkillBodyLoaded(
            p::SkillBodyLoadedPayload {
                skill: definition.metadata.id,
                trigger: trigger.as_ref(),
                scope: definition.metadata.scope,
                version: definition.metadata.version,
                trust: definition.metadata.trust,
            },
        ));
        Ok(definition.body)
    }
}

fn metadata_event(metadata: &SkillMetadata) -> p::EventPayload {
    p::EventPayload::SkillMetadataExposed(p::SkillMetadataExposedPayload {
        skills: vec![p::SkillDescriptorRef(metadata.id.0.clone())],
        scope: metadata.scope.clone(),
        version: metadata.version,
        trust: metadata.trust,
    })
}

fn search_terms(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn rank_skill(
    definition: &SkillDefinition,
    query: &str,
    terms: &[String],
) -> Option<SkillSearchHit> {
    let id = definition.metadata.id.0.to_lowercase();
    let summary = definition.metadata.summary.to_lowercase();
    let mut score = u32::from(id.contains(query)) * 20;
    let mut matched_terms = Vec::new();
    for term in terms {
        let id_match = id.contains(term);
        let summary_match = summary.contains(term);
        if id_match || summary_match {
            score = score
                .saturating_add(u32::from(id_match) * 10)
                .saturating_add(u32::from(summary_match) * 4);
            matched_terms.push(term.clone());
        }
    }
    (score > 0).then(|| SkillSearchHit {
        schema_version: p::SchemaVersion(1),
        metadata: definition.metadata.clone(),
        score,
        explanation: format!(
            "matched {} query terms within id or summary",
            matched_terms.len()
        ),
        matched_terms,
    })
}

fn scope_contains(granted: &p::Scope, requested: &p::Scope) -> bool {
    if granted.0 == "*" || granted == requested {
        return true;
    }
    requested
        .0
        .strip_prefix(&granted.0)
        .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('/'))
}
