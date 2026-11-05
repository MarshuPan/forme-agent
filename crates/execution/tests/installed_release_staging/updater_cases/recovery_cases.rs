//! Real filesystem, configuration writer and worker journal, controlled SCM/data.
use super::*;
use forme_execution::installed::{ReleaseRecoveryDataInspection, WorkerExecutorHost};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

struct Boundary {
    base: Platform,
    valid_services: AtomicBool,
    observations: AtomicUsize,
    fail_observation: AtomicUsize,
    data: Mutex<p::SchemaDigest>,
    held: Arc<AtomicUsize>,
    inspections: AtomicUsize,
    hold_checks: Arc<AtomicUsize>,
    fail_hold: Arc<AtomicUsize>,
}
impl Default for Boundary {
    fn default() -> Self {
        Self {
            base: Platform::default(),
            valid_services: AtomicBool::new(true),
            observations: AtomicUsize::new(0),
            fail_observation: AtomicUsize::new(0),
            data: Mutex::new(p::canonical_digest(&"database").unwrap()),
            held: Arc::new(AtomicUsize::new(0)),
            inspections: AtomicUsize::new(0),
            hold_checks: Arc::new(AtomicUsize::new(0)),
            fail_hold: Arc::new(AtomicUsize::new(0)),
        }
    }
}
struct DataGuard {
    identity: p::ProductReleaseDataIdentity,
    held: Arc<AtomicUsize>,
    hold_checks: Arc<AtomicUsize>,
    fail_hold: Arc<AtomicUsize>,
}
impl Drop for DataGuard {
    fn drop(&mut self) {
        assert_eq!(self.held.fetch_sub(1, Ordering::SeqCst), 1);
    }
}
impl ReleaseRecoveryDataInspection for DataGuard {
    fn identity(&self) -> &p::ProductReleaseDataIdentity {
        &self.identity
    }
    fn ensure_held(&mut self, deadline: Instant) -> p::Result<()> {
        let call = self.hold_checks.fetch_add(1, Ordering::SeqCst) + 1;
        if self.held.load(Ordering::SeqCst) != 1
            || Instant::now() >= deadline
            || self.fail_hold.load(Ordering::SeqCst) == call
        {
            return Err(p::Error("controlled lost data custody".into()));
        }
        Ok(())
    }
    fn release(&mut self) -> p::Result<()> {
        Ok(())
    }
}
impl ReleaseUpdaterPlatform for Boundary {
    fn observe_recovery_boundary(
        &self,
        _: &p::InstallationManifest,
        _: &Path,
    ) -> p::Result<p::SchemaDigest> {
        let call = self.observations.fetch_add(1, Ordering::SeqCst) + 1;
        if !self.valid_services.load(Ordering::SeqCst)
            || self.fail_observation.load(Ordering::SeqCst) == call
        {
            return Err(p::Error("controlled interrupted source observation".into()));
        }
        p::canonical_digest(&"exact-stopped-updater-and-brain-service-configuration")
    }
    fn inspect_recovery_data(
        &self,
        _: forme_execution::installed::InstalledReleaseDataWorker,
        request: p::ProductReleaseDataRequest,
        _: Instant,
    ) -> p::Result<Box<dyn ReleaseRecoveryDataInspection>> {
        assert_eq!(self.held.fetch_add(1, Ordering::SeqCst), 0);
        self.inspections.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(DataGuard {
            held: self.held.clone(),
            hold_checks: self.hold_checks.clone(),
            fail_hold: self.fail_hold.clone(),
            identity: p::ProductReleaseDataIdentity {
                schema_version: p::SchemaVersion(1),
                config_schema_version: 1,
                database_schema_version: 13,
                owner_digest: p::sha256_content_digest(request.owner.0.as_bytes()),
                store_identity: p::canonical_digest(&"store").unwrap(),
                schema_digest: p::canonical_digest(&"schema").unwrap(),
                database_digest: self.data.lock().unwrap().clone(),
                event_count: 7,
                last_stream_seq: 7,
            },
        }))
    }
    fn verify_host_image(&self, image: &Path, digest: &p::SchemaDigest) -> p::Result<()> {
        self.base.verify_host_image(image, digest)
    }
    fn verify_updater(&self, _: &p::InstallationManifest, _: &Path) -> p::Result<()> {
        Ok(())
    }
    fn launch(&self, _: &p::InstallationManifest, _: &Path) -> p::Result<()> {
        panic!("adjudication cannot dispatch an updater")
    }
    fn observe(
        &self,
        i: &p::InstallationManifest,
        r: &Path,
        s: p::BuiltinServiceId,
        n: Option<p::NativeServiceState>,
    ) -> p::Result<p::SchemaDigest> {
        self.base.observe(i, r, s, n)
    }
    fn change(
        &self,
        _: &p::InstallationManifest,
        _: &Path,
        _: p::BuiltinServiceId,
        _: p::NativeServiceAction,
        _: Instant,
    ) -> p::Result<p::SchemaDigest> {
        panic!("adjudication cannot change services")
    }
    fn switch_images(
        &self,
        _: forme_execution::installed::PreparedReleaseServiceSwitch,
        _: &ProductJournal,
        _: Instant,
    ) -> p::Result<p::ProductReleaseServiceReceipt> {
        panic!("adjudication cannot switch images")
    }
}
struct RecoveryFixture {
    f: Fixture,
    platform: Arc<Boundary>,
    broker: Arc<ProductHostBroker<NoServices>>,
    workers: WorkerExecutorHost,
    request: p::ProductMaintenanceRequest,
}
impl RecoveryFixture {
    fn new(baseline: bool) -> Self {
        let f = Fixture::new();
        let journal = f.stage();
        let revision = journal.revision().unwrap();
        drop(journal);
        let config = product_config(&f);
        for name in ["forme-cli", "forme-executord"] {
            fs::write(
                f.installation
                    .paths
                    .releases
                    .join(format!("bin/{name}.exe")),
                name,
            )
            .unwrap();
        }
        fs::write(
            f.installation.paths.config.join("worker.json"),
            b"fixed-worker-configuration",
        )
        .unwrap();
        let platform = Arc::new(Boundary::default());
        let updater = Arc::new(
            InstalledReleaseUpdater::new(
                f.installation.clone(),
                f.installation.paths.config.join("host.json"),
                Arc::new(f.stager()),
                platform.clone(),
            )
            .unwrap(),
        );
        let broker = Arc::new(make_broker(&f, config.clone(), updater));
        let workers = WorkerExecutorHost::open(
            &f.installation.paths.host_state.join("workers"),
            p::canonical_digest(&"agent-credential").unwrap(),
            p::AuthorityEpoch(1),
            [p::Scope("workspace:test".into())].into_iter().collect(),
        )
        .unwrap();
        let mut request = request(&f, revision);
        request.requested_deadline = now_millis() + 500;
        let auth = broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
        let result = if baseline {
            broker.prepare_owner_release_with_workers(
                &auth,
                request.clone(),
                &workers,
                now_millis(),
            )
        } else {
            broker.prepare_owner_release(&auth, request.clone(), now_millis())
        };
        assert!(result.unwrap().1.is_some());
        let wait = request
            .requested_deadline
            .saturating_sub(now_millis())
            .max(0) as u64
            + 2;
        std::thread::sleep(Duration::from_millis(wait));
        Self {
            f,
            platform,
            broker,
            workers,
            request,
        }
    }
    fn recovery(&self) -> p::ProductReleaseRecoveryRequest {
        let auth = self
            .broker
            .authenticate_web_configuration(WEB_TOKEN)
            .unwrap();
        let candidate = self
            .broker
            .release_recovery_candidate(
                &auth,
                &self.request.operation_id,
                &self.workers,
                now_millis(),
            )
            .unwrap();
        p::ProductReleaseRecoveryRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: "recovery:bounded".into(),
            idempotency_key: "key:bounded".into(),
            expected_revision: candidate.product_revision,
            owner: candidate.owner.clone(),
            decision: p::ProductReleaseRecoveryDecision::AbortBeforeSwitch,
            candidate,
            requested_deadline: now_millis() + 60_000,
        }
    }
}

