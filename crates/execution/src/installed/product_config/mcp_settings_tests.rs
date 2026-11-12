//! Finite edits preserve publisher material and the original config history.
use super::*;

fn server() -> p::ProductMcpServerConfig {
    p::ProductMcpServerConfig {
        schema_version: p::SchemaVersion(1),
        server_ref: "mcp:golden".into(),
        program_ref: format!("mcp-program:{}", "a".repeat(64)),
        scope: p::Scope("workspace:default".into()),
        allowlist: p::ProductMcpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: vec!["fixture.read".into()],
            resources: vec![],
        },
        enabled: false,
        trust: p::ProductSkillTrust::Untrusted,
        timeout: p::DurationMs(1000),
        output_budget: 8192,
    }
}
fn edit() -> p::ProductMcpSettingsPatch {
    p::ProductMcpSettingsPatch {
        schema_version: p::SchemaVersion(1),
        operation_id: "save:mcp-golden".into(),
        expected_revision: 1,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        mcp_servers: vec![server()],
    }
}
#[test]
fn narrow_mcp_edit_preserves_keys_sources_and_original_operation_across_reopen() {
    let (_directory, path, store) = tests::fixture();
    let mut store = store.into_inner().unwrap();
    assert!(store.apply_mcp_settings(edit()).is_err());
    let base = tests::patch("save:models", 0);
    let public = ed25519_dalek::SigningKey::from_bytes(&[47; 32])
        .verifying_key()
        .to_bytes();
    let managed = p::ProductManagedCapabilityPatch {
        schema_version: p::SchemaVersion(1),
        mcp_servers: vec![],
        plugins: vec![p::ProductPluginConfig {
            schema_version: p::SchemaVersion(1),
            plugin_ref: "plugin:retained".into(),
            source_ref: "source:retained".into(),
            expected_source_digest: p::sha256_content_digest(b"retained"),
            scope: p::Scope("workspace:default".into()),
            enabled: false,
            trust: p::ProductSkillTrust::Untrusted,
        }],
        publisher_keys: vec![p::ProductPublisherKeyPatch {
            schema_version: p::SchemaVersion(1),
            publisher_ref: "publisher:retained".into(),
            public_key_digest: p::sha256_content_digest(&public),
            public_key: public,
        }],
        computer: Default::default(),
    };
    store
        .apply_settings_v3(p::ProductConfigPatchV3 {
            schema_version: p::SchemaVersion(3),
            operation_id: base.operation_id,
            expected_revision: 0,
            owner: base.owner,
            models: base.models,
            brain: None,
            capabilities: None,
            network: None,
            proxy: None,
            managed_capabilities: Some(managed.clone()),
            channels: Some(p::ProductChannelSettings {
                schema_version: p::SchemaVersion(1),
                channels: vec![p::ProductChannelConfig {
                    schema_version: p::SchemaVersion(1),
                    channel_ref: "channel:retained".into(),
                    adapter_ref: p::ProductChannelAdapter::Loopback,
                    connector_ref: "connector:retained".into(),
                    scope: p::Scope("workspace:default".into()),
                    enabled: false,
                    participant_binding: "participant:retained".into(),
                    credential_ref: None,
                }],
            }),
        })
        .unwrap();
    let before = serde_json::to_value(&store.state).unwrap();
    let key_path = store.state.publisher_key_files["publisher:retained"].clone();
    let request = edit();
    let digest = request.digest().unwrap();
    let saved = store.apply_mcp_settings(request.clone()).unwrap();
    assert!(saved.applied);
    assert!(!saved.view.available);
    assert_eq!(saved.view.active_revision, 2);
    let mut expected = before;
    expected["revision"] = 2.into();
    expected["managed_capabilities"]["mcp_servers"] =
        serde_json::to_value(&request.mcp_servers).unwrap();
    assert_eq!(serde_json::to_value(&store.state).unwrap(), expected);
    assert_eq!(std::fs::read(&key_path).unwrap(), public);
    let receipt = store.mcp_settings_operation(&request.operation_id).unwrap();
    assert_eq!(receipt.request_digest, digest);
    for mutation in 0..4 {
        let mut wrong = request.clone();
        match mutation {
            0 => wrong.mcp_servers.clear(),
            1 => wrong.owner.0 = "owner:foreign".into(),
            2 => {
                wrong.operation_id = "save:stale".into();
            }
            _ => {
                wrong.operation_id = "save:enabled".into();
                wrong.expected_revision = 2;
                wrong.mcp_servers[0].enabled = true;
            }
        }
        assert!(store.apply_mcp_settings(wrong).is_err());
    }
    let core = std::fs::read(&store.template.core_config_path).unwrap();
    drop(store);
    let store = ProductConfigStore::open(&path).unwrap();
    let mut store = store.into_inner().unwrap();
    assert!(!store.apply_mcp_settings(request.clone()).unwrap().applied);
    assert_eq!(
        std::fs::read(&store.template.core_config_path).unwrap(),
        core
    );
    let mut next = request.clone();
    next.operation_id = "clear:mcp".into();
    next.expected_revision = 2;
    next.mcp_servers.clear();
    store.apply_mcp_settings(next).unwrap();
    assert!(store
        .apply_mcp_settings(request)
        .unwrap()
        .view
        .mcp_servers
        .is_empty());
    assert_eq!(
        store.mcp_settings_operation(&receipt.operation_id).unwrap(),
        receipt
    );
    assert_eq!(std::fs::read(key_path).unwrap(), public);
    assert_eq!(
        store.state.managed_capabilities.as_ref().unwrap().plugins,
        managed.plugins
    );
}

#[test]
fn mcp_settings_closed_shape_and_digest_match_browser() {
    let patch = edit();
    let digest = patch.digest().unwrap();
    assert_eq!(
        digest.0,
        "sha256:8bd56b2367c68b5af6824154be255750344591b44cf9b94f3a2353e49fe1792f"
    );
    let value = serde_json::to_value(patch).unwrap();
    for field in ["password", "program", "publisher_keys", "channels"] {
        let mut wrong = value.clone();
        wrong[field] = "forged".into();
        assert!(serde_json::from_value::<p::ProductMcpSettingsPatch>(wrong).is_err());
    }
}
