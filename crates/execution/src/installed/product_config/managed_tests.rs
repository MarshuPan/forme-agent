//! V3 must survive old-client saves and refuse invalid material before writes.
use super::{tests::*, *};

fn request(operation: &str, revision: u64) -> p::ProductConfigPatchV3 {
    let old = patch(operation, revision);
    let key = ed25519_dalek::SigningKey::from_bytes(&[91; 32])
        .verifying_key()
        .to_bytes();
    p::ProductConfigPatchV3 {
        schema_version: p::SchemaVersion(3),
        operation_id: old.operation_id,
        expected_revision: old.expected_revision,
        owner: old.owner,
        models: old.models,
        brain: None,
        capabilities: None,
        network: None,
        proxy: None,
        managed_capabilities: Some(p::ProductManagedCapabilityPatch {
            schema_version: p::SchemaVersion(1),
            mcp_servers: vec![],
            plugins: vec![],
            publisher_keys: vec![p::ProductPublisherKeyPatch {
                schema_version: p::SchemaVersion(1),
                publisher_ref: "publisher:fixture".into(),
                public_key_digest: p::sha256_content_digest(&key),
                public_key: key,
            }],
            computer: Default::default(),
        }),
        channels: Some(Default::default()),
    }
}

#[test]
fn v3_publisher_material_survives_restart_and_legacy_save_without_entering_views() {
    let (_root, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:old", 0)).unwrap();
    let original_core = std::fs::read(&store.template.core_config_path).unwrap();
    let journal_path = store
        .template
        .core_config_path
        .with_extension("journal.jsonl");
    let core_path = store.template.core_config_path.clone();
    drop(store);
    let original_journal = std::fs::read(&journal_path).unwrap();
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    store.snapshot_settings_v3().unwrap();
    drop(store);
    assert_eq!(std::fs::read(&core_path).unwrap(), original_core);
    assert_eq!(std::fs::read(&journal_path).unwrap(), original_journal);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    let request = request("save:managed", 1);
    let response = store.apply_settings_v3(request.clone()).unwrap();
    assert!(response.applied);
    assert_eq!(store.state.schema_version, p::SchemaVersion(10));
    let key_path = store.state.publisher_key_files["publisher:fixture"].clone();
    assert_eq!(
        std::fs::read(&key_path).unwrap(),
        request
            .managed_capabilities
            .as_ref()
            .unwrap()
            .publisher_keys[0]
            .public_key
    );
    let public = serde_json::to_value(&response).unwrap();
    assert!(public["view"]["managed_capabilities"]["publisher_keys"][0]
        .get("public_key")
        .is_none());
    let source = key_path.to_string_lossy();
    assert!(!serde_json::to_string(&public)
        .unwrap()
        .contains(source.as_ref()));
    let legacy = store.apply(patch("save:legacy-again", 2)).unwrap();
    assert_eq!(legacy.schema_version, p::SchemaVersion(1));
    assert_eq!(store.state.schema_version, p::SchemaVersion(10));
    drop(store);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    let replay = store.apply_settings_v3(request.clone()).unwrap();
    assert!(!replay.applied);
    assert_eq!(replay.view.active_revision, 3);
    assert_eq!(
        replay.view.managed_capabilities,
        response.view.managed_capabilities
    );
    assert_eq!(
        store.state.publisher_key_files["publisher:fixture"],
        key_path
    );
    let mut changed = request;
    changed.channels = None;
    assert!(store.apply_settings_v3(changed).is_err());
    drop(store);
    std::fs::write(&key_path, [1_u8; 32]).unwrap();
    assert!(ProductConfigStore::open(&path).is_err());
}

#[test]
fn v3_invalid_material_and_unbound_enabling_leave_no_new_revision_or_credential() {
    let (_root, _path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:base", 0)).unwrap();
    let keys = store.template.model_credentials_directory.clone();
    let before = std::fs::read_dir(&keys).unwrap().count();
    let core = std::fs::read(&store.template.core_config_path).unwrap();
    for case in 0..6 {
        let mut request = request(&format!("save:invalid-{case}"), 1);
        let managed = request.managed_capabilities.as_mut().unwrap();
        match case {
            0 => {
                managed.publisher_keys[0].public_key_digest = p::sha256_content_digest(b"different")
            }
            1 => {
                managed.publisher_keys[0].public_key = [0; 32];
                managed.publisher_keys[0].public_key_digest = p::sha256_content_digest(&[0; 32]);
            }
            2 => managed
                .publisher_keys
                .push(managed.publisher_keys[0].clone()),
            3 => {
                managed.computer.enabled = true;
                managed.computer.surface_refs.push("surface:unbound".into());
            }
            4 => managed.schema_version = p::SchemaVersion(99),
            _ => request.owner = p::OwnerPrincipalRef("owner:foreign".into()),
        }
        assert!(store.apply_settings_v3(request).is_err());
        assert_eq!(std::fs::read_dir(&keys).unwrap().count(), before);
        assert_eq!(
            std::fs::read(&store.template.core_config_path).unwrap(),
            core
        );
        assert_eq!(store.snapshot().unwrap().active_revision, 1);
    }
}

#[test]
fn settings_v3_is_closed_and_cannot_be_deserialized_as_a_legacy_patch() {
    let request = request("save:v3", 0);
    let mut wire = serde_json::to_value(&request).unwrap();
    assert!(serde_json::from_value::<p::ProductConfigPatchV2>(wire.clone()).is_err());
    wire["managed_capabilities"]["publisher_keys"][0]["trusted"] = true.into();
    assert!(serde_json::from_value::<p::ProductConfigPatchV3>(wire).is_err());
    assert_eq!(p::PRODUCT_SETTINGS_SCHEMA_VERSION, p::SchemaVersion(2));
    assert!(p::validate_product_settings_schema(p::SchemaVersion(3)).is_err());
}
