//! Governed project App API composition with a bounded connector registry.
use super::*;

impl ReactiveHarness {
    pub fn project_owned_app_api(runtime: ProjectAppApiRuntimeConfig) -> p::Result<Self> {
        if runtime.schema_version.0 == 0
            || runtime.credential_ref.is_some()
            || runtime.scope.0.trim().is_empty()
            || runtime.capability.0.trim().is_empty()
            || runtime.permission.0.trim().is_empty()
            || runtime.max_response_bytes == 0
        {
            return Err(p::Error(
                "project-owned App API runtime is incomplete or requires an external SecretRef resolver"
                    .into(),
            ));
        }
        let connector = ProjectAppApiConnector::new(
            runtime.connector.clone(),
            runtime.base_url.clone(),
            runtime.schema_digest.clone(),
            runtime.credential_ref.clone(),
            runtime.scope.clone(),
            runtime.capability.clone(),
            runtime.permission.clone(),
            runtime.allowed_mutations.clone(),
            runtime.requests_per_minute,
            runtime.timeout,
        )?;
        let revoked = connector.revocation_flag();
        let connectors = Arc::new(AppApiConnectorRegistry::with_connectors(vec![connector])?);
        connectors.configure(runtime.connector.clone())?;
        connectors.enable(runtime.connector.clone())?;
        connectors.bind_trust(
            runtime.connector.clone(),
            p::TrustTier::ApprovedSource,
            p::Actor::Owner,
        )?;
        connectors.grant(runtime.connector.clone(), runtime.envelope.clone())?;

        let contents = Arc::new(InMemoryContentResolver::default());
        for (reference, bytes) in runtime.contents {
            contents.insert(reference, bytes)?;
        }
        let driver = Arc::new(HttpAppApiDriver::new(
            runtime.connector.clone(),
            runtime.base_url.clone(),
            runtime.schema_digest.clone(),
            runtime.credential_ref.clone(),
            runtime.allowed_mutations.clone(),
            runtime.requests_per_minute,
            runtime.timeout,
            runtime.max_response_bytes,
            revoked,
        )?);
        let backends = Arc::new(ExecutionBackendRegistry::default());
        backends.register(Arc::new(AppApiBackend::new(
            runtime.connector.clone(),
            driver,
            Arc::new(RejectingSecretResolver),
            contents,
            OutputBudget::truncate_at(runtime.max_response_bytes),
            runtime.timeout,
        )?))?;

        let profile = action_only_model_profile()?;
        let model = Arc::new(ScriptedModelProvider::new(profile.clone(), Vec::new())?);
        let policy = PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::User,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::AppApi),
                    capability: Some(runtime.capability.clone()),
                    action_type: None,
                    parameters: ArgMatcher::Any,
                },
                effect: p::PolicyDecision::Ask,
                scope: runtime.scope.clone(),
            }],
        };
        let governance = GovernanceConfig {
            schema_version: p::SchemaVersion(1),
            layers: vec![policy],
            visible_capabilities: vec![runtime.capability.clone()],
            granted_permissions: vec![runtime.permission.clone()],
            allowed_scopes: vec![runtime.scope.clone()],
            shell_allowlist: Vec::new(),
            file_roots: Vec::new(),
            mcp_allowlist: Vec::new(),
            external: forme_policy::ExternalPolicyLimits {
                schema_version: p::SchemaVersion(1),
                browser_origins: Vec::new(),
                computer_surfaces: Vec::new(),
                pty_programs: Vec::new(),
                pty_roots: Vec::new(),
                app_api_connectors: vec![forme_policy::AppApiConnectorLimit {
                    schema_version: p::SchemaVersion(1),
                    connector: runtime.connector,
                    base_url: runtime.base_url,
                    schema_digest: runtime.schema_digest,
                    credential_ref: runtime.credential_ref,
                    allowed_mutations: runtime.allowed_mutations,
                    max_timeout: runtime.timeout,
                }],
            },
            network_allowed: true,
            sandbox_available: true,
            delegation: Some(DelegationGrant {
                schema_version: p::SchemaVersion(1),
                subject: DelegationSubject::Owner,
                envelope: runtime.envelope.clone(),
                granted_by: p::Actor::Owner,
                audit_ref: p::EventId("delegation:project-app-api".into()),
            }),
            envelope: Some(runtime.envelope),
        };
        let mut config = HarnessConfig::for_model(&profile);
        config.toolset_ref = p::ToolsetRef("toolset:project-app-api".into());
        config.workspace = p::WorkspaceRef(runtime.scope.0);
        Self::new(
            SqliteEventStore::open_in_memory(StoreOptions::default())?,
            model,
            backends,
            governance,
            config,
        )
        .map(|harness| harness.with_capability_rechecker(connectors))
    }
}
