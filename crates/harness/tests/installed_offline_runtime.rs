//! Installed and development compositions share one data/key lineage. Offline
//! inspection and recovery never call a provider or allocate a second brain.
#![forbid(unsafe_code)]
#[path = "support/core_brain.rs"]
mod core_brain;
use core_brain::Fixture;
use forme_harness::{
    offline_data::OfflineBrainMaintenance, ManagedBrainConfig, ProductionRuntimeBuilder,
};
use forme_protocol as p;
use std::fs;

fn managed(f: &Fixture) -> ManagedBrainConfig {
    let c = &f.config;
    let key = c.store_path.with_file_name("model-fixture.key");
    fs::write(&key, "core-b-local-fixture-key").unwrap();
    let model = &c.models[0];
    ManagedBrainConfig {
        schema_version: c.schema_version,
        owner: c.owner.clone(),
        store_path: c.store_path.clone(),
        content_key_directory: None,
        expected_restoration_fence: None,
        artifact_root: c.data.artifact_root.clone(),
        workspace: c.workspace.clone(),
        agent_profile: c.agent_profile.clone(),
        models: serde_json::from_value(serde_json::json!([{"schema_version":1,
            "profile":{"id":model.id,"provider":model.provider,"model":model.model,
                "base_url":model.base_url,"credential_env":model.credential_env,"default":true},
            "credential_path":key}]))
        .unwrap(),
        scheduler_enabled: false,
        scheduler: c.scheduler.clone(),
        cognition: c.cognition.clone(),
        authority_lease: c.authority_lease,
    }
}

#[test]
fn installed_offline_facade_restarts_a_restored_brain_with_the_original_key_vault() {
    let f = Fixture::new();
    let config = managed(&f);
    let runtime = f.build();
    let binding = runtime.services().authority.store_identity.clone();
    assert!(OfflineBrainMaintenance::open(&config, &f.owner(), None).is_err());
    drop(runtime);
    let root = f.config.store_path.parent().unwrap();
    let original = fs::read(&config.store_path).unwrap();
    let offline = OfflineBrainMaintenance::open(&config, &f.owner(), None).unwrap();
    assert_eq!(offline.identity().store_identity, binding);
    let backup = root.join("owner-backup.db");
    offline.backup(&backup).unwrap();
    drop(offline);
    let offline = OfflineBrainMaintenance::open(&config, &f.owner(), Some(&backup)).unwrap();
    fs::create_dir(root.join("restored-candidates")).unwrap();
    let target = root.join("restored-candidates/restored-brain.db");
    let plan = offline.plan_restore(&target).unwrap();
    let receipt = offline.restore(&target, &plan, &plan.digest).unwrap();
    assert!(!receipt.authority_switched);
    drop(offline);
    let mut restored = config.clone();
    restored.store_path = target.clone();
    assert!(
        ProductionRuntimeBuilder::from_managed_config(restored.clone())
            .unwrap()
            .build()
            .is_err()
    );
    assert!(!target.with_extension("content-keys").exists());
    restored.content_key_directory = Some(config.content_key_directory().unwrap());
    assert!(OfflineBrainMaintenance::open(&restored, &f.owner(), None).is_err());
    assert!(
        ProductionRuntimeBuilder::from_managed_config(restored.clone())
            .unwrap()
            .build()
            .is_err()
    );
    restored.expected_restoration_fence = Some(receipt.execution_fence_digest.clone());
    assert!(OfflineBrainMaintenance::open(&restored, &f.owner(), None).is_ok());
    let runtime = ProductionRuntimeBuilder::from_managed_config(restored.clone())
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.services().authority.store_identity, binding);
    assert_eq!(runtime.owner(), &f.config.owner);
    runtime
        .create_data_backup(&f.owner(), "after-restore.db")
        .unwrap();
    assert!(root.join("data-operations/after-restore.db").is_file());
    assert!(!root.join("restored-candidates/data-operations").exists());
    drop(runtime);
    assert_eq!(fs::read(&config.store_path).unwrap(), original);
    assert_eq!(f.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(OfflineBrainMaintenance::open(
        &restored,
        &p::VerifiedPrincipal("owner:foreign".into()),
        None
    )
    .is_err());
}

#[test]
fn installed_restart_never_regenerates_a_missing_original_vault() {
    let f = Fixture::new();
    let config = managed(&f);
    drop(f.build());
    let keys = config.content_key_directory().unwrap();
    let held = keys.with_extension("held-keys");
    fs::rename(&keys, &held).unwrap();
    assert!(
        ProductionRuntimeBuilder::from_managed_config(config.clone())
            .unwrap()
            .build()
            .is_err()
    );
    assert!(!keys.exists());
    fs::rename(held, keys).unwrap();
    let runtime = ProductionRuntimeBuilder::from_managed_config(config)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(runtime.owner(), &f.config.owner);
}