#[test]
fn release_recovery_host_commits_once_then_old_release_replay_and_queries_are_effect_free() {
    let f = RecoveryFixture::new(true);
    let request = f.recovery();
    let auth = f.broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
    let original_digest =
        file_digest(&f.f.installation.paths.releases.join("bin/forme-hostd.exe")).unwrap();
    let view = f
        .broker
        .owner_release_recovery(&auth, request.clone(), &f.workers, now_millis())
        .unwrap();
    assert_eq!(view.stage, p::ProductReleaseRecoveryStage::Committed);
    assert_eq!(f.platform.held.load(Ordering::SeqCst), 0);
    let inspections = f.platform.inspections.load(Ordering::SeqCst);
    let seq = f
        .broker
        .maintenance_snapshot(&auth)
        .unwrap()
        .product_revision;
    assert_eq!(
        f.broker
            .owner_release_recovery(&auth, request.clone(), &f.workers, now_millis())
            .unwrap(),
        view
    );
    assert_eq!(
        f.broker
            .owner_release_recovery_operation(&auth, &request.operation_id)
            .unwrap(),
        view
    );
    assert_eq!(
        f.broker.owner_release_recoveries(&auth).unwrap().operations,
        vec![view.clone()]
    );
    let (original, ticket) = f
        .broker
        .prepare_owner_release_with_workers(&auth, f.request.clone(), &f.workers, now_millis())
        .unwrap();
    assert_eq!(original.stage, p::MaintenanceStage::Aborted);
    assert!(ticket.is_none());
    assert_eq!(
        f.broker
            .maintenance_snapshot(&auth)
            .unwrap()
            .product_revision,
        seq
    );
    assert_eq!(f.platform.inspections.load(Ordering::SeqCst), inspections);
    assert_eq!(
        file_digest(&f.f.installation.paths.releases.join("bin/forme-hostd.exe")).unwrap(),
        original_digest
    );
    assert!(f
        .f
        .installation
        .paths
        .host_state
        .join("release-handoff.json")
        .exists());
    assert!(!f.f.installation.paths.backups.exists());
    assert_eq!(f.broker.configuration(&auth).unwrap().active_revision, 1);
    let mut conflict = request;
    conflict.requested_deadline += 1;
    assert!(f
        .broker
        .owner_release_recovery(&auth, conflict, &f.workers, now_millis())
        .is_err());
}

