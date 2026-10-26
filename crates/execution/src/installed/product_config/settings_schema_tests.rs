//! Wire-version changes must not rewrite old facts or grant executable access.
use super::{tests::*, *};

#[test]
fn v2_reads_do_not_rewrite_v1_bytes_or_change_independent_object_versions() {
    let (_root, path, config) = fixture();
    let mut config = config.into_inner().unwrap();
    config.apply(patch("save:legacy", 0)).unwrap();
    let journal = config
        .template
        .core_config_path
        .with_extension("journal.jsonl");
    let core = config.template.core_config_path.clone();
    let legacy = serde_json::to_value(config.snapshot().unwrap()).unwrap();
    assert_eq!(legacy["schema_version"], 1);
    assert!(legacy.get("availability").is_none());
    drop(config);
    let before_log = std::fs::read(&journal).unwrap();
    let before_core = std::fs::read(&core).unwrap();
    let mut config = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    let current = config.snapshot_with_schema(p::SchemaVersion(2)).unwrap();
    assert_eq!(current.schema_version, p::SchemaVersion(2));
    assert_eq!(current.models[0].schema_version, p::SchemaVersion(1));
    assert_eq!(current.brain.schema_version, p::SchemaVersion(1));
    let features = current.availability.unwrap().features;
    assert_eq!(features.len(), 8);
    assert_eq!(
        features
            .iter()
            .filter(|item| item.state == p::ProductSettingState::Configurable)
            .count(),
        4
    );
    for item in features {
        if matches!(
            item.feature,
            p::ProductSettingFeature::Mcp
                | p::ProductSettingFeature::Plugins
                | p::ProductSettingFeature::Packages
                | p::ProductSettingFeature::Channels
        ) {
            assert_eq!(item.state, p::ProductSettingState::Unavailable);
            assert_eq!(
                item.reason,
                Some(p::ProductSettingUnavailableReason::ManagedConsumerMissing)
            );
        }
    }
    assert_eq!(
        serde_json::to_value(config.snapshot().unwrap()).unwrap(),
        legacy
    );
    drop(config);
    assert_eq!(std::fs::read(journal).unwrap(), before_log);
    assert_eq!(std::fs::read(core).unwrap(), before_core);
}

#[test]
fn v2_save_and_reopen_preserve_legacy_digest_and_old_clients_preserve_preferences() {
    let (_root, path, config) = fixture();
    let mut first = patch("save:v2", 0);
    let old = first.digest().unwrap();
    first.schema_version = p::SchemaVersion(2);
    first.brain = Some(p::ProductBrainPreferences {
        schema_version: p::SchemaVersion(1),
        default_workspace: p::WorkspaceRef("workspace:settings-v2".into()),
        model_routing: Default::default(),
    });
    first.capabilities = Some(Default::default());
    first.network = Some(p::ProductNetworkConfig::loopback(
        "127.0.0.1:8898".parse().unwrap(),
    ));
    assert_ne!(first.digest().unwrap(), old);
    let result = config.lock().unwrap().apply(first.clone()).unwrap();
    assert_eq!(result.schema_version, p::SchemaVersion(2));
    assert!(result.view.availability.is_some());
    assert_eq!(
        result.view.brain.default_workspace.0,
        "workspace:settings-v2"
    );
    drop(config);
    let config = ProductConfigStore::open(&path).unwrap();
    let mut config = config.lock().unwrap();
    let replay = config.apply(first).unwrap();
    assert!(!replay.applied);
    assert_eq!(replay.view, result.view);
    let legacy = config.apply(patch("save:old-client", 1)).unwrap();
    assert_eq!(legacy.schema_version, p::SchemaVersion(1));
    assert!(legacy.view.availability.is_none());
    assert_eq!(legacy.view.brain, result.view.brain);
    assert_eq!(legacy.view.capabilities, result.view.capabilities);
    assert_eq!(legacy.view.network, result.view.network);
    let old_patch = patch("save:old-client", 1);
    assert_eq!(
        old_patch.digest().unwrap(),
        p::canonical_digest(&(
            &old_patch.schema_version,
            &old_patch.operation_id,
            &old_patch.expected_revision,
            &old_patch.owner,
            &old_patch.models,
        ))
        .unwrap()
    );
}

#[test]
fn same_operation_cannot_change_settings_version_even_after_reopen() {
    let (_root, path, config) = fixture();
    let request = patch("save:same-key", 0);
    let mut config = config.into_inner().unwrap();
    config.apply(request.clone()).unwrap();
    let journal = config
        .template
        .core_config_path
        .with_extension("journal.jsonl");
    drop(config);
    let original = std::fs::read(&journal).unwrap();
    let mut config = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    let mut changed = request.clone();
    changed.schema_version = p::SchemaVersion(2);
    assert!(config.apply(changed).is_err());
    assert!(!config.apply(request).unwrap().applied);
    drop(config);
    assert_eq!(std::fs::read(journal).unwrap(), original);
}

#[test]
fn unknown_settings_and_inner_schema_changes_fail_before_any_new_files() {
    let (root, path, config) = fixture();
    let config = config.into_inner().unwrap();
    let files_before = std::fs::read_dir(root.path()).unwrap().count();
    let journal = config
        .template
        .core_config_path
        .with_extension("journal.jsonl");
    drop(config);
    let original = std::fs::read(&journal).unwrap();
    let mut config = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    for version in [0, 3, u32::MAX] {
        assert!(config
            .snapshot_with_schema(p::SchemaVersion(version))
            .is_err());
        let mut request = patch("save:unsupported", 0);
        request.schema_version = p::SchemaVersion(version);
        assert!(config.apply(request).is_err());
    }
    let mut request = patch("save:wrong-inner", 0);
    request.schema_version = p::SchemaVersion(2);
    request.models[0].schema_version = p::SchemaVersion(2);
    assert!(config.apply(request).is_err());
    assert!(!config.template.model_credentials_directory.exists());
    drop(config);
    assert_eq!(std::fs::read(journal).unwrap(), original);
    assert_eq!(
        std::fs::read_dir(root.path()).unwrap().count(),
        files_before
    );
}

