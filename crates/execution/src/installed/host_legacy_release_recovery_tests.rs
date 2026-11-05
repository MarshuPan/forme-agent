//! Controlled collector and service observations, not native installation evidence.
use super::*;
use crate::installed::{
    file_digest,
    product_config::tests::{fixture, patch},
    HostCredentials, HostManagement, WorkerExecutorHost,
};
use ed25519_dalek::{Signer, SigningKey};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    },
};

const WEB: &str = "web-legacy-review-test-abcdefghijklmnopqrstuvwxyz";
const ADMIN: &str = "admin-legacy-review-test-abcdefghijklmnopqrstuvwxyz";
const AGENT: &str = "agent-legacy-review-test-abcdefghijklmnopqrstuvwxyz";
const ORIGINAL: &str = "upgrade:legacy-host";
type Host = ProductHostBroker<Services>;
fn hash(text: &str) -> p::SchemaDigest {
    p::sha256_content_digest(text.as_bytes())
}

#[derive(Clone)]
struct Services {
    brain_running: Arc<AtomicBool>,
    changes: Arc<AtomicUsize>,
}
impl NativeServiceManager for Services {
    fn inspect(&self, service: p::BuiltinServiceId) -> p::Result<p::NativeServiceStatus> {
        Ok(p::NativeServiceStatus {
            schema_version: p::SchemaVersion(1),
            service,
            state: if service != p::BuiltinServiceId::Brain
                || self.brain_running.load(Ordering::SeqCst)
            {
                p::NativeServiceState::Running
            } else {
                p::NativeServiceState::Stopped
            },
        })
    }
    fn apply(&self, _change: p::ValidatedServiceChange) -> p::Result<p::NativeReceipt> {
        self.changes.fetch_add(1, Ordering::SeqCst);
        Err(invalid())
    }
}

struct Collector {
    report: Mutex<p::SignedLegacyReleaseReview>,
    paths: Vec<PathBuf>,
    held: Arc<AtomicBool>,
    releases: Arc<AtomicUsize>,
    checks: Arc<AtomicUsize>,
    fail_second_check: Arc<AtomicBool>,
    unknown_updater: Arc<AtomicBool>,
    wrong_worker: Arc<AtomicBool>,
    reader_version: Arc<AtomicUsize>,
}

struct Inspection {
    evidence: Vec<p::LegacyReleaseEvidence>,
    paths: Vec<PathBuf>,
    worker: p::SchemaDigest,
    held: Arc<AtomicBool>,
    releases: Arc<AtomicUsize>,
    checks: Arc<AtomicUsize>,
    fail_second_check: Arc<AtomicBool>,
    unknown_updater: Arc<AtomicBool>,
    wrong_worker: Arc<AtomicBool>,
    reader_version: Arc<AtomicUsize>,
}

