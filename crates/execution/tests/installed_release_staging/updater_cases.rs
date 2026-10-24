//! Controlled platform evidence, not native SCM or installed-product acceptance.
use super::*;
use forme_execution::installed::{
    InstalledReleaseUpdater, ProductConfigStore, ReleaseUpdaterPlatform,
};
use std::time::Instant;

const WEB_TOKEN: &str = "updater-fixture-web-configuration-0000000000000001";

#[derive(Default)]
struct Platform {
    launches: AtomicUsize,
    changes: Mutex<Vec<(p::BuiltinServiceId, p::NativeServiceAction)>>,
}
impl ReleaseUpdaterPlatform for Platform {
    fn verify_host_image(&self, image: &Path, expected: &p::SchemaDigest) -> p::Result<()> {
        // Controlled platform observation only; the production adapter checks current_exe.
        if file_digest(image)? != *expected {
            return Err(p::Error("fixture target image mismatch".into()));
        }
        Ok(())
    }
    fn switch_images(
        &self,
        prepared: forme_execution::installed::PreparedReleaseServiceSwitch,
        journal: &ProductJournal,
        deadline: Instant,
    ) -> p::Result<p::ProductReleaseServiceReceipt> {
        prepared.execute(journal, &Images, deadline)
    }
    fn verify_updater(&self, _: &p::InstallationManifest, _: &Path) -> p::Result<()> {
        Ok(())
    }
    fn launch(&self, installation: &p::InstallationManifest, config: &Path) -> p::Result<()> {
        assert_eq!(config, installation.paths.config.join("host.json"));
        assert!(installation
            .paths
            .host_state
            .join("release-handoff.json")
            .is_file());
        self.launches.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn observe(
        &self,
        _: &p::InstallationManifest,
        _: &Path,
        id: p::BuiltinServiceId,
        _: Option<p::NativeServiceState>,
    ) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(id, "observed"))
    }
    fn change(
        &self,
        _: &p::InstallationManifest,
        _: &Path,
        id: p::BuiltinServiceId,
        action: p::NativeServiceAction,
        _: Instant,
    ) -> p::Result<p::SchemaDigest> {
        self.changes.lock().unwrap().push((id, action));
        p::canonical_digest(&(id, action))
    }
}
struct Images;
impl forme_execution::installed::ReleaseServiceImageManager for Images {
    fn verify_original(
        &self,
        change: &forme_execution::installed::ReleaseServiceImageChange,
    ) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(change.service(), "old-image-configuration"))
    }
    fn switch_and_verify(
        &self,
        change: &forme_execution::installed::ReleaseServiceImageChange,
        _: &p::SchemaDigest,
    ) -> p::Result<p::SchemaDigest> {
        p::canonical_digest(&(change.service(), "new-image-configuration"))
    }
}
fn product_config(f: &Fixture) -> Arc<Mutex<ProductConfigStore>> {
    product_config_at(f, "127.0.0.1:19901".parse().unwrap())
}
fn product_config_at(
    f: &Fixture,
    core_bind: std::net::SocketAddr,
) -> Arc<Mutex<ProductConfigStore>> {
    let paths = &f.installation.paths;
    fs::create_dir_all(paths.config.join("core")).unwrap();
    fs::create_dir_all(&paths.keys).unwrap();
    fs::write(
        paths.config.join("installation.json"),
        serde_json::to_vec(&f.installation).unwrap(),
    )
    .unwrap();
    for id in [
        p::BuiltinServiceId::Host,
        p::BuiltinServiceId::Web,
        p::BuiltinServiceId::Brain,
    ] {
        let image = paths.releases.join(format!("bin/{}.exe", id.binary_name()));
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        fs::write(image, "initial-image").unwrap();
    }
    fs::write(
        paths.releases.join("bin/forme-hostctl.exe"),
        "offline-data-fixture",
    )
    .unwrap();
    fs::write(paths.config.join("host.json"), "fixed-host-config").unwrap();
    fs::write(paths.config.join("web.json"), "fixed-web-config").unwrap();
    for (file, value) in [
        ("core-owner.token", "o".repeat(64)),
        ("core-web.token", "w".repeat(64)),
    ] {
        fs::write(paths.keys.join(file), value).unwrap();
    }
    let template = p::ProductConfigTemplate {
        schema_version: p::SchemaVersion(1),
        install_ref: f.installation.install_id.clone(),
        core_config_path: paths.config.join("core/core.json"),
        configuration_ack_path: paths.brain.join("core-ack.json"),
        model_credentials_directory: paths.keys.join("models"),
        brain_store_path: paths.brain.join("forme.db"),
        artifact_root: paths.brain.join("artifacts"),
        core_owner_credential_path: paths.keys.join("core-owner.token"),
        core_web_credential_path: paths.keys.join("core-web.token"),
        core_web_assertion_verifying_key: SigningKey::from_bytes(&[9; 32])
            .verifying_key()
            .to_bytes(),
        core_session_fence_path: paths.brain.join("fence.sqlite"),
        core_bind,
        web_bind: "127.0.0.1:19902".parse().unwrap(),
        web_network_config_path: None,
        web_material_directory: None,
        workspace: p::WorkspaceRef("workspace:test".into()),
        agent_profile: p::AgentProfileRef("agent:test".into()),
        scheduler_tick: p::DurationMs(1000),
        scheduler_lease: p::DurationMs(30000),
        scheduler_max_claims: 1,
        cognition_tick: p::DurationMs(60000),
        stable_evidence_max_age: p::DurationMs(86400000),
        authority_lease: p::DurationMs(30000),
        drain_timeout: p::DurationMs(30000),
    };
    let path = paths.config.join("product-template.json");
    fs::write(&path, serde_json::to_vec(&template).unwrap()).unwrap();
    let store = ProductConfigStore::open(&path).unwrap();
    {
        let mut config = store.lock().unwrap();
        config
            .bind_owner(p::ProductConfigOwnerBinding {
                schema_version: p::SchemaVersion(1),
                operation_id: "claim:updater".into(),
                expected_revision: 0,
                owner: p::OwnerPrincipalRef("owner:test".into()),
            })
            .unwrap();
        let patch: p::ProductConfigPatch = serde_json::from_value(serde_json::json!({"schema_version":1,"operation_id":"save:updater",
            "expected_revision":0,"owner":"owner:test","models":[{"schema_version":1,"id":"model:updater","provider":"openai-compatible",
                "model":"fixture","base_url":"https://fixture.invalid/v1","credential":"fixture-key-only","context_window":128000,
                "tool_use":true,"strength":"standard","input_microunits_per_million":0,"output_microunits_per_million":0,
                "requests_per_minute":60,"tokens_per_minute":1000000,"timeout_ms":120000,"fallback_group":null,
                "default":true,"tool_schema_versions":[1]}]})).unwrap();
        config.apply(patch).unwrap();
    }
    Arc::new(store)
}
fn make_broker(
    f: &Fixture,
    config: Arc<Mutex<ProductConfigStore>>,
    updater: Arc<InstalledReleaseUpdater>,
) -> ProductHostBroker<NoServices> {
    ProductHostBroker::open_with_service_manager_and_config(
        &f.journal(),
        &f.installation.install_id,
        HostCredentials::new(TOKEN.into(), None)
            .unwrap()
            .with_web_configuration(Some(WEB_TOKEN.into()))
            .unwrap(),
        f.installation.target,
        f.installation.release_id.clone(),
        Some(config),
        NoServices,
    )
    .unwrap()
    .with_release_stager(Arc::new(f.stager()))
    .unwrap()
    .with_release_updater(updater)
    .unwrap()
}
fn request(f: &Fixture, revision: u64) -> p::ProductMaintenanceRequest {
    p::ProductMaintenanceRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: "upgrade:deferred".into(),
        expected_revision: revision,
        owner: p::OwnerPrincipalRef("owner:test".into()),
        requested_deadline: now_millis() + 60000,
        action: p::ProductMaintenanceAction::Upgrade {
            release_id: RELEASE.into(),
            release_digest: f.release.digest().unwrap(),
            backup_ref: "backup:updater".into(),
        },
    }
}
fn prepared() -> (
    Fixture,
    Arc<Mutex<ProductConfigStore>>,
    Arc<Platform>,
    Arc<InstalledReleaseUpdater>,
    p::ProductMaintenanceRequest,
) {
    let f = Fixture::new();
    let journal = f.stage();
    let revision = journal.revision().unwrap();
    drop(journal);
    let config = product_config(&f);
    let platform = Arc::new(Platform::default());
    let updater = Arc::new(
        InstalledReleaseUpdater::new(
            f.installation.clone(),
            f.installation.paths.config.join("host.json"),
            Arc::new(f.stager()),
            platform.clone(),
        )
        .unwrap(),
    );
    let request = request(&f, revision);
    (f, config, platform, updater, request)
}

