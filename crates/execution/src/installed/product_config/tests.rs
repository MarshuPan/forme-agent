use super::*;
use std::fs;

fn template(root: &Path) -> p::ProductConfigTemplate {
    p::ProductConfigTemplate {
        schema_version: p::SchemaVersion(1),
        install_ref: "install:config-test".into(),
        core_config_path: root.join("config/core.json"),
        configuration_ack_path: root.join("config/ack.json"),
        model_credentials_directory: root.join("keys/models"),
        brain_store_path: root.join("brain/forme.db"),
        artifact_root: root.join("brain/artifacts"),
        core_owner_credential_path: root.join("keys/owner.token"),
        core_web_credential_path: root.join("keys/web.token"),
        core_web_assertion_verifying_key: [3; 32],
        core_session_fence_path: root.join("auth/fence.sqlite"),
        core_bind: "127.0.0.1:8770".parse().unwrap(),
        workspace: p::WorkspaceRef("workspace:test".into()),
        agent_profile: p::AgentProfileRef("agent:forme".into()),
        scheduler_tick: p::DurationMs(1000),
        scheduler_lease: p::DurationMs(30000),
        scheduler_max_claims: 1,
        cognition_tick: p::DurationMs(60000),
        stable_evidence_max_age: p::DurationMs(86400000),
        authority_lease: p::DurationMs(30000),
        drain_timeout: p::DurationMs(30000),
    }
}

fn profile() -> p::ProductModelProfilePatch {
    p::ProductModelProfilePatch {
        schema_version: p::SchemaVersion(1),
        id: "model:test".into(),
        provider: "openai-compatible".into(),
        model: "fixture".into(),
        base_url: "https://provider.invalid/v1".into(),
        credential: Some("fixture-key-only".into()),
        context_window: 128000,
        tool_use: true,
        strength: "standard".into(),
        input_microunits_per_million: 0,
        output_microunits_per_million: 0,
        requests_per_minute: 60,
        tokens_per_minute: 1000000,
        timeout_ms: 120000,
        fallback_group: None,
        default: true,
        tool_schema_versions: vec![1],
    }
}

pub(crate) fn patch(operation: &str, revision: u64) -> p::ProductConfigPatch {
    p::ProductConfigPatch {
        schema_version: p::SchemaVersion(1),
        operation_id: operation.into(),
        expected_revision: revision,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        models: vec![profile()],
        brain: None,
    }
}

pub(crate) fn fixture() -> (tempfile::TempDir, PathBuf, Mutex<ProductConfigStore>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("template.json");
    fs::write(&path, storage::encode(&template(directory.path())).unwrap()).unwrap();
    let store = ProductConfigStore::open(&path).unwrap();
    store
        .lock()
        .unwrap()
        .bind_owner(p::ProductConfigOwnerBinding {
            schema_version: p::SchemaVersion(1),
            operation_id: "claim:test".into(),
            expected_revision: 0,
            owner: p::OwnerPrincipalRef("owner:test".into()),
        })
        .unwrap();
    (directory, path, store)
}

#[test]
fn configuration_replays_old_operations_after_newer_saves_and_restart() {
    let (_directory, path, store) = fixture();
    let first = patch("save:one", 0);
    store.lock().unwrap().apply(first.clone()).unwrap();
    store.lock().unwrap().apply(patch("save:two", 1)).unwrap();
    drop(store);
    let store = ProductConfigStore::open(&path).unwrap();
    let mut store = store.lock().unwrap();
    let replay = store.apply(first.clone()).unwrap();
    assert!(!replay.applied);
    assert_eq!(replay.view.active_revision, 2);
    let mut altered = first;
    altered.models[0].model = "different".into();
    assert!(store.apply(altered).unwrap_err().0.contains("reused"));
    assert_eq!(store.snapshot().unwrap().active_revision, 2);
    assert!(!format!("{:?}", replay).contains("fixture-key-only"));
}

#[test]
fn invalid_late_profile_does_not_rotate_any_live_key_or_configuration() {
    let (_directory, _path, store) = fixture();
    let mut store = store.lock().unwrap();
    store.apply(patch("save:one", 0)).unwrap();
    let live_key = store.state.credentials["model:test"].clone();
    let core = fs::read(&store.template.core_config_path).unwrap();
    let mut bad = patch("save:bad", 1);
    bad.models[0].credential = Some("uncommitted-replacement".into());
    let mut second = profile();
    second.id = "model:missing".into();
    second.default = false;
    second.credential = None;
    bad.models.push(second);
    assert!(store.apply(bad).is_err());
    assert_eq!(fs::read(&live_key).unwrap(), b"fixture-key-only");
    assert_eq!(fs::read(&store.template.core_config_path).unwrap(), core);
    assert_eq!(store.snapshot().unwrap().active_revision, 1);
}

