//! Real filesystem publication and broker-journal evidence, never OS upgrade evidence.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, Weak,
};
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};
use forme_execution::installed::{
    file_digest, HostCredentials, HostManagement, NativeServiceManager, ProductHostBroker,
    ProductJournal, ReleaseStager, ReleaseStagingConfig, ReleaseWorkerProvider,
    ReleaseWorkerSession, UninstallScope,
};
use forme_protocol as p;

#[path = "installed_release_staging/service_switch_cases.rs"]
mod service_switch_cases;

const TOKEN: &str = "release-maintenance-test-token-00000000000001";
const RELEASE: &str = "0.0.2";
const OP: &str = "stage-operation";

struct NoServices;
impl NativeServiceManager for NoServices {
    fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service,
            state: p::NativeServiceState::Stopped,
        })
    }
    fn apply(&self, _: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        panic!("staging and scope planning must not change a service")
    }
}

struct ReleaseWorkerFixture {
    installation: p::InstallationManifest,
    effects: Arc<ReleaseEffects>,
}

struct ReleaseSession {
    request: p::ProductReleaseRequest,
    effects: Arc<ReleaseEffects>,
}

struct ReleaseEffects {
    calls: Mutex<Vec<p::ProductReleaseStep>>,
    fail_at: Option<p::ProductReleaseStep>,
    broker: Mutex<Weak<ProductHostBroker<NoServices>>>,
    tamper_after_backup: Mutex<Option<PathBuf>>,
}

impl ReleaseWorkerProvider for ReleaseWorkerFixture {
    fn installation(&self) -> &p::InstallationManifest {
        &self.installation
    }

    fn preflight(&self, _request: &p::ProductReleaseRequest) -> p::Result<()> {
        Ok(())
    }

    fn start(
        &self,
        request: p::ProductReleaseRequest,
        _deadline: std::time::Instant,
    ) -> p::Result<Box<dyn ReleaseWorkerSession>> {
        Ok(Box::new(ReleaseSession {
            request,
            effects: self.effects.clone(),
        }))
    }
}

impl ReleaseSession {
    fn response(
        &self,
        step: p::ProductReleaseStep,
        result: p::ProductReleaseWorkerResult,
    ) -> p::ProductReleaseWorkerResponse {
        p::ProductReleaseWorkerResponse {
            schema_version: p::SchemaVersion(1),
            operation_id: self.request.operation_id.clone(),
            command_digest: self.request.command_digest.clone(),
            step,
            result,
        }
    }
}

impl ReleaseWorkerSession for ReleaseSession {
    fn step(
        &mut self,
        _request: &p::ProductReleaseRequest,
        step: p::ProductReleaseStep,
    ) -> p::Result<p::ProductReleaseWorkerResponse> {
        if step == p::ProductReleaseStep::Release {
            let broker = self.effects.broker.lock().unwrap().upgrade().unwrap();
            let auth = broker.authenticate(TOKEN).unwrap();
            assert_eq!(
                broker
                    .operation(&auth, &self.request.operation_id)
                    .unwrap()
                    .stage,
                p::MaintenanceStage::Committed,
                "work admission must follow the durable commit"
            );
        }
        self.effects.calls.lock().unwrap().push(step);
        if step == p::ProductReleaseStep::Backup {
            if let Some(path) = self.effects.tamper_after_backup.lock().unwrap().take() {
                fs::write(path, b"changed-after-plan-and-backup").unwrap();
            }
        }
        if self.effects.fail_at == Some(step) {
            return Err(p::Error(
                "fixture release worker failed after intent".into(),
            ));
        }
        let result = match step {
            p::ProductReleaseStep::Quiesce => p::ProductReleaseWorkerResult::Quiesced {
                fence_digest: p::canonical_digest(&"release-fence").unwrap(),
            },
            p::ProductReleaseStep::Backup => p::ProductReleaseWorkerResult::BackedUp {
                backup_ref: self.request.backup_ref.clone(),
                backup_digest: p::canonical_digest(&"release-backup").unwrap(),
            },
            p::ProductReleaseStep::Switch => p::ProductReleaseWorkerResult::Switched {
                active_release_ref: self.request.target_release_ref.clone(),
                active_release_digest: self.request.target_release_digest.clone(),
            },
            p::ProductReleaseStep::Check => p::ProductReleaseWorkerResult::Checked {
                active_release_ref: self.request.target_release_ref.clone(),
                active_release_digest: self.request.target_release_digest.clone(),
                service_digest: p::canonical_digest(&"release-service").unwrap(),
            },
            p::ProductReleaseStep::Release => p::ProductReleaseWorkerResult::Released {},
        };
        Ok(self.response(step, result))
    }
}

