use super::{tests::*, *};

fn source() -> p::ProductPluginSource {
    serde_json::from_value(serde_json::json!({
        "schema_version":1,"source_ref":"source:declared","plugin_ref":"plugin:fixture",
        "manifest_ref":"manifest:fixture-v1","scope":"workspace:test",
        "skills":[{"schema_version":1,"id":"skill:plugin-fixture","summary":"plugin sample",
            "scope":"workspace:test","version":1,"body":"private-plugin-body",
            "enabled":false,"trust":"untrusted"}]
    }))
    .unwrap()
}
fn request(store: &ProductConfigStore, operation: &str) -> p::ProductPluginSettingsPatch {
    let source = source();
    p::ProductPluginSettingsPatch {
        schema_version: p::SchemaVersion(1),
        operation_id: operation.into(),
        expected_revision: store.state.revision,
        owner: store.state.owner.clone().unwrap(),
        plugins: vec![p::ProductPluginConfig {
            schema_version: p::SchemaVersion(1),
            source_ref: source.source_ref.clone(),
            plugin_ref: source.plugin_ref.clone(),
            scope: source.scope.clone(),
            expected_source_digest: source.digest().unwrap(),
            enabled: true,
            trust: p::ProductSkillTrust::OwnerInput,
        }],
    }
}
fn provision(store: &ProductConfigStore) {
    let catalog = p::ProductPluginSourceCatalog {
        schema_version: p::SchemaVersion(1),
        sources: vec![source()],
    };
    std::fs::write(
        store
            .template
            .core_config_path
            .with_extension("plugin-sources.json"),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .unwrap();
}

#[test]
fn plugin_sources_publish_preserve_old_saves_and_disable_without_reading_broken_source() {
    let (_root, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("base", 0)).unwrap();
    provision(&store);
    let command = request(&store, "plugin:enable");
    let view = store.apply_plugin_settings(command.clone()).unwrap();
    assert!(view.applied);
    assert_eq!(store.state.schema_version, p::SchemaVersion(13));
    assert_eq!(store.state.plugin_sources.len(), 1);
    let public = serde_json::to_string(&view).unwrap();
    assert!(!public.contains("private-plugin-body"));
    assert!(!public.contains("plugin_configuration_path"));
    let core: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&store.template.core_config_path).unwrap()).unwrap();
    assert_eq!(
        core["brain"]["plugin_sources"][0]["skills"][0]["body"],
        "private-plugin-body"
    );
    store.apply(patch("old-client", 2)).unwrap();
    assert_eq!(store.state.schema_version, p::SchemaVersion(13));
    let source_path = store
        .template
        .core_config_path
        .with_extension("plugin-sources.json");
    drop(store);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    assert!(
        !store
            .apply_plugin_settings(command.clone())
            .unwrap()
            .applied
    );
    std::fs::write(&source_path, b"broken source").unwrap();
    assert!(!store.plugin_settings().unwrap().sources_available);
    let mut disable = request(&store, "plugin:disable");
    disable.plugins[0].enabled = false;
    store.apply_plugin_settings(disable).unwrap();
    assert!(store.state.plugin_sources.is_empty());
    assert!(
        !store
            .apply_plugin_settings(command.clone())
            .unwrap()
            .applied
    );
    assert!(!store.state.managed_capabilities.as_ref().unwrap().plugins[0].enabled);
    assert_eq!(
        store
            .plugin_settings_operation(&command.operation_id)
            .unwrap()
            .request_digest,
        command.digest().unwrap()
    );
    let mut conflict = command;
    conflict.plugins.clear();
    assert!(store.apply_plugin_settings(conflict).is_err());
}

#[test]
fn plugin_bad_source_identity_digest_trust_scope_and_duplicate_skill_write_nothing() {
    let (_root, _path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("base", 0)).unwrap();
    let command = request(&store, "plugin:enable");
    assert!(store.apply_plugin_settings(command.clone()).is_err());
    provision(&store);
    let before = std::fs::read(&store.template.core_config_path).unwrap();
    for case in 0..5 {
        let mut bad = command.clone();
        match case {
            0 => bad.plugins[0].source_ref = "source:missing".into(),
            1 => bad.plugins[0].expected_source_digest = p::canonical_digest(&"wrong").unwrap(),
            2 => bad.plugins[0].trust = p::ProductSkillTrust::Untrusted,
            3 => bad.plugins[0].scope = p::Scope("workspace:foreign".into()),
            _ => bad.plugins[0].plugin_ref = "plugin:other".into(),
        }
        assert!(store.apply_plugin_settings(bad).is_err());
        assert_eq!(store.state.revision, 1);
        assert_eq!(
            std::fs::read(&store.template.core_config_path).unwrap(),
            before
        );
    }
    let mut old = patch("direct-skill", 1);
    old.capabilities = Some(p::ProductCapabilitySettings {
        schema_version: p::SchemaVersion(1),
        skills: source().skills,
    });
    store.apply(old).unwrap();
    let duplicate = request(&store, "plugin:duplicate");
    assert!(store.apply_plugin_settings(duplicate).is_err());
    assert_eq!(store.state.revision, 2);
}

#[test]
fn plugin_manifest_cannot_self_assign_authority_or_smuggle_executable_fields() {
    let source = source();
    source.validate().unwrap();
    for field in ["command", "path", "mcp", "hooks", "grant", "trust"] {
        let mut value = serde_json::to_value(&source).unwrap();
        value[field] = serde_json::json!("attack");
        assert!(serde_json::from_value::<p::ProductPluginSource>(value).is_err());
    }
    let mut trusted = source.clone();
    trusted.skills[0].trust = p::ProductSkillTrust::OwnerInput;
    assert!(trusted.validate().is_err());
    let mut duplicated = source.clone();
    duplicated.skills.push(source.skills[0].clone());
    assert!(duplicated.validate().is_err());
    let mut huge = source;
    huge.skills[0].body = "a".repeat(100_000);
    assert!(huge.validate().is_err());
}