#[test]
fn rotated_keys_keep_old_revision_immutable_and_validate_is_not_model_test() {
    let (_directory, _path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:one", 0)).unwrap();
    let old = store.state.credentials["model:test"].clone();
    let mut next = patch("save:two", 1);
    next.models[0].credential = Some("rotated-fixture-only".into());
    store.apply(next).unwrap();
    assert_ne!(store.state.credentials["model:test"], old);
    assert_eq!(fs::read(old).unwrap(), b"fixture-key-only");
    let report = store
        .validate_revision(2, &p::OwnerPrincipalRef("owner:test".into()))
        .unwrap();
    assert_eq!(report.model_test_status, p::ProductModelTestStatus::NotRun);
    assert!(report.valid && report.restart_required);
    let journal_path = store
        .template
        .core_config_path
        .with_extension("journal.jsonl");
    drop(store);
    // The journal's Windows lock also denies unrelated read handles.
    let journal = fs::read_to_string(journal_path).unwrap();
    for forbidden in [
        "fixture-key-only",
        "rotated-fixture-only",
        "provider.invalid",
        ".secret",
    ] {
        assert!(!journal.contains(forbidden));
    }
}

#[test]
fn pending_publication_recovers_original_revision_without_reapplying_command() {
    let (_directory, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:one", 0)).unwrap();
    let mut state = store.state.clone();
    state.revision = 2;
    state.models[0].model = "after-crash".into();
    let reference = materialize::stage(&store.template, &state).unwrap();
    store
        .journal
        .prepare(Intent {
            operation: "save:interrupted".into(),
            digest: p::canonical_digest(&"interrupted").unwrap(),
            snapshot: reference,
        })
        .unwrap();
    drop(store);
    let store = ProductConfigStore::open(&path).unwrap();
    let mut store = store.lock().unwrap();
    assert_eq!(store.snapshot().unwrap().active_revision, 2);
    assert!(store.journal.pending().is_none());
    let core: serde_json::Value =
        storage::json(&store.template.core_config_path, MAX_CONFIG_BYTES).unwrap();
    assert_eq!(
        core["brain"]["models"][0]["profile"]["model"],
        "after-crash"
    );
}

#[test]
fn corrupt_pending_files_and_torn_journal_fail_closed() {
    let (_directory, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    let reference = materialize::stage(&store.template, &store.state).unwrap();
    store
        .journal
        .prepare(Intent {
            operation: "claim:interrupted".into(),
            digest: p::canonical_digest(&"interrupted").unwrap(),
            snapshot: reference.clone(),
        })
        .unwrap();
    fs::write(
        materialize::revision_directory(&store.template, &reference.id).join("state.json"),
        b"{}",
    )
    .unwrap();
    let journal_path = store
        .template
        .core_config_path
        .with_extension("journal.jsonl");
    drop(store);
    assert!(ProductConfigStore::open(&path).is_err());
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(&journal_path)
        .unwrap()
        .write_all(b"{\"partial\":")
        .unwrap();
    assert!(ProductConfigStore::open(&path).is_err());
    assert!(fs::read_to_string(journal_path)
        .unwrap()
        .ends_with("{\"partial\":"));
}

#[test]
fn second_writer_stale_revision_and_other_owner_cannot_change_state() {
    let (_directory, path, store) = fixture();
    assert!(ProductConfigStore::open(&path).is_err());
    let mut store = store.lock().unwrap();
    store.apply(patch("save:one", 0)).unwrap();
    assert!(store.apply(patch("save:stale", 0)).is_err());
    let mut other = patch("save:other", 1);
    other.owner = p::OwnerPrincipalRef("owner:other".into());
    assert!(store.apply(other).is_err());
    assert_eq!(store.snapshot().unwrap().active_revision, 1);
}

#[test]
fn static_invalid_endpoint_and_unbound_owner_are_rejected_before_publishing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("template.json");
    fs::write(&path, storage::encode(&template(directory.path())).unwrap()).unwrap();
    let store = ProductConfigStore::open(&path).unwrap();
    assert!(store
        .lock()
        .unwrap()
        .apply(patch("save:unbound", 0))
        .is_err());
    drop(store);
    let (_directory, _path, store) = fixture();
    let mut store = store.lock().unwrap();
    for url in [
        "file:///etc/passwd",
        "https://key@provider.invalid/v1",
        "https://provider.invalid/v1?key=private",
    ] {
        let mut request = patch("save:bad", 0);
        request.models[0].base_url = url.into();
        assert!(store.apply(request).is_err());
    }
    assert!(!store.template.core_config_path.exists());
}

#[test]
fn existing_empty_configuration_journal_is_not_a_new_installation() {
    let (_directory, path, store) = fixture();
    let journal = store
        .lock()
        .unwrap()
        .template
        .core_config_path
        .with_extension("journal.jsonl");
    drop(store);
    fs::write(&journal, b"").unwrap();
    assert!(ProductConfigStore::open(&path).is_err());
    assert_eq!(fs::metadata(journal).unwrap().len(), 0);
}

#[test]
fn missing_journal_cannot_erase_bound_owner_at_revision_zero() {
    let (_directory, path, store) = fixture();
    let journal = store
        .lock()
        .unwrap()
        .template
        .core_config_path
        .with_extension("journal.jsonl");
    drop(store);
    fs::remove_file(&journal).unwrap();
    assert!(ProductConfigStore::open(&path).is_err());
    assert!(!journal.exists());
}

#[test]
fn pending_core_must_match_template_before_replacing_published_configuration() {
    let (_directory, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:one", 0)).unwrap();
    let core_path = store.template.core_config_path.clone();
    let original = fs::read(&core_path).unwrap();
    let mut state = store.state.clone();
    state.revision = 2;
    let mut reference = materialize::stage(&store.template, &state).unwrap();
    let mut core = materialize::core_config(&store.template, &state).unwrap();
    core["brain"]["owner"] = "owner:foreign".into();
    let bytes = storage::encode(&core).unwrap();
    fs::write(
        materialize::revision_directory(&store.template, &reference.id).join("core.json"),
        &bytes,
    )
    .unwrap();
    reference.core_digest = Some(materialize::bytes_digest(&bytes));
    store
        .journal
        .prepare(Intent {
            operation: "save:invalid-staging".into(),
            digest: p::canonical_digest(&"invalid-staging").unwrap(),
            snapshot: reference,
        })
        .unwrap();
    drop(store);
    assert!(ProductConfigStore::open(&path).is_err());
    assert_eq!(fs::read(core_path).unwrap(), original);
}

fn legacy_fixture(mismatch: bool) -> (tempfile::TempDir, PathBuf, Vec<u8>, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let template = template(directory.path());
    let path = directory.path().join("template.json");
    fs::write(&path, storage::encode(&template).unwrap()).unwrap();
    storage::directory(&template.model_credentials_directory).unwrap();
    let credential = template.model_credentials_directory.join(format!(
        "{}.secret",
        p::canonical_digest(&("forme-model-credential-path-v1", "model:test"))
            .unwrap()
            .0
            .trim_start_matches("sha256:")
    ));
    fs::write(&credential, b"fixture-key-only").unwrap();
    let models = materialize::prepare_models(&[profile()], &BTreeMap::new()).unwrap();
    let state = ConfigState {
        schema_version: p::SchemaVersion(2),
        install_ref: template.install_ref.clone(),
        owner: Some(p::OwnerPrincipalRef("owner:test".into())),
        revision: 1,
        models: models.into_iter().map(|(view, _)| view).collect(),
        credentials: BTreeMap::from([("model:test".into(), credential.clone())]),
        active_data: None,
        brain: None,
    };
    let mut core = materialize::core_config(&template, &state).unwrap();
    if mismatch {
        core["brain"]["owner"] = "owner:foreign".into();
    }
    storage::directory(template.core_config_path.parent().unwrap()).unwrap();
    fs::write(&template.core_config_path, storage::encode(&core).unwrap()).unwrap();
    let legacy = serde_json::json!({ "schema_version":1, "install_ref":template.install_ref,
        "owner":state.owner, "active_revision":1, "acknowledged_revision":1, "pending_restart":false,
        "models":state.models, "last_operation_id":"save:legacy", "last_patch_digest":p::canonical_digest(&"legacy").unwrap() });
    let bytes = storage::encode(&legacy).unwrap();
    fs::write(
        template.core_config_path.with_extension("state.json"),
        &bytes,
    )
    .unwrap();
    (directory, path, bytes, credential)
}

#[test]
fn known_legacy_import_preserves_original_files_and_acknowledgement() {
    let (_directory, path, legacy, key) = legacy_fixture(false);
    let store = ProductConfigStore::open(&path).unwrap();
    let mut store = store.into_inner().unwrap();
    let view = store.snapshot().unwrap();
    assert_eq!(view.active_revision, 1);
    assert_eq!(view.acknowledged_revision, Some(1));
    assert!(!view.pending_restart);
    store.apply(patch("save:post-import", 1)).unwrap();
    assert_eq!(fs::read(&key).unwrap(), b"fixture-key-only");
    assert_ne!(store.state.credentials["model:test"], key);
    assert_eq!(
        fs::read(store.template.core_config_path.with_extension("state.json")).unwrap(),
        legacy
    );
    drop(store);
    assert_eq!(
        ProductConfigStore::open(&path)
            .unwrap()
            .lock()
            .unwrap()
            .snapshot()
            .unwrap()
            .active_revision,
        2
    );
}

#[test]
fn mismatched_legacy_import_never_rewrites_original_core_or_key() {
    let (_directory, path, legacy, key) = legacy_fixture(true);
    let template: p::ProductConfigTemplate = storage::json(&path, MAX_CONFIG_BYTES).unwrap();
    let core = fs::read(&template.core_config_path).unwrap();
    for _ in 0..2 {
        assert!(ProductConfigStore::open(&path).is_err());
    }
    assert_eq!(fs::read(&template.core_config_path).unwrap(), core);
    assert_eq!(
        fs::read(template.core_config_path.with_extension("state.json")).unwrap(),
        legacy
    );
    assert_eq!(fs::read(key).unwrap(), b"fixture-key-only");
}

#[test]
fn restored_binding_survives_model_save_reopen_and_rejects_stale_ack() {
    let (_directory, path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:before-restore", 0)).unwrap();
    let binding = p::ProductDataBinding {
        schema_version: p::PRODUCT_DATA_BINDING_SCHEMA_VERSION,
        candidate_ref: "restored-brain.db".into(),
        store_path: store
            .template
            .brain_store_path
            .parent()
            .unwrap()
            .join("data-operations/restored-brain.db"),
        content_key_directory: store
            .template
            .brain_store_path
            .with_extension("content-keys"),
        owner_digest: p::sha256_content_digest(b"owner:test"),
        store_identity: p::sha256_content_digest(b"store-fixture"),
        manifest_digest: p::sha256_content_digest(b"manifest-fixture"),
        execution_fence_digest: Some(p::sha256_content_digest(b"fence-fixture")),
    };
    // Exercise only publication of a host-verified result. This fixture does
    // not claim to execute the restore worker or its native lifecycle.
    let mut state = store.state.clone();
    state.revision = 2;
    state.active_data = Some(binding.clone());
    let mut old_state = state.clone();
    old_state.schema_version = p::SchemaVersion(2);
    assert!(old_state.validate(&store.template).is_err());
    store
        .commit_state(
            "restore:fixture".into(),
            p::canonical_digest(&binding).unwrap(),
            state,
        )
        .unwrap();
    assert!(store
        .acknowledge(p::ProductConfigAcknowledgement {
            schema_version: p::SchemaVersion(1),
            install_ref: store.template.install_ref.clone(),
            revision: 1,
            data_identity: None,
        })
        .is_err());
    let mut ack = p::ProductConfigAcknowledgement {
        schema_version: p::SchemaVersion(1),
        install_ref: store.template.install_ref.clone(),
        revision: 2,
        data_identity: None,
    };
    assert!(store.acknowledge(ack.clone()).is_err());
    let identity = p::ProductDataAcknowledgement {
        schema_version: p::SchemaVersion(1),
        owner_digest: binding.owner_digest.clone(),
        store_identity: binding.store_identity.clone(),
        execution_fence_digest: binding.execution_fence_digest.clone(),
        composition_digest: p::sha256_content_digest(b"actual-composition-fixture"),
    };
    ack.data_identity = Some(p::ProductDataAcknowledgement {
        store_identity: p::sha256_content_digest(b"another-store"),
        ..identity.clone()
    });
    assert!(store.acknowledge(ack.clone()).is_err());
    assert!(store.snapshot().unwrap().pending_restart);
    ack.data_identity = Some(identity);
    store.acknowledge(ack).unwrap();
    assert!(!store.snapshot().unwrap().pending_restart);
    let saved = store.apply(patch("save:after-restore", 2)).unwrap();
    assert_eq!(saved.view.active_revision, 3);
    assert!(saved.view.pending_restart);
    drop(store);
    let store = ProductConfigStore::open(&path).unwrap();
    let mut store = store.into_inner().unwrap();
    assert_eq!(store.snapshot().unwrap().active_revision, 3);
    assert_eq!(store.state.active_data, Some(binding.clone()));
    let core: serde_json::Value =
        storage::json(&store.template.core_config_path, MAX_CONFIG_BYTES).unwrap();
    assert_eq!(
        core["brain"]["store_path"],
        binding.store_path.to_string_lossy().as_ref()
    );
    assert_eq!(
        core["brain"]["content_key_directory"],
        binding.content_key_directory.to_string_lossy().as_ref()
    );
    assert_eq!(
        core["brain"]["expected_restoration_fence"],
        serde_json::to_value(binding.execution_fence_digest).unwrap()
    );
}