struct Fixture {
    root: tempfile::TempDir,
    installation: p::InstallationManifest,
    release: p::ReleaseManifest,
    config: ReleaseStagingConfig,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        let paths = p::ProductPaths {
            schema_version: p::SchemaVersion(1),
            releases: base.join("releases"),
            config: base.join("config"),
            brain: base.join("brain"),
            web_auth: base.join("web-auth"),
            host_state: base.join("host-state"),
            keys: base.join("keys"),
            logs: base.join("logs"),
            backups: base.join("backups"),
            workspaces: base.join("workspaces"),
            ipc: base.join("ipc"),
        };
        fs::create_dir(&paths.host_state).unwrap();
        let target = p::ProductTarget::WindowsX86_64;
        let installation = p::InstallationManifest {
            schema_version: p::SchemaVersion(1),
            install_id: "release-test-install".into(),
            target,
            release_id: "0.0.1".into(),
            release_digest: p::canonical_digest(&"installed-seed").unwrap(),
            paths,
            services: [
                p::BuiltinServiceId::Host,
                p::BuiltinServiceId::Web,
                p::BuiltinServiceId::Brain,
            ]
            .into_iter()
            .map(|service| p::ServiceIdentityBinding {
                schema_version: p::SchemaVersion(1),
                service,
                account: service.required_account(target).into(),
            })
            .collect(),
            created_at: 1,
        };
        let source = installation
            .paths
            .host_state
            .join("release-inbox")
            .join(RELEASE);
        let mut files = Vec::new();
        for name in [
            "bin/forme-hostd.exe",
            "bin/forme-webd.exe",
            "bin/forme-gatewayd.exe",
            "bin/forme-cli.exe",
            "bin/forme-hostctl.exe",
            "web/index.html",
        ] {
            let path = source.join("content").join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                &path,
                format!("self-authored bounded release fixture {name}"),
            )
            .unwrap();
            files.push(p::ReleaseFile {
                schema_version: p::SchemaVersion(1),
                relative_path: name.into(),
                size: fs::metadata(&path).unwrap().len(),
                digest: file_digest(&path).unwrap(),
            });
        }
        let ui: Vec<_> = files
            .iter()
            .filter(|file| file.relative_path.starts_with("web/"))
            .cloned()
            .collect();
        let range = p::SchemaReadWriteRange {
            schema_version: p::SchemaVersion(1),
            read_min: 1,
            read_max: 1,
            write_version: 1,
        };
        let release = p::ReleaseManifest {
            schema_version: p::SchemaVersion(1),
            release_id: RELEASE.into(),
            target,
            files,
            ui_digest: p::canonical_digest(&ui).unwrap(),
            managed_dependencies: vec![],
            config_range: range.clone(),
            data_range: range,
            minimum_upgrader: 1,
            signature_ref: "fixture-key".into(),
        };
        let signing = SigningKey::from_bytes(&[0x36; 32]);
        fs::write(
            source.join("manifest.json"),
            serde_json::to_vec(&release).unwrap(),
        )
        .unwrap();
        fs::write(
            source.join("signature.ed25519"),
            signing
                .sign(release.digest().unwrap().0.as_bytes())
                .to_bytes(),
        )
        .unwrap();
        let config = ReleaseStagingConfig {
            schema_version: p::SchemaVersion(1),
            install_ref: installation.install_id.clone(),
            pinned_keys: BTreeMap::from([(
                "fixture-key".into(),
                signing.verifying_key().to_bytes(),
            )]),
        };
        Self {
            root,
            installation,
            release,
            config,
        }
    }
    fn journal(&self) -> PathBuf {
        self.installation.paths.host_state.join("host.journal")
    }
    fn source(&self) -> PathBuf {
        self.installation
            .paths
            .host_state
            .join("release-inbox")
            .join(RELEASE)
    }
    fn broker(&self) -> ProductHostBroker<NoServices> {
        let broker = ProductHostBroker::open_with_service_manager(
            &self.journal(),
            &self.installation.install_id,
            HostCredentials::new(TOKEN.into(), None).unwrap(),
            self.installation.target,
            self.installation.release_id.clone(),
            NoServices,
        )
        .unwrap()
        .with_release_stager(Arc::new(
            ReleaseStager::open(self.installation.clone(), self.config.clone()).unwrap(),
        ))
        .unwrap();
        broker
            .record_installation_verification(
                self.installation.digest().unwrap(),
                self.installation.release_digest.clone(),
            )
            .unwrap();
        broker
    }
    fn request(&self, kind: p::MaintenanceKind, revision: u64) -> p::HostManagementRequest {
        p::HostManagementRequest {
            schema_version: p::SchemaVersion(1),
            command: p::MaintenanceCommand {
                schema_version: p::SchemaVersion(1),
                operation_id: OP.into(),
                idempotency_key: "stage-key".into(),
                expected_revision: revision,
                actor_proof: p::MaintenanceProofRef {
                    schema_version: p::SchemaVersion(1),
                    proof_ref: p::canonical_digest(&TOKEN).unwrap().0,
                },
                kind,
                requested_deadline: 500_000,
            },
            references: p::MaintenanceReferences {
                schema_version: p::SchemaVersion(1),
                original_release_ref: "0.0.1".into(),
                target_release_ref: RELEASE.into(),
                data_candidate_ref: None,
                backup_ref: None,
            },
        }
    }
    fn stage_request(&self) -> p::HostManagementRequest {
        self.request(
            p::MaintenanceKind::StageSignedRelease {
                release_id: RELEASE.into(),
                release_digest: self.release.digest().unwrap(),
            },
            2,
        )
    }
    fn upgrade_request(&self, operation_id: &str, revision: u64) -> p::HostManagementRequest {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        p::HostManagementRequest {
            schema_version: p::SchemaVersion(1),
            command: p::MaintenanceCommand {
                schema_version: p::SchemaVersion(1),
                operation_id: operation_id.into(),
                idempotency_key: format!("{operation_id}-key"),
                expected_revision: revision,
                actor_proof: p::MaintenanceProofRef {
                    schema_version: p::SchemaVersion(1),
                    proof_ref: p::canonical_digest(&TOKEN).unwrap().0,
                },
                kind: p::MaintenanceKind::Upgrade {
                    release_id: RELEASE.into(),
                    release_digest: self.release.digest().unwrap(),
                    backup_ref: "backup:release-fixture".into(),
                },
                requested_deadline: now + 60_000,
            },
            references: p::MaintenanceReferences {
                schema_version: p::SchemaVersion(1),
                original_release_ref: "0.0.1".into(),
                target_release_ref: RELEASE.into(),
                data_candidate_ref: None,
                backup_ref: Some("backup:release-fixture".into()),
            },
        }
    }
    fn destination(&self) -> PathBuf {
        self.installation.paths.releases.join(RELEASE)
    }
    fn stager(&self) -> ReleaseStager {
        ReleaseStager::open(self.installation.clone(), self.config.clone()).unwrap()
    }
    fn stage(&self) -> ProductJournal {
        let broker = self.broker();
        let auth = broker.authenticate(TOKEN).unwrap();
        broker.plan(&auth, self.stage_request(), 1).unwrap();
        assert_eq!(
            broker.execute(&auth, OP, 2).unwrap().stage,
            p::MaintenanceStage::Committed
        );
        drop(broker);
        ProductJournal::open(&self.journal(), &self.installation.install_id).unwrap()
    }
    fn record(&self) -> PathBuf {
        let name = p::canonical_digest(&OP).unwrap();
        self.installation
            .paths
            .host_state
            .join("release-records")
            .join(format!("{}.json", &name.0[7..]))
    }
}

