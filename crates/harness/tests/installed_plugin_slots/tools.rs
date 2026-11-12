//! Managed composition with a real model transport; no installed worker is claimed.
use super::*;
use forme_execution::installed::{ManagedWorkerBinding, ManagedWorkerEndpoint};

const SLOT: &str = "plugin-tool:bounded-file";
fn with_tools(f: &Fixture) -> ManagedBrainConfig {
    let mut value = config(f);
    value.models[0].profile.tool_use = true;
    value.models[0].profile.tool_schema_versions = vec![1];
    let root = f.config.store_path.with_file_name("work-files");
    fs::create_dir(&root).unwrap();
    let credential = f.config.store_path.with_extension("agent-key");
    fs::write(&credential, b"local-fixture-worker-credential-32-bytes").unwrap();
    value.worker_binding = Some(ManagedWorkerBinding {
        schema_version: p::SchemaVersion(1),
        #[cfg(windows)]
        endpoint: ManagedWorkerEndpoint::WindowsNamedPipe {
            name: format!(
                r"\\.\pipe\forme-agent-execution-v1-plugin-fixture-{}",
                std::process::id()
            ),
        },
        #[cfg(not(windows))]
        endpoint: ManagedWorkerEndpoint::LoopbackTcp {
            address: "127.0.0.1:9".parse().unwrap(),
        },
        credential_path: credential,
        authority_epoch: p::AuthorityEpoch(1),
        sandbox_profile_ref: "sandbox:plugin-fixture".into(),
        backend_kinds: [p::BackendKind::File].into_iter().collect(),
    });
    value
        .worker_file_roots
        .insert(value.workspace.0.clone(), root);
    value.plugin_sources[0].schema_version = p::SchemaVersion(2);
    value.plugin_sources[0].tool_slots = vec![
        p::ProductPluginToolSlot {
            schema_version: p::SchemaVersion(1),
            id: p::ToolRef(SLOT.into()),
            binding: p::ToolRef("capability:file".into()),
            hooks: vec![p::ProductPluginHookSlot {
                schema_version: p::SchemaVersion(1),
                id: p::HookRef("plugin-hook:check-file".into()),
                point: p::ProductPluginHookPoint::BeforeExecution,
                transform: p::ProductPluginHookTransform::Identity,
            }],
        },
        p::ProductPluginToolSlot {
            schema_version: p::SchemaVersion(1),
            id: p::ToolRef("plugin-tool:missing-target".into()),
            binding: p::ToolRef("capability:unconfigured".into()),
            hooks: vec![],
        },
    ];
    republish(&mut value);
    value
}
fn republish(config: &mut ManagedBrainConfig) {
    config.managed_capabilities.as_mut().unwrap().plugins[0].expected_source_digest =
        config.plugin_sources[0].digest().unwrap();
    publish(config);
}
#[test]
fn enabled_tool_slots_without_a_managed_worker_are_not_exposed_to_the_model() {
    let f = Fixture::new();
    let mut config = with_tools(&f);
    config.worker_binding = None;
    config.worker_file_roots.clear();
    publish(&config);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config)
        .unwrap()
        .build()
        .unwrap();
    turn(&runtime, "no-worker", "Describe available methods");
    let requests = f.requests.lock().unwrap();
    let body = requests.last().unwrap().split_once("\r\n\r\n").unwrap().1;
    let payload: serde_json::Value = serde_json::from_str(body).unwrap();
    let alias = forme_models::ToolDefinition::catalog_alias(SLOT).unwrap();
    assert!(!payload["tools"].to_string().contains(&alias));
    assert!(!runtime.config().capabilities.plugins.is_empty());
}
