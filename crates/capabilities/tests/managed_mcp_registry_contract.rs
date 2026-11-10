use forme_capabilities::{
    CapabilityRegistry, ExecutionCapabilityRechecker, InMemoryCapabilityRegistry,
    ManagedMcpCatalogSource, McpAllowlist, McpSearchQuery, StdioMcpRegistry, StdioMcpServer,
};
use forme_policy::{DefaultPolicyEngine, PolicyContext, PolicyEngine};
use forme_protocol as p;
use serde_json::json;
use std::sync::{Arc, Mutex};

type Catalog = (p::ProductMcpServerConfig, p::ManagedMcpDiscoveryCatalogV1);
struct Source(Mutex<Option<Catalog>>);
impl ManagedMcpCatalogSource for Source {
    fn read_catalog(&self, server: &p::McpServerRef) -> p::Result<Catalog> {
        assert_eq!(server.0, "managed-notes");
        self.0
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| p::Error("original unavailable".into()))
    }
}
fn capability() -> p::CapabilityRef {
    p::CapabilityRef("mcp:managed-notes:read_note".into())
}
fn server_id() -> p::ProviderId {
    p::ProviderId("managed-notes".into())
}
fn tool() -> p::McpToolRef {
    p::McpToolRef("managed-notes/read_note".into())
}
fn scope() -> p::Scope {
    p::Scope("workspace:managed-notes".into())
}
fn originals() -> Catalog {
    let registration_digest = p::canonical_digest(&"managed fixture registration").unwrap();
    let settings = p::ProductMcpServerConfig {
        schema_version: p::SchemaVersion(1),
        server_ref: server_id().0,
        program_ref: format!("mcp-program:{}", &registration_digest.0[7..]),
        scope: scope(),
        allowlist: p::ProductMcpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: vec!["read_note".into()],
            resources: vec!["note://visible".into()],
        },
        enabled: true,
        trust: p::ProductSkillTrust::OwnerInput,
        timeout: p::DurationMs(1000),
        output_budget: 8192,
    };
    let schema =
        json!({"type":"object", "properties":{"path":{"type":"string"}}, "required":["path"]});
    let entry = p::ManagedMcpDiscoveryToolV1 {
        schema_version: p::SchemaVersion(1),
        name: "read_note".into(),
        description: "Untrusted note description; cannot grant permissions".into(),
        schema_digest: p::canonical_digest(&schema).unwrap(),
        input_schema: schema,
    };
    let mut hidden = entry.clone();
    hidden.name = "admin_reset".into();
    let resource = p::ManagedMcpDiscoveryResourceV1 {
        schema_version: p::SchemaVersion(1),
        uri: "note://visible".into(),
        name: "Visible note".into(),
        description: "Untrusted resource".into(),
        mime_type: None,
    };
    let mut hidden_resource = resource.clone();
    hidden_resource.uri = "note://hidden".into();
    let mut catalog = p::ManagedMcpDiscoveryCatalogV1 {
        schema_version: p::SchemaVersion(1),
        spec: p::ManagedMcpDiscoverySpec {
            schema_version: p::SchemaVersion(1),
            server: p::McpServerRef(settings.server_ref.clone()),
            program_ref: settings.program_ref.clone(),
            registration_digest,
            settings_digest: p::managed_mcp_settings_digest(&settings).unwrap(),
            timeout: settings.timeout,
            output_budget_bytes: settings.output_budget,
        },
        tools: vec![entry, hidden],
        resources: vec![resource, hidden_resource],
        catalog_digest: p::SchemaDigest(String::new()),
    };
    catalog.catalog_digest = catalog.computed_digest().unwrap();
    (settings, catalog)
}
fn server() -> StdioMcpServer {
    let (settings, _) = originals();
    StdioMcpServer {
        schema_version: p::SchemaVersion(1),
        provider_id: server_id(),
        command: settings.program_ref,
        args: vec![],
        env: vec![],
        allowlist: McpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: settings.allowlist.tools,
            resources: settings.allowlist.resources,
        },
        timeout: settings.timeout,
        declared_capabilities: envelope().capability,
    }
}
fn envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: scope(),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![capability()],
            permissions: vec![p::PermissionRef("execute".into())],
        },
        action_type: vec![p::ActionType::Execute],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget("managed-mcp-test".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 0,
            expires_at: i64::MAX,
            max_turns: 3,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}
