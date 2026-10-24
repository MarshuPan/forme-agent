//! Managed skills use the existing durable configuration CAS, never a second registry.
use super::{
    tests::{fixture, patch},
    *,
};
use std::fs;

fn skills() -> p::ProductCapabilitySettings {
    serde_json::from_value(serde_json::json!({"schema_version":1,"skills":[{
        "schema_version":1,"id":"skill:fixture","summary":"fixture knowledge",
        "scope":"workspace:test","version":2,"body":"private-skill-fixture-body-82",
        "enabled":true,"trust":"owner_input"
    }]}))
    .unwrap()
}

#[test]
fn skill_settings_preserve_legacy_digest_materialization_old_saves_and_explicit_clear() {
    let (_dir, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    let legacy = patch("skills:legacy", 0);
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
    assert!(store.snapshot().unwrap().capabilities.skills.is_empty());
    store.apply(legacy.clone()).unwrap();
    let old = fs::read(&store.template.core_config_path).unwrap();
    assert!(
        serde_json::from_slice::<serde_json::Value>(&old).unwrap()["brain"]
            .get("capabilities")
            .is_none()
    );
    let mut change = patch("skills:save", 1);
    change.capabilities = Some(skills());
    let saved = store.apply(change.clone()).unwrap();
    assert_eq!(saved.view.capabilities, skills());
    assert!(saved.view.pending_restart);
    assert_eq!(
        saved.view.capabilities_source,
        p::ProductPreferenceSource::ManagedConfiguration
    );
    assert_eq!(store.state.schema_version, p::SchemaVersion(5));
    let core: serde_json::Value =
        storage::json(&store.template.core_config_path, MAX_CONFIG_BYTES).unwrap();
    assert_eq!(
        core["brain"]["capabilities"],
        serde_json::to_value(skills()).unwrap()
    );
    store.apply(patch("skills:old-client", 2)).unwrap();
    drop(store);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    assert_eq!(store.snapshot().unwrap().capabilities, skills());
    assert!(!store.apply(change.clone()).unwrap().applied);
    assert!(!store.apply(legacy).unwrap().applied);
    change.capabilities.as_mut().unwrap().skills[0].body = "changed".into();
    assert!(store.apply(change).is_err());
    let mut clear = patch("skills:clear", 3);
    clear.capabilities = Some(Default::default());
    store.apply(clear).unwrap();
    drop(store);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    assert!(store.snapshot().unwrap().capabilities.skills.is_empty());
    assert_eq!(store.state.schema_version, p::SchemaVersion(5));
}

#[test]
fn invalid_stale_duplicate_and_authority_injecting_skills_have_zero_publication() {
    let (_dir, _path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("skills:first", 0)).unwrap();
    let original = fs::read(&store.template.core_config_path).unwrap();
    let credential_count = fs::read_dir(&store.template.model_credentials_directory)
        .unwrap()
        .count();
    let valid = serde_json::to_value(skills()).unwrap();
    for field in ["permissions", "grant", "command", "environment", "actor"] {
        let mut injected = valid.clone();
        injected["skills"][0][field] = serde_json::json!("forbidden");
        assert!(serde_json::from_value::<p::ProductCapabilitySettings>(injected).is_err());
    }
    let mut forged = valid.clone();
    forged["skills"][0]["trust"] = "VerifiedProcess".into();
    assert!(serde_json::from_value::<p::ProductCapabilitySettings>(forged).is_err());
    for (index, bad) in [
        {
            let mut s = skills();
            s.schema_version = p::SchemaVersion(2);
            s
        },
        {
            let mut s = skills();
            s.skills.push(s.skills[0].clone());
            s
        },
        {
            let mut s = skills();
            s.skills[0].body = "界".repeat(5_462);
            s
        },
        {
            let mut s = skills();
            s.skills[0].body = "bad\0control".into();
            s
        },
        {
            let mut s = skills();
            s.skills[0].scope.0 = "../outside".into();
            s
        },
        {
            let mut s = skills();
            s.skills[0].version = p::Version(0);
            s
        },
        {
            let mut s = skills();
            s.skills[0].summary = " ".into();
            s
        },
        {
            let mut s = skills();
            s.skills[0].schema_version = p::SchemaVersion(2);
            s
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = patch(&format!("skills:bad-{index}"), 1);
        request.capabilities = Some(bad);
        assert!(store.apply(request).is_err());
    }
    let mut stale = patch("skills:stale", 0);
    stale.capabilities = Some(skills());
    assert!(store.apply(stale).is_err());
    assert_eq!(store.state.revision, 1);
    assert_eq!(
        fs::read(&store.template.core_config_path).unwrap(),
        original
    );
    assert_eq!(
        fs::read_dir(&store.template.model_credentials_directory)
            .unwrap()
            .count(),
        credential_count
    );
    let mut defaults = valid;
    defaults["skills"][0]
        .as_object_mut()
        .unwrap()
        .remove("trust");
    defaults["skills"][0]
        .as_object_mut()
        .unwrap()
        .remove("enabled");
    let parsed: p::ProductCapabilitySettings = serde_json::from_value(defaults).unwrap();
    assert!(!parsed.skills[0].enabled);
    assert_eq!(parsed.skills[0].trust, p::ProductSkillTrust::Untrusted);
    assert!(!format!("{:?}", skills()).contains("private-skill-fixture-body"));
}

#[test]
fn interrupted_skill_publication_recovers_only_the_original_journal_intent() {
    let (_dir, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("skills:before-interruption", 0)).unwrap();
    let mut update = patch("skills:interrupted", 1);
    update.capabilities = Some(skills());
    let mut next = store.state.clone();
    next.schema_version = p::SchemaVersion(5);
    next.revision = 2;
    next.capabilities = Some(skills());
    let snapshot = materialize::stage(&store.template, &next).unwrap();
    store
        .journal
        .prepare(Intent {
            operation: update.operation_id.clone(),
            digest: update.digest().unwrap(),
            snapshot,
        })
        .unwrap();
    drop(store);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    assert!(!store.apply(update).unwrap().applied);
    assert_eq!(store.snapshot().unwrap().capabilities, skills());
    assert_eq!(store.state.revision, 2);
    assert!(store.journal.pending().is_none());
}

#[test]
fn restore_and_subsequent_model_save_keep_the_exact_managed_skills() {
    let (_dir, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    let mut save = patch("skills:before-restore", 0);
    save.capabilities = Some(skills());
    store.apply(save).unwrap();
    let binding = p::ProductDataBinding {
        schema_version: p::SchemaVersion(1),
        candidate_ref: "restored-skills.db".into(),
        store_path: store
            .template
            .brain_store_path
            .parent()
            .unwrap()
            .join("data-operations/restored-skills.db"),
        content_key_directory: store
            .template
            .brain_store_path
            .with_extension("content-keys"),
        owner_digest: p::sha256_content_digest(b"owner:test"),
        store_identity: p::sha256_content_digest(b"skills:store"),
        manifest_digest: p::sha256_content_digest(b"skills:manifest"),
        execution_fence_digest: Some(p::sha256_content_digest(b"skills:fence")),
    };
    let request = p::ProductRestoreRequest {
        schema_version: p::SchemaVersion(1),
        install_ref: store.template.install_ref.clone(),
        operation_id: "restore:skills".into(),
        command_digest: p::sha256_content_digest(b"restore:skills"),
        expected_config_revision: 1,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        candidate_ref: binding.candidate_ref.clone(),
        candidate_digest: binding.manifest_digest.clone(),
    };
    assert_eq!(
        store
            .publish_restored_data(&request, binding.clone())
            .unwrap()
            .capabilities,
        skills()
    );
    store.apply(patch("skills:post-restore", 2)).unwrap();
    drop(store);
    let mut store = ProductConfigStore::open(&path)
        .unwrap()
        .into_inner()
        .unwrap();
    assert_eq!(store.snapshot().unwrap().capabilities, skills());
    assert_eq!(store.state.active_data, Some(binding));
    assert_eq!(store.state.schema_version, p::SchemaVersion(5));
}

#[test]
fn aggregate_size_rejection_precedes_any_new_credentials_or_configuration_files() {
    let (_dir, _path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("skills:before-size-test", 0)).unwrap();
    let original = fs::read(&store.template.core_config_path).unwrap();
    let credential_count = fs::read_dir(&store.template.model_credentials_directory)
        .unwrap()
        .count();
    let mut request = patch("skills:too-large-state", 1);
    request.capabilities = Some(skills());
    let prototype = request.models[0].clone();
    request.models = (0..32)
        .map(|index| {
            let mut model = prototype.clone();
            model.id = format!("model:size-{index}");
            model.default = index == 0;
            model.provider = "p".repeat(2_048);
            model.model = "m".repeat(2_048);
            model
        })
        .collect();
    request.validate().unwrap();
    assert!(store.apply(request).is_err());
    assert_eq!(store.state.revision, 1);
    assert_eq!(
        fs::read(&store.template.core_config_path).unwrap(),
        original
    );
    assert_eq!(
        fs::read_dir(&store.template.model_credentials_directory)
            .unwrap()
            .count(),
        credential_count
    );
}
