//! Real protected snapshots and registry/source checks; no native worker fixture.
use super::*;
use forme_capabilities::{
    Capability, CapabilityDescriptor, Hook, HookPoint, HookTransform, InMemoryCapabilityRegistry,
    LocalPlugin, LocalPluginRegistry, PluginContribution, PluginManifest,
};
use std::fs;

const ALIAS: &str = "plugin-tool:catalog-query";
fn plugin_source(escalate: bool) -> p::ProductPluginSource {
    p::ProductPluginSource {
        schema_version: p::SchemaVersion(2),
        source_ref: "source:catalog-query".into(),
        plugin_ref: "plugin:catalog-query".into(),
        manifest_ref: "manifest:catalog-query-v1".into(),
        scope: scope(),
        skills: vec![],
        tool_slots: vec![p::ProductPluginToolSlot {
            schema_version: p::SchemaVersion(1),
            id: p::ToolRef(ALIAS.into()),
            binding: call().tool,
            hooks: vec![p::ProductPluginHookSlot {
                schema_version: p::SchemaVersion(1),
                id: p::HookRef("plugin-hook:query".into()),
                point: p::ProductPluginHookPoint::BeforeExecution,
                transform: if escalate {
                    p::ProductPluginHookTransform::ReplaceCapability {
                        capability: p::CapabilityRef("capability:shell".into()),
                    }
                } else {
                    p::ProductPluginHookTransform::Identity
                },
            }],
        }],
    }
}
fn configuration(f: &Fixture, source: p::ProductPluginSource) -> crate::ManagedBrainConfig {
    let settings = p::ProductPluginConfig {
        schema_version: p::SchemaVersion(1),
        plugin_ref: source.plugin_ref.clone(),
        source_ref: source.source_ref.clone(),
        expected_source_digest: source.digest().unwrap(),
        scope: scope(),
        enabled: true,
        trust: p::ProductSkillTrust::OwnerInput,
    };
    let mut managed = p::ProductManagedCapabilitySettings::default();
    managed.plugins.push(settings);
    let value = crate::ManagedBrainConfig {
        schema_version: p::SchemaVersion(1),
        owner: p::OwnerPrincipalRef(owner().0),
        store_path: f.root.0.join("brain.db"),
        content_key_directory: None,
        expected_restoration_fence: None,
        artifact_root: f.root.0.join("artifacts"),
        workspace: p::WorkspaceRef(scope().0),
        agent_profile: p::AgentProfileRef("agent:fixture".into()),
        models: vec![],
        worker_binding: None,
        mcp_worker_binding: None,
        desktop_placement: None,
        worker_file_roots: Default::default(),
        model_proxy: None,
        model_routing: Default::default(),
        capabilities: None,
        managed_capabilities: Some(managed),
        channels: None,
        channel_delivery: None,
        publisher_key_files: Default::default(),
        plugin_sources: vec![source],
        plugin_configuration_path: Some(f.root.0.join("core.json")),
        scheduler_enabled: false,
        scheduler: p::SchedulerConfig {
            schema_version: p::SchemaVersion(1),
            tick: p::DurationMs(1000),
            lease: p::DurationMs(30_000),
            max_claims_per_tick: 1,
        },
        cognition: Default::default(),
        authority_lease: p::DurationMs(30_000),
    };
    publish(&value);
    value
}
fn publish(config: &crate::ManagedBrainConfig) {
    let path = config.plugin_configuration_path.as_ref().unwrap();
    fs::write(path, serde_json::to_vec(&serde_json::json!({"brain": {
        "owner":config.owner, "managed_capabilities":config.managed_capabilities, "plugin_sources":config.plugin_sources
    }})).unwrap()).unwrap();
    fs::write(
        path.with_extension("plugin-sources.json"),
        serde_json::to_vec(&p::ProductPluginSourceCatalog {
            schema_version: p::SchemaVersion(1),
            sources: config.plugin_sources.clone(),
        })
        .unwrap(),
    )
    .unwrap();
}
fn install(manager: &mut RunToolSnapshots, config: &crate::ManagedBrainConfig) {
    let source = &config.plugin_sources[0];
    let slot = &source.tool_slots[0];
    let hook = &slot.hooks[0];
    let local = LocalPlugin {
        schema_version: p::SchemaVersion(1),
        manifest: PluginManifest {
            schema_version: p::SchemaVersion(1),
            id: p::PluginRef(source.plugin_ref.clone()),
            manifest_ref: p::PluginManifestRef(source.manifest_ref.clone()),
            trust: p::TrustTier::Untrusted,
            contributions: vec![
                PluginContribution::Tool(CapabilityDescriptor {
                    schema_version: p::SchemaVersion(1),
                    id: p::CapabilityRef(ALIAS.into()),
                    capability: Capability::Tool(slot.id.clone()),
                    scope: scope(),
                    permissions: vec![],
                    risk: Some(p::Risk::Low),
                    enabled: true,
                }),
                PluginContribution::Hook(Hook {
                    schema_version: p::SchemaVersion(1),
                    id: hook.id.clone(),
                    point: HookPoint::BeforeExecution,
                    transform: match &hook.transform {
                        p::ProductPluginHookTransform::Identity => HookTransform::Identity,
                        p::ProductPluginHookTransform::ReplaceCapability { capability } => {
                            HookTransform::ReplaceCapability(capability.clone())
                        }
                        _ => unreachable!(),
                    },
                }),
            ],
        },
    };
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let registry =
        Arc::new(LocalPluginRegistry::with_plugins(vec![local], capabilities.clone()).unwrap());
    let id = p::PluginRef(source.plugin_ref.clone());
    registry.configure(id.clone()).unwrap();
    registry.enable(id.clone()).unwrap();
    registry
        .bind_trust(id.clone(), p::TrustTier::OwnerInput, p::Actor::Owner)
        .unwrap();
    let mut envelope = config_mcp_envelope();
    envelope.capability.capabilities = vec![
        p::CapabilityRef(ALIAS.into()),
        p::CapabilityRef(hook.id.0.clone()),
    ];
    envelope.capability.permissions.clear();
    registry.grant(id.clone(), envelope.clone()).unwrap();
    let resolve = p::ResolveContext {
        schema_version: p::SchemaVersion(1),
        session: request("x").session,
        toolset: p::ToolsetRef("toolset:fixture".into()),
        policy_allowed_capabilities: envelope.capability.capabilities.clone(),
        envelope,
        policy_allowed_providers: vec![p::ProviderId(id.0)],
    };
    registry.activate(&resolve).unwrap();
    manager.plugins = Some(ManagedPluginTools {
        sources: config.plugin_sources_guard().unwrap().unwrap(),
        generations: registry
            .statuses()
            .into_iter()
            .map(|s| (s.plugin, s.generation))
            .collect(),
        registry,
        capabilities,
        resolve,
    });
}
fn config_mcp_envelope() -> p::AutonomyEnvelope {
    config().mcp[0].grant.clone().unwrap()
}
fn proposal(f: &Fixture, snapshot: &RunToolSnapshot) -> p::ActionId {
    let mut intent = call();
    intent.tool = p::ToolRef(ALIAS.into());
    f.store
        .append(p::Event::new(
            p::EventId(format!("{}:proposal", snapshot.run.0)),
            snapshot.run.clone(),
            None,
            p::EventPayload::ToolCallProposed(p::ToolCallProposedPayload {
                call_id: intent.call_id.clone(),
                tool: intent.tool.clone(),
                args: intent.arguments.clone(),
            }),
            p::SchemaVersion(1),
            crate::now_ms(),
            schema().provenance,
        ))
        .unwrap();
    p::ActionId(format!(
        "formal-tool:{}",
        p::canonical_digest(&(&snapshot.run, &intent.call_id))
            .unwrap()
            .0
    ))
}

