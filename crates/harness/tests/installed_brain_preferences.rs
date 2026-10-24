//! Real managed composition and loopback provider evidence, not native IP acceptance.
#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod support;
use forme_cognition::{self as c, MemoryGraph};
use forme_harness::*;
use forme_protocol as p;
use forme_store::{BrainAuthorityStore, EventStore, SqliteEventStore, StoreOptions};
use std::{fs, sync::atomic::Ordering};
use support::{events, now, Fixture};

fn managed(f: &Fixture) -> ManagedBrainConfig {
    let key = f.config.store_path.with_file_name("managed-model.key");
    fs::write(&key, "managed-preference-fixture-key").unwrap();
    let profiles = ["default", "foreground", "background"].map(|name| {
        serde_json::json!({
            "schema_version":1, "credential_path":key,
            "profile":{"id":format!("model:{name}"), "provider":"managed", "model":name,
                "base_url":f.config.models[0].base_url,"credential_env":"NEVER_RESOLVE_MANAGED_ENV",
                "context_window":32768,"tool_use":false,"strength":"standard",
                "requests_per_minute":600,"timeout_ms":5000,"default":name == "default"}
        })
    });
    ManagedBrainConfig {
        schema_version: p::SchemaVersion(1),
        owner: f.config.owner.clone(),
        store_path: f.config.store_path.clone(),
        content_key_directory: None,
        expected_restoration_fence: None,
        artifact_root: f.config.data.artifact_root.clone(),
        workspace: p::WorkspaceRef("workspace:managed".into()),
        agent_profile: f.config.agent_profile.clone(),
        models: serde_json::from_value(serde_json::json!(profiles)).unwrap(),
        model_routing: p::ProductModelRouting {
            schema_version: p::SchemaVersion(1),
            foreground: Some(p::ModelProfileRef("model:foreground".into())),
            cognition: Some(p::ModelProfileRef("model:background".into())),
        },
        scheduler_enabled: false,
        scheduler: f.config.scheduler.clone(),
        cognition: f.config.cognition.clone(),
        authority_lease: f.config.authority_lease,
    }
}

fn turn(runtime: &ProductionRuntime, key: &str) -> p::RunId {
    let run = runtime
        .harness()
        .submit_run(p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::UserTurn,
            session: p::SessionRef(format!("session:{key}")),
            agent_profile: runtime.config().agent_profile.clone(),
            input: p::RunInput("Tell me about this workspace".into()),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey(key.into())),
        })
        .unwrap();
    runtime.harness().wait(run.clone()).unwrap();
    run
}

