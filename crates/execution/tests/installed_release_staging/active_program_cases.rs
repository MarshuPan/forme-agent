//! Signed fixture and journal evidence; no SCM, desktop or native install.
use super::*;
use forme_execution::installed::{WorkerLaunchProfileConfig, WorkerRuntimeConfig};

fn configuration(f: &Fixture) -> WorkerRuntimeConfig {
    let workspace = f.installation.paths.workspaces.join("default");
    let program = f
        .installation
        .paths
        .releases
        .join("bin/forme-executord.exe");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(program.parent().unwrap()).unwrap();
    fs::write(&program, b"initial installed file entry fixture").unwrap();
    WorkerRuntimeConfig {
        schema_version: p::SchemaVersion(1),
        authority_epoch: p::AuthorityEpoch(1),
        journal_path: f.installation.paths.host_state.join("file-worker.journal"),
        runtime_root: f.installation.paths.host_state.join("file-worker-runtime"),
        allowed_scopes: vec![p::Scope("workspace:default".into())],
        workspace_roots: BTreeMap::from([("workspace:default".into(), workspace)]),
        launch_profiles: vec![WorkerLaunchProfileConfig {
            schema_version: p::SchemaVersion(1),
            sandbox_profile_ref: forme_execution::installed::INSTALLED_FILE_PROFILE.into(),
            program,
            args: vec!["--installed-file-worker".into()],
        }],
    }
}

fn upgrade(f: &Fixture) -> Arc<ProductHostBroker<NoServices>> {
    let journal = f.stage();
    let revision = journal.revision().unwrap();
    drop(journal);
    let (worker, effects) = release_worker(f, None);
    let broker = Arc::new(f.broker().with_release_worker(worker).unwrap());
    *effects.broker.lock().unwrap() = Arc::downgrade(&broker);
    // A committed staging candidate alone cannot replace the active image.
    assert!(broker.verified_active_release().unwrap().is_none());
    let auth = broker.authenticate(TOKEN).unwrap();
    broker
        .plan(
            &auth,
            f.upgrade_request("active-programs-upgrade", revision),
            now_millis(),
        )
        .unwrap();
    let result = broker
        .execute(&auth, "active-programs-upgrade", now_millis())
        .unwrap();
    assert_eq!(result.stage, p::MaintenanceStage::Committed);
    broker
}

#[test]
fn active_programs_follow_only_the_journal_release_across_restart_without_rewriting_config() {
    let f = Fixture::new();
    let config = configuration(&f);
    let original = serde_json::to_vec(&config).unwrap();
    fs::create_dir_all(&f.installation.paths.config).unwrap();
    let path = f.installation.paths.config.join("worker.json");
    fs::write(&path, &original).unwrap();
    let broker = upgrade(&f);
    let before = fs::metadata(f.journal()).unwrap().len();
    let release = broker.verified_active_release().unwrap().unwrap();
    assert_eq!(release.content_root(), f.destination());
    let selected = config.for_active_release(&release).unwrap();
    assert_eq!(
        selected.launch_profiles[0].program,
        f.destination().join("bin/forme-executord.exe")
    );
    assert_ne!(
        selected.launch_profiles[0].program,
        config.launch_profiles[0].program
    );
    assert_eq!(
        selected.launch_profiles[0].args,
        config.launch_profiles[0].args
    );
    assert_eq!(selected.workspace_roots, config.workspace_roots);
    assert_eq!(selected.authority_epoch, config.authority_epoch);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::metadata(f.journal()).unwrap().len(), before);
    drop(broker);
    let reopened = f.broker();
    assert_eq!(
        reopened
            .verified_active_release()
            .unwrap()
            .unwrap()
            .content_root(),
        f.destination()
    );
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn active_programs_refuse_lost_staging_or_tampered_files_without_initial_image_fallback() {
    let f = Fixture::new();
    let config = configuration(&f);
    let broker = upgrade(&f);
    let release = broker.verified_active_release().unwrap().unwrap();
    drop(broker);
    let without_stager = ProductHostBroker::open_with_service_manager(
        &f.journal(),
        &f.installation.install_id,
        HostCredentials::new(TOKEN.into(), None).unwrap(),
        f.installation.target,
        f.installation.release_id.clone(),
        NoServices,
    )
    .unwrap();
    assert!(without_stager.verified_active_release().is_err());
    drop(without_stager);
    let broker = f.broker();
    let before = fs::metadata(f.journal()).unwrap().len();
    fs::write(
        f.destination().join("bin/forme-gatewayd.exe"),
        b"replaced fixture",
    )
    .unwrap();
    assert!(broker.verified_active_release().is_err());
    assert!(config.for_active_release(&release).is_err());
    assert_eq!(fs::metadata(f.journal()).unwrap().len(), before);
    assert!(config.launch_profiles[0].program.is_file());
}

#[test]
fn active_file_program_preserves_custom_profiles_and_rejects_replaced_reserved_arguments() {
    let f = Fixture::new();
    let mut config = configuration(&f);
    let broker = upgrade(&f);
    let release = broker.verified_active_release().unwrap().unwrap();
    config.launch_profiles[0].args.push("--foreign".into());
    assert!(config.for_active_release(&release).is_err());
    config.launch_profiles[0].sandbox_profile_ref = "sandbox:custom-entry".into();
    assert_eq!(config.for_active_release(&release).unwrap(), config);
}
