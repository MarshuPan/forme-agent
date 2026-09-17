use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use forme_protocol as p;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    Tool(p::ToolRef),
    Skill(p::SkillRef),
    McpTool(p::McpToolRef),
    PluginContribution(p::PluginContributionRef),
    Hook(p::HookRef),
    AppApi(p::ProviderId),
    AgentProfile(p::AgentProfileRef),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDescriptor {
    pub schema_version: p::SchemaVersion,
    pub id: p::CapabilityRef,
    pub capability: Capability,
    pub scope: p::Scope,
    pub permissions: Vec<p::PermissionRef>,
    pub risk: Option<p::Risk>,
    pub enabled: bool,
}

impl CapabilityDescriptor {
    pub fn id(&self) -> p::CapabilityRef {
        self.id.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySource {
    pub schema_version: p::SchemaVersion,
    pub source_ref: p::CapabilitySourceRef,
    pub trust: p::TrustTier,
    pub entries: Vec<CapabilityDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsetItem {
    pub schema_version: p::SchemaVersion,
    pub id: p::CapabilityRef,
    pub capability: Capability,
    pub sources: Vec<p::CapabilitySourceRef>,
}

impl ToolsetItem {
    pub fn id(&self) -> p::CapabilityRef {
        self.id.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolset {
    pub schema_version: p::SchemaVersion,
    pub toolset_ref: p::ToolsetRef,
    pub items: Vec<ToolsetItem>,
    pub scope: p::Scope,
    pub sources: Vec<p::CapabilitySourceRef>,
}

pub type CapabilityEvidence = p::CapabilityEvidence;

pub trait CapabilityRegistry {
    fn index(&self, source: CapabilitySource) -> p::Result<()>;
    fn resolve_toolset(&self, context: &p::ResolveContext) -> p::Result<Toolset>;
}

pub trait ToolsetResolver {
    fn resolve(&self, context: &p::ResolveContext)
        -> p::Result<(Toolset, Vec<CapabilityEvidence>)>;
}

#[derive(Default)]
pub struct InMemoryCapabilityRegistry {
    state: Mutex<RegistryState>,
}

pub type ToolRegistry = InMemoryCapabilityRegistry;

#[derive(Default)]
struct RegistryState {
    entries: BTreeMap<(p::CapabilityRef, p::CapabilitySourceRef), IndexedCapability>,
    evidence: Vec<CapabilityEvidence>,
    events: Vec<p::EventPayload>,
}

#[derive(Debug, Clone)]
struct IndexedCapability {
    descriptor: CapabilityDescriptor,
    source: p::CapabilitySourceRef,
    trust: p::TrustTier,
}

impl InMemoryCapabilityRegistry {
    pub fn remove_source(&self, source: &p::CapabilitySourceRef) -> p::Result<()> {
        let mut state = self.lock_state()?;
        state
            .entries
            .retain(|(_, entry_source), _| entry_source != source);
        Ok(())
    }

    pub fn replace_sources(
        &self,
        managed_sources: &[p::CapabilitySourceRef],
        sources: Vec<CapabilitySource>,
    ) -> p::Result<()> {
        if managed_sources
            .iter()
            .any(|source| source.0.trim().is_empty())
        {
            return Err(p::Error("managed capability source is unidentified".into()));
        }
        let managed = managed_sources.iter().cloned().collect::<BTreeSet<_>>();
        let mut replacements = Vec::new();
        let mut keys = BTreeSet::new();
        for source in sources {
            validate_source(&source)?;
            if !managed.contains(&source.source_ref) {
                return Err(p::Error(
                    "replacement capability source is outside the managed set".into(),
                ));
            }
            for descriptor in source.entries {
                let key = (descriptor.id.clone(), source.source_ref.clone());
                if !keys.insert(key.clone()) {
                    return Err(p::Error(
                        "replacement capability descriptor is duplicated".into(),
                    ));
                }
                replacements.push((
                    key,
                    IndexedCapability {
                        descriptor,
                        source: source.source_ref.clone(),
                        trust: source.trust,
                    },
                ));
            }
        }
        let mut state = self.lock_state()?;
        state
            .entries
            .retain(|(_, source), _| !managed.contains(source));
        for (key, indexed) in replacements {
            state.events.push(p::EventPayload::CapabilityIndexed(
                p::CapabilityIndexedPayload {
                    capability: indexed.descriptor.id.clone(),
                    sources: vec![indexed.source.clone()],
                },
            ));
            state.entries.insert(key, indexed);
        }
        Ok(())
    }

    pub fn record_evidence(&self, evidence: CapabilityEvidence) -> p::Result<()> {
        if evidence.capability.0.trim().is_empty()
            || evidence.outcome.0.trim().is_empty()
            || evidence.reliability.0.trim().is_empty()
        {
            return Err(p::Error("capability evidence is incomplete".into()));
        }
        let mut state = self.lock_state()?;
        state
            .events
            .push(p::EventPayload::CapabilityEvidenceRecorded(
                p::CapabilityEvidenceRecordedPayload {
                    capability: evidence.capability.clone(),
                    outcome: evidence.outcome.clone(),
                    reliability: evidence.reliability.clone(),
                },
            ));
        state.evidence.push(evidence);
        Ok(())
    }

    pub fn evidence(&self) -> Vec<CapabilityEvidence> {
        self.lock_state()
            .map(|state| state.evidence.clone())
            .unwrap_or_default()
    }

    pub fn take_events(&self) -> Vec<p::EventPayload> {
        self.lock_state()
            .map(|mut state| std::mem::take(&mut state.events))
            .unwrap_or_default()
    }

    fn lock_state(&self) -> p::Result<MutexGuard<'_, RegistryState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("capability registry state is unavailable".into()))
    }

    fn resolve_internal(
        &self,
        context: &p::ResolveContext,
    ) -> p::Result<(Toolset, Vec<CapabilityEvidence>)> {
        let mut state = self.lock_state()?;
        let mut resolved: BTreeMap<
            p::CapabilityRef,
            (Capability, BTreeSet<p::CapabilitySourceRef>),
        > = BTreeMap::new();

        for indexed in state.entries.values() {
            let descriptor = &indexed.descriptor;
            let Some(risk) = descriptor.risk else {
                continue;
            };
            if !descriptor.enabled
                || indexed.trust == p::TrustTier::Untrusted
                || risk > context.envelope.risk_limit
                || !context.policy_allowed_capabilities.contains(&descriptor.id)
                || !context
                    .envelope
                    .capability
                    .capabilities
                    .contains(&descriptor.id)
                || !descriptor
                    .permissions
                    .iter()
                    .all(|permission| context.envelope.capability.permissions.contains(permission))
                || !scope_contains(&descriptor.scope, &context.envelope.scope)
            {
                continue;
            }

            let entry = resolved
                .entry(descriptor.id.clone())
                .or_insert_with(|| (descriptor.capability.clone(), BTreeSet::new()));
            entry.1.insert(indexed.source.clone());
        }

        let mut all_sources = BTreeSet::new();
        let mut evidence = Vec::new();
        let items = resolved
            .into_iter()
            .map(|(id, (capability, sources))| {
                all_sources.extend(sources.iter().cloned());
                evidence.push(CapabilityEvidence {
                    schema_version: p::SchemaVersion(1),
                    capability: id.clone(),
                    outcome: p::CapabilityOutcome("available".into()),
                    reliability: p::Reliability("declared".into()),
                });
                ToolsetItem {
                    schema_version: p::SchemaVersion(1),
                    id,
                    capability,
                    sources: sources.into_iter().collect(),
                }
            })
            .collect();
        let sources: Vec<_> = all_sources.into_iter().collect();
        let toolset = Toolset {
            schema_version: p::SchemaVersion(1),
            toolset_ref: context.toolset.clone(),
            items,
            scope: context.envelope.scope.clone(),
            sources: sources.clone(),
        };
        state.events.push(p::EventPayload::ToolsetResolved(
            p::ToolsetResolvedPayload {
                toolset_ref: context.toolset.clone(),
                sources,
            },
        ));
        Ok((toolset, evidence))
    }
}

impl CapabilityRegistry for InMemoryCapabilityRegistry {
    fn index(&self, source: CapabilitySource) -> p::Result<()> {
        validate_source(&source)?;

        let mut state = self.lock_state()?;
        state
            .entries
            .retain(|(_, entry_source), _| entry_source != &source.source_ref);
        for descriptor in source.entries {
            state.events.push(p::EventPayload::CapabilityIndexed(
                p::CapabilityIndexedPayload {
                    capability: descriptor.id.clone(),
                    sources: vec![source.source_ref.clone()],
                },
            ));
            state.entries.insert(
                (descriptor.id.clone(), source.source_ref.clone()),
                IndexedCapability {
                    descriptor,
                    source: source.source_ref.clone(),
                    trust: source.trust,
                },
            );
        }
        Ok(())
    }

    fn resolve_toolset(&self, context: &p::ResolveContext) -> p::Result<Toolset> {
        self.resolve_internal(context).map(|(toolset, _)| toolset)
    }
}

impl ToolsetResolver for InMemoryCapabilityRegistry {
    fn resolve(
        &self,
        context: &p::ResolveContext,
    ) -> p::Result<(Toolset, Vec<CapabilityEvidence>)> {
        self.resolve_internal(context)
    }
}

fn validate_source(source: &CapabilitySource) -> p::Result<()> {
    if source.schema_version.0 == 0 || source.source_ref.0.trim().is_empty() {
        return Err(p::Error("capability source is unidentified".into()));
    }
    for descriptor in &source.entries {
        if descriptor.schema_version.0 == 0
            || descriptor.id.0.trim().is_empty()
            || descriptor.scope.0.trim().is_empty()
        {
            return Err(p::Error("capability descriptor is incomplete".into()));
        }
    }
    Ok(())
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