fn provider_model(f: &Fixture, index: usize) -> String {
    let requests = f.requests.lock().unwrap();
    let body = requests[index].split_once("\r\n\r\n").unwrap().1;
    serde_json::from_str::<serde_json::Value>(body).unwrap()["model"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn managed_preferences_drive_formal_runs_cognition_and_restart_without_env_fallback() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "A managed workspace answer".into();
    let config = managed(&f);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        f.calls.load(Ordering::SeqCst),
        0,
        "reading and assembling config does not call a model"
    );
    let identity = runtime.services().authority.store_identity.clone();
    let run = turn(&runtime, "managed-first");
    let all = runtime
        .services()
        .store
        .read_run(run)
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert!(all.iter().any(|event| matches!(&event.payload,
        p::EventPayload::SessionBound(binding) if binding.workspace.0 == "workspace:managed")));
    assert!(all.iter().any(|event| matches!(&event.payload,
        p::EventPayload::ModelCallStarted(call) if call.model_profile.0 == "managed:foreground")));
    assert_eq!(provider_model(&f, 0), "foreground");
    assert!(f.requests.lock().unwrap()[0].contains("managed-preference-fixture-key"));
    let scope = c::MemoryScope {
        schema_version: p::SchemaVersion(1),
        slice: c::TimeSlice::LongTerm,
        workspace: Some(config.workspace.clone()),
        scope: p::Scope(config.workspace.0.clone()),
    };
    for (id, kind) in [("topic", c::NodeKind::Fact), ("useful", c::NodeKind::Goal)] {
        runtime
            .services()
            .memory
            .add_node(c::MemoryNode {
                schema_version: p::SchemaVersion(1),
                id: p::NodeId(id.into()),
                kind,
                content_ref: p::ContentRef(format!("reference:{id}")),
                tier: p::StabilityTier::Working,
                confidence: p::Confidence(0.7),
                scope: scope.clone(),
                resting_activation: p::RestingActivation(0.0),
                recency: now(),
                provenance: p::Provenance {
                    source: p::Source::Internal,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::VerifiedProcess,
                    caused_by: None,
                },
            })
            .unwrap();
    }
    *f.answer.lock().unwrap() = "[\"useful\"]".into();
    let before = f.calls.load(Ordering::SeqCst);
    let report = runtime
        .compare_relevance(
            &f.owner(),
            RelevanceCase {
                schema_version: p::SchemaVersion(1),
                case_id: "managed-role-relevance".into(),
                cue: c::RecallCue {
                    schema_version: p::SchemaVersion(1),
                    scope,
                    query: "Find a useful goal".into(),
                },
                seeds: vec![p::NodeId("topic".into())],
                shape: c::ActivationShape::Association,
                k: 1,
                graph_version: None,
                ground_truth: vec![p::NodeId("useful".into())],
                label_source: p::RelevanceLabelSource::IndependentFixture,
                evidence_level: p::RelevanceEvidenceLevel::ControlledHttp,
                at: now(),
            },
        )
        .unwrap();
    assert_eq!(report.model_profile.0, "managed:background");
    assert_eq!(provider_model(&f, before as usize), "background");
    assert_eq!(f.calls.load(Ordering::SeqCst), before + 1);
    let before = f.calls.load(Ordering::SeqCst);
    let CognitiveTickOutcome::Ran(tick) = runtime.cognitive_tick_at(now()).unwrap() else {
        panic!("expected cognitive tick")
    };
    assert!(!tick.failed());
    assert!(tick.phases[0].model_calls > 0);
    for index in before..f.calls.load(Ordering::SeqCst) {
        assert_eq!(provider_model(&f, index as usize), "background");
    }
    assert!(!events(&runtime)
        .iter()
        .any(|event| event.kind == p::EventKind::ActionStarted));
    drop(runtime);
    *f.answer.lock().unwrap() = "A restarted managed answer".into();
    let before = f.calls.load(Ordering::SeqCst);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.services().authority.store_identity, identity);
    assert_eq!(f.calls.load(Ordering::SeqCst), before);
    turn(&runtime, "managed-restarted");
    assert_eq!(provider_model(&f, before as usize), "foreground");
    drop(runtime);
    let mut unavailable = config;
    unavailable.models[2].credential_path = unavailable.store_path.with_file_name("absent.key");
    let before = f.calls.load(Ordering::SeqCst);
    assert!(ProductionRuntimeBuilder::from_managed_config(unavailable)
        .unwrap()
        .build()
        .is_err());
    assert_eq!(
        f.calls.load(Ordering::SeqCst),
        before,
        "missing routed credential cannot call a fallback"
    );
}

#[test]
fn managed_store_entry_shares_composition_and_cannot_create_a_second_or_foreign_brain() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "A durable managed composition answer".into();
    let config = managed(&f);
    let store = SqliteEventStore::open(&config.store_path, StoreOptions::default()).unwrap();
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build_with_store(store.clone())
        .unwrap();
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    let manifest = runtime.manifest().clone();
    let first_run = turn(&runtime, "managed-store-entry");
    let first_events = runtime
        .services()
        .store
        .read_run(first_run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert!(first_events.iter().any(|event| matches!(&event.payload,
        p::EventPayload::SessionBound(binding)
            if binding.workspace == config.workspace
                && binding.composition_manifest.as_ref() == Some(&manifest.digest))));
    assert_eq!(provider_model(&f, 0), "foreground");
    assert_eq!(
        runtime.services().model_registry.cognition_ref().0,
        "model:background"
    );
    let calls = f.calls.load(Ordering::SeqCst);
    let event_count = events(&runtime).len();
    assert!(
        ProductionRuntimeBuilder::from_managed_config(config.clone())
            .unwrap()
            .build()
            .is_err()
    );
    assert!(
        ProductionRuntimeBuilder::from_managed_config(config.clone())
            .unwrap()
            .build_with_store(store.clone())
            .is_err()
    );
    let foreign = SqliteEventStore::open(
        config.store_path.with_file_name("another-brain.db"),
        StoreOptions::default(),
    )
    .unwrap();
    assert!(
        ProductionRuntimeBuilder::from_managed_config(config.clone())
            .unwrap()
            .build_with_store(foreign.clone())
            .is_err()
    );
    assert!(foreign.brain_authority().unwrap().is_none());
    assert!(foreign.run_ids().unwrap().is_empty());
    assert_eq!(events(&runtime).len(), event_count);
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    drop(runtime);
    drop(store);
    drop(foreign);

    let runtime = ProductionRuntimeBuilder::from_managed_config(config)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.manifest().store_identity, manifest.store_identity);
    assert_eq!(runtime.manifest().aggregate_refs, manifest.aggregate_refs);
    assert_eq!(runtime.manifest().digest, manifest.digest);
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    turn(&runtime, "managed-store-restart");
    assert_eq!(provider_model(&f, calls as usize), "foreground");
}