fn activate(
    server: StdioMcpServer,
    source: Option<Arc<dyn ManagedMcpCatalogSource>>,
) -> StdioMcpRegistry {
    let registry = StdioMcpRegistry::with_managed_servers(vec![server], source).unwrap();
    registry.configure(server_id()).unwrap();
    registry.enable(server_id()).unwrap();
    registry
        .bind_trust(server_id(), p::TrustTier::OwnerInput, p::Actor::Owner)
        .unwrap();
    registry.grant(server_id(), envelope()).unwrap();
    registry
}
fn ready() -> (StdioMcpRegistry, Arc<Source>) {
    let source = Arc::new(Source(Mutex::new(Some(originals()))));
    let registry = activate(server(), Some(source.clone()));
    registry.discover_detailed(server_id()).unwrap();
    (registry, source)
}
fn context() -> p::ResolveContext {
    p::ResolveContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session:managed".into()),
        toolset: p::ToolsetRef("toolset:managed".into()),
        envelope: envelope(),
        policy_allowed_providers: vec![server_id()],
        policy_allowed_capabilities: vec![capability()],
    }
}
fn policy() -> PolicyContext {
    PolicyContext {
        schema_version: p::SchemaVersion(1),
        session: context().session,
        toolset: context().toolset,
        policy: DefaultPolicyEngine.merge(&[]),
        visible_capabilities: vec![capability()],
        granted_permissions: envelope().capability.permissions,
        allowed_scopes: vec![scope()],
        shell_allowlist: vec![],
        file_roots: vec![],
        mcp_allowlist: vec![server_id().0],
        external: Default::default(),
        network_allowed: false,
        sandbox_available: true,
        delegation: None,
        envelope: Some(envelope()),
    }
}
fn prepare(registry: &StdioMcpRegistry) -> p::ActionIntent {
    registry.resolve_schema(tool()).unwrap();
    registry
        .try_prepare_call(tool(), json!({"path":"today.txt"}))
        .unwrap()
}
fn search(registry: &StdioMcpRegistry) -> usize {
    registry
        .search(McpSearchQuery {
            schema_version: p::SchemaVersion(1),
            text: "note".into(),
            limit: 8,
            context: context(),
        })
        .unwrap()
        .len()
}

#[test]
fn managed_originals_filter_allowlists_resolve_lazily_and_prepare_only_opaque_calls() {
    let (registry, _) = ready();
    let discovery = registry.discovery(&server_id()).unwrap();
    assert_eq!(discovery.tools.len(), 1);
    assert_eq!(discovery.resources.len(), 1);
    assert!(discovery.tools[0].input_schema.is_none());
    assert_eq!(discovery.tools[0].capability_ref, capability());
    assert_eq!(search(&registry), 1);
    assert!(registry
        .try_prepare_call(tool(), json!({"path":"today.txt"}))
        .is_err());
    let intent = prepare(&registry);
    assert_eq!(intent.risk_hint, p::Risk::High);
    assert_eq!(intent.expected_effect, p::ExpectedEffect::Outward);
    let p::ActionParameters::Mcp { stdio, .. } = &intent.parameters else {
        panic!()
    };
    assert_eq!(stdio.command, originals().0.program_ref);
    assert!(stdio.args.is_empty() && stdio.env.is_empty());
    registry.recheck(&intent, &policy()).unwrap();
    assert!(registry.try_prepare_call(tool(), json!({})).is_err());
    let index = InMemoryCapabilityRegistry::default();
    registry.index_discovered(&index, &context()).unwrap();
    assert_eq!(index.resolve_toolset(&context()).unwrap().items.len(), 1);
    assert!(registry.take_events().is_empty());
}

