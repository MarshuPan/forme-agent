//! The managed configuration feeds the existing production skill/context path.
#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod support;
use forme_harness::*;
use forme_protocol as p;
use forme_store::EventStore;
use std::{fs, sync::atomic::Ordering};
use support::Fixture;

const BODY: &str = "managed-skill-private-fixture-body-64";

fn settings(scope: &str) -> p::ProductCapabilitySettings {
    serde_json::from_value(serde_json::json!({"schema_version":1,"skills":[{
        "schema_version":1,"id":"skill:quantumorbits","summary":"quantumorbits",
        "scope":scope,"version":1,"body":BODY,"enabled":true,"trust":"owner_input"
    }]}))
    .unwrap()
}

fn managed(f: &Fixture) -> ManagedBrainConfig {
    let key = f
        .config
        .store_path
        .with_file_name("managed-skills-model.secret");
    fs::write(&key, "managed-skills-http-fixture-credential").unwrap();
    ManagedBrainConfig {
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
            credential_path: key,
        }],
        worker_binding: None,
        worker_file_roots: Default::default(),
        model_proxy: None,
        model_routing: Default::default(),
        capabilities: Some(settings(&f.config.workspace.0)),
        scheduler_enabled: false,
        scheduler: f.config.scheduler.clone(),
        cognition: f.config.cognition.clone(),
        authority_lease: f.config.authority_lease,
    }
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
fn managed_skill_is_selected_only_when_trusted_enabled_scoped_and_relevant() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "A managed skill fixture answer".into();
    let mut config = managed(&f);
    // Saving an enabled but untrusted entry cannot silently promote trust.
    config.capabilities.as_mut().unwrap().skills[0].trust = p::ProductSkillTrust::Untrusted;
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    let identity = runtime.services().authority.store_identity.clone();
    let denied = turn(&runtime, "skills-untrusted", "quantumorbits");
    assert!(!denied
        .iter()
        .any(|e| e.kind == p::EventKind::SkillBodyLoaded));
    assert!(!f.requests.lock().unwrap()[0].contains(BODY));
    drop(runtime);
    config.capabilities.as_mut().unwrap().skills[0].trust = p::ProductSkillTrust::OwnerInput;
    config.capabilities.as_mut().unwrap().skills[0].enabled = false;
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    let denied = turn(&runtime, "skills-disabled", "quantumorbits");
    assert!(!denied
        .iter()
        .any(|e| e.kind == p::EventKind::SkillBodyLoaded));
    assert!(!f.requests.lock().unwrap().last().unwrap().contains(BODY));
    drop(runtime);
    config.capabilities.as_mut().unwrap().skills[0].enabled = true;
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.services().authority.store_identity, identity);
    let irrelevant = turn(&runtime, "skills-unselected", "walkoutside");
    assert!(!irrelevant
        .iter()
        .any(|e| e.kind == p::EventKind::SkillBodyLoaded));
    assert!(!f.requests.lock().unwrap().last().unwrap().contains(BODY));
    let selected = turn(&runtime, "skills-selected", "quantumorbits");
    assert!(selected
        .iter()
        .any(|e| matches!(&e.payload, p::EventPayload::SkillBodyLoaded(v)
        if v.skill.0 == "skill:quantumorbits" && v.trust == p::TrustTier::OwnerInput)));
    assert!(f.requests.lock().unwrap().last().unwrap().contains(BODY));
    assert!(!selected
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
    assert!(!runtime.config().capabilities.file.enabled);
    assert!(!runtime.config().capabilities.shell.enabled);
    assert!(runtime.config().capabilities.mcp.is_empty());
    drop(runtime);
    config.capabilities = Some(settings("workspace:other"));
    let runtime = ProductionRuntimeBuilder::from_managed_config(config)
        .unwrap()
        .build()
        .unwrap();
    let denied = turn(&runtime, "skills-foreign-scope", "quantumorbits");
    assert!(!denied
        .iter()
        .any(|e| e.kind == p::EventKind::SkillBodyLoaded));
}

#[test]
fn invalid_managed_skills_and_executable_settings_fail_before_provider_calls() {
    let f = Fixture::new();
    let mut config = managed(&f);
    config.capabilities.as_mut().unwrap().skills[0].body = " ".into();
    assert!(ProductionRuntimeBuilder::from_managed_config(config).is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    for extra in ["shell", "mcp", "plugins", "environment", "permission"] {
        let mut value = serde_json::to_value(settings(&f.config.workspace.0)).unwrap();
        value[extra] = serde_json::json!({"enabled":true});
        assert!(serde_json::from_value::<p::ProductCapabilitySettings>(value).is_err());
    }
}
