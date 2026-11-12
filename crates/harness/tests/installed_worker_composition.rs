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
        managed_capabilities: None,
        channels: None,
        channel_delivery: None,
        publisher_key_files: Default::default(),
        plugin_sources: Vec::new(),
        plugin_configuration_path: None,
        worker_binding: None,
        mcp_worker_binding: None,
        desktop_placement: None,
        worker_file_roots: Default::default(),
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

#[test]
fn installed_file_roots_are_scope_bound_rechecked_and_part_of_composition() {
    let f = Fixture::new();
    let mut config = managed(&f);
    let root = config
        .store_path
        .parent()
        .unwrap()
        .join("installed-workspace");
    fs::create_dir(&root).unwrap();
    let mut worker = binding(&f);
    worker.backend_kinds = [p::BackendKind::File].into_iter().collect();
    config.worker_binding = Some(worker);
    config
        .worker_file_roots
        .insert(config.workspace.0.clone(), root.clone());
    let runtime_config = config.runtime_config().unwrap();
    assert!(runtime_config.capabilities.file.enabled);
    assert_eq!(
        runtime_config.capabilities.file.roots,
        vec![root.to_string_lossy().replace('\\', "/")]
    );
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    let first = runtime.manifest().clone();
    assert!(!format!("{first:?}").contains("installed-workspace"));
    // A missing transport credential is never a reason to use FileBackend.
    assert!(runtime
        .services()
        .backends
        .backend(p::BackendKind::File)
        .is_err());
    drop(runtime);
    let next_root = root.with_file_name("installed-workspace-next");
    fs::create_dir(&next_root).unwrap();
    config
        .worker_file_roots
        .insert(config.workspace.0.clone(), next_root);
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_ne!(runtime.manifest().digest, first.digest);
    drop(runtime);
    let mut unknown = config.clone();
    unknown.workspace = p::WorkspaceRef("workspace:unregistered".into());
    assert!(!unknown.runtime_config().unwrap().capabilities.file.enabled);
    let mut unbound = config.clone();
    unbound.worker_binding = None;
    assert!(unbound.runtime_config().is_err());
    config
        .worker_binding
        .as_mut()
        .unwrap()
        .backend_kinds
        .clear();
    assert!(config.runtime_config().is_err());
    let mut unsafe_root = unknown;
    unsafe_root.worker_file_roots = [(
        unsafe_root.workspace.0.clone(),
        unsafe_root.store_path.parent().unwrap().to_owned(),
    )]
    .into_iter()
    .collect();
    assert!(unsafe_root.runtime_config().is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn installed_computer_requires_its_own_placement_and_matching_protected_configuration() {
    use forme_execution::installed::{INSTALLED_DESKTOP_PROFILE, INSTALLED_DESKTOP_SURFACE};
    let f = Fixture::new();
    let mut config = managed(&f);
    config.workspace = p::WorkspaceRef("workspace:default".into());
    let mut settings = p::ProductManagedCapabilitySettings::default();
    settings.computer.enabled = true;
    settings.computer.scope = p::Scope(config.workspace.0.clone());
    settings.computer.surface_refs = vec![INSTALLED_DESKTOP_SURFACE.into()];
    config.managed_capabilities = Some(settings);
    assert!(config.runtime_config().is_err());
    let mut file = binding(&f);
    file.backend_kinds.insert(p::BackendKind::Computer);
    config.worker_binding = Some(file);
    assert!(
        config.runtime_config().is_err(),
        "adding Computer to File must not supply desktop placement"
    );
    config.worker_binding = None;
    let projection = serde_json::json!({
        "schema_version":1,"binding":{"schema_version":1,
            "endpoint":{"transport":"windows_named_pipe","name":r"\\.\pipe\forme-agent-execution-v1-composition-fixture"},
            "credential_path":f.config.store_path.with_file_name("desktop.token"),
            "authority_epoch":1,"sandbox_profile_ref":INSTALLED_DESKTOP_PROFILE,"backend_kinds":["Computer"]},
        "scope":"workspace:default","surface_refs":[INSTALLED_DESKTOP_SURFACE]
    });
    config.desktop_placement = Some(serde_json::from_value(projection.clone()).unwrap());
    let runtime = config.runtime_config().unwrap();
    assert!(runtime.capabilities.computer.enabled);
    assert!(!runtime.capabilities.file.enabled);
    assert_eq!(
        runtime.capabilities.computer.surfaces[0].surface.0,
        INSTALLED_DESKTOP_SURFACE
    );
    let mut foreign = config.clone();
    foreign.workspace = p::WorkspaceRef("workspace:foreign".into());
    assert!(
        !foreign
            .runtime_config()
            .unwrap()
            .capabilities
            .computer
            .enabled
    );
    config.worker_binding = Some(binding(&f));
    assert!(
        config.runtime_config().is_err(),
        "different protected worker authorities must not be combined"
    );
    config.worker_binding = None;
    config
        .managed_capabilities
        .as_mut()
        .unwrap()
        .computer
        .enabled = false;
    assert!(
        !config
            .runtime_config()
            .unwrap()
            .capabilities
            .computer
            .enabled
    );
    let mut changed = projection;
    changed["binding"]["sandbox_profile_ref"] = "sandbox:installed-file-v1".into();
    config.desktop_placement = Some(serde_json::from_value(changed).unwrap());
    assert!(config.runtime_config().is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}
