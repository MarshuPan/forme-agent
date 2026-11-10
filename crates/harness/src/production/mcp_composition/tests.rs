use super::*;
use forme_capabilities::{ManagedMcpCatalogSource, McpAllowlist, StdioMcpServer};
use std::sync::{atomic::AtomicUsize, Mutex};

struct Source {
    reads: AtomicUsize,
    original: Mutex<Option<(p::ProductMcpServerConfig, p::ManagedMcpDiscoveryCatalogV1)>>,
}
impl ManagedMcpCatalogSource for Source {
    fn read_catalog(
        &self,
        _: &p::McpServerRef,
    ) -> p::Result<(p::ProductMcpServerConfig, p::ManagedMcpDiscoveryCatalogV1)> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.original
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| p::Error("fixture has no completed discovery".into()))
    }
}
fn settings() -> p::ProductMcpServerConfig {
    p::ProductMcpServerConfig {
        schema_version: p::SchemaVersion(1),
        server_ref: "bootstrap-fixture".into(),
        program_ref: format!("mcp-program:{}", "a".repeat(64)),
        scope: p::Scope("workspace:bootstrap".into()),
        enabled: true,
        trust: p::ProductSkillTrust::OwnerInput,
        timeout: p::DurationMs(1000),
        output_budget: 8192,
        allowlist: p::ProductMcpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: vec!["fixture.inspect".into()],
            resources: vec![],
        },
    }
}
fn original() -> (p::ProductMcpServerConfig, p::ManagedMcpDiscoveryCatalogV1) {
    let settings = settings();
    let schema = serde_json::json!({"type":"object","properties":{}});
    let mut catalog = p::ManagedMcpDiscoveryCatalogV1 {
        schema_version: p::SchemaVersion(1),
        spec: p::ManagedMcpDiscoverySpec {
            schema_version: p::SchemaVersion(1),
            server: p::McpServerRef(settings.server_ref.clone()),
            program_ref: settings.program_ref.clone(),
            registration_digest: p::SchemaDigest(format!("sha256:{}", "a".repeat(64))),
            settings_digest: p::managed_mcp_settings_digest(&settings).unwrap(),
            timeout: settings.timeout,
            output_budget_bytes: settings.output_budget,
        },
        tools: vec![p::ManagedMcpDiscoveryToolV1 {
            schema_version: p::SchemaVersion(1),
            name: "fixture.inspect".into(),
            description: "Repository-owned bootstrap fixture".into(),
            schema_digest: p::canonical_digest(&schema).unwrap(),
            input_schema: schema,
        }],
        resources: vec![],
        catalog_digest: p::SchemaDigest(String::new()),
    };
    catalog.catalog_digest = catalog.computed_digest().unwrap();
    catalog.validate_for(&catalog.spec).unwrap();
    (settings, catalog)
}
fn config() -> CapabilityRuntimeConfig {
    let settings = settings();
    let mut config = CapabilityRuntimeConfig::default();
    let capabilities = p::CapabilitySet {
        schema_version: p::SchemaVersion(1),
        capabilities: vec![p::CapabilityRef(
            "mcp:bootstrap-fixture:fixture.inspect".into(),
        )],
        permissions: vec![p::PermissionRef("execute".into())],
    };
    config.mcp.push(McpRuntimeConfig {
        server: StdioMcpServer {
            schema_version: p::SchemaVersion(1),
            provider_id: p::ProviderId(settings.server_ref),
            command: settings.program_ref,
            args: vec![],
            env: vec![],
            timeout: settings.timeout,
            declared_capabilities: capabilities.clone(),
            allowlist: McpAllowlist {
                schema_version: p::SchemaVersion(1),
                tools: settings.allowlist.tools,
                resources: vec![],
            },
        },
        inherit_env: vec![],
        enabled: true,
        trust: Some(p::TrustTier::OwnerInput),
        discover_on_start: true,
        grant: Some(p::AutonomyEnvelope {
            schema_version: p::SchemaVersion(1),
            scope: settings.scope,
            capability: capabilities,
            action_type: vec![p::ActionType::Execute],
            risk_limit: p::Risk::High,
            approval_rule: p::ApprovalRule::Ask,
            budget: config.envelope_budget.clone(),
            timebox: p::Timebox {
                schema_version: p::SchemaVersion(1),
                starts_at: 0,
                expires_at: i64::MAX,
                max_turns: 1,
            },
            rollback: p::RollbackReq {
                schema_version: p::SchemaVersion(1),
                required: false,
                boundary: None,
            },
        }),
    });
    config
}
fn registry(config: &CapabilityRuntimeConfig) -> (StdioMcpRegistry, Arc<Source>) {
    let source = Arc::new(Source {
        reads: AtomicUsize::new(0),
        original: Mutex::new(None),
    });
    (
        StdioMcpRegistry::with_managed_servers(
            config
                .mcp
                .iter()
                .map(|entry| entry.server.clone())
                .collect(),
            Some(source.clone()),
        )
        .unwrap(),
        source,
    )
}
fn prepare_for(config: &CapabilityRuntimeConfig, registry: &StdioMcpRegistry) -> PreparedMcp {
    prepare(config, registry, &BTreeSet::new(), true, &mut vec![]).unwrap()
}
#[test]
fn first_catalog_absence_does_not_remove_worker_eligibility_or_expose_tools() {
    for discover_on_start in [false, true] {
        let mut config = config();
        config.mcp[0].discover_on_start = discover_on_start;
        let (registry, source) = registry(&config);
        let prepared = prepare_for(&config, &registry);
        assert!(prepared.needs_worker);
        assert!(prepared.active.is_empty());
        assert!(prepared.tools.is_empty());
        assert!(source.reads.load(Ordering::SeqCst) > 0);
        assert!(registry
            .resolve_schema(p::McpToolRef("bootstrap-fixture/fixture.inspect".into()))
            .is_err());
    }
}
#[test]
fn completed_original_can_be_loaded_after_first_absence_without_reenabling_provider() {
    let config = config();
    let (registry, source) = registry(&config);
    assert!(prepare_for(&config, &registry).active.is_empty());
    *source.original.lock().unwrap() = Some(original());
    let discovery = registry
        .discover_detailed(config.mcp[0].server.provider_id.clone())
        .unwrap();
    assert_eq!(discovery.tools.len(), 1);
    assert_eq!(discovery.tools[0].name, "fixture.inspect");
    *source.original.lock().unwrap() = None;
    assert!(registry
        .resolve_schema(discovery.tools[0].tool_ref.clone())
        .is_err());
}
#[test]
fn disabled_untrusted_ungranted_or_unresolved_server_has_no_worker_eligibility() {
    for mutation in 0..6 {
        let mut config = config();
        match mutation {
            0 => config.mcp[0].enabled = false,
            1 => config.mcp[0].trust = Some(p::TrustTier::Untrusted),
            2 => config.mcp[0].grant = None,
            3 => config.mcp[0].grant.as_mut().unwrap().budget = p::Budget("other".into()),
            5 => config.mcp[0].trust = Some(p::TrustTier::VerifiedProcess),
            _ => (),
        }
        let (registry, source) = registry(&config);
        let unresolved = if mutation == 4 {
            [config.mcp[0].server.provider_id.clone()]
                .into_iter()
                .collect()
        } else {
            BTreeSet::new()
        };
        let prepared = prepare(&config, &registry, &unresolved, true, &mut vec![]).unwrap();
        assert!(!prepared.needs_worker, "mutation {mutation}");
        assert!(prepared.active.is_empty());
        assert!(prepared.tools.is_empty());
        assert_eq!(source.reads.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn valid_original_is_required_even_when_start_discovery_flag_is_false() {
    let mut config = config();
    config.mcp[0].discover_on_start = false;
    let (registry, source) = registry(&config);
    *source.original.lock().unwrap() = Some(original());
    let prepared = prepare_for(&config, &registry);
    assert!(prepared.needs_worker);
    assert_eq!(
        prepared.active,
        [config.mcp[0].server.provider_id.clone()]
            .into_iter()
            .collect()
    );
    assert_eq!(prepared.tools.len(), 1);
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
}

#[test]
fn managed_resolution_rejects_process_parameters_before_registering_any_server() {
    for mutation in 0..3 {
        let mut config = config();
        match mutation {
            0 => config.mcp[0]
                .inherit_env
                .push("FORME_BOOTSTRAP_FIXTURE_SECRET".into()),
            1 => config.mcp[0]
                .server
                .env
                .push(("FIXTURE_PRIVATE".into(), "must-stay-out".into())),
            _ => config.mcp[0]
                .server
                .args
                .push("--caller-selected-program-option".into()),
        }
        let (resolved, unavailable) =
            super::super::managed_workers::resolve_mcp_servers(&config, true);
        assert!(resolved.is_empty());
        assert_eq!(
            unavailable,
            [config.mcp[0].server.provider_id.clone()]
                .into_iter()
                .collect()
        );
    }
    let config = config();
    let (resolved, unavailable) = super::super::managed_workers::resolve_mcp_servers(&config, true);
    assert_eq!(resolved, vec![config.mcp[0].server.clone()]);
    assert!(unavailable.is_empty());
}