impl LegacyReviewCollector for Collector {
    fn load(&self, reference: &str) -> p::Result<p::SignedLegacyReleaseReview> {
        let report = self.report.lock().unwrap();
        if reference != report.review.binding.review_id && reference != ORIGINAL {
            return Err(invalid());
        }
        Ok(report.clone())
    }
    fn inspect(
        &self,
        review: &p::LegacyReleaseReview,
        worker: &p::SchemaDigest,
        _deadline: Instant,
    ) -> p::Result<Box<dyn LegacyReviewInspection>> {
        if self.held.swap(true, Ordering::SeqCst) {
            return Err(invalid());
        }
        Ok(Box::new(Inspection {
            evidence: review.evidence.clone(),
            paths: self.paths.clone(),
            worker: worker.clone(),
            held: self.held.clone(),
            releases: self.releases.clone(),
            checks: self.checks.clone(),
            fail_second_check: self.fail_second_check.clone(),
            unknown_updater: self.unknown_updater.clone(),
            wrong_worker: self.wrong_worker.clone(),
            reader_version: self.reader_version.clone(),
        }))
    }
}
impl LegacyReviewInspection for Inspection {
    fn recheck(&self, _deadline: Instant) -> p::Result<LegacyReviewObservations> {
        assert!(self.held.load(Ordering::SeqCst));
        let check = self.checks.fetch_add(1, Ordering::SeqCst);
        if self.fail_second_check.load(Ordering::SeqCst) && check == 1 {
            return Err(invalid());
        }
        let mut evidence = self.evidence.clone();
        for (item, path) in evidence.iter_mut().zip(&self.paths) {
            item.artifact_digest = file_digest(path)?;
        }
        Ok(LegacyReviewObservations {
            evidence,
            updater_capabilities: p::ProductReleaseRecoveryCapabilities {
                schema_version: p::SchemaVersion(1),
                journal_read_max: self.reader_version.load(Ordering::SeqCst) as u32,
                updater_version: self.reader_version.load(Ordering::SeqCst) as u32,
            },
            worker_digest: if self.wrong_worker.load(Ordering::SeqCst) {
                hash("different-worker")
            } else {
                self.worker.clone()
            },
            updater_state: if self.unknown_updater.load(Ordering::SeqCst) {
                p::NativeServiceState::Unknown
            } else {
                p::NativeServiceState::Stopped
            },
        })
    }
    fn release(self: Box<Self>) -> p::Result<()> {
        assert!(self.held.swap(false, Ordering::SeqCst));
        self.releases.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    config: Arc<Mutex<ProductConfigStore>>,
    host: Host,
    workers: WorkerExecutorHost,
    collector: Arc<Collector>,
    key: SigningKey,
    auth: HostAuth,
    request: p::LegacyReleaseRecoveryRequest,
    services: Services,
}

impl Fixture {
    fn new() -> Self {
        let (root, _, config) = fixture();
        let config = Arc::new(config);
        config
            .lock()
            .unwrap()
            .apply(patch("save:legacy", 0))
            .unwrap();
        let identity = config
            .lock()
            .unwrap()
            .release_configuration_identity()
            .unwrap();
        let services = Services {
            brain_running: Arc::new(AtomicBool::new(false)),
            changes: Arc::new(AtomicUsize::new(0)),
        };
        let credentials = HostCredentials::new(ADMIN.into(), Some(AGENT.into()))
            .unwrap()
            .with_web_configuration(Some(WEB.into()))
            .unwrap();
        let mut host = ProductHostBroker::open_with_service_manager_and_config(
            &root.path().join("product.jsonl"),
            "install:config-test",
            credentials,
            p::ProductTarget::WindowsX86_64,
            "0.1.0".into(),
            Some(config.clone()),
            services.clone(),
        )
        .unwrap();
        let at = timestamp().unwrap();
        host.record_installation_verification(hash("installation"), hash("source"))
            .unwrap();
        let command = p::MaintenanceCommand {
            schema_version: p::SchemaVersion(1),
            operation_id: ORIGINAL.into(),
            idempotency_key: "key:original-host".into(),
            expected_revision: host.journal.revision().unwrap(),
            actor_proof: p::MaintenanceProofRef {
                schema_version: p::SchemaVersion(1),
                proof_ref: "proof:original".into(),
            },
            kind: p::MaintenanceKind::Upgrade {
                release_id: "0.2.0".into(),
                release_digest: hash("target"),
                backup_ref: "backup:host".into(),
            },
            requested_deadline: at - 1_000,
        };
        let original = host
            .journal
            .plan(
                command,
                p::MaintenanceActorClass::OwnerReauthenticated,
                p::MaintenanceReferences {
                    schema_version: p::SchemaVersion(1),
                    original_release_ref: "0.1.0".into(),
                    target_release_ref: "0.2.0".into(),
                    data_candidate_ref: None,
                    backup_ref: Some("backup:host".into()),
                },
                at - 5_000,
            )
            .unwrap();
        host.journal
            .begin_stage(
                ORIGINAL,
                host.journal.revision().unwrap(),
                p::MaintenanceStage::Validated,
                p::MaintenanceActorClass::OwnerReauthenticated,
                at - 4_999,
            )
            .unwrap();
        host.journal
            .finish_stage(
                ORIGINAL,
                host.journal.revision().unwrap(),
                p::MaintenanceStage::Validated,
                vec![hash("validated")],
                at - 4_998,
            )
            .unwrap();
        host.journal
            .prepare_release_takeover(
                host.journal.revision().unwrap(),
                p::ProductReleaseTakeover {
                    schema_version: p::SchemaVersion(1),
                    release: p::ProductReleaseRequest {
                        schema_version: p::SchemaVersion(1),
                        install_ref: "install:config-test".into(),
                        operation_id: ORIGINAL.into(),
                        command_digest: original.command_digest.clone(),
                        change: p::ProductReleaseChange::Upgrade,
                        original_release_ref: "0.1.0".into(),
                        original_release_digest: hash("source"),
                        target_release_ref: "0.2.0".into(),
                        target_release_digest: hash("target"),
                        backup_ref: "backup:host".into(),
                    },
                    installation_digest: hash("installation"),
                    owner: identity.0.clone(),
                    expected_config_revision: identity.1,
                    expected_config_digest: identity.2.clone(),
                    deadline: at - 1_000,
                    readiness_nonce: "b".repeat(64),
                },
                at - 4_997,
            )
            .unwrap();
        host.journal
            .begin_stage(
                ORIGINAL,
                host.journal.revision().unwrap(),
                p::MaintenanceStage::Quiescing,
                p::MaintenanceActorClass::OwnerReauthenticated,
                at - 4_996,
            )
            .unwrap();
        let checkpoint = host.journal.checkpoint().unwrap();
        let binding = p::LegacyReleaseReviewBinding {
            schema_version: p::SchemaVersion(1),
            review_id: "review:host".into(),
            install_ref: "install:config-test".into(),
            owner: identity.0.clone(),
            original_operation_id: ORIGINAL.into(),
            original_command_digest: original.command_digest,
            original_release_ref: "0.1.0".into(),
            original_release_digest: hash("source"),
            config_revision: identity.1,
            config_digest: identity.2,
            journal_seq: checkpoint.0,
            journal_head: checkpoint.1,
            expires_at: at + 90_000,
        };
        let mut paths = Vec::new();
        let evidence = p::LegacyReleaseEvidenceKind::ALL
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                let path = root.path().join(format!("artifact-{index}.json"));
                fs::write(&path, format!("original, controlled evidence {index}")).unwrap();
                paths.push(path.clone());
                p::LegacyReleaseEvidence {
                    schema_version: p::SchemaVersion(1),
                    kind,
                    artifact_digest: file_digest(&path).unwrap(),
                    binding_digest: binding.digest().unwrap(),
                    observation_digest: hash(&format!("semantic-observation-{index}")),
                }
            })
            .collect();
        let report = p::LegacyReleaseReview {
            schema_version: p::SchemaVersion(1),
            binding,
            evidence,
            status: p::LegacyReleaseReviewStatus::Verified,
            issues: vec![],
        };
        let key = SigningKey::from_bytes(&[43; 32]);
        let signature = key
            .sign(report.digest().unwrap().0.as_bytes())
            .to_bytes()
            .to_vec();
        let signed = p::SignedLegacyReleaseReview {
            schema_version: p::SchemaVersion(1),
            review: report,
            signature,
        };
        let request = p::LegacyReleaseRecoveryRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: "recover:host".into(),
            idempotency_key: "key:recover-host".into(),
            expected_revision: checkpoint.0,
            owner: identity.0,
            review_id: "review:host".into(),
            review_digest: signed.review_digest().unwrap(),
            requested_deadline: at + 60_000,
        };
        let collector = Arc::new(Collector {
            report: Mutex::new(signed),
            paths,
            held: Arc::new(AtomicBool::new(false)),
            releases: Arc::new(AtomicUsize::new(0)),
            checks: Arc::new(AtomicUsize::new(0)),
            fail_second_check: Arc::new(AtomicBool::new(false)),
            unknown_updater: Arc::new(AtomicBool::new(false)),
            wrong_worker: Arc::new(AtomicBool::new(false)),
            reader_version: Arc::new(AtomicUsize::new(3)),
        });
        host.legacy_release_reviewer = Some(LegacyReleaseReviewer {
            key: key.verifying_key(),
            collector: collector.clone(),
        });
        let workers = WorkerExecutorHost::open(
            &root.path().join("workers.jsonl"),
            hash("agent"),
            p::AuthorityEpoch(1),
            BTreeSet::from([p::Scope("workspace:legacy-test".into())]),
        )
        .unwrap();
        let auth = host.authenticate_web_configuration(WEB).unwrap();
        Self {
            _root: root,
            config,
            host,
            workers,
            collector,
            key,
            auth,
            request,
            services,
        }
    }

    fn apply(&self) -> p::Result<p::LegacyReleaseRecoveryView> {
        self.host.owner_legacy_release_recovery(
            &self.auth,
            self.request.clone(),
            &self.workers,
            timestamp().unwrap(),
        )
    }

    fn resign(&mut self) {
        let mut signed = self.collector.report.lock().unwrap();
        signed.signature = self
            .key
            .sign(signed.review.digest().unwrap().0.as_bytes())
            .to_bytes()
            .to_vec();
        self.request.review_digest = signed.review_digest().unwrap();
    }
}

