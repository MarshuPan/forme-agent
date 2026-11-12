use super::*;
use crate::production_artifact_store::worker_fixture::{owner, scope, Fixture};
use forme_capabilities::{ManagedMcpCatalogSource, McpAllowlist, StdioMcpServer};
use std::sync::atomic::{AtomicUsize, Ordering};
#[path = "tests/plugins.rs"]
mod plugins;

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
        scope: scope(),
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
    let schema = serde_json::json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false});
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
fn config() -> crate::CapabilityRuntimeConfig {
    let settings = settings();
    let mut config = crate::CapabilityRuntimeConfig::default();
    let capabilities = p::CapabilitySet {
        schema_version: p::SchemaVersion(1),
        capabilities: vec![p::CapabilityRef(
            "mcp:bootstrap-fixture:fixture.inspect".into(),
        )],
        permissions: vec![p::PermissionRef("execute".into())],
    };
    config.mcp.push(crate::McpRuntimeConfig {
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

fn snapshots(f: &Fixture, source: Arc<Source>) -> RunToolSnapshots {
    let config = config();
    let entry = &config.mcp[0];
    let registry = Arc::new(
        StdioMcpRegistry::with_managed_servers(vec![entry.server.clone()], Some(source.clone()))
            .unwrap(),
    );
    let id = entry.server.provider_id.clone();
    registry.configure(id.clone()).unwrap();
    registry.enable(id.clone()).unwrap();
    registry
        .bind_trust(id.clone(), p::TrustTier::OwnerInput, p::Actor::Owner)
        .unwrap();
    registry.grant(id, entry.grant.clone().unwrap()).unwrap();
    let mut envelope = entry.grant.clone().unwrap();
    envelope.capability.capabilities.clear();
    envelope.capability.permissions.clear();
    envelope.timebox.starts_at = 123;
    envelope.timebox.expires_at = 999_999;
    RunToolSnapshots {
        store: Arc::new(f.store.clone()),
        owner: owner(),
        composition: OnceLock::from(p::canonical_digest(&"run-tools-fixture").unwrap()),
        access: Mutex::new(()),
        bindings: BTreeMap::new(),
        envelope,
        registry,
        managed_source: Some(source),
        managed_configs: vec![settings()],
        plugins: None,
    }
}
fn request(run: &str) -> ContextHydrationRequest {
    ContextHydrationRequest {
        schema_version: p::SchemaVersion(1),
        run: p::RunId(run.into()),
        session: p::SessionId("session:tools".into()),
        workspace: p::WorkspaceRef(scope().0),
        scope: scope(),
        brain_call: true,
        selected_skills: vec![],
        model: p::ModelProfileRef("model:fixture".into()),
        budget: forme_context::ContextBudget {
            schema_version: p::SchemaVersion(1),
            max_tokens: 4096,
            reserve: 256,
        },
    }
}
fn schema() -> ToolSchema {
    ToolSchema {
        schema_version: p::SchemaVersion(1),
        version: p::Version(1),
        entries: vec![],
        provenance: p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        },
    }
}
fn source() -> Arc<Source> {
    Arc::new(Source {
        reads: AtomicUsize::new(0),
        original: Mutex::new(None),
    })
}
fn call() -> forme_models::ModelToolCall {
    forme_models::ModelToolCall {
        schema_version: p::SchemaVersion(1),
        call_id: p::ToolCallId("call:snapshot".into()),
        tool: p::ToolRef("mcp:bootstrap-fixture:fixture.inspect".into()),
        arguments: serde_json::json!({"query":"safe data"}),
        intent: None,
    }
}
fn accepted(f: &Fixture, snapshot: &RunToolSnapshot) {
    for (id, payload) in [
        (
            "accepted",
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::UserTurn,
                session_ref: snapshot.session.clone(),
                input_ref: p::InputRef("request".into()),
                idempotency_key: None,
            }),
        ),
        (
            "bound",
            p::EventPayload::SessionBound(p::SessionBoundPayload {
                policy_profile: p::PolicyProfileRef("policy:fixture".into()),
                model_profile: p::ModelProfileRef("model:fixture".into()),
                toolset_ref: snapshot.reference().unwrap(),
                workspace: snapshot.workspace.clone(),
                effect_mode: None,
                evolution_snapshot: None,
                federation_snapshot: None,
                composition_manifest: Some(snapshot.composition.clone()),
            }),
        ),
    ] {
        f.store
            .append(p::Event::new(
                p::EventId(format!("{}:{id}", snapshot.run.0)),
                snapshot.run.clone(),
                None,
                payload,
                p::SchemaVersion(1),
                crate::now_ms(),
                schema().provenance,
            ))
            .unwrap();
    }
}

#[test]
fn startup_catalog_is_rebuilt_once_and_cannot_preserve_a_disabled_managed_tool() {
    let f = Fixture::new();
    let source = source();
    *source.original.lock().unwrap() = Some(original());
    let mut manager = snapshots(&f, source.clone());
    let first = manager
        .select(&request("before-restart"), &schema())
        .unwrap();
    // Production assembly indexes a retained catalog before creating the
    // hydration source. It must coexist with request-scoped selection.
    manager.bindings = first.bindings.clone();
    let startup_schema = first.schema();
    let after = manager
        .select(&request("after-restart"), &startup_schema)
        .unwrap();
    assert_eq!(after.entries.len(), 1);
    assert_eq!(after.bindings.len(), 1);
    assert_eq!(after.managed.len(), 1);
    assert!(manager.prepare_managed(&after, &call()).unwrap().is_some());
    *source.original.lock().unwrap() = None;
    let disabled = manager
        .select(&request("disabled-after-start"), &startup_schema)
        .unwrap();
    assert!(disabled.entries.is_empty());
    assert!(disabled.bindings.is_empty());
    assert!(disabled.envelope.capability.capabilities.is_empty());
    assert!(disabled.envelope.capability.permissions.is_empty());
    assert!(manager.prepare_managed(&after, &call()).is_err());
}