#[test]
fn changing_only_model_roles_rebinds_composition_without_rewriting_history_or_calling_on_load() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "A model-role composition answer".into();
    let mut config = managed(&f);
    config.model_routing = p::ProductModelRouting::default();
    // Make the legacy compatible choice unambiguous, without any role hint.
    config.models[1].profile.input_microunits_per_million = 2;
    config.models[2].profile.input_microunits_per_million = 2;
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    let legacy = runtime.manifest().clone();
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    let original_run = turn(&runtime, "roles-legacy");
    assert_eq!(provider_model(&f, 0), "default");
    let original_events = runtime
        .services()
        .store
        .read_run(original_run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    drop(runtime);

    config.model_routing.foreground = Some(p::ModelProfileRef("model:foreground".into()));
    let calls = f.calls.load(Ordering::SeqCst);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    let foreground = runtime.manifest().clone();
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    assert_ne!(foreground.digest, legacy.digest);
    assert_eq!(foreground.model_profiles, legacy.model_profiles);
    assert_eq!(foreground.service_states, legacy.service_states);
    assert_eq!(foreground.aggregate_refs, legacy.aggregate_refs);
    assert_eq!(foreground.store_identity, legacy.store_identity);
    let run = turn(&runtime, "roles-foreground");
    assert_eq!(provider_model(&f, calls as usize), "foreground");
    assert!(runtime
        .services()
        .store
        .read_run(run)
        .map(Result::unwrap)
        .any(
            |event| matches!(&event.payload, p::EventPayload::SessionBound(binding)
            if binding.composition_manifest.as_ref() == Some(&foreground.digest))
        ));
    drop(runtime);

    // Rotating both the credential file path and its value is not a change in
    // routing semantics. The actual provider must nevertheless consume it.
    let rotated = config.store_path.with_file_name("rotated-model.key");
    fs::write(&rotated, "rotated-managed-fixture-key").unwrap();
    for model in &mut config.models {
        model.credential_path = rotated.clone();
        model.profile.credential_env = "ROTATED_UNREAD_ENV".into();
    }
    let calls = f.calls.load(Ordering::SeqCst);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.manifest().digest, foreground.digest);
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    turn(&runtime, "roles-rotated");
    assert_eq!(provider_model(&f, calls as usize), "foreground");
    assert!(f.requests.lock().unwrap()[calls as usize].contains("rotated-managed-fixture-key"));
    drop(runtime);

    config.model_routing.cognition = Some(p::ModelProfileRef("model:background".into()));
    let calls = f.calls.load(Ordering::SeqCst);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    let background = runtime.manifest().clone();
    assert_ne!(background.digest, foreground.digest);
    assert_eq!(background.model_profiles, foreground.model_profiles);
    assert_eq!(background.service_states, foreground.service_states);
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    assert_eq!(
        runtime.services().model_registry.cognition_ref().0,
        "model:background"
    );
    assert_eq!(
        runtime
            .services()
            .store
            .read_run(original_run.clone())
            .collect::<p::Result<Vec<_>>>()
            .unwrap(),
        original_events
    );
    drop(runtime);

    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.manifest().digest, background.digest);
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    drop(runtime);

    config.models[1].profile.model = "foreground-v2".into();
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_ne!(runtime.manifest().digest, background.digest);
    assert_eq!(runtime.manifest().model_profiles, background.model_profiles);
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    turn(&runtime, "roles-actual-model-change");
    assert_eq!(provider_model(&f, calls as usize), "foreground-v2");
    drop(runtime);

    config.models[1].profile.model = "foreground".into();
    config.model_routing = p::ProductModelRouting::default();
    let calls = f.calls.load(Ordering::SeqCst);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.manifest().digest, legacy.digest);
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    assert_eq!(
        runtime
            .services()
            .store
            .read_run(original_run)
            .collect::<p::Result<Vec<_>>>()
            .unwrap(),
        original_events
    );
}