fn release_worker(
    fixture: &Fixture,
    fail_at: Option<p::ProductReleaseStep>,
) -> (Arc<ReleaseWorkerFixture>, Arc<ReleaseEffects>) {
    let effects = Arc::new(ReleaseEffects {
        calls: Mutex::new(Vec::new()),
        fail_at,
        broker: Mutex::new(Weak::new()),
        tamper_after_backup: Mutex::new(None),
    });
    let worker = Arc::new(ReleaseWorkerFixture {
        installation: fixture.installation.clone(),
        effects: effects.clone(),
    });
    (worker, effects)
}

#[test]
fn typed_release_worker_commits_upgrade_and_replays_active_release() {
    let fixture = Fixture::new();
    let journal = fixture.stage();
    let revision = journal.revision().unwrap();
    drop(journal);

    let (worker, effects) = release_worker(&fixture, None);
    let broker = Arc::new(fixture.broker().with_release_worker(worker).unwrap());
    *effects.broker.lock().unwrap() = Arc::downgrade(&broker);
    let auth = broker.authenticate(TOKEN).unwrap();
    let request = fixture.upgrade_request("upgrade-operation", revision);
    let planned = broker
        .plan(&auth, request, now_millis())
        .expect("upgrade should be admitted after signed staging");
    assert_eq!(planned.stage, p::MaintenanceStage::Planned);
    let committed = broker
        .execute(&auth, "upgrade-operation", now_millis())
        .unwrap();
    assert_eq!(
        committed.stage,
        p::MaintenanceStage::Committed,
        "unexpected release operation state: {committed:?}"
    );
    assert_eq!(
        *effects.calls.lock().unwrap(),
        [
            p::ProductReleaseStep::Quiesce,
            p::ProductReleaseStep::Backup,
            p::ProductReleaseStep::Switch,
            p::ProductReleaseStep::Check,
            p::ProductReleaseStep::Release,
        ]
    );
    assert!(broker
        .status(&auth)
        .unwrap()
        .allowed_operations
        .contains(&"upgrade".into()));
    assert_eq!(broker.status(&auth).unwrap().active_release, RELEASE);
    drop(broker);

    let reopened = fixture.broker();
    let reopened_auth = reopened.authenticate(TOKEN).unwrap();
    assert_eq!(
        reopened.status(&reopened_auth).unwrap().active_release,
        RELEASE
    );
}

