//! Real config journal and program identity; no installed-service acceptance claim.
use super::*;
use crate::installed::{
    managed_mcp_program::ManagedMcpProgramSource, worker::WorkerJournal,
    worker_results::WorkerResultStore, InstalledManagedMcpPrograms,
};
use std::{fs, sync::Arc};

fn patch(
    settings: p::ProductMcpServerConfig,
    operation: &str,
    revision: u64,
) -> p::ProductConfigPatchV3 {
    let base = tests::patch(operation, revision);
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
fn enable_requires_live_consumer_and_registered_program_and_publishes_separate_binding() {
    let (temp, template_path, config) = tests::fixture();
    let config = Arc::new(config);
    let placement = worker_tests::placement(&config.lock().unwrap(), temp.path());
    config.lock().unwrap().installed_file = Some(placement);
    let credential = config
        .lock()
        .unwrap()
        .mcp_placement()
        .unwrap()
        .credential_path;
    fs::create_dir_all(credential.parent().unwrap()).unwrap();
    fs::write(&credential, "a".repeat(32)).unwrap();
    let execution_authority = crate::installed::worker_results::installation_ref(
        &p::canonical_digest(&"a".repeat(32)).unwrap(),
    )
    .unwrap();
    let root = temp.path().join("programs");
    fs::create_dir(&root).unwrap();
    let program = root.join("fixture.exe");
    fs::write(&program, b"inert source, no process launched").unwrap();
    let source = ManagedMcpProgramSource {
        schema_version: p::SchemaVersion(1),
        source_ref: "source:config".into(),
        scope: p::Scope("workspace:default".into()),
        sandbox_profile_ref: p::MANAGED_MCP_PROGRAM_PROFILE.into(),
        expected_program_digest: crate::installed::file_digest(&program).unwrap(),
        program: program.clone(),
        args: vec![],
    };
    fs::write(
        root.join("sources.json"),
        serde_json::to_vec(&serde_json::json!({"schema_version":1,"sources":[source]})).unwrap(),
    )
    .unwrap();
    let programs = Arc::new(
        InstalledManagedMcpPrograms::new(
            temp.path().join("programs.jsonl"),
            "install:config-test".into(),
            p::AuthorityEpoch(1),
            config.clone(),
            root.clone(),
            vec![source.scope.clone()],
            vec![],
        )
        .unwrap(),
    );
    let registration = programs
        .with_registry(|r| {
            let c = r.catalog()?;
            r.apply(
                p::ManagedMcpProgramCommand {
                    schema_version: p::SchemaVersion(1),
                    installation_ref: c.installation_ref,
                    owner_ref: c.owner_ref,
                    authority_epoch: c.authority_epoch,
                    operation_id: "register:config".into(),
                    expected_revision: c.revision,
                    requested_deadline: 10_000,
                    action: p::ManagedMcpProgramAction::Register {
                        source_ref: c.choices[0].source_ref.clone(),
                        source_digest: c.choices[0].source_digest.clone(),
                    },
                },
                1_000,
            )
        })
        .unwrap();
    let settings = p::ProductMcpServerConfig {
        schema_version: p::SchemaVersion(1),
        server_ref: "server:config".into(),
        program_ref: registration.program.program_ref.clone(),
        scope: source.scope.clone(),
        enabled: true,
        trust: p::ProductSkillTrust::OwnerInput,
        timeout: p::DurationMs(1000),
        output_budget: 8192,
        allowlist: p::ProductMcpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: vec!["fixture.read".into()],
            resources: vec![],
        },
    };
    let request = patch(settings.clone(), "enable:config", 0);
    assert!(config
        .lock()
        .unwrap()
        .apply_settings_v3(request.clone())
        .is_err());
    config.lock().unwrap().bind_mcp_consumer(&programs);
    assert!(config
        .lock()
        .unwrap()
        .apply_settings_v3(request.clone())
        .is_err());
    let journal = Arc::new(WorkerJournal::open(&temp.path().join("worker.jsonl")).unwrap());
    let results = Arc::new(WorkerResultStore::open(&temp.path().join("results")).unwrap());
    programs
        .bind_source_reader(
            &journal,
            &results,
            &execution_authority,
            p::AuthorityEpoch(1),
        )
        .unwrap();
    fs::write(&credential, "b".repeat(32)).unwrap();
    assert!(config
        .lock()
        .unwrap()
        .apply_settings_v3(request.clone())
        .is_err());
    fs::write(&credential, "a".repeat(32)).unwrap();
    for mutation in 0..4 {
        let mut wrong = settings.clone();
        match mutation {
            0 => wrong.trust = p::ProductSkillTrust::Untrusted,
            1 => wrong.program_ref = format!("mcp-program:{}", "f".repeat(64)),
            2 => wrong.scope = p::Scope("workspace:other".into()),
            _ => wrong.allowlist.tools.clear(),
        }
        assert!(config
            .lock()
            .unwrap()
            .apply_settings_v3(patch(wrong, "invalid:settings", 0))
            .is_err());
        assert_eq!(
            config.lock().unwrap().snapshot().unwrap().active_revision,
            0
        );
    }
    let saved = config
        .lock()
        .unwrap()
        .apply_settings_v3(request.clone())
        .unwrap();
    assert!(saved.applied);
    let core_path = config.lock().unwrap().template.core_config_path.clone();
    let core: serde_json::Value = serde_json::from_slice(&fs::read(&core_path).unwrap()).unwrap();
    assert_eq!(
        core["brain"]["worker_binding"]["backend_kinds"],
        serde_json::json!(["File"])
    );
    assert_eq!(
        core["brain"]["mcp_worker_binding"]["backend_kinds"],
        serde_json::json!(["Mcp"])
    );
    assert_eq!(
        core["brain"]["mcp_worker_binding"]["sandbox_profile_ref"],
        p::MANAGED_MCP_PROGRAM_PROFILE
    );
    assert_eq!(
        config.lock().unwrap().state.schema_version,
        p::SchemaVersion(12)
    );
    let public = serde_json::to_string(&saved).unwrap();
    assert!(!public.contains("agent-execution.token"));
    assert!(!public.contains("mcp_worker_binding"));
    let old_bytes = fs::read(&core_path).unwrap();
    fs::write(&program, b"changed program").unwrap();
    assert!(config
        .lock()
        .unwrap()
        .apply_settings_v3(patch(settings.clone(), "changed:source", 1))
        .is_err());
    assert_eq!(fs::read(&core_path).unwrap(), old_bytes);
    let replay = config.lock().unwrap().apply_settings_v3(request).unwrap();
    assert!(!replay.applied);
    assert_eq!(fs::read(&core_path).unwrap(), old_bytes);
    drop(results);
    assert!(config
        .lock()
        .unwrap()
        .apply_settings_v3(patch(settings.clone(), "missing:worker", 1))
        .is_err());
    assert_eq!(fs::read(&core_path).unwrap(), old_bytes);
    assert_eq!(Arc::strong_count(&programs), 1); // Configuration retains only Weak.
    drop(programs);
    drop(config);
    let reopened = ProductConfigStore::open(&template_path).unwrap();
    assert_eq!(fs::read(&core_path).unwrap(), old_bytes);
    let mut disabled = settings;
    disabled.enabled = false;
    reopened
        .lock()
        .unwrap()
        .apply_settings_v3(patch(disabled, "disable:offline", 1))
        .unwrap();
    assert_eq!(
        reopened.lock().unwrap().state.schema_version,
        p::SchemaVersion(12)
    );
    // Legacy clients preserve the original placement and managed state.
    reopened
        .lock()
        .unwrap()
        .apply(tests::patch("legacy:preserve", 2))
        .unwrap();
    let core: serde_json::Value = serde_json::from_slice(&fs::read(&core_path).unwrap()).unwrap();
    assert_eq!(
        core["brain"]["mcp_worker_binding"]["backend_kinds"],
        serde_json::json!(["Mcp"])
    );
}
