//! Real managed composition and loopback provider evidence, not native IP acceptance.
#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod support;
use forme_cognition::{self as c, MemoryGraph};
use forme_harness::*;
use forme_protocol as p;
use forme_store::EventStore;
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
