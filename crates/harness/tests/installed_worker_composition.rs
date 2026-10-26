//! Managed placement is explicit and never falls back to a brain-process tool.
#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod support;

use std::{fs, sync::atomic::Ordering};

use forme_execution::installed::{ManagedWorkerBinding, ManagedWorkerEndpoint};
use forme_harness::*;
use forme_protocol as p;
use forme_store::EventStore;
use support::Fixture;

fn managed(f: &Fixture) -> ManagedBrainConfig {
    let key = f
        .config
        .store_path
        .with_file_name("managed-worker-model.secret");
    fs::write(&key, "repository-owned-worker-model-fixture-credential").unwrap();
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
        model_proxy: None,
        model_routing: Default::default(),
        capabilities: None,
        scheduler_enabled: false,
        scheduler: f.config.scheduler.clone(),
        cognition: f.config.cognition.clone(),
        authority_lease: f.config.authority_lease,
    }
}

fn binding(f: &Fixture) -> ManagedWorkerBinding {
    ManagedWorkerBinding {
        schema_version: p::SchemaVersion(1),
        endpoint: ManagedWorkerEndpoint::LoopbackTcp {
            address: "127.0.0.1:9".parse().unwrap(),
        },
        credential_path: f.config.store_path.with_file_name("missing-agent.token"),
        authority_epoch: p::AuthorityEpoch(1),
        sandbox_profile_ref: "sandbox:composition".into(),
        backend_kinds: [p::BackendKind::Notification].into_iter().collect(),
    }
}

#[test]
fn managed_missing_or_invalid_worker_stays_blocked_without_local_fallback() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "Managed composition fixture response".into();
    let mut config = managed(&f);
    for invalid in [None, Some(binding(&f))] {
        config.worker_binding = invalid;
        let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        for kind in [
            p::BackendKind::Notification,
            p::BackendKind::File,
            p::BackendKind::Shell,
            p::BackendKind::Mcp,
            p::BackendKind::AppApi,
            p::BackendKind::Pty,
            p::BackendKind::Browser,
            p::BackendKind::Computer,
        ] {
            assert!(
                runtime.services().backends.backend(kind).is_err(),
                "{kind:?}"
            );
        }
        assert!(runtime
            .manifest()
            .service_states
            .iter()
            .any(|state| state.name == "notification-backend"
                && state.status == RuntimeServiceStatus::Blocking));
        drop(runtime);
    }
    let runtime = ProductionRuntimeBuilder::from_managed_config(managed(&f))
        .unwrap()
        .build()
        .unwrap();
    let run = runtime
        .harness()
        .submit_run(p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::UserTurn,
            session: p::SessionRef("session:managed-worker-dialogue".into()),
            agent_profile: runtime.config().agent_profile.clone(),
            input: p::RunInput("fixture dialogue".into()),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey("managed-worker-dialogue".into())),
        })
        .unwrap();
    runtime.harness().wait(run.clone()).unwrap();
    let events = runtime
        .services()
        .store
        .read_run(run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert!(events
        .iter()
        .any(|event| event.kind == p::EventKind::ModelCallStarted));
    assert!(!events.iter().any(|event| matches!(
        event.kind,
        p::EventKind::ActionStarted
            | p::EventKind::ActionCompleted
            | p::EventKind::ActionOutputDelta
    )));
}

#[test]
fn managed_worker_placement_is_composition_bound_stable_and_secret_free() {
    let f = Fixture::new();
    let mut config = managed(&f);
    let original = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    let legacy = original.manifest().clone();
    drop(original);
    config.worker_binding = Some(binding(&f));
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    let bound = runtime.manifest().clone();
    assert_eq!(bound.store_identity, legacy.store_identity);
    assert_eq!(bound.aggregate_refs, legacy.aggregate_refs);
    assert_ne!(bound.digest, legacy.digest);
    let public = format!("{bound:?}");
    assert!(!public.contains("missing-agent.token"));
    assert!(!public.contains("repository-owned-worker-model-fixture-credential"));
    drop(runtime);

    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.manifest().digest, bound.digest);
    drop(runtime);
    config.worker_binding.as_mut().unwrap().authority_epoch = p::AuthorityEpoch(2);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_ne!(runtime.manifest().digest, bound.digest);
    drop(runtime);
    config.worker_binding = None;
    let runtime = ProductionRuntimeBuilder::from_managed_config(config)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.manifest().digest, legacy.digest);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
