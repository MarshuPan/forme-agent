use forme_protocol::{
    ActionType, ActiveSet, Actor, ApprovalRule, AutonomyEnvelope, Budget, CapabilityRef,
    CapabilitySet, ExternalProvider, InMemoryProviderRegistry, PermissionRef, ProviderConfig,
    ProviderId, ProviderKind, ProviderLifecycle, ProviderRegistry, ResolveContext, Risk,
    RollbackReq, SchemaVersion, Scope, SessionId, Timebox, ToolsetRef, TrustTier,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureProvider {
    id: ProviderId,
    capabilities: CapabilitySet,
}

impl ExternalProvider for FixtureProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Mcp
    }

    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn declared_capabilities(&self) -> CapabilitySet {
        self.capabilities.clone()
    }

    fn trust_default(&self) -> TrustTier {
        TrustTier::Untrusted
    }
}

#[test]
fn lifecycle_reaches_resolve_only_after_config_enable_trust_and_grant() {
    let provider = provider("mcp-local");
    let registry = InMemoryProviderRegistry::with_discovered(vec![provider.clone()]).unwrap();
    assert_eq!(registry.discover(), vec![provider.clone()]);

    let id = registry.register(provider, config()).unwrap();
    assert_eq!(
        registry.record(&id).unwrap().state,
        ProviderLifecycle::Configured
    );
    registry.set_enabled(id.clone(), true).unwrap();
    assert_eq!(
        registry.record(&id).unwrap().state,
        ProviderLifecycle::Enabled
    );
    registry
        .bind_trust(id.clone(), TrustTier::ApprovedSource, Actor::Owner)
        .unwrap();
    assert_eq!(
        registry.record(&id).unwrap().state,
        ProviderLifecycle::Trusted(TrustTier::ApprovedSource)
    );
    registry
        .grant(id.clone(), envelope("workspace:alpha"))
        .unwrap();

    let active = registry.resolve(&resolve_context(vec![id.clone()], "workspace:alpha"));
    assert_active(&active, &id);
    assert_eq!(active.providers[0].trust, TrustTier::ApprovedSource);

    registry.set_enabled(id.clone(), false).unwrap();
    assert!(registry
        .resolve(&resolve_context(vec![id], "workspace:alpha"))
        .providers
        .is_empty());

    let transitions = registry.take_transitions();
    assert_eq!(
        transitions.first().unwrap().to,
        ProviderLifecycle::Discovered
    );
    assert_eq!(transitions.last().unwrap().to, ProviderLifecycle::Disabled);
}

#[test]
fn invalid_transition_untrusted_and_policy_exclusion_never_expose_provider() {
    let provider = provider("mcp-guarded");
    let id = provider.id();
    let registry = InMemoryProviderRegistry::with_discovered(vec![provider.clone()]).unwrap();

    assert!(registry.set_enabled(id.clone(), true).is_err());
    registry.register(provider, config()).unwrap();
    assert!(registry
        .bind_trust(id.clone(), TrustTier::ApprovedSource, Actor::Owner)
        .is_err());
    assert!(registry
        .grant(id.clone(), envelope("workspace:alpha"))
        .is_err());

    registry.set_enabled(id.clone(), true).unwrap();
    assert!(registry
        .bind_trust(id.clone(), TrustTier::Untrusted, Actor::Owner)
        .is_err());
    assert!(registry
        .resolve(&resolve_context(vec![id.clone()], "workspace:alpha"))
        .providers
        .is_empty());

    registry
        .bind_trust(id.clone(), TrustTier::ApprovedSource, Actor::Owner)
        .unwrap();
    registry
        .grant(id.clone(), envelope("workspace:alpha"))
        .unwrap();
    assert!(registry
        .resolve(&resolve_context(Vec::new(), "workspace:alpha"))
        .providers
        .is_empty());
}

#[test]
fn resolve_rejects_a_provider_grant_broader_than_the_runtime_envelope() {
    let broad_provider = provider("mcp-bounded");
    let id = broad_provider.id();
    let registry = InMemoryProviderRegistry::with_discovered(vec![broad_provider.clone()]).unwrap();
    registry.register(broad_provider, config()).unwrap();
    registry.set_enabled(id.clone(), true).unwrap();
    registry
        .bind_trust(id.clone(), TrustTier::ApprovedSource, Actor::Owner)
        .unwrap();
    registry.grant(id.clone(), envelope("workspace")).unwrap();

    assert!(registry
        .resolve(&resolve_context(vec![id.clone()], "workspace:alpha"))
        .providers
        .is_empty());

    let narrow_provider = provider("mcp-narrow");
    let narrow_id = narrow_provider.id();
    let narrow = InMemoryProviderRegistry::with_discovered(vec![narrow_provider.clone()]).unwrap();
    narrow.register(narrow_provider, config()).unwrap();
    narrow.set_enabled(narrow_id.clone(), true).unwrap();
    narrow
        .bind_trust(narrow_id.clone(), TrustTier::ApprovedSource, Actor::Owner)
        .unwrap();
    narrow
        .grant(narrow_id.clone(), envelope("workspace:alpha"))
        .unwrap();
    assert_active(
        &narrow.resolve(&resolve_context(vec![narrow_id.clone()], "workspace")),
        &narrow_id,
    );
}

fn provider(id: &str) -> FixtureProvider {
    FixtureProvider {
        id: ProviderId(id.into()),
        capabilities: CapabilitySet {
            schema_version: SchemaVersion(1),
            capabilities: vec![CapabilityRef("tool:mcp-read".into())],
            permissions: vec![PermissionRef("read".into())],
        },
    }
}

fn config() -> ProviderConfig {
    ProviderConfig {
        schema_version: SchemaVersion(1),
        credential_ref: None,
        endpoint: None,
        manifest: Some(serde_json::json!({ "transport": "stdio" })),
    }
}

fn envelope(scope: &str) -> AutonomyEnvelope {
    AutonomyEnvelope {
        schema_version: SchemaVersion(1),
        scope: Scope(scope.into()),
        capability: CapabilitySet {
            schema_version: SchemaVersion(1),
            capabilities: vec![CapabilityRef("tool:mcp-read".into())],
            permissions: vec![PermissionRef("read".into())],
        },
        action_type: vec![ActionType::Observe],
        risk_limit: Risk::Low,
        approval_rule: ApprovalRule::Allow,
        budget: Budget("provider-budget".into()),
        timebox: Timebox {
            schema_version: SchemaVersion(1),
            starts_at: 0,
            expires_at: 100,
            max_turns: 2,
        },
        rollback: RollbackReq {
            schema_version: SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

fn resolve_context(allowed: Vec<ProviderId>, scope: &str) -> ResolveContext {
    ResolveContext {
        schema_version: SchemaVersion(1),
        session: SessionId("session-1".into()),
        toolset: ToolsetRef("toolset-1".into()),
        envelope: envelope(scope),
        policy_allowed_providers: allowed,
        policy_allowed_capabilities: vec![CapabilityRef("tool:mcp-read".into())],
    }
}

fn assert_active(active: &ActiveSet<FixtureProvider>, id: &ProviderId) {
    assert_eq!(active.providers.len(), 1);
    assert_eq!(active.providers[0].provider.id(), id.clone());
}
