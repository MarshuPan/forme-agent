//! Real production hydration, HTTP model transport and persistent SQLite.
#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod support;
use forme_harness::*;
use forme_protocol as p;
use forme_store::EventStore;
use std::{fs, sync::atomic::Ordering};
use support::Fixture;
#[path = "installed_plugin_slots/tools.rs"]
mod tools;

const BODY: &str = "private-declarative-quantumorbits-plugin-method";
fn source(scope: &str) -> p::ProductPluginSource {
    serde_json::from_value(
        serde_json::json!({"schema_version":1,"source_ref":"source:methods",
        "plugin_ref":"plugin:methods","manifest_ref":"manifest:methods-v1","scope":scope,
        "skills":[{"schema_version":1,"id":"skill:quantumorbits","summary":"quantumorbits",
            "scope":scope,"version":1,"body":BODY,"enabled":false,"trust":"untrusted"}]}),
    )
    .unwrap()
}
fn config(f: &Fixture) -> ManagedBrainConfig {
    let credential = f.config.store_path.with_extension("model-key");
    fs::write(&credential, b"local-model-fixture").unwrap();
    let source = source(&f.config.workspace.0);
    let mut managed = p::ProductManagedCapabilitySettings::default();
    managed.plugins.push(p::ProductPluginConfig {
        schema_version: p::SchemaVersion(1),
        plugin_ref: source.plugin_ref.clone(),
        source_ref: source.source_ref.clone(),
        expected_source_digest: source.digest().unwrap(),
        scope: source.scope.clone(),
        enabled: true,
        trust: p::ProductSkillTrust::OwnerInput,
    });
    let config = ManagedBrainConfig {
        schema_version: p::SchemaVersion(1),
        owner: f.config.owner.clone(),
        store_path: f.config.store_path.clone(),
        content_key_directory: None,
        expected_restoration_fence: None,
        artifact_root: f.config.data.artifact_root.clone(),
        workspace: f.config.workspace.clone(),
        agent_profile: f.config.agent_profile.clone(),
        models: vec![ManagedModelProfile {
            schema_version: p::SchemaVersion(1),
            profile: f.config.models[0].clone(),
            credential_path: credential,
        }],
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
        plugin_configuration_path: Some(f.config.store_path.with_extension("core.json")),
        scheduler_enabled: false,
        scheduler: f.config.scheduler.clone(),
        cognition: f.config.cognition.clone(),
        authority_lease: f.config.authority_lease,
    };
    publish(&config);
    config
}
fn publish(config: &ManagedBrainConfig) {
    let path = config.plugin_configuration_path.as_ref().unwrap();
    fs::write(path,serde_json::to_vec(&serde_json::json!({"brain":{"owner":config.owner,"managed_capabilities":config.managed_capabilities,"plugin_sources":config.plugin_sources}})).unwrap()).unwrap();
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
fn turn(runtime: &ProductionRuntime, key: &str, text: &str) -> Vec<p::Event> {
    let run = runtime
        .harness()
        .submit_run(p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::UserTurn,
            session: p::SessionRef(format!("session:{key}")),
            agent_profile: runtime.config().agent_profile.clone(),
            input: p::RunInput(text.into()),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey(key.into())),
        })
        .unwrap();
    runtime.harness().wait(run.clone()).unwrap();
    runtime
        .services()
        .store
        .read_run(run)
        .map(Result::unwrap)
        .collect()
}
#[test]
fn installed_plugin_body_is_lazy_and_live_disable_fences_further_hydration() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "Local fixture response".into();
    let mut config = config(&f);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    let identity = runtime.services().authority.store_identity.clone();
    let unused = turn(&runtime, "unused", "walkoutside");
    assert!(!unused
        .iter()
        .any(|e| e.kind == p::EventKind::SkillBodyLoaded));
    let selected = turn(&runtime, "selected", "quantumorbits");
    assert!(selected.iter().any(|e|matches!(&e.payload,p::EventPayload::SkillBodyLoaded(v) if v.skill.0=="skill:quantumorbits")));
    assert!(f.requests.lock().unwrap().last().unwrap().contains(BODY));
    assert!(!selected
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
    config.managed_capabilities.as_mut().unwrap().plugins[0].enabled = false;
    config.plugin_sources.clear();
    publish(&config);
    let disabled = turn(&runtime, "disabled", "quantumorbits");
    assert!(!disabled
        .iter()
        .any(|e| e.kind == p::EventKind::SkillBodyLoaded));
    assert!(!f.requests.lock().unwrap().last().unwrap().contains(BODY));
    drop(runtime);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.services().authority.store_identity, identity);
    assert!(runtime.config().capabilities.plugins.is_empty());
}
#[test]
fn installed_plugin_rejects_substituted_source_self_trust_or_duplicate_skill_before_model() {
    let f = Fixture::new();
    let original = config(&f);
    for case in 0..4 {
        let mut bad = original.clone();
        match case {
            0 => bad.plugin_sources[0].skills[0].body.push_str("changed"),
            1 => {
                bad.managed_capabilities.as_mut().unwrap().plugins[0].trust =
                    p::ProductSkillTrust::Untrusted
            }
            2 => {
                bad.capabilities = Some(p::ProductCapabilitySettings {
                    schema_version: p::SchemaVersion(1),
                    skills: bad.plugin_sources[0].skills.clone(),
                })
            }
            _ => {
                bad.plugin_configuration_path =
                    Some(f.config.data.artifact_root.join("forged.json"))
            }
        }
        assert!(ProductionRuntimeBuilder::from_managed_config(bad).is_err());
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }
    let mut replacement = source(&f.config.workspace.0);
    replacement.skills[0].body.push_str("replaced");
    fs::write(
        original
            .plugin_configuration_path
            .as_ref()
            .unwrap()
            .with_extension("plugin-sources.json"),
        serde_json::to_vec(&p::ProductPluginSourceCatalog {
            schema_version: p::SchemaVersion(1),
            sources: vec![replacement],
        })
        .unwrap(),
    )
    .unwrap();
    assert!(ProductionRuntimeBuilder::from_managed_config(original).is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
