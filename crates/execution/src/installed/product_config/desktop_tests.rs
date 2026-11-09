//! Desktop enablement is an independent protected placement and remains revocable.
use super::super::{tests::*, *};
use crate::installed::{
    InstalledDesktopProvisioning, INSTALLED_DESKTOP_PROFILE, INSTALLED_DESKTOP_SURFACE,
};

fn placement(store: &ProductConfigStore) -> InstalledDesktopProvisioning {
    let digest =
        p::canonical_digest(&("forme-agent-execution-pipe-v1", &store.template.install_ref))
            .unwrap();
    serde_json::from_value(serde_json::json!({
        "schema_version":1,
        "binding": {"schema_version":1,
            "endpoint":{"transport":"windows_named_pipe", "name":format!(r"\\.\pipe\forme-agent-execution-v1-{}", &digest.0[7..])},
            "credential_path":store.template.model_credentials_directory.parent().unwrap().join("agent-execution.token"),
            "authority_epoch":1,"sandbox_profile_ref":INSTALLED_DESKTOP_PROFILE,"backend_kinds":["Computer"]},
        "scope":"workspace:default","surface_refs":[INSTALLED_DESKTOP_SURFACE]
    })).unwrap()
}
fn update(operation: &str, revision: u64, enabled: bool) -> p::ProductConfigPatchV3 {
    let old = patch(operation, revision);
    let mut managed = p::ProductManagedCapabilityPatch {
        schema_version: p::SchemaVersion(1),
        mcp_servers: vec![],
        plugins: vec![],
        publisher_keys: vec![],
        computer: Default::default(),
    };
    managed.computer.enabled = enabled;
    managed.computer.scope = p::Scope("workspace:default".into());
    managed.computer.surface_refs = vec![INSTALLED_DESKTOP_SURFACE.into()];
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
        managed_capabilities: Some(managed),
        channels: None,
    }
}
#[test]
fn desktop_enable_requires_exact_placement_and_legacy_saves_preserve_it_across_restart() {
    let (_root, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:base", 0)).unwrap();
    let core_path = store.template.core_config_path.clone();
    let original = std::fs::read(&core_path).unwrap();
    assert!(store
        .apply_settings_v3(update("save:no-placement", 1, true))
        .is_err());
    assert_eq!(std::fs::read(&core_path).unwrap(), original);
    let placement = placement(&store);
    drop(store);
    let mut store =
        ProductConfigStore::open_with_installed_placements(&path, None, Some(placement.clone()))
            .unwrap()
            .into_inner()
            .unwrap();
    assert_eq!(std::fs::read(&core_path).unwrap(), original);
    assert!(
        !store
            .snapshot_settings_v3()
            .unwrap()
            .managed_capabilities
            .computer
            .enabled
    );
    let mut wrong = update("save:wrong-scope", 1, true);
    wrong.managed_capabilities.as_mut().unwrap().computer.scope =
        p::Scope("workspace:other".into());
    assert!(store.apply_settings_v3(wrong).is_err());
    assert_eq!(std::fs::read(&core_path).unwrap(), original);
    store
        .apply_settings_v3(update("save:desktop", 1, true))
        .unwrap();
    assert_eq!(store.state.schema_version, p::SchemaVersion(11));
    assert!(store.enabled_desktop().is_ok());
    store.apply(patch("save:legacy", 2)).unwrap();
    let core: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&core_path).unwrap()).unwrap();
    assert_eq!(
        core["brain"]["desktop_placement"]["binding"]["sandbox_profile_ref"],
        INSTALLED_DESKTOP_PROFILE
    );
    assert_eq!(
        core["brain"]["managed_capabilities"]["computer"]["enabled"],
        true
    );
    drop(store);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    assert!(store.enabled_desktop().is_ok());
    assert!(store.state.desktop.as_ref() == Some(&placement));
    store
        .apply_settings_v3(update("save:disable-desktop", 3, false))
        .unwrap();
    assert!(store.enabled_desktop().is_err());
    store.apply(patch("save:legacy-after-disable", 4)).unwrap();
    assert!(
        !store
            .snapshot_settings_v3()
            .unwrap()
            .managed_capabilities
            .computer
            .enabled
    );
    drop(store);
    assert!(ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap()
        .enabled_desktop()
        .is_err());
}
#[test]
fn desktop_projection_rejects_file_profile_foreign_endpoint_and_unknown_fields() {
    let (_root, _path, store) = fixture();
    let store = store.into_inner().unwrap();
    let value = serde_json::to_value(placement(&store)).unwrap();
    for case in 0..5 {
        let mut changed = value.clone();
        match case {
            0 => changed["binding"]["sandbox_profile_ref"] = "sandbox:installed-file-v1".into(),
            1 => changed["binding"]["backend_kinds"] = serde_json::json!(["File", "Computer"]),
            2 => {
                changed["binding"]["endpoint"]["name"] =
                    r"\\.\pipe\forme-agent-execution-v1-foreign".into()
            }
            3 => changed["scope"] = "workspace:foreign".into(),
            _ => changed["helper_program"] = "caller-controlled.exe".into(),
        }
        let parsed = serde_json::from_value::<InstalledDesktopProvisioning>(changed);
        assert!(parsed.is_err() || parsed.unwrap().validate_template(&store.template).is_err());
    }
}
