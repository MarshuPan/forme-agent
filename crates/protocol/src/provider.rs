use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderKind {
    Model,
    Channel,
    Mcp,
    Plugin,
    AppApi,
}

pub trait ExternalProvider {
    fn kind(&self) -> ProviderKind;
    fn id(&self) -> ProviderId;
    fn declared_capabilities(&self) -> CapabilitySet;
    fn trust_default(&self) -> TrustTier;
}

pub trait ModelProvider: ExternalProvider {}

pub trait ChannelAdapter: ExternalProvider {}

pub trait McpServer: ExternalProvider {}

pub trait Plugin: ExternalProvider {}

pub trait AppApiConnector: ExternalProvider {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderLifecycle {
    Discovered,
    Configured,
    Enabled,
    Trusted(TrustTier),
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub schema_version: SchemaVersion,
    pub credential_ref: Option<CredentialRef>,
    pub endpoint: Option<String>,
    pub manifest: Option<serde_json::Value>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            schema_version: SchemaVersion(1),
            credential_ref: None,
            endpoint: None,
            manifest: None,
        }
    }
}

impl ProviderConfig {
    fn is_configured(&self) -> bool {
        self.credential_ref.is_some()
            || self
                .endpoint
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
            || self.manifest.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRecord<P: ExternalProvider> {
    pub schema_version: SchemaVersion,
    pub provider: P,
    pub state: ProviderLifecycle,
    pub config: ProviderConfig,
    pub grant: Option<AutonomyEnvelope>,
    pub trust_bound_by: Option<Actor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveContext {
    pub schema_version: SchemaVersion,
    pub session: SessionId,
    pub toolset: ToolsetRef,
    pub envelope: AutonomyEnvelope,
    pub policy_allowed_providers: Vec<ProviderId>,
    pub policy_allowed_capabilities: Vec<CapabilityRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveProvider<P: ExternalProvider> {
    pub schema_version: SchemaVersion,
    pub provider: P,
    pub trust: TrustTier,
    pub grant: AutonomyEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSet<P: ExternalProvider> {
    pub schema_version: SchemaVersion,
    pub providers: Vec<ActiveProvider<P>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTransition {
    pub schema_version: SchemaVersion,
    pub provider_id: ProviderId,
    pub kind: ProviderKind,
    pub from: Option<ProviderLifecycle>,
    pub to: ProviderLifecycle,
    pub by: Option<Actor>,
}

pub trait ProviderRegistry<P: ExternalProvider> {
    fn discover(&self) -> Vec<P>;
    fn register(&self, provider: P, config: ProviderConfig) -> Result<ProviderId>;
    fn set_enabled(&self, id: ProviderId, on: bool) -> Result<()>;
    fn bind_trust(&self, id: ProviderId, tier: TrustTier, by: Actor) -> Result<()>;
    fn grant(&self, id: ProviderId, envelope: AutonomyEnvelope) -> Result<()>;
    fn resolve(&self, context: &ResolveContext) -> ActiveSet<P>;
}

pub struct InMemoryProviderRegistry<P: ExternalProvider + Clone> {
    state: Mutex<RegistryState<P>>,
}

struct RegistryState<P: ExternalProvider> {
    records: BTreeMap<ProviderId, ProviderRecord<P>>,
    transitions: Vec<ProviderTransition>,
}

impl<P> Default for InMemoryProviderRegistry<P>
where
    P: ExternalProvider + Clone,
{
    fn default() -> Self {
        Self {
            state: Mutex::new(RegistryState {
                records: BTreeMap::new(),
                transitions: Vec::new(),
            }),
        }
    }
}

impl<P> InMemoryProviderRegistry<P>
where
    P: ExternalProvider + Clone,
{
    pub fn with_discovered(providers: Vec<P>) -> Result<Self> {
        let registry = Self::default();
        {
            let mut state = registry.lock_state()?;
            for provider in providers {
                validate_external_default(&provider)?;
                let id = provider.id();
                if state.records.contains_key(&id) {
                    return Err(Error("provider discovery returned a duplicate id".into()));
                }
                state.transitions.push(ProviderTransition {
                    schema_version: SchemaVersion(1),
                    provider_id: id.clone(),
                    kind: provider.kind(),
                    from: None,
                    to: ProviderLifecycle::Discovered,
                    by: None,
                });
                state.records.insert(
                    id,
                    ProviderRecord {
                        schema_version: SchemaVersion(1),
                        provider,
                        state: ProviderLifecycle::Discovered,
                        config: ProviderConfig::default(),
                        grant: None,
                        trust_bound_by: None,
                    },
                );
            }
        }
        Ok(registry)
    }

    pub fn record(&self, id: &ProviderId) -> Result<ProviderRecord<P>> {
        self.lock_state()?
            .records
            .get(id)
            .cloned()
            .ok_or_else(|| Error("provider is not registered".into()))
    }

    pub fn take_transitions(&self) -> Vec<ProviderTransition> {
        self.lock_state()
            .map(|mut state| std::mem::take(&mut state.transitions))
            .unwrap_or_default()
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, RegistryState<P>>> {
        self.state
            .lock()
            .map_err(|_| Error("provider registry state is unavailable".into()))
    }

    fn transition(
        state: &mut RegistryState<P>,
        id: &ProviderId,
        from: ProviderLifecycle,
        to: ProviderLifecycle,
        by: Option<Actor>,
    ) -> Result<()> {
        let kind = state
            .records
            .get(id)
            .map(|record| record.provider.kind())
            .ok_or_else(|| Error("provider is not registered".into()))?;
        state.transitions.push(ProviderTransition {
            schema_version: SchemaVersion(1),
            provider_id: id.clone(),
            kind,
            from: Some(from),
            to,
            by,
        });
        Ok(())
    }
}

impl<P> ProviderRegistry<P> for InMemoryProviderRegistry<P>
where
    P: ExternalProvider + Clone,
{
    fn discover(&self) -> Vec<P> {
        self.lock_state()
            .map(|state| {
                state
                    .records
                    .values()
                    .map(|record| record.provider.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn register(&self, provider: P, config: ProviderConfig) -> Result<ProviderId> {
        validate_external_default(&provider)?;
        if !config.is_configured() {
            return Err(Error("provider configuration has no usable source".into()));
        }
        let id = provider.id();
        let mut state = self.lock_state()?;
        let record = state
            .records
            .get_mut(&id)
            .ok_or_else(|| Error("provider must be discovered before registration".into()))?;
        if record.state != ProviderLifecycle::Discovered {
            return Err(Error("provider can only be configured once".into()));
        }
        if record.provider.kind() != provider.kind() {
            return Err(Error("provider kind changed after discovery".into()));
        }
        let from = record.state.clone();
        record.provider = provider;
        record.config = config;
        record.state = ProviderLifecycle::Configured;
        Self::transition(&mut state, &id, from, ProviderLifecycle::Configured, None)?;
        Ok(id)
    }

    fn set_enabled(&self, id: ProviderId, on: bool) -> Result<()> {
        let mut state = self.lock_state()?;
        let record = state
            .records
            .get_mut(&id)
            .ok_or_else(|| Error("provider is not registered".into()))?;
        let from = record.state.clone();
        let to = if on {
            match record.state {
                ProviderLifecycle::Configured | ProviderLifecycle::Disabled => {
                    ProviderLifecycle::Enabled
                }
                _ => return Err(Error("provider is not ready to be enabled".into())),
            }
        } else {
            match record.state {
                ProviderLifecycle::Configured
                | ProviderLifecycle::Enabled
                | ProviderLifecycle::Trusted(_) => ProviderLifecycle::Disabled,
                _ => return Err(Error("provider is not active or configured".into())),
            }
        };
        record.state = to.clone();
        if !on {
            record.grant = None;
            record.trust_bound_by = None;
        }
        Self::transition(&mut state, &id, from, to, None)
    }

    fn bind_trust(&self, id: ProviderId, tier: TrustTier, by: Actor) -> Result<()> {
        if tier == TrustTier::Untrusted {
            return Err(Error(
                "trust binding must raise the provider above untrusted".into(),
            ));
        }
        let mut state = self.lock_state()?;
        let record = state
            .records
            .get_mut(&id)
            .ok_or_else(|| Error("provider is not registered".into()))?;
        if record.state != ProviderLifecycle::Enabled {
            return Err(Error(
                "provider must be enabled before trust is bound".into(),
            ));
        }
        let from = record.state.clone();
        let to = ProviderLifecycle::Trusted(tier);
        record.state = to.clone();
        record.trust_bound_by = Some(by.clone());
        Self::transition(&mut state, &id, from, to, Some(by))
    }

    fn grant(&self, id: ProviderId, envelope: AutonomyEnvelope) -> Result<()> {
        validate_envelope(&envelope)?;
        let mut state = self.lock_state()?;
        let record = state
            .records
            .get_mut(&id)
            .ok_or_else(|| Error("provider is not registered".into()))?;
        if !matches!(record.state, ProviderLifecycle::Trusted(tier) if tier != TrustTier::Untrusted)
        {
            return Err(Error("provider must have an explicit trust binding".into()));
        }
        if !capability_set_within(
            &record.provider.declared_capabilities(),
            &envelope.capability,
        ) {
            return Err(Error("provider grant omits a declared capability".into()));
        }
        record.grant = Some(envelope);
        Ok(())
    }

    fn resolve(&self, context: &ResolveContext) -> ActiveSet<P> {
        let providers = self
            .lock_state()
            .map(|state| {
                state
                    .records
                    .values()
                    .filter_map(|record| {
                        let ProviderLifecycle::Trusted(trust) = record.state else {
                            return None;
                        };
                        if trust == TrustTier::Untrusted
                            || !context
                                .policy_allowed_providers
                                .contains(&record.provider.id())
                        {
                            return None;
                        }
                        let grant = record.grant.as_ref()?;
                        let declared = record.provider.declared_capabilities();
                        if !envelope_within(grant, &context.envelope)
                            || !capability_set_within(&declared, &grant.capability)
                            || !capability_set_within(&declared, &context.envelope.capability)
                        {
                            return None;
                        }
                        Some(ActiveProvider {
                            schema_version: SchemaVersion(1),
                            provider: record.provider.clone(),
                            trust,
                            grant: grant.clone(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        ActiveSet {
            schema_version: SchemaVersion(1),
            providers,
        }
    }
}

fn validate_external_default<P: ExternalProvider>(provider: &P) -> Result<()> {
    if provider.id().0.trim().is_empty() {
        return Err(Error("provider id cannot be empty".into()));
    }
    if provider.trust_default() != TrustTier::Untrusted {
        return Err(Error("external providers must default to untrusted".into()));
    }
    Ok(())
}

fn validate_envelope(envelope: &AutonomyEnvelope) -> Result<()> {
    if envelope.scope.0.trim().is_empty()
        || envelope.capability.capabilities.is_empty()
        || envelope.action_type.is_empty()
        || envelope.budget.0.trim().is_empty()
        || envelope.timebox.max_turns == 0
        || envelope.timebox.expires_at < envelope.timebox.starts_at
    {
        return Err(Error("provider envelope is incomplete".into()));
    }
    Ok(())
}

fn envelope_within(grant: &AutonomyEnvelope, ceiling: &AutonomyEnvelope) -> bool {
    scope_contains(&ceiling.scope, &grant.scope)
        && capability_set_within(&grant.capability, &ceiling.capability)
        && grant
            .action_type
            .iter()
            .all(|action| ceiling.action_type.contains(action))
        && grant.risk_limit <= ceiling.risk_limit
        && approval_rank(grant.approval_rule) >= approval_rank(ceiling.approval_rule)
        && grant.budget == ceiling.budget
        && grant.timebox.starts_at >= ceiling.timebox.starts_at
        && grant.timebox.expires_at <= ceiling.timebox.expires_at
        && grant.timebox.max_turns <= ceiling.timebox.max_turns
        && rollback_within(&grant.rollback, &ceiling.rollback)
}

fn capability_set_within(candidate: &CapabilitySet, ceiling: &CapabilitySet) -> bool {
    candidate
        .capabilities
        .iter()
        .all(|capability| ceiling.capabilities.contains(capability))
        && candidate
            .permissions
            .iter()
            .all(|permission| ceiling.permissions.contains(permission))
}

fn scope_contains(ceiling: &Scope, candidate: &Scope) -> bool {
    if ceiling.0 == "*" || ceiling == candidate {
        return true;
    }
    candidate
        .0
        .strip_prefix(&ceiling.0)
        .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('/'))
}

fn approval_rank(rule: ApprovalRule) -> u8 {
    match rule {
        ApprovalRule::Allow => 0,
        ApprovalRule::Ask => 1,
        ApprovalRule::Deny => 2,
    }
}

fn rollback_within(grant: &RollbackReq, ceiling: &RollbackReq) -> bool {
    if ceiling.required && !grant.required {
        return false;
    }
    match &ceiling.boundary {
        Some(boundary) => grant.boundary.as_ref() == Some(boundary),
        None => true,
    }
}