#[test]
fn failed_release_worker_enters_recovery_and_never_replays_the_effect() {
    let fixture = Fixture::new();
    let journal = fixture.stage();
    let revision = journal.revision().unwrap();
    drop(journal);

    let (worker, effects) = release_worker(&fixture, Some(p::ProductReleaseStep::Backup));
    let broker = fixture.broker().with_release_worker(worker).unwrap();
    let auth = broker.authenticate(TOKEN).unwrap();
    broker
        .plan(
            &auth,
            fixture.upgrade_request("failed-upgrade", revision),
            now_millis(),
        )
        .unwrap();
    let failed = broker
        .execute(&auth, "failed-upgrade", now_millis())
        .unwrap();
    assert_eq!(failed.stage, p::MaintenanceStage::RecoveryRequired);
    assert_eq!(failed.pending_stage, Some(p::MaintenanceStage::BackedUp));
    let calls = effects.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        [
            p::ProductReleaseStep::Quiesce,
            p::ProductReleaseStep::Backup,
        ]
    );
    assert!(broker
        .execute(&auth, "failed-upgrade", now_millis())
        .is_err());
    assert_eq!(*effects.calls.lock().unwrap(), calls);
    assert_eq!(broker.status(&auth).unwrap().active_release, "0.0.1");
    drop(broker);

    let reopened = fixture.broker();
    let reopened_auth = reopened.authenticate(TOKEN).unwrap();
    assert_eq!(
        reopened.status(&reopened_auth).unwrap().product_state,
        p::ProductState::RecoveryRequired
    );
    assert_eq!(
        reopened.status(&reopened_auth).unwrap().active_release,
        "0.0.1"
    );
}

#[test]
fn initial_release_binding_is_usable_without_reopening_the_broker() {
    let fixture = Fixture::new();
    let (worker, _) = release_worker(&fixture, None);
    let broker = fixture.broker().with_release_worker(worker).unwrap();
    let auth = broker.authenticate(TOKEN).unwrap();
    broker.plan(&auth, fixture.stage_request(), 1).unwrap();
    let staged = broker.execute(&auth, OP, 2).unwrap();
    assert!(broker
        .plan(
            &auth,
            fixture.upgrade_request("same-process-upgrade", staged.revision),
            now_millis()
        )
        .is_ok());
}

#[test]
fn failed_admission_release_preserves_commit_and_does_not_replay_lifecycle() {
    let fixture = Fixture::new();
    let journal = fixture.stage();
    let revision = journal.revision().unwrap();
    drop(journal);
    let (worker, effects) = release_worker(&fixture, Some(p::ProductReleaseStep::Release));
    let broker = Arc::new(fixture.broker().with_release_worker(worker).unwrap());
    *effects.broker.lock().unwrap() = Arc::downgrade(&broker);
    let auth = broker.authenticate(TOKEN).unwrap();
    broker
        .plan(
            &auth,
            fixture.upgrade_request("release-failed", revision),
            now_millis(),
        )
        .unwrap();
    assert!(broker
        .execute(&auth, "release-failed", now_millis())
        .is_err());
    let calls = effects.calls.lock().unwrap().clone();
    assert_eq!(
        broker
            .execute(&auth, "release-failed", now_millis())
            .unwrap()
            .stage,
        p::MaintenanceStage::Committed
    );
    assert_eq!(*effects.calls.lock().unwrap(), calls);
}

