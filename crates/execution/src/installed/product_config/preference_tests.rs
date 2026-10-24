//! Configuration preferences share the durable CAS and publication boundary.
use super::{
    tests::{fixture, patch},
    *,
};
use std::fs;

fn preferences() -> p::ProductBrainPreferences {
    p::ProductBrainPreferences {
        schema_version: p::SchemaVersion(1),
        default_workspace: p::WorkspaceRef("workspace:managed".into()),
        model_routing: p::ProductModelRouting {
            schema_version: p::SchemaVersion(1),
            foreground: Some(p::ModelProfileRef("model:test".into())),
            cognition: Some(p::ModelProfileRef("model:test".into())),
        },
    }
}

#[test]
fn preferences_are_materialized_and_preserved_by_old_model_saves_and_replay() {
    let (_dir, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    let initial = store.snapshot().unwrap();
    assert_eq!(initial.brain.default_workspace.0, "workspace:test");
    assert_eq!(
        initial.brain_source,
        p::ProductPreferenceSource::InstallationTemplate
    );
    let legacy = patch("save:legacy", 0);
    assert_eq!(
        legacy.digest().unwrap(),
        p::canonical_digest(&(
            &legacy.schema_version,
            &legacy.operation_id,
            &legacy.expected_revision,
            &legacy.owner,
            &legacy.models,
        ))
        .unwrap()
    );
    store.apply(legacy.clone()).unwrap();
    let old_core = fs::read(&store.template.core_config_path).unwrap();
    let old_json: serde_json::Value = serde_json::from_slice(&old_core).unwrap();
    assert!(old_json["brain"].get("model_routing").is_none());
    let mut update = patch("save:preferences", 1);
    update.brain = Some(preferences());
    let saved = store.apply(update.clone()).unwrap();
    assert_eq!(saved.view.brain, preferences());
    assert!(saved.view.pending_restart);
    assert_eq!(
        saved.view.brain_source,
        p::ProductPreferenceSource::ManagedConfiguration
    );
    let core: serde_json::Value =
        storage::json(&store.template.core_config_path, MAX_CONFIG_BYTES).unwrap();
    assert_eq!(core["brain"]["workspace"], "workspace:managed");
    assert_eq!(core["brain"]["model_routing"]["cognition"], "model:test");
    store.apply(patch("save:old-client", 2)).unwrap();
    let live_core = fs::read(&store.template.core_config_path).unwrap();
    drop(store);
    let store = ProductConfigStore::open(&path).unwrap();
    let mut store = store.into_inner().unwrap();
    assert!(!store.apply(legacy).unwrap().applied);
    assert!(!store.apply(update.clone()).unwrap().applied);
    assert_eq!(store.snapshot().unwrap().brain, preferences());
    assert_eq!(store.state.revision, 3);
    assert_eq!(
        fs::read(&store.template.core_config_path).unwrap(),
        live_core
    );
    update.brain.as_mut().unwrap().default_workspace.0 = "workspace:altered".into();
    assert!(store.apply(update).unwrap_err().0.contains("reused"));
    assert_eq!(store.state.revision, 3);
}

#[test]
fn invalid_or_stale_preferences_do_not_rotate_keys_or_publish() {
    let (_dir, _path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    let mut first = patch("save:first", 0);
    first.brain = Some(preferences());
    store.apply(first).unwrap();
    let key = store.state.credentials["model:test"].clone();
    let core = fs::read(&store.template.core_config_path).unwrap();
    for (index, brain) in [
        {
            let mut b = preferences();
            b.model_routing.cognition = Some(p::ModelProfileRef("model:missing".into()));
            b
        },
        {
            let mut b = preferences();
            b.default_workspace.0 = "C:\\private".into();
            b
        },
        {
            let mut b = preferences();
            b.default_workspace.0 = "../escape".into();
            b
        },
        {
            let mut b = preferences();
            b.default_workspace.0 = "\nworkspace".into();
            b
        },
        {
            let mut b = preferences();
            b.schema_version = p::SchemaVersion(2);
            b
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut bad = patch(&format!("save:bad-{index}"), 1);
        bad.brain = Some(brain);
        bad.models[0].credential = Some("never-staged-key".into());
        assert!(store.apply(bad).is_err());
    }
    let mut remove_referenced = patch("save:removed", 1);
    remove_referenced.models[0].id = "model:replacement".into();
    assert!(store.apply(remove_referenced).is_err());
    let mut stale = patch("save:stale", 0);
    stale.brain = Some(preferences());
    assert!(store.apply(stale).is_err());
    assert_eq!(store.state.credentials["model:test"], key);
    assert_eq!(fs::read(&store.template.core_config_path).unwrap(), core);
    assert_eq!(store.state.revision, 1);
}

#[test]
fn interrupted_preference_publication_recovers_exact_original_intent() {
    let (_dir, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:first", 0)).unwrap();
    let mut update = patch("save:interrupted", 1);
    update.brain = Some(preferences());
    let mut next = store.state.clone();
    next.schema_version = p::SchemaVersion(4);
    next.revision = 2;
    next.brain = update.brain.clone();
    let reference = materialize::stage(&store.template, &next).unwrap();
    store
        .journal
        .prepare(Intent {
            operation: update.operation_id.clone(),
            digest: update.digest().unwrap(),
            snapshot: reference,
        })
        .unwrap();
    drop(store);
    let store = ProductConfigStore::open(&path).unwrap();
    let mut store = store.into_inner().unwrap();
    assert!(!store.apply(update).unwrap().applied);
    assert_eq!(store.snapshot().unwrap().brain, preferences());
    assert_eq!(store.state.revision, 2);
    assert_eq!(store.state.schema_version, p::SchemaVersion(4));
    let core: serde_json::Value =
        storage::json(&store.template.core_config_path, MAX_CONFIG_BYTES).unwrap();
    assert_eq!(core["brain"]["workspace"], "workspace:managed");
    assert!(store.journal.pending().is_none());
}

#[test]
fn restored_brain_publication_preserves_preferences_and_rejects_stale_request() {
    let (_dir, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    let mut update = patch("save:before-restore", 0);
    update.brain = Some(preferences());
    store.apply(update).unwrap();
    let binding = p::ProductDataBinding {
        schema_version: p::PRODUCT_DATA_BINDING_SCHEMA_VERSION,
        candidate_ref: "restored-preferences.db".into(),
        store_path: store
            .template
            .brain_store_path
            .parent()
            .unwrap()
            .join("data-operations/restored-preferences.db"),
        content_key_directory: store
            .template
            .brain_store_path
            .with_extension("content-keys"),
        owner_digest: p::sha256_content_digest(b"owner:test"),
        store_identity: p::sha256_content_digest(b"store:preferences"),
        manifest_digest: p::sha256_content_digest(b"manifest:preferences"),
        execution_fence_digest: Some(p::sha256_content_digest(b"fence:preferences")),
    };
    let request = p::ProductRestoreRequest {
        schema_version: p::SchemaVersion(1),
        install_ref: store.template.install_ref.clone(),
        operation_id: "restore:preferences".into(),
        command_digest: p::sha256_content_digest(b"restore:preferences"),
        expected_config_revision: 1,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        candidate_ref: binding.candidate_ref.clone(),
        candidate_digest: binding.manifest_digest.clone(),
    };
    let original_core = fs::read(&store.template.core_config_path).unwrap();
    let mut foreign = request.clone();
    foreign.install_ref = "install:another".into();
    assert!(store
        .publish_restored_data(&foreign, binding.clone())
        .is_err());
    assert_eq!(store.state.revision, 1);
    assert_eq!(
        fs::read(&store.template.core_config_path).unwrap(),
        original_core
    );
    let view = store
        .publish_restored_data(&request, binding.clone())
        .unwrap();
    assert_eq!(view.active_revision, 2);
    assert_eq!(view.brain, preferences());
    let core = fs::read(&store.template.core_config_path).unwrap();
    let config: serde_json::Value = serde_json::from_slice(&core).unwrap();
    assert_eq!(
        config["brain"]["store_path"],
        binding.store_path.to_str().unwrap()
    );
    assert_eq!(config["brain"]["workspace"], "workspace:managed");
    assert_eq!(config["brain"]["model_routing"]["cognition"], "model:test");
    drop(store);
    let store = ProductConfigStore::open(&path).unwrap();
    let mut store = store.into_inner().unwrap();
    assert_eq!(store.snapshot().unwrap().brain, preferences());
    assert_eq!(store.state.schema_version, p::SchemaVersion(4));
    assert!(store.publish_restored_data(&request, binding).is_err());
    assert_eq!(store.state.revision, 2);
    assert_eq!(fs::read(&store.template.core_config_path).unwrap(), core);
}