#[test]
fn legacy_host_verifies_signed_files_and_commits_once_with_no_service_effects() {
    let f = Fixture::new();
    let revision = f.host.journal.revision().unwrap();
    let report = f
        .host
        .legacy_release_review(&f.auth, ORIGINAL, &f.workers, timestamp().unwrap())
        .unwrap();
    assert_eq!(report.status, p::LegacyReleaseReviewStatus::Verified);
    assert_eq!(f.host.journal.revision().unwrap(), revision);
    let committed = f.apply().unwrap();
    assert_eq!(committed.stage, p::LegacyReleaseRecoveryStage::Committed);
    assert_eq!(
        f.host.journal.operation(ORIGINAL).unwrap().stage,
        p::MaintenanceStage::Aborted
    );
    assert_eq!(
        f.host.journal.active_release().unwrap(),
        Some(("0.1.0".into(), hash("source")))
    );
    let checks = f.collector.checks.load(Ordering::SeqCst);
    let mut expired = f.request.clone();
    expired.requested_deadline = 1;
    assert!(f
        .host
        .owner_legacy_release_recovery(&f.auth, expired, &f.workers, timestamp().unwrap())
        .is_err());
    assert_eq!(f.apply().unwrap(), committed);
    assert_eq!(f.collector.checks.load(Ordering::SeqCst), checks);
    assert_eq!(f.services.changes.load(Ordering::SeqCst), 0);
    assert!(!f.collector.held.load(Ordering::SeqCst));
    assert_eq!(
        f.host
            .owner_legacy_release_recoveries(&f.auth)
            .unwrap()
            .operations,
        vec![committed]
    );
}