#[test]
fn updater_manifest_must_match_the_initial_installation_verification() {
    let fixture = Fixture::new();
    let (mut worker, _) = release_worker(&fixture, None);
    Arc::get_mut(&mut worker).unwrap().installation.created_at += 1;
    let broker = fixture.broker().with_release_worker(worker).unwrap();
    assert!(broker
        .record_installation_verification(
            fixture.installation.digest().unwrap(),
            fixture.installation.release_digest.clone()
        )
        .is_err());
}

#[test]
fn release_plan_without_worker_has_no_new_journal_fact() {
    let fixture = Fixture::new();
    let journal = fixture.stage();
    let revision = journal.revision().unwrap();
    drop(journal);

    let broker = fixture.broker();
    let auth = broker.authenticate(TOKEN).unwrap();
    assert!(broker
        .plan(
            &auth,
            fixture.upgrade_request("no-worker-upgrade", revision),
            now_millis(),
        )
        .is_err());
    drop(broker);
    assert_eq!(
        ProductJournal::open(&fixture.journal(), &fixture.installation.install_id)
            .unwrap()
            .revision()
            .unwrap(),
        revision
    );
}

#[test]
fn release_worker_with_matching_id_but_foreign_manifest_is_rejected_before_plan() {
    let fixture = Fixture::new();
    let journal = fixture.stage();
    let revision = journal.revision().unwrap();
    drop(journal);
    let (mut worker, effects) = release_worker(&fixture, None);
    Arc::get_mut(&mut worker).unwrap().installation.paths.logs =
        fixture.root.path().join("foreign-logs");
    let broker = fixture.broker().with_release_worker(worker).unwrap();
    let auth = broker.authenticate(TOKEN).unwrap();
    assert!(broker
        .plan(
            &auth,
            fixture.upgrade_request("foreign-manifest", revision),
            now_millis()
        )
        .is_err());
    assert!(effects.calls.lock().unwrap().is_empty());
    drop(broker);
    assert_eq!(
        ProductJournal::open(&fixture.journal(), &fixture.installation.install_id)
            .unwrap()
            .revision()
            .unwrap(),
        revision
    );
}

#[test]
fn release_rechecks_target_after_backup_before_any_switch_effect() {
    let fixture = Fixture::new();
    let journal = fixture.stage();
    let revision = journal.revision().unwrap();
    drop(journal);
    let (worker, effects) = release_worker(&fixture, None);
    let relative = &fixture.release.files[0].relative_path;
    *effects.tamper_after_backup.lock().unwrap() = Some(fixture.destination().join(relative));
    let broker = fixture.broker().with_release_worker(worker).unwrap();
    let auth = broker.authenticate(TOKEN).unwrap();
    broker
        .plan(
            &auth,
            fixture.upgrade_request("changed-before-switch", revision),
            now_millis(),
        )
        .unwrap();
    let result = broker
        .execute(&auth, "changed-before-switch", now_millis())
        .unwrap();
    assert_eq!(result.stage, p::MaintenanceStage::RecoveryRequired);
    assert_eq!(
        *effects.calls.lock().unwrap(),
        [
            p::ProductReleaseStep::Quiesce,
            p::ProductReleaseStep::Backup
        ]
    );
    assert_eq!(broker.status(&auth).unwrap().active_release, "0.0.1");
    assert!(broker
        .execute(&auth, "changed-before-switch", now_millis())
        .is_err());
    assert_eq!(effects.calls.lock().unwrap().len(), 2);
}