#[test]
fn deferred_release_admission_is_exact_once_and_restart_cannot_recreate_dispatch_authority() {
    let (f, config, platform, updater, request) = prepared();
    let broker = make_broker(&f, config.clone(), updater.clone());
    let auth = broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
    let (operation, ticket) = broker
        .prepare_owner_release(&auth, request.clone(), now_millis())
        .unwrap();
    assert_eq!(operation.stage, p::MaintenanceStage::Validated);
    assert_eq!(
        operation.pending_stage,
        Some(p::MaintenanceStage::Quiescing)
    );
    assert_eq!(platform.launches.load(Ordering::SeqCst), 0);
    assert!(broker
        .with_installed_work_admission(|| -> p::Result<()> {
            panic!("no worker may start during takeover")
        })
        .is_err());
    assert!(broker
        .owner_maintenance(&auth, request.clone(), now_millis())
        .is_err());
    assert!(broker
        .prepare_owner_release(&auth, request.clone(), now_millis())
        .unwrap()
        .1
        .is_none());
    broker.execute_owner_release(ticket.unwrap()).unwrap();
    assert_eq!(platform.launches.load(Ordering::SeqCst), 1);
    assert!(platform.changes.lock().unwrap().is_empty());
    drop(broker);
    drop(config);
    let config = Arc::new(
        ProductConfigStore::open(&f.installation.paths.config.join("product-template.json"))
            .unwrap(),
    );
    let broker = make_broker(&f, config, updater);
    let auth = broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
    assert!(broker
        .prepare_owner_release(&auth, request, now_millis())
        .unwrap()
        .1
        .is_none());
    assert_eq!(platform.launches.load(Ordering::SeqCst), 1);
}

