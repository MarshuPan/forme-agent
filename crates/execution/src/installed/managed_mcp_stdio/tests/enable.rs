//! Public save -> isolated discovery/call -> disable, using real worker facts.
use super::*;

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_narrow_settings_enable_and_disable_reach_the_same_worker_authority() {
    let mut fixture = Fixture::create("normal", 10_000, false);
    config_tests::provision_mcp_fixture(
        &mut fixture.config.lock().unwrap(),
        fixture._temp.path(),
        &fixture.programs,
    );
    fixture
        .config
        .lock()
        .unwrap()
        .apply_settings_v3(save(&fixture, "configure:native", 0, false))
        .unwrap();
    let enable = p::ProductMcpSettingsPatch {
        schema_version: p::SchemaVersion(1),
        operation_id: "narrow:enable".into(),
        expected_revision: 1,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        mcp_servers: vec![fixture.settings.clone()],
    };
    assert!(fixture
        .config
        .lock()
        .unwrap()
        .apply_mcp_settings(enable.clone())
        .is_err());
    let auth = auth(&fixture);
    let worker = host(&fixture);
    let saved = fixture
        .config
        .lock()
        .unwrap()
        .apply_mcp_settings(enable.clone())
        .unwrap();
    assert!(saved.view.available);
    assert!(saved.view.mcp_servers[0].enabled);
    fixture.request = request(&fixture.registration, &fixture.settings);
    discover(&fixture, &worker, &auth);
    let mut disable = enable.clone();
    disable.operation_id = "narrow:disable".into();
    disable.expected_revision = 2;
    disable.mcp_servers[0].enabled = false;
    fixture
        .config
        .lock()
        .unwrap()
        .apply_mcp_settings(disable)
        .unwrap();
    assert!(dispatch(&worker, &auth, &call(&fixture, "narrow-disabled", |_| {})).is_err());
    let replay = fixture
        .config
        .lock()
        .unwrap()
        .apply_mcp_settings(enable.clone())
        .unwrap();
    assert!(!replay.applied);
    assert!(!replay.view.mcp_servers[0].enabled);
    assert_eq!(
        fixture
            .config
            .lock()
            .unwrap()
            .mcp_settings_operation(&enable.operation_id)
            .unwrap()
            .request_digest,
        enable.digest().unwrap()
    );
}

fn save(f: &Fixture, operation: &str, revision: u64, enabled: bool) -> p::ProductConfigPatchV3 {
    let base = config_tests::patch(operation, revision);
    let mut settings = f.settings.clone();
    settings.enabled = enabled;
    p::ProductConfigPatchV3 {
        schema_version: p::SchemaVersion(3),
        operation_id: base.operation_id,
        expected_revision: revision,
        owner: base.owner,
        models: base.models,
        brain: None,
        capabilities: None,
        network: None,
        proxy: None,
        channels: None,
        managed_capabilities: Some(p::ProductManagedCapabilityPatch {
            schema_version: p::SchemaVersion(1),
            mcp_servers: vec![settings],
            plugins: vec![],
            publisher_keys: vec![],
            computer: Default::default(),
        }),
    }
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_public_enable_discovers_calls_and_disable_fences_without_rediscovery() {
    let mut fixture = Fixture::create("normal", 10_000, false);
    config_tests::provision_mcp_fixture(
        &mut fixture.config.lock().unwrap(),
        fixture._temp.path(),
        &fixture.programs,
    );
    let auth = auth(&fixture);
    let enable = save(&fixture, "enable:native", 0, true);
    // Program registration and installation placement alone do not enable it.
    assert!(fixture
        .config
        .lock()
        .unwrap()
        .apply_settings_v3(enable.clone())
        .is_err());
    assert!(!fixture.runtime.exists());
    let worker = host(&fixture);
    let saved = fixture
        .config
        .lock()
        .unwrap()
        .apply_settings_v3(enable.clone())
        .unwrap();
    assert!(saved.applied);
    assert_eq!(saved.view.active_revision, 1);
    assert!(saved.view.managed_capabilities.mcp_servers[0].enabled);
    assert!(!fixture
        .runtime
        .join("calls")
        .read_dir()
        .unwrap()
        .any(|entry| entry.unwrap().path().is_dir()));
    fixture.request = request(&fixture.registration, &fixture.settings);
    let original = discover(&fixture, &worker, &auth);
    let req = call(&fixture, "public-enabled", |_| {});
    assert_eq!(
        dispatch(&worker, &auth, &req).unwrap().state,
        p::WorkerState::Running
    );
    let completed = finish(&worker, &auth, &req);
    assert_eq!(completed.state, p::WorkerState::Completed);
    let (_, bytes) = read_body(&worker, &auth, &req);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["structuredContent"]["arguments"], json!({"value":7}));
    assert_eq!(
        serde_json::from_str::<Vec<String>>(body["content"][0]["text"].as_str().unwrap()).unwrap(),
        ["initialize", "notifications/initialized", "tools/call"]
    );
    fixture
        .config
        .lock()
        .unwrap()
        .apply_settings_v3(save(&fixture, "disable:native", 1, false))
        .unwrap();
    let next = call(&fixture, "disabled-call", |_| {});
    assert!(dispatch(&worker, &auth, &next).is_err());
    assert!(!directory(&fixture, &next).exists());
    let replay = fixture
        .config
        .lock()
        .unwrap()
        .apply_settings_v3(enable)
        .unwrap();
    assert!(!replay.applied);
    assert!(!replay.view.managed_capabilities.mcp_servers[0].enabled);
    drop(worker);
    let restarted = host(&fixture);
    assert!(dispatch(&restarted, &auth, &next).is_err());
    assert_eq!(
        dispatch(&restarted, &auth, &req).unwrap().receipt_ref,
        completed.receipt_ref
    );
    assert_eq!(
        restarted
            .result_manifest(&auth, &fixture.request.job.job_id, recovery_timestamp())
            .unwrap(),
        original
    );
    drop(restarted);
    let facts = fs::read_to_string(fixture._temp.path().join("call-worker.jsonl")).unwrap();
    assert_eq!(
        facts
            .lines()
            .filter(
                |line| serde_json::from_str::<Value>(line).unwrap()["fact"]["record"] == "started"
            )
            .count(),
        2
    );
}