#[test]
fn legacy_host_rejects_foreign_identity_missing_collector_and_forged_signatures_before_append() {
    for mode in 0..6 {
        let mut f = Fixture::new();
        let before = f.host.journal.checkpoint().unwrap();
        match mode {
            0 => {
                f.auth = f.host.authenticate(ADMIN).unwrap();
            }
            1 => {
                f.request.owner = p::OwnerPrincipalRef("owner:foreign".into());
            }
            2 => {
                f.host.legacy_release_reviewer = None;
            }
            3 => {
                f.collector.report.lock().unwrap().signature[0] ^= 1;
            }
            4 => {
                f.host.legacy_release_reviewer.as_mut().unwrap().key =
                    SigningKey::from_bytes(&[44; 32]).verifying_key();
            }
            _ => f.auth = f.host.authenticate_agent(AGENT).unwrap(),
        }
        assert!(f.apply().is_err(), "mode {mode}");
        assert_eq!(f.host.journal.checkpoint().unwrap(), before);
        assert_eq!(f.collector.checks.load(Ordering::SeqCst), 0);
        assert_eq!(f.services.changes.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn legacy_host_rejects_each_missing_or_changed_artifact_despite_valid_signature() {
    for mode in 0..16 {
        let f = Fixture::new();
        let before = f.host.journal.checkpoint().unwrap();
        let path = &f.collector.paths[mode / 2];
        if mode % 2 == 0 {
            fs::write(path, "changed material").unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
        assert!(f.apply().is_err(), "mode {mode}");
        assert_eq!(f.host.journal.checkpoint().unwrap(), before);
        assert_eq!(f.services.changes.load(Ordering::SeqCst), 0);
        assert_eq!(f.collector.releases.load(Ordering::SeqCst), 1);
        assert!(!f.collector.held.load(Ordering::SeqCst));
    }
}

#[test]
fn legacy_host_rejects_service_worker_configuration_and_review_drift() {
    for mode in 0..7 {
        let mut f = Fixture::new();
        let before = f.host.journal.checkpoint().unwrap();
        match mode {
            0 => f.services.brain_running.store(true, Ordering::SeqCst),
            1 => f.collector.unknown_updater.store(true, Ordering::SeqCst),
            2 => f.collector.wrong_worker.store(true, Ordering::SeqCst),
            3 => {
                f.config
                    .lock()
                    .unwrap()
                    .apply(patch("save:changed", 1))
                    .unwrap();
            }
            4 => {
                f.collector.report.lock().unwrap().review.status =
                    p::LegacyReleaseReviewStatus::Unverifiable;
                f.collector.report.lock().unwrap().review.issues =
                    vec!["issue:missing-history".into()];
                f.resign();
            }
            5 => {
                let mut signed = f.collector.report.lock().unwrap();
                signed.review.binding.journal_head = hash("changed-head");
                let binding = signed.review.binding.digest().unwrap();
                for item in &mut signed.review.evidence {
                    item.binding_digest = binding.clone();
                }
                drop(signed);
                f.resign();
            }
            _ => f.collector.reader_version.store(2, Ordering::SeqCst),
        }
        assert!(f.apply().is_err(), "mode {mode}");
        assert_eq!(f.host.journal.checkpoint().unwrap(), before);
        assert_eq!(f.services.changes.load(Ordering::SeqCst), 0);
        assert!(!f.collector.held.load(Ordering::SeqCst));
    }
}

#[test]
fn legacy_host_interrupted_intent_is_query_only_until_original_verification() {
    let mut f = Fixture::new();
    f.collector.fail_second_check.store(true, Ordering::SeqCst);
    assert!(f.apply().is_err());
    let intent = f
        .host
        .owner_legacy_release_recovery_operation(&f.auth, &f.request.operation_id)
        .unwrap();
    assert_eq!(intent.stage, p::LegacyReleaseRecoveryStage::Intent);
    let checks = f.collector.checks.load(Ordering::SeqCst);
    assert_eq!(f.apply().unwrap(), intent);
    assert_eq!(f.collector.checks.load(Ordering::SeqCst), checks);
    assert!(!f.collector.held.load(Ordering::SeqCst));
    // Replay uses the original locked file; it neither collects nor commits.
    let path = f._root.path().join("product.jsonl");
    let replacement = crate::installed::ProductJournal::open(
        &f._root.path().join("empty.jsonl"),
        "install:config-test",
    )
    .unwrap();
    let old = std::mem::replace(&mut f.host.journal, replacement);
    drop(old);
    f.host.journal = crate::installed::ProductJournal::open(&path, "install:config-test").unwrap();
    assert_eq!(
        f.host
            .owner_legacy_release_recovery_operation(&f.auth, &f.request.operation_id)
            .unwrap(),
        intent
    );
    assert_eq!(f.collector.checks.load(Ordering::SeqCst), checks);
    let checkpoint = f.host.journal.checkpoint().unwrap();
    // Reload may happen after the first request expired. Read its saved report
    // without a collector call; only a separately authorized verify can commit.
    let saved = f
        .host
        .legacy_release_review(
            &f.auth,
            ORIGINAL,
            &f.workers,
            f.request.requested_deadline + 1,
        )
        .unwrap();
    assert_eq!(saved.digest().unwrap(), f.request.review_digest);
    assert_eq!(f.host.journal.checkpoint().unwrap(), checkpoint);
    assert_eq!(f.collector.checks.load(Ordering::SeqCst), checks);
    assert!(f
        .host
        .legacy_release_review(&f.auth, ORIGINAL, &f.workers, saved.binding.expires_at)
        .is_err());
    let mut verify = p::LegacyReleaseRecoveryVerification {
        schema_version: p::SchemaVersion(1),
        operation_id: f.request.operation_id.clone(),
        request_digest: f.request.digest().unwrap(),
        expected_revision: intent.revision,
        owner: f.request.owner.clone(),
        requested_deadline: timestamp().unwrap() + 30_000,
    };
    verify.request_digest = hash("replacement-request");
    assert!(f
        .host
        .verify_owner_legacy_release_recovery(
            &f.auth,
            verify.clone(),
            &f.workers,
            timestamp().unwrap()
        )
        .is_err());
    verify.request_digest = f.request.digest().unwrap();
    let committed = f
        .host
        .verify_owner_legacy_release_recovery(&f.auth, verify, &f.workers, timestamp().unwrap())
        .unwrap();
    assert_eq!(committed.stage, p::LegacyReleaseRecoveryStage::Committed);
    assert_eq!(f.services.changes.load(Ordering::SeqCst), 0);
}
