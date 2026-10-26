use super::*;
use crate::installed::product_config::tests::{fixture, patch};

fn digest(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}

fn takeover(store: &mut ProductConfigStore) -> p::ProductReleaseTakeover {
    let (owner, revision, config) = store.release_configuration_identity().unwrap();
    p::ProductReleaseTakeover {
        schema_version: p::SchemaVersion(1),
        release: p::ProductReleaseRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: store.template.install_ref.clone(),
            operation_id: "upgrade:test".into(),
            command_digest: digest("command"),
            change: p::ProductReleaseChange::Upgrade,
            original_release_ref: "0.0.1".into(),
            original_release_digest: digest("old"),
            target_release_ref: "0.0.2".into(),
            target_release_digest: digest("new"),
            backup_ref: "backup:release".into(),
        },
        installation_digest: digest("installation"),
        owner,
        expected_config_revision: revision,
        expected_config_digest: config,
        deadline: 1000,
        readiness_nonce: "a".repeat(64),
    }
}
fn binding(takeover: &p::ProductReleaseTakeover) -> p::ProductReleaseBinding {
    p::ProductReleaseBinding {
        schema_version: p::SchemaVersion(1),
        install_ref: takeover.release.install_ref.clone(),
        operation_id: takeover.release.operation_id.clone(),
        command_digest: takeover.release.command_digest.clone(),
        target_release_ref: takeover.release.target_release_ref.clone(),
        target_release_digest: takeover.release.target_release_digest.clone(),
        core_image_digest: digest("image"),
        owner_digest: p::sha256_content_digest(takeover.owner.0.as_bytes()),
        store_identity: digest("store"),
        config_revision: takeover.expected_config_revision + 1,
        readiness_nonce: takeover.readiness_nonce.clone(),
    }
}
fn readiness(binding: &p::ProductReleaseBinding, revision: u64) -> p::ProductReleaseReadiness {
    p::ProductReleaseReadiness {
        schema_version: p::SchemaVersion(1),
        binding: binding.clone(),
        config_revision: revision,
        actual_image_digest: binding.core_image_digest.clone(),
        data_identity: p::ProductDataAcknowledgement {
            schema_version: p::SchemaVersion(1),
            owner_digest: binding.owner_digest.clone(),
            store_identity: binding.store_identity.clone(),
            execution_fence_digest: None,
            composition_digest: digest("composition"),
        },
    }
}