#[test]
fn release_recovery_host_rejects_missing_baseline_and_service_data_file_worker_drift() {
    for case in ["legacy", "service", "data", "image", "config"] {
        let f = RecoveryFixture::new(case != "legacy");
        let auth = f.broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
        let revision = f
            .broker
            .maintenance_snapshot(&auth)
            .unwrap()
            .product_revision;
        match case {
            "service" => f.platform.valid_services.store(false, Ordering::SeqCst),
            "data" => {
                *f.platform.data.lock().unwrap() = p::canonical_digest(&"changed-database").unwrap()
            }
            "image" => fs::write(
                f.f.installation.paths.releases.join("bin/forme-cli.exe"),
                "replaced-image",
            )
            .unwrap(),
            "config" => fs::write(
                f.f.installation.paths.config.join("worker.json"),
                "changed-config",
            )
            .unwrap(),
            _ => {}
        }
        assert!(
            f.broker
                .release_recovery_candidate(
                    &auth,
                    &f.request.operation_id,
                    &f.workers,
                    now_millis()
                )
                .is_err(),
            "{case}"
        );
        assert_eq!(
            f.broker
                .maintenance_snapshot(&auth)
                .unwrap()
                .product_revision,
            revision
        );
        assert!(f
            .broker
            .owner_release_recoveries(&auth)
            .unwrap()
            .operations
            .is_empty());
        assert_eq!(f.platform.held.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn release_recovery_rechecks_original_data_custody_before_committing_and_keeps_intent() {
    for fail_before_intent in [true, false] {
        let f = RecoveryFixture::new(true);
        let request = f.recovery();
        let auth = f.broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
        f.platform.fail_hold.store(
            f.platform.hold_checks.load(Ordering::SeqCst) + if fail_before_intent { 1 } else { 2 },
            Ordering::SeqCst,
        );
        assert!(f
            .broker
            .owner_release_recovery(&auth, request.clone(), &f.workers, now_millis())
            .is_err());
        assert_eq!(f.platform.held.load(Ordering::SeqCst), 0);
        let operations = f.broker.owner_release_recoveries(&auth).unwrap();
        if fail_before_intent {
            assert!(operations.operations.is_empty());
            assert_eq!(operations.product_revision, request.expected_revision);
            continue;
        }
        let intent = f
            .broker
            .owner_release_recovery_operation(&auth, &request.operation_id)
            .unwrap();
        assert_eq!(intent.stage, p::ProductReleaseRecoveryStage::Intent);
        let checks = f.platform.hold_checks.load(Ordering::SeqCst);
        let inspections = f.platform.inspections.load(Ordering::SeqCst);
        assert_eq!(
            f.broker
                .owner_release_recovery(&auth, request.clone(), &f.workers, now_millis())
                .unwrap(),
            intent
        );
        assert_eq!(f.platform.hold_checks.load(Ordering::SeqCst), checks);
        assert_eq!(f.platform.inspections.load(Ordering::SeqCst), inspections);
        let verification = p::ProductReleaseRecoveryVerification {
            schema_version: p::SchemaVersion(1),
            operation_id: request.operation_id,
            request_digest: intent.request_digest.clone(),
            expected_revision: intent.revision,
            owner: request.owner,
            requested_deadline: now_millis() + 60_000,
        };
        assert_eq!(
            f.broker
                .verify_owner_release_recovery(&auth, verification, &f.workers, now_millis())
                .unwrap()
                .stage,
            p::ProductReleaseRecoveryStage::Committed
        );
        assert_eq!(
            f.platform.inspections.load(Ordering::SeqCst),
            inspections + 1
        );
        assert_eq!(f.platform.held.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn release_recovery_host_intent_fault_requires_explicit_verification_and_never_retries_on_replay() {
    let f = RecoveryFixture::new(true);
    let request = f.recovery();
    let auth = f.broker.authenticate_web_configuration(WEB_TOKEN).unwrap();
    // Two observations guard data acquisition; the third is the final recheck,
    // after the durable intent and while the data guard is still held.
    f.platform.fail_observation.store(
        f.platform.observations.load(Ordering::SeqCst) + 3,
        Ordering::SeqCst,
    );
    assert!(f
        .broker
        .owner_release_recovery(&auth, request.clone(), &f.workers, now_millis())
        .is_err());
    let intent = f
        .broker
        .owner_release_recovery_operation(&auth, &request.operation_id)
        .unwrap();
    assert_eq!(intent.stage, p::ProductReleaseRecoveryStage::Intent);
    let observed = f.platform.observations.load(Ordering::SeqCst);
    assert_eq!(
        f.broker
            .owner_release_recovery(&auth, request.clone(), &f.workers, now_millis())
            .unwrap(),
        intent
    );
    assert_eq!(f.platform.observations.load(Ordering::SeqCst), observed);
    let revision = f
        .broker
        .owner_release_recoveries(&auth)
        .unwrap()
        .product_revision;
    let verify = p::ProductReleaseRecoveryVerification {
        schema_version: p::SchemaVersion(1),
        operation_id: request.operation_id,
        request_digest: intent.request_digest.clone(),
        expected_revision: revision,
        owner: request.owner,
        requested_deadline: now_millis() + 60_000,
    };
    let mut stale = verify.clone();
    stale.expected_revision -= 1;
    assert!(f
        .broker
        .verify_owner_release_recovery(&auth, stale, &f.workers, now_millis())
        .is_err());
    assert_eq!(
        f.broker
            .verify_owner_release_recovery(&auth, verify, &f.workers, now_millis())
            .unwrap()
            .stage,
        p::ProductReleaseRecoveryStage::Committed
    );
    assert_eq!(f.platform.held.load(Ordering::SeqCst), 0);
}