#[test]
fn old_installation_reports_network_as_unavailable_without_migrating_template() {
    let (_root, path, config) = fixture_with_network(false);
    let original = std::fs::read(&path).unwrap();
    let view = config
        .lock()
        .unwrap()
        .snapshot_with_schema(p::SchemaVersion(2))
        .unwrap();
    let feature = view
        .availability
        .unwrap()
        .features
        .into_iter()
        .find(|item| item.feature == p::ProductSettingFeature::Network)
        .unwrap();
    assert_eq!(feature.state, p::ProductSettingState::Unavailable);
    assert_eq!(
        feature.reason,
        Some(p::ProductSettingUnavailableReason::NetworkPathsMissing)
    );
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn v2_proxy_is_materialized_without_endpoint_in_public_views_and_survives_v1_save() {
    let (_root, path, config) = fixture();
    let base = patch("save:proxy", 0);
    let request = p::ProductConfigPatchV2 {
        schema_version: p::PRODUCT_SETTINGS_SCHEMA_VERSION,
        operation_id: "save:proxy".into(),
        expected_revision: 0,
        owner: base.owner.clone(),
        models: base.models.clone(),
        brain: None,
        capabilities: None,
        network: None,
        proxy: Some(p::ProductProxyConfig {
            schema_version: p::PRODUCT_PROXY_SCHEMA_VERSION,
            mode: p::ProductProxyMode::Http,
            endpoint: Some("http://proxy.invalid:8080".into()),
        }),
    };
    let result = config
        .lock()
        .unwrap()
        .apply_settings_v2(request.clone())
        .unwrap();
    assert_eq!(result.view.proxy.mode, p::ProductProxyMode::Http);
    assert!(result.view.proxy.endpoint_configured);
    assert_eq!(
        result.view.proxy_source,
        p::ProductPreferenceSource::ManagedConfiguration
    );
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("proxy.invalid"));

    let core_path = config.lock().unwrap().template.core_config_path.clone();
    let core: serde_json::Value = storage::json(&core_path, MAX_CONFIG_BYTES).unwrap();
    assert_eq!(core["model_proxy"]["mode"], "http");
    assert_eq!(core["model_proxy"]["endpoint"], "http://proxy.invalid:8080");
    drop(config);

    let config = ProductConfigStore::open(&path).unwrap();
    let mut config = config.into_inner().unwrap();
    assert_eq!(
        config.snapshot_settings_v2().unwrap().proxy.mode,
        p::ProductProxyMode::Http
    );
    let old_client = config.apply(patch("save:old-proxy-client", 1)).unwrap();
    assert_eq!(old_client.schema_version, p::PRODUCT_CONFIG_SCHEMA_VERSION);
    assert!(old_client.view.availability.is_none());
    assert_eq!(
        storage::json::<serde_json::Value>(&core_path, MAX_CONFIG_BYTES).unwrap()["model_proxy"]
            ["endpoint"],
        "http://proxy.invalid:8080"
    );

    let direct_base = patch("save:direct", 2);
    let direct = config
        .apply_settings_v2(p::ProductConfigPatchV2 {
            schema_version: p::PRODUCT_SETTINGS_SCHEMA_VERSION,
            operation_id: "save:direct".into(),
            expected_revision: 2,
            owner: direct_base.owner,
            models: direct_base.models,
            brain: None,
            capabilities: None,
            network: None,
            proxy: Some(p::ProductProxyConfig::direct()),
        })
        .unwrap();
    assert_eq!(direct.view.proxy.mode, p::ProductProxyMode::Direct);
    assert!(!direct.view.proxy.endpoint_configured);
    assert_eq!(
        storage::json::<serde_json::Value>(&core_path, MAX_CONFIG_BYTES).unwrap()["model_proxy"]
            ["mode"],
        "direct"
    );
}

#[test]
fn proxy_endpoint_is_rejected_before_credentials_or_revisions_are_staged() {
    for (index, endpoint) in [
        "http://proxy.invalid:8080/secret",
        "http://user@proxy.invalid:8080",
        "http://proxy.invalid:8080?token=secret",
        "http://proxy.invalid:8080#fragment",
        "socks5://proxy.invalid:1080",
        "http://proxy.invalid:65536",
        "http://proxy.invalid:8080\nheader: injected",
    ]
    .into_iter()
    .enumerate()
    {
        let (root, _path, config) = fixture();
        let base = patch(&format!("save:bad-proxy-{index}"), 0);
        let request = p::ProductConfigPatchV2 {
            schema_version: p::PRODUCT_SETTINGS_SCHEMA_VERSION,
            operation_id: format!("save:bad-proxy-{index}"),
            expected_revision: 0,
            owner: base.owner,
            models: base.models,
            brain: None,
            capabilities: None,
            network: None,
            proxy: Some(p::ProductProxyConfig {
                schema_version: p::PRODUCT_PROXY_SCHEMA_VERSION,
                mode: p::ProductProxyMode::Http,
                endpoint: Some(endpoint.into()),
            }),
        };
        assert!(config.lock().unwrap().apply_settings_v2(request).is_err());
        assert!(!root.path().join("keys/models").exists());
        assert!(!root.path().join("config/core.json").exists());
    }
}