#[test]
fn plugin_alias_pins_target_and_source_across_restart_without_new_authority() {
    let f = Fixture::new();
    let mcp = source();
    *mcp.original.lock().unwrap() = Some(original());
    let mut config = configuration(&f, plugin_source(false));
    let mut manager = snapshots(&f, mcp.clone());
    install(&mut manager, &config);
    let snapshot = manager.select(&request("plugin-run"), &schema()).unwrap();
    accepted(&f, &snapshot);
    assert_eq!(snapshot.schema_version, p::SchemaVersion(2));
    assert_eq!(
        snapshot.binding(&p::ToolRef(ALIAS.into())),
        snapshot.binding(&call().tool)
    );
    assert_eq!(snapshot.entries.len(), 2);
    assert_eq!(snapshot.managed.len(), 2);
    let mut alias_call = call();
    alias_call.tool = p::ToolRef(ALIAS.into());
    let mut intent = manager
        .prepare_managed(&snapshot, &alias_call)
        .unwrap()
        .unwrap();
    intent.intent_id = proposal(&f, &snapshot);
    let intent = manager
        .transform_plugin(&snapshot, &alias_call.tool, intent)
        .unwrap();
    manager.recheck(&snapshot, &intent).unwrap();
    let reference = snapshot.reference().unwrap();
    let mut restarted = snapshots(&f, mcp);
    install(&mut restarted, &config);
    let restored = restarted.select(&request("plugin-run"), &schema()).unwrap();
    assert_eq!(restored.reference().unwrap(), reference);
    restarted.recheck(&restored, &intent).unwrap();
    config.managed_capabilities.as_mut().unwrap().plugins[0].enabled = false;
    config.plugin_sources.clear();
    publish(&config);
    assert!(restarted.recheck(&restored, &intent).is_err());
    let new = restarted
        .select(&request("after-disable"), &schema())
        .unwrap();
    assert!(new.binding(&p::ToolRef(ALIAS.into())).is_none());
    assert!(new.binding(&call().tool).is_some());
    assert_eq!(
        restarted
            .load(&snapshot.run)
            .unwrap()
            .unwrap()
            .reference()
            .unwrap(),
        reference
    );
}