#[test]
fn updater_consumes_marker_before_stops_and_never_competes_for_the_live_host_writer() {
    let (f, config, platform, updater, request) = prepared();
    let broker = make_broker(&f, config, updater.clone());
    let auth = broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
    broker
        .prepare_owner_release(&auth, request, now_millis())
        .unwrap();
    // Controlled SCM says stopped; the real Host writer is deliberately retained.
    // Updater must fail writer transfer without touching backup/config/images.
    assert!(updater.run().is_err());
    let first = platform.changes.lock().unwrap().clone();
    assert_eq!(
        first,
        vec![
            (p::BuiltinServiceId::Brain, p::NativeServiceAction::Stop),
            (p::BuiltinServiceId::Web, p::NativeServiceAction::Stop),
            (p::BuiltinServiceId::Host, p::NativeServiceAction::Stop)
        ]
    );
    assert!(updater.run().is_err());
    assert_eq!(*platform.changes.lock().unwrap(), first);
    assert!(!f
        .installation
        .paths
        .host_state
        .join("data-worker-started")
        .exists());
    assert_eq!(
        broker
            .owner_maintenance_operation(&auth, "upgrade:deferred")
            .unwrap()
            .pending_stage,
        Some(p::MaintenanceStage::Quiescing)
    );
}

#[test]
fn altered_handoff_or_tampered_target_produces_zero_native_stops() {
    for corrupt_binding in [true, false] {
        let (f, config, platform, updater, request) = prepared();
        let broker = make_broker(&f, config, updater.clone());
        let auth = broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
        broker
            .prepare_owner_release(&auth, request, now_millis())
            .unwrap();
        if corrupt_binding {
            let path = f.installation.paths.host_state.join("release-handoff.json");
            let mut t: p::ProductReleaseTakeover =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            t.installation_digest = p::sha256_content_digest(b"foreign");
            fs::write(path, serde_json::to_vec(&t).unwrap()).unwrap();
        } else {
            fs::write(f.destination().join("bin/forme-gatewayd.exe"), "tampered").unwrap();
        }
        assert!(updater.run().is_err());
        assert!(platform.changes.lock().unwrap().is_empty());
    }
}

