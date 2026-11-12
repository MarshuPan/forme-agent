use super::{tests::*, *};
fn source() -> p::ProductChannelSource {
    p::ProductChannelSource {
        delivery: None,
        schema_version: p::SchemaVersion(1),
        channel_ref: "channel:fixture".into(),
        connector_ref: "connector:fixture".into(),
        scope: p::Scope("workspace:test".into()),
        participant_binding: "mapping:guest".into(),
        participant: p::ParticipantId("participant:guest".into()),
        credential_ref: "credential:channel".into(),
        credential_digest: p::sha256_content_digest(b"channel-fixture-credential-32-bytes"),
    }
}
fn request(operation: &str, revision: u64) -> p::ProductConfigPatchV3 {
    let old = patch(operation, revision);
    p::ProductConfigPatchV3 {
        schema_version: p::SchemaVersion(3),
        operation_id: old.operation_id,
        expected_revision: revision,
        owner: old.owner,
        models: old.models,
        brain: None,
        capabilities: None,
        network: None,
        proxy: None,
        managed_capabilities: None,
        channels: Some(p::ProductChannelSettings {
            schema_version: p::SchemaVersion(1),
            channels: vec![p::ProductChannelConfig {
                schema_version: p::SchemaVersion(1),
                channel_ref: source().channel_ref,
                adapter_ref: p::ProductChannelAdapter::Text,
                connector_ref: source().connector_ref,
                scope: source().scope,
                enabled: true,
                participant_binding: source().participant_binding,
                credential_ref: Some(source().credential_ref),
            }],
        }),
    }
}
fn provision(store: &ProductConfigStore, source: p::ProductChannelSource) {
    for path in [
        &store.template.core_owner_credential_path,
        &store.template.core_web_credential_path,
    ] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"distinct-system-management-credential").unwrap();
    }
    std::fs::write(
        store
            .template
            .core_config_path
            .with_extension("channel-sources.json"),
        serde_json::to_vec(&p::ProductChannelSourceCatalog {
            schema_version: p::SchemaVersion(1),
            sources: vec![source],
        })
        .unwrap(),
    )
    .unwrap();
}
#[test]
fn managed_channel_requires_exact_provisioned_identity_and_preserves_disable_and_legacy_saves() {
    let (_root, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("base", 0)).unwrap();
    assert!(store.apply_settings_v3(request("enable", 1)).is_err());
    provision(&store, source());
    let before = std::fs::read(&store.template.core_config_path).unwrap();
    for case in 0..5 {
        let mut bad = request("bad", 1);
        let channel = &mut bad.channels.as_mut().unwrap().channels[0];
        match case {
            0 => channel.participant_binding = "mapping:owner".into(),
            1 => channel.connector_ref = "connector:other".into(),
            2 => channel.scope = p::Scope("workspace:other".into()),
            3 => channel.credential_ref = None,
            _ => channel.adapter_ref = p::ProductChannelAdapter::Loopback,
        }
        assert!(store.apply_settings_v3(bad).is_err());
        assert_eq!(
            std::fs::read(&store.template.core_config_path).unwrap(),
            before
        );
    }
    store.apply_settings_v3(request("enable", 1)).unwrap();
    assert_eq!(store.state.schema_version, p::SchemaVersion(14));
    let view = serde_json::to_string(&store.snapshot_settings_v3().unwrap()).unwrap();
    assert!(!view.contains(&source().credential_digest.0));
    store.apply(patch("legacy", 2)).unwrap();
    assert_eq!(store.state.channel_sources.len(), 1);
    let original = store.state.channel_sources.clone();
    let source_path = store
        .template
        .core_config_path
        .with_extension("channel-sources.json");
    drop(store);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    assert!(store.state.channel_sources == original);
    std::fs::write(source_path, b"corrupt source").unwrap();
    let mut disable = request("disable", 3);
    disable.channels.as_mut().unwrap().channels[0].enabled = false;
    store.apply_settings_v3(disable).unwrap();
    assert!(store.state.channel_sources.is_empty());
    assert!(
        !store
            .apply_settings_v3(request("enable", 1))
            .unwrap()
            .applied
    );
    assert!(!store.state.channels.as_ref().unwrap().channels[0].enabled);
}
#[test]
fn managed_channel_source_cannot_reuse_management_identity_or_token() {
    let (_root, _path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("base", 0)).unwrap();
    for case in 0..3 {
        let mut bad = source();
        match case {
            0 => bad.participant = p::ParticipantId("owner:test".into()),
            1 => bad.participant = p::ParticipantId("owner".into()),
            _ => {
                bad.credential_digest =
                    p::sha256_content_digest(b"distinct-system-management-credential")
            }
        }
        provision(&store, bad);
        assert!(store.apply_settings_v3(request("bad", 1)).is_err());
        assert_eq!(store.state.revision, 1);
    }
}