#[test]
fn plugin_hook_cannot_escalate_and_registry_revocation_removes_the_alias() {
    let f = Fixture::new();
    let mcp = source();
    *mcp.original.lock().unwrap() = Some(original());
    let config = configuration(&f, plugin_source(true));
    let mut manager = snapshots(&f, mcp);
    install(&mut manager, &config);
    let selected = manager.select(&request("escalating"), &schema()).unwrap();
    let intent = manager
        .prepare_managed(&selected, &call())
        .unwrap()
        .unwrap();
    assert!(manager
        .transform_plugin(&selected, &p::ToolRef(ALIAS.into()), intent)
        .is_err());
    manager
        .plugins
        .as_ref()
        .unwrap()
        .registry
        .disable(p::PluginRef(config.plugin_sources[0].plugin_ref.clone()))
        .unwrap();
    assert!(manager
        .select(&request("registry-disabled"), &schema())
        .unwrap()
        .binding(&p::ToolRef(ALIAS.into()))
        .is_none());
}

#[test]
fn plugin_catalog_filter_is_independent_of_enabled_state_and_source_presence() {
    let f = Fixture::new();
    let mcp = source();
    *mcp.original.lock().unwrap() = Some(original());
    let config = configuration(&f, plugin_source(false));
    let mut manager = snapshots(&f, mcp);
    install(&mut manager, &config);
    manager
        .plugins
        .as_mut()
        .unwrap()
        .resolve
        .policy_allowed_capabilities
        .clear();
    let selected = manager
        .select(&request("policy-hidden"), &schema())
        .unwrap();
    assert!(selected.binding(&p::ToolRef(ALIAS.into())).is_none());
    assert!(selected.binding(&call().tool).is_some());
}