#[test]
fn discovery_changes_only_new_runs_and_raw_arguments_use_fixed_authority() {
    let f = Fixture::new();
    let source = source();
    let snapshots = snapshots(&f, source.clone());
    let before = snapshots.select(&request("before"), &schema()).unwrap();
    accepted(&f, &before);
    assert!(before.entries.is_empty());
    *source.original.lock().unwrap() = Some(original());
    let after = snapshots.select(&request("after"), &schema()).unwrap();
    assert_eq!(after.entries.len(), 1);
    assert!(snapshots
        .select(&request("before"), &schema())
        .unwrap()
        .entries
        .is_empty());
    assert!(snapshots
        .prepare_managed(&before, &call())
        .unwrap()
        .is_none());
    let intent = snapshots.prepare_managed(&after, &call()).unwrap().unwrap();
    assert_eq!(intent.risk_hint, p::Risk::High);
    assert_eq!(intent.expected_effect, p::ExpectedEffect::Outward);
    let p::ActionParameters::Mcp {
        stdio, arguments, ..
    } = &intent.parameters
    else {
        panic!()
    };
    assert_eq!(stdio.command, settings().program_ref);
    assert_eq!(arguments, &call().arguments);
    let mut attack = call();
    attack.arguments = serde_json::json!({"query":"x","command":"arbitrary.exe"});
    let with_data = snapshots.prepare_managed(&after, &attack).unwrap().unwrap();
    let p::ActionParameters::Mcp { stdio, .. } = with_data.parameters else {
        panic!()
    };
    assert_eq!(stdio.command, settings().program_ref);
    attack.arguments = serde_json::json!({"command":"arbitrary.exe"});
    assert!(snapshots.prepare_managed(&after, &attack).is_err());
    let events = f
        .store
        .read_run(before.run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        vec![p::EventKind::RunAccepted, p::EventKind::SessionBound]
    );
}

#[test]
fn restart_preserves_original_schema_timebox_and_missing_source_denies_execution() {
    let f = Fixture::new();
    let source = source();
    *source.original.lock().unwrap() = Some(original());
    let first = snapshots(&f, source.clone());
    let original = first.select(&request("restart"), &schema()).unwrap();
    accepted(&f, &original);
    let reference = original.reference().unwrap();
    let root = f.root.clone();
    drop(first);
    drop(f);
    let f = Fixture::open(root);
    let recovered = snapshots(&f, source.clone());
    let calls = source.reads.load(Ordering::SeqCst);
    let loaded = recovered.select(&request("restart"), &schema()).unwrap();
    assert_eq!(loaded.reference().unwrap(), reference);
    assert_eq!(loaded.envelope.timebox.expires_at, 999_999);
    assert_eq!(source.reads.load(Ordering::SeqCst), calls);
    let intent = recovered
        .prepare_managed(&loaded, &call())
        .unwrap()
        .unwrap();
    *source.original.lock().unwrap() = None;
    assert!(recovered.recheck(&loaded, &intent).is_err());
    assert!(recovered.prepare_managed(&loaded, &call()).is_err());
    assert_eq!(
        recovered
            .select(&request("restart"), &schema())
            .unwrap()
            .reference()
            .unwrap(),
        reference
    );
    assert!(recovered
        .select(&request("new-disabled"), &schema())
        .unwrap()
        .entries
        .is_empty());
}

#[test]
fn changed_schema_registration_scope_or_composition_cannot_rebind_an_old_run() {
    let f = Fixture::new();
    let source = source();
    *source.original.lock().unwrap() = Some(original());
    let manager = snapshots(&f, source.clone());
    let pinned = manager.select(&request("pinned"), &schema()).unwrap();
    let intent = manager.prepare_managed(&pinned, &call()).unwrap().unwrap();
    for mutation in 0..3 {
        let mut changed = original();
        match mutation {
            0 => changed.1.tools[0].description.push_str("changed"),
            1 => changed.0.enabled = false,
            _ => {
                changed.1.spec.registration_digest =
                    p::canonical_digest(&"new registration").unwrap()
            }
        }
        *source.original.lock().unwrap() = Some(changed);
        assert!(manager.recheck(&pinned, &intent).is_err());
    }
    let mut other = request("pinned");
    other.scope = p::Scope("workspace:another".into());
    assert!(manager.select(&other, &schema()).is_err());
    let mut other = request("pinned");
    other.session = p::SessionId("session:another".into());
    assert!(manager.select(&other, &schema()).is_err());
    let mut replacement = snapshots(&f, source);
    replacement.composition =
        OnceLock::from(p::canonical_digest(&"different composition").unwrap());
    assert!(replacement.load(&pinned.run).is_err());
}

#[test]
fn concurrent_first_hydration_pins_one_original_and_deletion_does_not_recreate_it() {
    let f = Fixture::new();
    let source = source();
    *source.original.lock().unwrap() = Some(original());
    let manager = Arc::new(snapshots(&f, source));
    let threads = (0..8)
        .map(|_| {
            let manager = manager.clone();
            std::thread::spawn(move || {
                manager
                    .select(&request("concurrent"), &schema())
                    .unwrap()
                    .reference()
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let references = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    assert!(references.iter().all(|r| r == &references[0]));
    let original = manager
        .load(&p::RunId("concurrent".into()))
        .unwrap()
        .unwrap();
    accepted(&f, &original);
    f.protection
        .lifecycle_service(f.store.clone())
        .apply(f.deletion())
        .unwrap();
    assert!(manager.select(&request("concurrent"), &schema()).is_err());
}