#[test]
fn updater_transfers_real_writers_and_child_pipe_then_leaves_target_checking_closed() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut f = Fixture::new();
    let binary = f.source().join("content/bin/forme-hostctl.exe");
    fs::copy(env!("CARGO_BIN_EXE_forme-host-storage-fixture"), &binary).unwrap();
    let file = f
        .release
        .files
        .iter_mut()
        .find(|file| file.relative_path == "bin/forme-hostctl.exe")
        .unwrap();
    file.size = fs::metadata(&binary).unwrap().len();
    file.digest = file_digest(&binary).unwrap();
    f.release.data_range.read_max = 13;
    f.release.data_range.write_version = 13;
    fs::write(
        f.source().join("manifest.json"),
        serde_json::to_vec(&f.release).unwrap(),
    )
    .unwrap();
    fs::write(
        f.source().join("signature.ed25519"),
        SigningKey::from_bytes(&[0x36; 32])
            .sign(f.release.digest().unwrap().0.as_bytes())
            .to_bytes(),
    )
    .unwrap();
    let journal = f.stage();
    let revision = journal.revision().unwrap();
    drop(journal);
    let config = product_config_at(&f, listener.local_addr().unwrap());
    fs::copy(
        env!("CARGO_BIN_EXE_forme-host-storage-fixture"),
        f.installation.paths.releases.join("bin/forme-hostctl.exe"),
    )
    .unwrap();
    let platform = Arc::new(Platform::default());
    let updater = Arc::new(
        InstalledReleaseUpdater::new(
            f.installation.clone(),
            f.installation.paths.config.join("host.json"),
            Arc::new(f.stager()),
            platform.clone(),
        )
        .unwrap(),
    );
    let broker = make_broker(&f, config.clone(), updater.clone());
    let auth = broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
    let (_, ticket) = broker
        .prepare_owner_release(&auth, request(&f, revision), now_millis())
        .unwrap();
    broker.execute_owner_release(ticket.unwrap()).unwrap();
    drop(broker);
    drop(config);
    updater.run().unwrap();
    assert_eq!(
        *platform.changes.lock().unwrap(),
        vec![
            (p::BuiltinServiceId::Brain, p::NativeServiceAction::Stop),
            (p::BuiltinServiceId::Web, p::NativeServiceAction::Stop),
            (p::BuiltinServiceId::Host, p::NativeServiceAction::Stop),
            (p::BuiltinServiceId::Host, p::NativeServiceAction::Start),
            (p::BuiltinServiceId::Web, p::NativeServiceAction::Start),
            (p::BuiltinServiceId::Brain, p::NativeServiceAction::Start)
        ]
    );
    let journal = ProductJournal::open(&f.journal(), &f.installation.install_id).unwrap();
    let operation = journal.operation("upgrade:deferred").unwrap();
    assert_eq!(operation.stage, p::MaintenanceStage::Switching);
    assert_eq!(operation.pending_stage, Some(p::MaintenanceStage::Checking));
    let checking = journal
        .release_checking("upgrade:deferred")
        .unwrap()
        .unwrap();
    assert_eq!(checking.data_identity.database_schema_version, 13);
    assert_eq!(
        journal.active_release().unwrap(),
        Some((RELEASE.into(), f.release.digest().unwrap()))
    );
    let admission: p::ProductReleaseAdmission = serde_json::from_slice(
        &fs::read(
            f.installation
                .paths
                .config
                .join("core/core.release-admission.json"),
        )
        .unwrap(),
    )
    .unwrap();
    admission.validate_for(&checking.binding).unwrap();
    assert!(!admission.released);
    let published: serde_json::Value = serde_json::from_slice(
        &fs::read(f.installation.paths.config.join("core/core.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(published["revision"], 2);
    assert_eq!(
        published["transport"]["release_binding"]["operation_id"],
        "upgrade:deferred"
    );
    let effects = platform.changes.lock().unwrap().len();
    drop(journal);
    assert!(updater.run().is_err());
    assert_eq!(platform.changes.lock().unwrap().len(), effects);

    // A controlled target runtime writes its config acknowledgement and
    // answers a real private HTTP challenge. This does not run a native daemon.
    let ready = p::ProductReleaseReadiness {
        schema_version: p::SchemaVersion(1),
        binding: checking.binding.clone(),
        config_revision: 2,
        actual_image_digest: checking.binding.core_image_digest.clone(),
        data_identity: p::ProductDataAcknowledgement {
            schema_version: p::SchemaVersion(1),
            owner_digest: checking.binding.owner_digest.clone(),
            store_identity: checking.binding.store_identity.clone(),
            execution_fence_digest: None,
            composition_digest: p::sha256_content_digest(b"fixture-composition"),
        },
    };
    fs::create_dir_all(&f.installation.paths.brain).unwrap();
    let ack_path = f.installation.paths.brain.join("core-ack.json");
    fs::write(
        ack_path.with_extension("release-readiness.json"),
        serde_json::to_vec(&ready).unwrap(),
    )
    .unwrap();
    fs::write(
        &ack_path,
        serde_json::to_vec(&p::ProductConfigAcknowledgement {
            schema_version: p::SchemaVersion(1),
            install_ref: f.installation.install_id.clone(),
            revision: 2,
            data_identity: Some(ready.data_identity.clone()),
        })
        .unwrap(),
    )
    .unwrap();
    let config = Arc::new(
        ProductConfigStore::open(&f.installation.paths.config.join("product-template.json"))
            .unwrap(),
    );
    let broker = make_broker(&f, config.clone(), updater.clone());
    let proof_server = std::thread::spawn(move || serve_proofs(listener, ready));
    broker.reconcile_release_checking().unwrap(); // first response has a foreign signing key
    let auth = broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
    assert_ne!(
        broker
            .owner_maintenance_operation(&auth, "upgrade:deferred")
            .unwrap()
            .stage,
        p::MaintenanceStage::Committed
    );
    broker.reconcile_release_checking().unwrap(); // fresh valid proof permits commit
    assert_eq!(
        broker
            .owner_maintenance_operation(&auth, "upgrade:deferred")
            .unwrap()
            .stage,
        p::MaintenanceStage::Committed
    );
    let admission_path = f
        .installation
        .paths
        .config
        .join("core/core.release-admission.json");
    let mut admission: p::ProductReleaseAdmission =
        serde_json::from_slice(&fs::read(&admission_path).unwrap()).unwrap();
    assert!(admission.released);
    admission.released = false;
    fs::write(&admission_path, serde_json::to_vec(&admission).unwrap()).unwrap();
    drop(broker);
    drop(config);
    let config = Arc::new(
        ProductConfigStore::open(&f.installation.paths.config.join("product-template.json"))
            .unwrap(),
    );
    let broker = make_broker(&f, config, updater);
    broker.reconcile_release_checking().unwrap(); // lost post-commit opening only rechecks this operation
    proof_server.join().unwrap();
    let admission: p::ProductReleaseAdmission =
        serde_json::from_slice(&fs::read(&admission_path).unwrap()).unwrap();
    assert!(admission.released);
    assert_eq!(platform.changes.lock().unwrap().len(), effects);
}

fn serve_proofs(listener: std::net::TcpListener, readiness: p::ProductReleaseReadiness) {
    use std::io::{BufRead, Read, Write};
    use std::time::Duration;
    listener.set_nonblocking(true).unwrap();
    for forged in [true, false, false] {
        let deadline = Instant::now() + Duration::from_secs(15);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(pair) => break pair,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("proof was not requested: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut input = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut line = String::new();
            input.read_line(&mut line).unwrap();
            assert!(!line.to_ascii_lowercase().starts_with("authorization:"));
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
        }
        assert!(length > 0 && length < 4096);
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes).unwrap();
        let request: p::ProductReleaseReadinessProbe = serde_json::from_slice(&bytes).unwrap();
        let mut proof = p::ProductReleaseReadinessProof {
            schema_version: p::SchemaVersion(1),
            nonce: request.nonce,
            readiness: readiness.clone(),
            signature: vec![],
        };
        let seed = if forged {
            [23; 32]
        } else {
            p::product_release_probe_seed("o".repeat(64).as_bytes()).unwrap()
        };
        proof.signature = SigningKey::from_bytes(&seed)
            .sign(proof.signing_digest().unwrap().0.as_bytes())
            .to_bytes()
            .to_vec();
        let bytes = serde_json::to_vec(&proof).unwrap();
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                )
                .as_bytes(),
            )
            .unwrap();
        stream.write_all(&bytes).unwrap();
    }
}