#[test]
fn release_publication_preserves_installed_worker_in_schema_nine() {
    let (directory, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    let worker = super::super::worker_tests::placement(&store, directory.path());
    store.installed_file = Some(worker.clone());
    store.apply(patch("worker:initial", 0)).unwrap();
    let takeover = takeover(&mut store);
    store
        .publish_release_binding(&takeover, binding(&takeover), 100)
        .unwrap();
    assert_eq!(store.state.schema_version, p::SchemaVersion(9));
    assert_eq!(store.state.worker, Some(worker.clone()));
    drop(store);
    let reopened =
        ProductConfigStore::open_with_installed_file(&path, Some(worker.clone())).unwrap();
    assert_eq!(reopened.lock().unwrap().state.worker, Some(worker));
}

#[test]
fn release_publication_preserves_old_revision_and_cannot_acknowledge_old_readiness() {
    let (_root, template_path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:initial", 0)).unwrap();
    let prior = store.journal.current().unwrap().clone();
    let prior_state = storage::read(
        &materialize::revision_directory(&store.template, &prior.id).join("state.json"),
        MAX_STATE_BYTES,
    )
    .unwrap();
    let prior_core = materialize::core_bytes(&store.template, &prior).unwrap();
    let takeover = takeover(&mut store);
    let binding = binding(&takeover);
    store
        .publish_release_binding(&takeover, binding.clone(), 100)
        .unwrap();
    assert_eq!(store.state.schema_version, p::SchemaVersion(7));
    assert_eq!(store.state.revision, 2);
    assert_eq!(
        materialize::core_bytes(&store.template, &prior).unwrap(),
        prior_core
    );
    assert_eq!(
        storage::read(
            &materialize::revision_directory(&store.template, &prior.id).join("state.json"),
            MAX_STATE_BYTES
        )
        .unwrap(),
        prior_state
    );
    let hold_path = store
        .template
        .core_config_path
        .with_extension("release-admission.json");
    let hold: p::ProductReleaseAdmission = storage::json(&hold_path, 8192).unwrap();
    assert!(!hold.released);
    let ack = p::ProductConfigAcknowledgement {
        schema_version: p::SchemaVersion(1),
        install_ref: binding.install_ref.clone(),
        revision: 2,
        data_identity: Some(readiness(&binding, 2).data_identity),
    };
    assert!(store.acknowledge(ack.clone()).is_err());
    let ready_path = store
        .template
        .configuration_ack_path
        .with_extension("release-readiness.json");
    let mut ready = readiness(&binding, 2);
    ready.binding.readiness_nonce = "b".repeat(64);
    storage::publish(&ready_path, &storage::encode(&ready).unwrap()).unwrap();
    assert!(store.acknowledge(ack.clone()).is_err());
    storage::publish(
        &ready_path,
        &storage::encode(&readiness(&binding, 2)).unwrap(),
    )
    .unwrap();
    store.acknowledge(ack).unwrap();
    store.release_work_admission(&binding, 200).unwrap();
    let hold: p::ProductReleaseAdmission = storage::json(&hold_path, 8192).unwrap();
    assert!(hold.released);
    let mut later = patch("save:later", 2);
    later.models[0].credential = None;
    store.apply(later).unwrap();
    assert_eq!(store.release_binding(), Some(&binding));
    assert_eq!(store.state.schema_version, p::SchemaVersion(7));
    drop(store);
    let reopened = ProductConfigStore::open(&template_path).unwrap();
    assert_eq!(reopened.lock().unwrap().release_binding(), Some(&binding));
}

#[test]
fn release_publication_preserves_model_proxy_in_schema_eight() {
    let (_root, template_path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    let base = patch("save:proxy", 0);
    store
        .apply_settings_v2(p::ProductConfigPatchV2 {
            schema_version: p::PRODUCT_SETTINGS_SCHEMA_VERSION,
            operation_id: base.operation_id,
            expected_revision: base.expected_revision,
            owner: base.owner,
            models: base.models,
            brain: base.brain,
            capabilities: base.capabilities,
            network: None,
            proxy: Some(p::ProductProxyConfig {
                schema_version: p::PRODUCT_PROXY_SCHEMA_VERSION,
                mode: p::ProductProxyMode::Http,
                endpoint: Some("http://proxy.invalid:8080".into()),
            }),
        })
        .unwrap();
    let takeover = takeover(&mut store);
    let binding = binding(&takeover);
    store
        .publish_release_binding(&takeover, binding.clone(), 100)
        .unwrap();
    assert_eq!(store.state.schema_version, p::SchemaVersion(8));
    assert_eq!(
        store.state.proxy.as_ref().unwrap().mode,
        p::ProductProxyMode::Http
    );
    assert_eq!(store.release_binding(), Some(&binding));
    drop(store);

    let reopened = ProductConfigStore::open(&template_path).unwrap();
    let reopened = reopened.lock().unwrap();
    assert_eq!(reopened.state.schema_version, p::SchemaVersion(8));
    assert_eq!(reopened.release_binding(), Some(&binding));
}

#[test]
fn stale_owner_revision_digest_nonce_and_expiry_cannot_publish_a_release() {
    for case in 0..5 {
        let (_root, _template, store) = fixture();
        let mut store = store.into_inner().unwrap();
        store.apply(patch("save:initial", 0)).unwrap();
        let mut takeover = takeover(&mut store);
        let binding = binding(&takeover);
        match case {
            0 => takeover.owner = p::OwnerPrincipalRef("owner:foreign".into()),
            1 => takeover.expected_config_revision += 1,
            2 => takeover.expected_config_digest = digest("changed-config"),
            3 => takeover.readiness_nonce = "b".repeat(64),
            _ => takeover.deadline = 100,
        }
        let before = std::fs::read(&store.template.core_config_path).unwrap();
        assert!(store
            .publish_release_binding(&takeover, binding, 100)
            .is_err());
        assert_eq!(store.state.revision, 1);
        assert_eq!(
            std::fs::read(&store.template.core_config_path).unwrap(),
            before
        );
        assert!(!store
            .template
            .core_config_path
            .with_extension("release-admission.json")
            .exists());
    }
}
