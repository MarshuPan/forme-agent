use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use forme_protocol as p;
use sha2::{Digest, Sha256};

use crate::{
    Capability, CapabilityDescriptor, CapabilitySource, InMemoryCapabilityRegistry,
    SkillDefinition, StdioMcpServer,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    pub schema_version: p::SchemaVersion,
    pub id: p::PluginRef,
    pub manifest_ref: p::PluginManifestRef,
    pub trust: p::TrustTier,
    pub contributions: Vec<PluginContribution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginContribution {
    Tool(CapabilityDescriptor),
    Skill(SkillDefinition),
    McpServer(StdioMcpServer),
    Hook(Hook),
    Config(PluginConfigContribution),
}

impl PluginContribution {
    fn reference(&self) -> p::PluginContributionRef {
        let value = match self {
            Self::Tool(tool) => format!("tool:{}", tool.id.0),
            Self::Skill(skill) => format!("skill:{}", skill.metadata.id.0),
            Self::McpServer(server) => format!("mcp:{}", server.provider_id.0),
            Self::Hook(hook) => format!("hook:{}", hook.id.0),
            Self::Config(config) => format!("config:{}", config.key),
        };
        p::PluginContributionRef(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginConfigContribution {
    pub schema_version: p::SchemaVersion,
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPoint {
    BeforePolicy,
    BeforeExecution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookTransform {
    Identity,
    ReplaceCapability(p::CapabilityRef),
    ReplaceShellProgram(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hook {
    pub schema_version: p::SchemaVersion,
    pub id: p::HookRef,
    pub point: HookPoint,
    pub transform: HookTransform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookProposal {
    pub schema_version: p::SchemaVersion,
    pub intent: p::ActionIntent,
    pub requires_policy_recheck: p::RequiredTrue,
}

#[derive(Default)]
pub struct HookRegistry {
    state: Mutex<BTreeMap<p::HookRef, (p::CapabilitySourceRef, Hook)>>,
}

impl HookRegistry {
    fn remove_source(&self, source: &p::CapabilitySourceRef) -> p::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| p::Error("hook registry state is unavailable".into()))?;
        state.retain(|_, (entry_source, _)| entry_source != source);
        Ok(())
    }

    fn replace_sources(
        &self,
        managed_sources: &[p::CapabilitySourceRef],
        replacements: Vec<(p::CapabilitySourceRef, Vec<Hook>)>,
    ) -> p::Result<()> {
        let staged = Self::validate_replacements(managed_sources, &replacements)?;
        let managed = managed_sources.iter().cloned().collect::<BTreeSet<_>>();
        let mut state = self
            .state
            .lock()
            .map_err(|_| p::Error("hook registry state is unavailable".into()))?;
        state.retain(|_, (source, _)| !managed.contains(source));
        state.extend(staged);
        Ok(())
    }

    fn validate_replacements(
        managed_sources: &[p::CapabilitySourceRef],
        replacements: &[(p::CapabilitySourceRef, Vec<Hook>)],
    ) -> p::Result<BTreeMap<p::HookRef, (p::CapabilitySourceRef, Hook)>> {
        let managed = managed_sources.iter().cloned().collect::<BTreeSet<_>>();
        let mut staged = BTreeMap::new();
        for (source, hooks) in replacements {
            if !managed.contains(source) {
                return Err(p::Error("hook source is outside the managed set".into()));
            }
            for hook in hooks {
                if hook.schema_version.0 == 0 || hook.id.0.trim().is_empty() {
                    return Err(p::Error("hook contribution is incomplete".into()));
                }
                if staged
                    .insert(hook.id.clone(), (source.clone(), hook.clone()))
                    .is_some()
                {
                    return Err(p::Error("active hook id is duplicated".into()));
                }
            }
        }
        Ok(staged)
    }

    pub fn apply(&self, id: p::HookRef, mut intent: p::ActionIntent) -> p::Result<HookProposal> {
        let hook = self
            .state
            .lock()
            .map_err(|_| p::Error("hook registry state is unavailable".into()))?
            .get(&id)
            .map(|(_, hook)| hook.clone())
            .ok_or_else(|| p::Error("hook is not active".into()))?;
        match hook.transform {
            HookTransform::Identity => {}
            HookTransform::ReplaceCapability(capability) => {
                intent.capability_ref = capability;
            }
            HookTransform::ReplaceShellProgram(program) => {
                let p::ActionParameters::Shell {
                    program: current, ..
                } = &mut intent.parameters
                else {
                    return Err(p::Error("hook expected shell parameters".into()));
                };
                *current = program;
            }
        }
        Ok(HookProposal {
            schema_version: p::SchemaVersion(1),
            intent,
            requires_policy_recheck: p::RequiredTrue,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPlugin {
    pub schema_version: p::SchemaVersion,
    pub manifest: PluginManifest,
}

impl LocalPlugin {
    pub fn id(&self) -> p::PluginRef {
        self.manifest.id.clone()
    }

    fn source_ref(&self) -> p::CapabilitySourceRef {
        p::CapabilitySourceRef(format!("plugin:{}", self.manifest.id.0))
    }
}

impl p::ExternalProvider for LocalPlugin {
    fn kind(&self) -> p::ProviderKind {
        p::ProviderKind::Plugin
    }

    fn id(&self) -> p::ProviderId {
        p::ProviderId(self.manifest.id.0.clone())
    }

    fn declared_capabilities(&self) -> p::CapabilitySet {
        let mut capabilities = BTreeSet::new();
        let mut permissions = BTreeSet::new();
        for contribution in &self.manifest.contributions {
            match contribution {
                PluginContribution::Tool(tool) => {
                    capabilities.insert(tool.id.clone());
                    permissions.extend(tool.permissions.iter().cloned());
                }
                PluginContribution::Skill(skill) => {
                    capabilities.insert(p::CapabilityRef(skill.metadata.id.0.clone()));
                }
                PluginContribution::McpServer(server) => {
                    capabilities.extend(server.declared_capabilities.capabilities.iter().cloned());
                    permissions.extend(server.declared_capabilities.permissions.iter().cloned());
                }
                PluginContribution::Hook(hook) => {
                    capabilities.insert(p::CapabilityRef(hook.id.0.clone()));
                }
                PluginContribution::Config(_) => {}
            }
        }
        p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: capabilities.into_iter().collect(),
            permissions: permissions.into_iter().collect(),
        }
    }

    fn trust_default(&self) -> p::TrustTier {
        p::TrustTier::Untrusted
    }
}

impl p::Plugin for LocalPlugin {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginActivation {
    pub schema_version: p::SchemaVersion,
    pub skills: Vec<SkillDefinition>,
    pub mcp_servers: Vec<StdioMcpServer>,
    pub config: Vec<PluginConfigContribution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginRuntimeState {
    Discovered,
    Configured,
    Enabled,
    Trusted,
    Active,
    Disabled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatus {
    pub schema_version: p::SchemaVersion,
    pub plugin: p::PluginRef,
    pub manifest: p::PluginManifestRef,
    pub state: PluginRuntimeState,
    pub generation: u64,
    pub last_failure: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginReloadOutcome {
    Activated,
    RetainedPrevious,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginReloadReport {
    pub schema_version: p::SchemaVersion,
    pub plugin: p::PluginRef,
    pub outcome: PluginReloadOutcome,
    pub generation: u64,
    pub failure: Option<String>,
}

#[derive(Debug, Clone)]
struct PluginContributionSnapshot {
    plugin: LocalPlugin,
    source: p::CapabilitySourceRef,
    trust: p::TrustTier,
    entries: Vec<CapabilityDescriptor>,
    hooks: Vec<Hook>,
    activation: PluginActivation,
    managed_binding: Option<p::ManagedPluginBinding>,
    managed_snapshot: Option<p::ManagedPluginSnapshot>,
}

pub trait ManagedPluginSignatureVerifier: Send + Sync {
    fn verify(&self, plugin: &LocalPlugin, binding: &p::ManagedPluginBinding) -> p::Result<()>;
}

pub struct LocalPluginRegistry {
    providers: p::InMemoryProviderRegistry<LocalPlugin>,
    capabilities: Arc<InMemoryCapabilityRegistry>,
    hooks: HookRegistry,
    state: Mutex<PluginState>,
}

#[derive(Default)]
struct PluginState {
    events: Vec<p::EventPayload>,
    plugins: BTreeMap<p::PluginRef, LocalPlugin>,
    statuses: BTreeMap<p::PluginRef, PluginStatus>,
    active: BTreeMap<p::PluginRef, PluginContributionSnapshot>,
    generation: u64,
    managed_policy: Option<p::ManagedPluginPolicy>,
}

impl LocalPluginRegistry {
    pub fn with_plugins(
        plugins: Vec<LocalPlugin>,
        capabilities: Arc<InMemoryCapabilityRegistry>,
    ) -> p::Result<Self> {
        for plugin in &plugins {
            validate_plugin(plugin)?;
        }
        let indexed = plugins
            .iter()
            .map(|plugin| (plugin.id(), plugin.clone()))
            .collect::<BTreeMap<_, _>>();
        let statuses = plugins
            .iter()
            .map(|plugin| {
                (
                    plugin.id(),
                    PluginStatus {
                        schema_version: p::SchemaVersion(1),
                        plugin: plugin.id(),
                        manifest: plugin.manifest.manifest_ref.clone(),
                        state: PluginRuntimeState::Discovered,
                        generation: 0,
                        last_failure: None,
                    },
                )
            })
            .collect();
        Ok(Self {
            providers: p::InMemoryProviderRegistry::with_discovered(plugins)?,
            capabilities,
            hooks: HookRegistry::default(),
            state: Mutex::new(PluginState {
                events: Vec::new(),
                plugins: indexed,
                statuses,
                active: BTreeMap::new(),
                generation: 0,
                managed_policy: None,
            }),
        })
    }

    pub fn configure(&self, id: p::PluginRef) -> p::Result<()> {
        let plugin = self.plugin(&id)?;
        p::ProviderRegistry::register(
            &self.providers,
            plugin.clone(),
            p::ProviderConfig {
                schema_version: p::SchemaVersion(1),
                credential_ref: None,
                endpoint: None,
                manifest: Some(serde_json::json!({
                    "manifest_ref": plugin.manifest.manifest_ref.0,
                })),
            },
        )?;
        let mut state = self.lock_state()?;
        state
            .events
            .push(p::EventPayload::PluginContributionRegistered(
                p::PluginContributionRegisteredPayload {
                    manifest: plugin.manifest.manifest_ref,
                    contributions: plugin
                        .manifest
                        .contributions
                        .iter()
                        .map(PluginContribution::reference)
                        .collect(),
                    enabled: false,
                    trust: p::TrustTier::Untrusted,
                    managed_snapshot: None,
                },
            ));
        update_status(&mut state, &id, PluginRuntimeState::Configured, None)?;
        Ok(())
    }

    pub fn enable(&self, id: p::PluginRef) -> p::Result<()> {
        p::ProviderRegistry::set_enabled(&self.providers, p::ProviderId(id.0.clone()), true)?;
        self.push_toggle(id.clone(), true, p::TrustTier::Untrusted)?;
        let mut state = self.lock_state()?;
        update_status(&mut state, &id, PluginRuntimeState::Enabled, None)
    }

    pub fn bind_trust(&self, id: p::PluginRef, trust: p::TrustTier, by: p::Actor) -> p::Result<()> {
        p::ProviderRegistry::bind_trust(&self.providers, p::ProviderId(id.0.clone()), trust, by)?;
        self.push_toggle(id.clone(), true, trust)?;
        let mut state = self.lock_state()?;
        update_status(&mut state, &id, PluginRuntimeState::Trusted, None)
    }

    pub fn grant(&self, id: p::PluginRef, envelope: p::AutonomyEnvelope) -> p::Result<()> {
        p::ProviderRegistry::grant(&self.providers, p::ProviderId(id.0), envelope)
    }

    pub fn disable(&self, id: p::PluginRef) -> p::Result<()> {
        let provider_id = p::ProviderId(id.0.clone());
        let trust = match self.providers.record(&provider_id)?.state {
            p::ProviderLifecycle::Trusted(trust) => trust,
            _ => p::TrustTier::Untrusted,
        };
        p::ProviderRegistry::set_enabled(&self.providers, provider_id, false)?;
        let source = p::CapabilitySourceRef(format!("plugin:{}", id.0));
        self.capabilities.remove_source(&source)?;
        self.hooks.remove_source(&source)?;
        self.push_toggle(id.clone(), false, trust)?;
        let mut state = self.lock_state()?;
        state.active.remove(&id);
        update_status(&mut state, &id, PluginRuntimeState::Disabled, None)
    }

    pub fn activate(&self, context: &p::ResolveContext) -> p::Result<PluginActivation> {
        if self.lock_state()?.managed_policy.is_some() {
            return Err(p::Error(
                "managed plugin registry requires managed activation".into(),
            ));
        }
        let snapshots = self.build_snapshots(context, None)?;
        self.commit_snapshots(snapshots)
    }

    pub fn activate_managed(
        &self,
        policy: &p::ManagedPluginPolicy,
        bindings: &BTreeMap<p::PluginRef, p::ManagedPluginBinding>,
        verifier: &dyn ManagedPluginSignatureVerifier,
        context: &p::ResolveContext,
    ) -> p::Result<PluginActivation> {
        self.validate_managed_policy_transition(policy)?;
        let snapshots = self.build_managed_snapshots(policy, bindings, verifier, context, None)?;
        self.commit_snapshots_with_policy(snapshots, Some(policy))
    }

    pub fn reload_managed(
        &self,
        plugin: LocalPlugin,
        policy: &p::ManagedPluginPolicy,
        bindings: &BTreeMap<p::PluginRef, p::ManagedPluginBinding>,
        verifier: &dyn ManagedPluginSignatureVerifier,
        context: &p::ResolveContext,
    ) -> PluginReloadReport {
        let id = plugin.id();
        let generation = self
            .status(&id)
            .map(|status| status.generation)
            .unwrap_or(0);
        let failure = self
            .validate_managed_policy_transition(policy)
            .and_then(|()| {
                self.build_managed_snapshots(
                    policy,
                    bindings,
                    verifier,
                    context,
                    Some(plugin.clone()),
                )
            })
            .and_then(|snapshots| {
                self.commit_snapshots_with_policy(snapshots, Some(policy))
                    .map(|_| ())
            })
            .err()
            .map(|error| error.to_string());
        if failure.is_none() {
            return PluginReloadReport {
                schema_version: p::SchemaVersion(1),
                plugin: id.clone(),
                outcome: PluginReloadOutcome::Activated,
                generation: self
                    .status(&id)
                    .map(|status| status.generation)
                    .unwrap_or(generation),
                failure: None,
            };
        }
        if let Ok(mut state) = self.lock_state() {
            if let Some(status) = state.statuses.get_mut(&id) {
                status.last_failure = failure.clone();
            }
        }
        PluginReloadReport {
            schema_version: p::SchemaVersion(1),
            plugin: id,
            outcome: PluginReloadOutcome::RetainedPrevious,
            generation,
            failure,
        }
    }

    pub fn reload(&self, plugin: LocalPlugin, context: &p::ResolveContext) -> PluginReloadReport {
        let id = plugin.id();
        let current = self.status(&id);
        let generation = current
            .as_ref()
            .map(|status| status.generation)
            .unwrap_or(0);
        let managed_active = self
            .lock_state()
            .ok()
            .and_then(|state| state.active.get(&id).cloned())
            .and_then(|snapshot| snapshot.managed_snapshot)
            .is_some();
        let failure = if managed_active {
            Some("managed plugin reload requires managed policy verification".to_owned())
        } else if current.is_none() {
            Some("plugin is not discovered".to_owned())
        } else if current.as_ref().map(|status| status.state) != Some(PluginRuntimeState::Active) {
            Some("plugin must be active before reload".to_owned())
        } else if validate_plugin(&plugin).is_err() {
            Some("replacement plugin manifest failed validation".to_owned())
        } else {
            match self
                .build_snapshots(context, Some(plugin.clone()))
                .and_then(|snapshots| self.commit_snapshots(snapshots))
            {
                Ok(_) => {
                    if let Ok(record) = self.providers.record(&p::ProviderId(id.0.clone())) {
                        let trust = match record.state {
                            p::ProviderLifecycle::Trusted(trust) => trust,
                            _ => p::TrustTier::Untrusted,
                        };
                        if let Ok(mut state) = self.lock_state() {
                            state
                                .events
                                .push(p::EventPayload::PluginContributionRegistered(
                                    p::PluginContributionRegisteredPayload {
                                        manifest: plugin.manifest.manifest_ref.clone(),
                                        contributions: plugin
                                            .manifest
                                            .contributions
                                            .iter()
                                            .map(PluginContribution::reference)
                                            .collect(),
                                        enabled: true,
                                        trust,
                                        managed_snapshot: None,
                                    },
                                ));
                        }
                    }
                    let generation = self
                        .status(&id)
                        .map(|status| status.generation)
                        .unwrap_or(generation);
                    return PluginReloadReport {
                        schema_version: p::SchemaVersion(1),
                        plugin: id,
                        outcome: PluginReloadOutcome::Activated,
                        generation,
                        failure: None,
                    };
                }
                Err(error) => Some(error.to_string()),
            }
        };
        if let Ok(mut state) = self.lock_state() {
            if let Some(status) = state.statuses.get_mut(&id) {
                status.last_failure = failure.clone();
            }
        }
        PluginReloadReport {
            schema_version: p::SchemaVersion(1),
            plugin: id,
            outcome: PluginReloadOutcome::RetainedPrevious,
            generation,
            failure,
        }
    }

    pub fn mark_failed(&self, id: p::PluginRef, failure: String) -> p::Result<()> {
        if failure.trim().is_empty() {
            return Err(p::Error("plugin failure detail is empty".into()));
        }
        let provider_id = p::ProviderId(id.0.clone());
        let trust = match self.providers.record(&provider_id)?.state {
            p::ProviderLifecycle::Trusted(trust) => trust,
            _ => p::TrustTier::Untrusted,
        };
        p::ProviderRegistry::set_enabled(&self.providers, provider_id, false)?;
        let source = p::CapabilitySourceRef(format!("plugin:{}", id.0));
        self.capabilities.remove_source(&source)?;
        self.hooks.remove_source(&source)?;
        let mut state = self.lock_state()?;
        state.active.remove(&id);
        state
            .events
            .push(p::EventPayload::PluginToggled(p::PluginToggledPayload {
                plugin: id.clone(),
                enabled: false,
                trust,
                managed_policy: None,
                managed_snapshot: None,
            }));
        update_status(&mut state, &id, PluginRuntimeState::Failed, Some(failure))
    }

    pub fn status(&self, id: &p::PluginRef) -> Option<PluginStatus> {
        self.lock_state()
            .ok()
            .and_then(|state| state.statuses.get(id).cloned())
    }

    pub fn statuses(&self) -> Vec<PluginStatus> {
        self.lock_state()
            .map(|state| state.statuses.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn apply_hook(&self, id: p::HookRef, intent: p::ActionIntent) -> p::Result<HookProposal> {
        self.hooks.apply(id, intent)
    }

    pub fn take_events(&self) -> Vec<p::EventPayload> {
        self.lock_state()
            .map(|mut state| std::mem::take(&mut state.events))
            .unwrap_or_default()
    }

    pub fn take_provider_transitions(&self) -> Vec<p::ProviderTransition> {
        self.providers.take_transitions()
    }

    fn plugin(&self, id: &p::PluginRef) -> p::Result<LocalPlugin> {
        self.lock_state()?
            .plugins
            .get(id)
            .cloned()
            .ok_or_else(|| p::Error("plugin is not discovered".into()))
    }

    fn build_snapshots(
        &self,
        context: &p::ResolveContext,
        replacement: Option<LocalPlugin>,
    ) -> p::Result<Vec<PluginContributionSnapshot>> {
        let plugins = self.lock_state()?.plugins.clone();
        p::ProviderRegistry::resolve(&self.providers, context)
            .providers
            .into_iter()
            .map(|provider| {
                let id = p::PluginRef(provider.provider.id().0);
                let plugin = replacement
                    .as_ref()
                    .filter(|plugin| plugin.id() == id)
                    .cloned()
                    .or_else(|| plugins.get(&id).cloned())
                    .ok_or_else(|| p::Error("active plugin snapshot is unavailable".into()))?;
                stage_plugin(plugin, provider.trust, &provider.grant, context)
            })
            .collect()
    }

    fn build_managed_snapshots(
        &self,
        policy: &p::ManagedPluginPolicy,
        bindings: &BTreeMap<p::PluginRef, p::ManagedPluginBinding>,
        verifier: &dyn ManagedPluginSignatureVerifier,
        context: &p::ResolveContext,
        replacement: Option<LocalPlugin>,
    ) -> p::Result<Vec<PluginContributionSnapshot>> {
        policy.validate()?;
        let plugins = self.lock_state()?.plugins.clone();
        let mut snapshots = Vec::new();
        for provider in p::ProviderRegistry::resolve(&self.providers, context).providers {
            let id = p::PluginRef(provider.provider.id().0);
            let plugin = replacement
                .as_ref()
                .filter(|plugin| plugin.id() == id)
                .cloned()
                .or_else(|| plugins.get(&id).cloned())
                .ok_or_else(|| p::Error("active plugin snapshot is unavailable".into()))?;
            if policy.denied_plugins.contains(&id) || !policy.allowed_plugins.contains(&id) {
                continue;
            }
            let binding = bindings
                .get(&id)
                .cloned()
                .ok_or_else(|| p::Error("managed plugin binding is missing".into()))?;
            binding.validate()?;
            if !policy.allows(&id, &binding) {
                continue;
            }
            let actual = plugin_manifest_digest(&plugin);
            if binding.manifest_digest != actual {
                return Err(p::Error(
                    "managed plugin manifest digest does not match its binding".into(),
                ));
            }
            verifier.verify(&plugin, &binding)?;
            let mut snapshot = stage_plugin(plugin, provider.trust, &provider.grant, context)?;
            snapshot.managed_binding = Some(binding);
            snapshots.push(snapshot);
        }
        Ok(snapshots)
    }

    fn validate_managed_policy_transition(&self, policy: &p::ManagedPluginPolicy) -> p::Result<()> {
        policy.validate()?;
        if let Some(current) = &self.lock_state()?.managed_policy {
            if current.reference != policy.reference || policy.version < current.version {
                return Err(p::Error(
                    "managed plugin policy identity or version moved backwards".into(),
                ));
            }
        }
        Ok(())
    }

    fn commit_snapshots(
        &self,
        snapshots: Vec<PluginContributionSnapshot>,
    ) -> p::Result<PluginActivation> {
        self.commit_snapshots_with_policy(snapshots, None)
    }

    fn commit_snapshots_with_policy(
        &self,
        mut snapshots: Vec<PluginContributionSnapshot>,
        managed_policy: Option<&p::ManagedPluginPolicy>,
    ) -> p::Result<PluginActivation> {
        let managed = self
            .lock_state()?
            .plugins
            .values()
            .map(LocalPlugin::source_ref)
            .collect::<Vec<_>>();
        let hook_replacements = snapshots
            .iter()
            .map(|snapshot| (snapshot.source.clone(), snapshot.hooks.clone()))
            .collect::<Vec<_>>();
        HookRegistry::validate_replacements(&managed, &hook_replacements)?;
        self.capabilities.replace_sources(
            &managed,
            snapshots
                .iter()
                .map(|snapshot| CapabilitySource {
                    schema_version: p::SchemaVersion(1),
                    source_ref: snapshot.source.clone(),
                    trust: snapshot.trust,
                    entries: snapshot.entries.clone(),
                })
                .collect(),
        )?;
        self.hooks.replace_sources(&managed, hook_replacements)?;
        let mut activation = PluginActivation {
            schema_version: p::SchemaVersion(1),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            config: Vec::new(),
        };
        for snapshot in &snapshots {
            activation
                .skills
                .extend(snapshot.activation.skills.iter().cloned());
            activation
                .mcp_servers
                .extend(snapshot.activation.mcp_servers.iter().cloned());
            activation
                .config
                .extend(snapshot.activation.config.iter().cloned());
        }
        let mut state = self.lock_state()?;
        state.generation = state.generation.saturating_add(1);
        let generation = state.generation;
        if let Some(policy) = managed_policy {
            for snapshot in &mut snapshots {
                let binding = snapshot
                    .managed_binding
                    .as_ref()
                    .ok_or_else(|| p::Error("managed plugin snapshot has no binding".into()))?;
                let managed = p::ManagedPluginSnapshot {
                    schema_version: p::SchemaVersion(1),
                    reference: p::ManagedPluginSnapshotRef(format!(
                        "managed-snapshot:{}:{}:{generation}",
                        policy.reference.0,
                        snapshot.plugin.id().0,
                    )),
                    policy: policy.reference.clone(),
                    policy_version: policy.version,
                    plugin: snapshot.plugin.id(),
                    source: binding.source.clone(),
                    manifest: snapshot.plugin.manifest.manifest_ref.clone(),
                    manifest_digest: binding.manifest_digest.clone(),
                    generation,
                };
                managed.validate()?;
                snapshot.managed_snapshot = Some(managed);
            }
        }
        let active_ids = snapshots
            .iter()
            .map(|snapshot| snapshot.plugin.id())
            .collect::<BTreeSet<_>>();
        let removed = state
            .active
            .iter()
            .filter(|(id, _)| !active_ids.contains(*id))
            .map(|(id, snapshot)| (id.clone(), snapshot.managed_snapshot.clone()))
            .collect::<Vec<_>>();
        for status in state.statuses.values_mut() {
            if status.state == PluginRuntimeState::Active && !active_ids.contains(&status.plugin) {
                status.state = PluginRuntimeState::Trusted;
            }
        }
        state.active.clear();
        for snapshot in snapshots {
            let id = snapshot.plugin.id();
            state.plugins.insert(id.clone(), snapshot.plugin.clone());
            let status = state
                .statuses
                .get_mut(&id)
                .ok_or_else(|| p::Error("plugin status is unavailable".into()))?;
            status.manifest = snapshot.plugin.manifest.manifest_ref.clone();
            status.state = PluginRuntimeState::Active;
            status.generation = generation;
            status.last_failure = None;
            if let Some(managed) = &snapshot.managed_snapshot {
                state
                    .events
                    .push(p::EventPayload::PluginContributionRegistered(
                        p::PluginContributionRegisteredPayload {
                            manifest: snapshot.plugin.manifest.manifest_ref.clone(),
                            contributions: snapshot
                                .plugin
                                .manifest
                                .contributions
                                .iter()
                                .map(PluginContribution::reference)
                                .collect(),
                            enabled: true,
                            trust: snapshot.trust,
                            managed_snapshot: Some(managed.reference.clone()),
                        },
                    ));
            }
            state.active.insert(id, snapshot);
        }
        if let Some(policy) = managed_policy {
            state.managed_policy = Some(policy.clone());
            for (plugin, previous_snapshot) in removed {
                state
                    .events
                    .push(p::EventPayload::PluginToggled(p::PluginToggledPayload {
                        plugin,
                        enabled: false,
                        trust: p::TrustTier::Untrusted,
                        managed_policy: Some(policy.reference.clone()),
                        managed_snapshot: previous_snapshot.map(|snapshot| snapshot.reference),
                    }));
            }
        }
        Ok(activation)
    }

    fn push_toggle(&self, id: p::PluginRef, enabled: bool, trust: p::TrustTier) -> p::Result<()> {
        self.lock_state()?
            .events
            .push(p::EventPayload::PluginToggled(p::PluginToggledPayload {
                plugin: id,
                enabled,
                trust,
                managed_policy: None,
                managed_snapshot: None,
            }));
        Ok(())
    }

    fn lock_state(&self) -> p::Result<MutexGuard<'_, PluginState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("plugin registry state is unavailable".into()))
    }
}

fn update_status(
    state: &mut PluginState,
    id: &p::PluginRef,
    next: PluginRuntimeState,
    failure: Option<String>,
) -> p::Result<()> {
    let status = state
        .statuses
        .get_mut(id)
        .ok_or_else(|| p::Error("plugin status is unavailable".into()))?;
    status.state = next;
    status.last_failure = failure;
    Ok(())
}

fn stage_plugin(
    plugin: LocalPlugin,
    trust: p::TrustTier,
    grant: &p::AutonomyEnvelope,
    context: &p::ResolveContext,
) -> p::Result<PluginContributionSnapshot> {
    validate_plugin(&plugin)?;
    if trust == p::TrustTier::Untrusted {
        return Err(p::Error(
            "plugin snapshot has no runtime trust binding".into(),
        ));
    }
    let source = plugin.source_ref();
    let mut entries = Vec::new();
    let mut hooks = Vec::new();
    let mut activation = PluginActivation {
        schema_version: p::SchemaVersion(1),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        config: Vec::new(),
    };
    for contribution in &plugin.manifest.contributions {
        match contribution {
            PluginContribution::Tool(tool) => entries.push(tool.clone()),
            PluginContribution::Skill(skill) => {
                if skill.schema_version.0 == 0
                    || skill.metadata.schema_version.0 == 0
                    || skill.metadata.id.0.trim().is_empty()
                    || skill.metadata.summary.trim().is_empty()
                    || skill.metadata.scope.0.trim().is_empty()
                    || skill.body.0.trim().is_empty()
                {
                    return Err(p::Error("plugin skill contribution is incomplete".into()));
                }
                let mut skill = skill.clone();
                skill.metadata.trust = trust;
                entries.push(CapabilityDescriptor {
                    schema_version: p::SchemaVersion(1),
                    id: p::CapabilityRef(skill.metadata.id.0.clone()),
                    capability: Capability::Skill(skill.metadata.id.clone()),
                    scope: skill.metadata.scope.clone(),
                    permissions: Vec::new(),
                    risk: Some(p::Risk::Low),
                    enabled: true,
                });
                activation.skills.push(skill);
            }
            PluginContribution::McpServer(server) => {
                if server.schema_version.0 == 0
                    || server.provider_id.0.trim().is_empty()
                    || server.command.trim().is_empty()
                    || server.timeout.0 == 0
                    || server.allowlist.schema_version.0 == 0
                    || !server
                        .declared_capabilities
                        .capabilities
                        .iter()
                        .all(|capability| {
                            grant.capability.capabilities.contains(capability)
                                && context
                                    .envelope
                                    .capability
                                    .capabilities
                                    .contains(capability)
                        })
                    || !server
                        .declared_capabilities
                        .permissions
                        .iter()
                        .all(|permission| {
                            grant.capability.permissions.contains(permission)
                                && context.envelope.capability.permissions.contains(permission)
                        })
                {
                    return Err(p::Error(
                        "plugin MCP contribution is outside its grant".into(),
                    ));
                }
                activation.mcp_servers.push(server.clone());
            }
            PluginContribution::Hook(hook) => {
                if hook.schema_version.0 == 0 || hook.id.0.trim().is_empty() {
                    return Err(p::Error("plugin hook contribution is incomplete".into()));
                }
                entries.push(CapabilityDescriptor {
                    schema_version: p::SchemaVersion(1),
                    id: p::CapabilityRef(hook.id.0.clone()),
                    capability: Capability::Hook(hook.id.clone()),
                    scope: grant.scope.clone(),
                    permissions: Vec::new(),
                    risk: Some(p::Risk::Low),
                    enabled: true,
                });
                hooks.push(hook.clone());
            }
            PluginContribution::Config(config) => {
                if config.schema_version.0 == 0 || config.key.trim().is_empty() {
                    return Err(p::Error("plugin config contribution is incomplete".into()));
                }
                activation.config.push(config.clone());
            }
        }
    }
    for entry in &entries {
        let Some(risk) = entry.risk else {
            return Err(p::Error("plugin capability has unknown risk".into()));
        };
        if entry.schema_version.0 == 0
            || entry.id.0.trim().is_empty()
            || entry.scope.0.trim().is_empty()
            || !grant.capability.capabilities.contains(&entry.id)
            || !context.envelope.capability.capabilities.contains(&entry.id)
            || !entry
                .permissions
                .iter()
                .all(|permission| grant.capability.permissions.contains(permission))
            || !entry
                .permissions
                .iter()
                .all(|permission| context.envelope.capability.permissions.contains(permission))
            || !scope_contains(&grant.scope, &entry.scope)
            || !scope_contains(&context.envelope.scope, &entry.scope)
            || risk > grant.risk_limit
            || risk > context.envelope.risk_limit
        {
            return Err(p::Error(
                "plugin capability contribution is outside its validated snapshot grant".into(),
            ));
        }
    }
    Ok(PluginContributionSnapshot {
        plugin,
        source,
        trust,
        entries,
        hooks,
        activation,
        managed_binding: None,
        managed_snapshot: None,
    })
}

pub fn plugin_manifest_digest(plugin: &LocalPlugin) -> p::SchemaDigest {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, plugin.manifest.id.0.as_bytes());
    hash_field(&mut hasher, plugin.manifest.manifest_ref.0.as_bytes());
    hash_field(
        &mut hasher,
        format!("{:?}", plugin.manifest.trust).as_bytes(),
    );
    for contribution in &plugin.manifest.contributions {
        hash_field(&mut hasher, contribution.reference().0.as_bytes());
        hash_field(&mut hasher, format!("{contribution:?}").as_bytes());
    }
    p::SchemaDigest(format!("sha256:{:x}", hasher.finalize()))
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
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

fn validate_plugin(plugin: &LocalPlugin) -> p::Result<()> {
    if plugin.schema_version.0 == 0
        || plugin.manifest.schema_version.0 == 0
        || plugin.manifest.id.0.trim().is_empty()
        || plugin.manifest.manifest_ref.0.trim().is_empty()
        || plugin.manifest.contributions.is_empty()
    {
        return Err(p::Error("plugin manifest is incomplete".into()));
    }
    if plugin.manifest.trust != p::TrustTier::Untrusted {
        return Err(p::Error(
            "plugin manifests cannot self-assign runtime trust".into(),
        ));
    }
    let mut refs = BTreeSet::new();
    for contribution in &plugin.manifest.contributions {
        if !refs.insert(contribution.reference()) {
            return Err(p::Error("plugin contribution is duplicated".into()));
        }
    }
    Ok(())
}