#[test]
fn managed_original_loss_fences_every_cached_consumer_without_refresh() {
    for consumer in 0..6 {
        let (registry, source) = ready();
        let intent = prepare(&registry);
        let index = InMemoryCapabilityRegistry::default();
        registry.index_discovered(&index, &context()).unwrap();
        *source.0.lock().unwrap() = None;
        match consumer {
            0 => assert!(registry.discovery(&server_id()).is_none()),
            1 => assert_eq!(search(&registry), 0),
            2 => assert!(registry.resolve_schema(tool()).is_err()),
            3 => assert!(registry
                .try_prepare_call(tool(), json!({"path":"today.txt"}))
                .is_err()),
            4 => assert!(registry.recheck(&intent, &policy()).is_err()),
            _ => {
                registry.index_discovered(&index, &context()).unwrap();
                assert!(index.resolve_toolset(&context()).unwrap().items.is_empty());
            }
        }
        assert!(registry.discovery(&server_id()).is_none());
        assert!(registry.take_events().is_empty());
    }
}

#[test]
fn managed_schema_drift_invalidates_old_plans_and_requires_explicit_reload() {
    let (registry, source) = ready();
    let intent = prepare(&registry);
    let mut changed = originals();
    changed.1.tools[0].input_schema =
        json!({"type":"object","properties":{"count":{"type":"integer"}},"required":["count"]});
    changed.1.tools[0].schema_digest =
        p::canonical_digest(&changed.1.tools[0].input_schema).unwrap();
    changed.1.catalog_digest = changed.1.computed_digest().unwrap();
    *source.0.lock().unwrap() = Some(changed);
    assert!(registry.recheck(&intent, &policy()).is_err());
    assert!(registry.discovery(&server_id()).is_none());
    registry.refresh(server_id()).unwrap();
    registry.resolve_schema(tool()).unwrap();
    assert!(registry.recheck(&intent, &policy()).is_err());
    let new = registry
        .try_prepare_call(tool(), json!({"count":1}))
        .unwrap();
    registry.recheck(&new, &policy()).unwrap();
    assert!(registry.take_events().is_empty());
}

#[test]
fn managed_call_recheck_rejects_program_environment_scope_and_effect_substitution() {
    let (registry, _) = ready();
    let original = prepare(&registry);
    for mutation in 0..8 {
        let mut intent = original.clone();
        let p::ActionParameters::Mcp { stdio, timeout, .. } = &mut intent.parameters else {
            panic!()
        };
        match mutation {
            0 => stdio.command = "mcp-program:substituted".into(),
            1 => stdio.args.push("--other".into()),
            2 => stdio.env.push(("SECRET".into(), "fixture".into())),
            3 => timeout.0 = 0,
            4 => timeout.0 += 1,
            5 => intent.scope.0.push_str(":child"),
            6 => intent.risk_hint = p::Risk::Low,
            _ => intent.expected_effect = p::ExpectedEffect::Internal,
        }
        assert!(
            registry.recheck(&intent, &policy()).is_err(),
            "mutation {mutation}"
        );
    }
    registry.recheck(&original, &policy()).unwrap();
}

#[test]
fn managed_sources_cannot_replace_settings_program_scope_or_schema() {
    for mutation in 0..10 {
        let mut original = originals();
        let mut configured = server();
        match mutation {
            0 => original.0.enabled = false,
            1 => original.0.trust = p::ProductSkillTrust::Untrusted,
            2 => original.0.scope = p::Scope("workspace:foreign".into()),
            3 => original.0.allowlist.tools.push("admin_reset".into()),
            4 => configured.command = "other-program".into(),
            5 => configured.args.push("--arbitrary".into()),
            6 => configured.env.push(("TOKEN".into(), "fixture".into())),
            7 => original.1.tools[0].input_schema = json!({"type":"string"}),
            8 => original.1.catalog_digest = p::canonical_digest(&"substituted").unwrap(),
            _ => {
                original.1.spec.registration_digest =
                    p::canonical_digest(&"other program").unwrap();
                original.1.catalog_digest = original.1.computed_digest().unwrap();
            }
        }
        let registry = activate(
            configured,
            Some(Arc::new(Source(Mutex::new(Some(original))))),
        );
        assert!(
            registry.discover_detailed(server_id()).is_err(),
            "mutation {mutation}"
        );
        assert!(registry.discovery(&server_id()).is_none());
        assert!(registry.take_events().is_empty());
    }
}