#[test]
fn release_plan_rejects_stale_identity_digest_and_target_before_planning() {
    let fixture = Fixture::new();
    let journal = fixture.stage();
    let revision = journal.revision().unwrap();
    drop(journal);
    let (worker, _effects) = release_worker(&fixture, None);
    let broker = fixture.broker().with_release_worker(worker).unwrap();
    let auth = broker.authenticate(TOKEN).unwrap();
    let base = fixture.upgrade_request("invalid-upgrade", revision);

    let mut stale = base.clone();
    stale.references.original_release_ref = "0.0.9".into();
    assert!(broker.plan(&auth, stale, now_millis()).is_err());

    let mut wrong_digest = base.clone();
    if let p::MaintenanceKind::Upgrade { release_digest, .. } = &mut wrong_digest.command.kind {
        *release_digest = p::canonical_digest(&"wrong-release").unwrap();
    }
    assert!(broker.plan(&auth, wrong_digest, now_millis()).is_err());

    let mut wrong_target = base;
    wrong_target.references.target_release_ref = "0.0.3".into();
    assert!(broker.plan(&auth, wrong_target, now_millis()).is_err());
    drop(broker);
    assert_eq!(
        ProductJournal::open(&fixture.journal(), &fixture.installation.install_id)
            .unwrap()
            .revision()
            .unwrap(),
        revision
    );
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn verified_staged_release_uses_committed_facts_without_inbox_or_mutation() {
    let fixture = Fixture::new();
    let journal = fixture.stage();
    fs::rename(fixture.source(), fixture.root.path().join("retained-inbox")).unwrap();
    let before = fs::metadata(fixture.journal()).unwrap().len();
    let revision = journal.revision().unwrap();
    let handle = fixture
        .stager()
        .verified_staged(&journal, RELEASE, &fixture.release.digest().unwrap())
        .unwrap();
    assert_eq!(handle.manifest(), &fixture.release);
    assert_eq!(handle.digest(), &fixture.release.digest().unwrap());
    assert_eq!(handle.content_root(), fixture.destination());
    assert_eq!(handle.staging_operation(), OP);
    handle.recheck().unwrap();
    assert_eq!(journal.revision().unwrap(), revision);
    assert_eq!(fs::metadata(fixture.journal()).unwrap().len(), before);
}

#[test]
fn verified_staged_release_refuses_uncommitted_or_misbound_installation() {
    let fixture = Fixture::new();
    let broker = fixture.broker();
    let auth = broker.authenticate(TOKEN).unwrap();
    broker.plan(&auth, fixture.stage_request(), 1).unwrap();
    drop(broker);
    let journal =
        ProductJournal::open(&fixture.journal(), &fixture.installation.install_id).unwrap();
    assert!(fixture
        .stager()
        .verified_staged(&journal, RELEASE, &fixture.release.digest().unwrap())
        .is_err());
    drop(journal);
    let fixture = Fixture::new();
    let journal = fixture.stage();
    assert!(fixture
        .stager()
        .verified_staged(
            &journal,
            RELEASE,
            &p::canonical_digest(&"foreign-release").unwrap()
        )
        .is_err());
    let mut installation = fixture.installation.clone();
    installation.created_at += 1;
    let stager = ReleaseStager::open(installation, fixture.config.clone()).unwrap();
    assert!(stager
        .verified_staged(&journal, RELEASE, &fixture.release.digest().unwrap())
        .is_err());
}

#[test]
fn verified_staged_release_rechecks_receipt_and_exact_content_before_switch() {
    for case in 0..3 {
        let fixture = Fixture::new();
        let journal = fixture.stage();
        let handle = fixture
            .stager()
            .verified_staged(&journal, RELEASE, &fixture.release.digest().unwrap())
            .unwrap();
        let before = fs::metadata(fixture.journal()).unwrap().len();
        let revision = journal.revision().unwrap();
        match case {
            0 => {
                let mut record: serde_json::Value =
                    serde_json::from_slice(&fs::read(fixture.record()).unwrap()).unwrap();
                record["operation_id"] = serde_json::json!("another-operation");
                fs::write(fixture.record(), serde_json::to_vec(&record).unwrap()).unwrap();
            }
            1 => fs::write(
                fixture.destination().join("web/index.html"),
                b"replaced after authentication",
            )
            .unwrap(),
            _ => fs::write(
                fixture.destination().join("unsigned-file"),
                b"not in the manifest",
            )
            .unwrap(),
        }
        assert!(handle.recheck().is_err());
        assert!(fixture
            .stager()
            .verified_staged(&journal, RELEASE, &fixture.release.digest().unwrap())
            .is_err());
        assert_eq!(journal.revision().unwrap(), revision);
        assert_eq!(fs::metadata(fixture.journal()).unwrap().len(), before);
    }
}

#[test]
fn staged_compatibility_requires_observed_database_schema_and_real_migrator() {
    let fixture = Fixture::new();
    let journal = fixture.stage();
    let handle = fixture
        .stager()
        .verified_staged(&journal, RELEASE, &fixture.release.digest().unwrap())
        .unwrap();
    handle.check_runtime_compatibility(1, 1, 1).unwrap();
    for (config, data, upgrader) in [(0, 1, 1), (1, 0, 1), (1, 1, 0), (2, 1, 1), (1, 10, 1)] {
        assert!(handle
            .check_runtime_compatibility(config, data, upgrader)
            .is_err());
    }
}

#[test]
fn stage_publishes_exact_files_once_and_replays_without_the_source() {
    let fixture = Fixture::new();
    let broker = fixture.broker();
    let auth = broker.authenticate(TOKEN).unwrap();
    let request = fixture.stage_request();
    let plan = broker.plan(&auth, request.clone(), 1).unwrap();
    assert_eq!(plan.stage, p::MaintenanceStage::Planned);
    assert!(!fixture.destination().exists());
    let committed = broker.execute(&auth, OP, 2).unwrap();
    assert_eq!(committed.stage, p::MaintenanceStage::Committed);
    for entry in &fixture.release.files {
        assert_eq!(
            file_digest(&fixture.destination().join(&entry.relative_path)).unwrap(),
            entry.digest
        );
    }
    assert_eq!(broker.status(&auth).unwrap().active_release, "0.0.1");
    assert!(fixture
        .root
        .path()
        .join("brain")
        .try_exists()
        .is_ok_and(|exists| !exists));
    drop(broker);
    fs::remove_dir_all(fixture.source()).unwrap(); // only this test-owned, explicit source
    let before = fs::read(fixture.journal()).unwrap();
    let broker = fixture.broker();
    let auth = broker.authenticate(TOKEN).unwrap();
    assert_eq!(broker.plan(&auth, request, 600_000).unwrap(), committed);
    assert_eq!(broker.execute(&auth, OP, 600_000).unwrap(), committed);
    drop(broker);
    assert_eq!(fs::read(fixture.journal()).unwrap(), before);
}

#[test]
fn tampered_inbox_at_plan_has_no_fact_and_after_plan_keeps_unknown_intent() {
    for tamper_after_plan in [false, true] {
        let fixture = Fixture::new();
        let broker = fixture.broker();
        let auth = broker.authenticate(TOKEN).unwrap();
        let request = fixture.stage_request();
        if tamper_after_plan {
            broker.plan(&auth, request.clone(), 1).unwrap();
        }
        fs::write(
            fixture.source().join("content/web/index.html"),
            b"replaced after signature",
        )
        .unwrap();
        if tamper_after_plan {
            let failed = broker.execute(&auth, OP, 2).unwrap();
            assert_eq!(failed.stage, p::MaintenanceStage::RecoveryRequired);
            assert_eq!(failed.pending_stage, Some(p::MaintenanceStage::Checking));
            assert!(broker.execute(&auth, OP, 3).is_err());
            assert!(!fixture.destination().exists());
        } else {
            assert!(broker.plan(&auth, request, 1).is_err());
            drop(broker);
            let journal =
                ProductJournal::open(&fixture.journal(), &fixture.installation.install_id).unwrap();
            assert_eq!(journal.revision().unwrap(), 2);
        }
    }
}

#[test]
fn existing_destination_extra_files_and_wrong_digest_are_rejected_without_publication() {
    for case in 0..3 {
        let fixture = Fixture::new();
        let broker = fixture.broker();
        let auth = broker.authenticate(TOKEN).unwrap();
        let mut request = fixture.stage_request();
        match case {
            0 => {
                fs::create_dir_all(fixture.destination()).unwrap();
                fs::write(fixture.destination().join("original"), b"retain").unwrap();
            }
            1 => fs::write(fixture.source().join("content/extra"), b"unsigned").unwrap(),
            _ => {
                request.command.kind = p::MaintenanceKind::StageSignedRelease {
                    release_id: RELEASE.into(),
                    release_digest: p::canonical_digest(&"wrong").unwrap(),
                }
            }
        }
        assert!(broker.plan(&auth, request, 1).is_err());
        if case == 0 {
            assert_eq!(
                fs::read(fixture.destination().join("original")).unwrap(),
                b"retain"
            );
        }
        drop(broker);
        assert_eq!(
            ProductJournal::open(&fixture.journal(), &fixture.installation.install_id)
                .unwrap()
                .revision()
                .unwrap(),
            2
        );
    }
}

#[test]
fn concurrent_execution_publishes_a_single_immutable_record() {
    let fixture = Fixture::new();
    let broker = Arc::new(fixture.broker());
    let auth = broker.authenticate(TOKEN).unwrap();
    broker.plan(&auth, fixture.stage_request(), 1).unwrap();
    let success = Arc::new(AtomicUsize::new(0));
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let (broker, auth, success) = (broker.clone(), auth.clone(), success.clone());
            std::thread::spawn(move || {
                if broker
                    .execute(&auth, OP, 2)
                    .is_ok_and(|op| op.stage == p::MaintenanceStage::Committed)
                {
                    success.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();
    for task in tasks {
        task.join().unwrap();
    }
    assert!(success.load(Ordering::SeqCst) >= 1);
    assert_eq!(
        broker.operation(&auth, OP).unwrap().stage,
        p::MaintenanceStage::Committed
    );
    assert_eq!(
        fs::read_dir(
            fixture
                .installation
                .paths
                .host_state
                .join("release-records")
        )
        .unwrap()
        .count(),
        1
    );
    assert_eq!(
        fs::read_dir(&fixture.installation.paths.releases)
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn uninstall_scope_retains_every_data_domain_and_never_uninstalls() {
    let fixture = Fixture::new();
    let broker = fixture.broker();
    let auth = broker.authenticate(TOKEN).unwrap();
    let mut request = fixture.request(
        p::MaintenanceKind::UninstallPlan {
            preserve_owner_data: true,
        },
        2,
    );
    request.references.target_release_ref = "0.0.1".into();
    broker.plan(&auth, request, 1).unwrap();
    let done = broker.execute(&auth, OP, 2).unwrap();
    assert_eq!(done.stage, p::MaintenanceStage::Committed);
    let record = fs::read_dir(
        fixture
            .installation
            .paths
            .host_state
            .join("release-records"),
    )
    .unwrap()
    .next()
    .unwrap()
    .unwrap()
    .path();
    let scope: UninstallScope = serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    assert_eq!(scope.services.len(), 3);
    assert_eq!(scope.preserved_paths.len(), 9);
    assert!(scope
        .preserved_paths
        .contains(&fixture.installation.paths.keys));
    assert!(scope
        .preserved_paths
        .contains(&fixture.installation.paths.workspaces));
    assert!(scope
        .preserved_paths
        .contains(&fixture.installation.paths.brain));
    assert!(!fixture.destination().exists());
    assert!(Path::new(&scope.program_releases).is_absolute());
}

#[test]
fn wrong_installation_worker_cannot_attach() {
    let fixture = Fixture::new();
    let mut config = fixture.config.clone();
    config.install_ref = "another-install".into();
    assert!(ReleaseStager::open(fixture.installation.clone(), config).is_err());
}

#[test]
fn interrupted_stage_directory_is_preserved_and_cannot_be_reused() {
    let fixture = Fixture::new();
    let broker = fixture.broker();
    let auth = broker.authenticate(TOKEN).unwrap();
    broker.plan(&auth, fixture.stage_request(), 1).unwrap();
    let digest = p::canonical_digest(&OP).unwrap().0;
    let interrupted = fixture
        .installation
        .paths
        .releases
        .join(format!(".stage-{}", &digest[7..]));
    fs::create_dir_all(&interrupted).unwrap();
    fs::write(
        interrupted.join("keep-for-recovery"),
        b"partial previous publication",
    )
    .unwrap();
    let failed = broker.execute(&auth, OP, 2).unwrap();
    assert_eq!(failed.stage, p::MaintenanceStage::RecoveryRequired);
    assert_eq!(
        fs::read(interrupted.join("keep-for-recovery")).unwrap(),
        b"partial previous publication"
    );
    assert!(!fixture.destination().exists());
    drop(broker);
    let before = fs::read(fixture.journal()).unwrap();
    let broker = fixture.broker();
    assert!(broker
        .execute(&broker.authenticate(TOKEN).unwrap(), OP, 3)
        .is_err());
    drop(broker);
    assert_eq!(fs::read(fixture.journal()).unwrap(), before);
}

#[test]
fn restart_rejects_misbound_stop_receipt_before_attempting_start() {
    struct WrongStop(Arc<AtomicUsize>);
    impl NativeServiceManager for WrongStop {
        fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
            NoServices.inspect(service)
        }
        fn apply(&self, change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(p::NativeReceipt {
                schema_version: p::SchemaVersion(1),
                service: p::BuiltinServiceId::Web,
                requested: change.action,
                observed: p::NativeServiceState::Stopped,
            })
        }
    }
    let fixture = Fixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = ProductHostBroker::open_with_service_manager(
        &fixture.journal(),
        &fixture.installation.install_id,
        HostCredentials::new(TOKEN.into(), None).unwrap(),
        fixture.installation.target,
        "0.0.1".into(),
        WrongStop(calls.clone()),
    )
    .unwrap();
    let auth = broker.authenticate(TOKEN).unwrap();
    let mut request = fixture.request(p::MaintenanceKind::RestartCore, 1);
    request.references.target_release_ref = "0.0.1".into();
    broker.plan(&auth, request, 1).unwrap();
    assert_eq!(
        broker.execute(&auth, OP, 2).unwrap().stage,
        p::MaintenanceStage::RecoveryRequired
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