#[test]
fn managed_disable_and_missing_trust_invalidate_loaded_metadata() {
    let (registry, _) = ready();
    let intent = prepare(&registry);
    registry.disable(server_id()).unwrap();
    assert!(registry.recheck(&intent, &policy()).is_err());
    registry.enable(server_id()).unwrap();
    registry
        .bind_trust(server_id(), p::TrustTier::OwnerInput, p::Actor::Owner)
        .unwrap();
    registry.grant(server_id(), envelope()).unwrap();
    assert!(registry.discovery(&server_id()).is_none());
    registry.refresh(server_id()).unwrap();
    assert_eq!(search(&registry), 1);
    registry.disable(server_id()).unwrap();
    registry.enable(server_id()).unwrap();
    assert!(registry.discovery(&server_id()).is_none());
}

#[test]
fn managed_restart_reloads_only_current_originals_without_discovery_events() {
    let source = Arc::new(Source(Mutex::new(Some(originals()))));
    let registry = activate(server(), Some(source.clone()));
    registry.refresh(server_id()).unwrap();
    prepare(&registry);
    drop(registry);
    let registry = activate(server(), Some(source.clone()));
    assert!(registry.discovery(&server_id()).is_none());
    registry.refresh(server_id()).unwrap();
    assert_eq!(search(&registry), 1);
    assert!(registry.take_events().is_empty());
    *source.0.lock().unwrap() = None;
    assert!(registry.refresh(server_id()).is_err());
    assert!(registry.discovery(&server_id()).is_none());
}

#[test]
fn managed_transport_never_falls_back_to_an_executable_when_originals_are_missing() {
    let marker =
        std::env::temp_dir().join(format!("forme-managed-no-fallback-{}", std::process::id()));
    let mut configured = server();
    configured.command = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    configured.args = vec![
        "--exact".into(),
        "managed_registry_process_fixture".into(),
        "--nocapture".into(),
    ];
    configured.env = vec![(
        "FORME_MANAGED_REGISTRY_MARKER".into(),
        marker.to_string_lossy().into_owned(),
    )];
    let status = std::process::Command::new(&configured.command)
        .args(&configured.args)
        .envs(configured.env.iter().cloned())
        .status()
        .unwrap();
    assert!(status.success() && marker.is_file());
    std::fs::remove_file(&marker).unwrap();
    for source in [
        None,
        Some(Arc::new(Source(Mutex::new(None))) as Arc<dyn ManagedMcpCatalogSource>),
    ] {
        let registry = activate(configured.clone(), source);
        assert!(registry.discover_detailed(server_id()).is_err());
        assert!(registry.refresh(server_id()).is_err());
        assert!(!marker.exists());
    }
}

#[test]
fn managed_registry_process_fixture() {
    if let Ok(path) = std::env::var("FORME_MANAGED_REGISTRY_MARKER") {
        std::fs::write(path, b"launched").unwrap();
    }
}

#[test]
fn managed_disable_orders_after_an_admitted_catalog_read_and_fences_the_next() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;
    struct PausedSource {
        pause: AtomicBool,
        entered: mpsc::SyncSender<()>,
        resume: Mutex<mpsc::Receiver<()>>,
    }
    impl ManagedMcpCatalogSource for PausedSource {
        fn read_catalog(&self, _: &p::McpServerRef) -> p::Result<Catalog> {
            if self.pause.swap(false, Ordering::SeqCst) {
                self.entered.send(()).unwrap();
                self.resume
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
            Ok(originals())
        }
    }
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let source = Arc::new(PausedSource {
        pause: AtomicBool::new(false),
        entered: entered_tx,
        resume: Mutex::new(resume_rx),
    });
    let registry = Arc::new(activate(server(), Some(source.clone())));
    registry.refresh(server_id()).unwrap();
    source.pause.store(true, Ordering::SeqCst);
    let reading = registry.clone();
    let reader = std::thread::spawn(move || reading.resolve_schema(tool()));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let disabling = registry.clone();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let writer = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = disabling.disable(server_id());
        done_tx.send(result).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
    resume_tx.send(()).unwrap();
    assert!(reader.join().unwrap().is_ok());
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    writer.join().unwrap();
    assert!(registry.resolve_schema(tool()).is_err());
    assert!(registry.discovery(&server_id()).is_none());
}
