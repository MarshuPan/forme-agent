use super::*;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;

const INSTALL: &str = "install:legacy-review";
const ORIGINAL: &str = "upgrade:legacy";
const OWNER: &str = "owner:legacy";

fn hash(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}

fn bytes(journal: &ProductJournal) -> Vec<u8> {
    let mut writer = journal.writer.lock().unwrap();
    let position = writer.file.stream_position().unwrap();
    writer.file.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    writer.file.read_to_end(&mut bytes).unwrap();
    writer.file.seek(SeekFrom::Start(position)).unwrap();
    bytes
}

struct Fixture {
    _root: tempfile::TempDir,
    path: PathBuf,
    journal: Arc<ProductJournal>,
    command: p::MaintenanceCommand,
    historical: (u64, p::SchemaDigest, p::SchemaDigest),
    current: (u64, p::SchemaDigest, p::SchemaDigest),
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("product.jsonl");
        let journal = Arc::new(ProductJournal::open(&path, INSTALL).unwrap());
        journal
            .record_installation_verification_with_active_release(
                hash("installation"),
                hash("source-release"),
                Some(("0.1.0", &hash("source-release"))),
            )
            .unwrap();
        let command = p::MaintenanceCommand {
            schema_version: p::SchemaVersion(1),
            operation_id: ORIGINAL.into(),
            idempotency_key: "key:legacy-original".into(),
            expected_revision: journal.revision().unwrap(),
            actor_proof: p::MaintenanceProofRef {
                schema_version: p::SchemaVersion(1),
                proof_ref: "proof:legacy".into(),
            },
            kind: p::MaintenanceKind::Upgrade {
                release_id: "0.2.0".into(),
                release_digest: hash("target-release"),
                backup_ref: "backup:legacy".into(),
            },
            requested_deadline: 10_000,
        };
        let operation = journal
            .plan(
                command.clone(),
                p::MaintenanceActorClass::OwnerReauthenticated,
                p::MaintenanceReferences {
                    schema_version: p::SchemaVersion(1),
                    original_release_ref: "0.1.0".into(),
                    target_release_ref: "0.2.0".into(),
                    data_candidate_ref: None,
                    backup_ref: Some("backup:legacy".into()),
                },
                1,
            )
            .unwrap();
        stage(&journal, ORIGINAL, p::MaintenanceStage::Validated, 2, 3);
        let takeover = p::ProductReleaseTakeover {
            schema_version: p::SchemaVersion(1),
            release: p::ProductReleaseRequest {
                schema_version: p::SchemaVersion(1),
                install_ref: INSTALL.into(),
                operation_id: ORIGINAL.into(),
                command_digest: operation.command_digest.clone(),
                change: p::ProductReleaseChange::Upgrade,
                original_release_ref: "0.1.0".into(),
                original_release_digest: hash("source-release"),
                target_release_ref: "0.2.0".into(),
                target_release_digest: hash("target-release"),
                backup_ref: "backup:legacy".into(),
            },
            installation_digest: hash("installation"),
            owner: p::OwnerPrincipalRef(OWNER.into()),
            expected_config_revision: 1,
            expected_config_digest: hash("config"),
            deadline: 10_000,
            readiness_nonce: "a".repeat(64),
        };
        journal
            .prepare_release_takeover(journal.revision().unwrap(), takeover.clone(), 4)
            .unwrap();
        let historical = journal.checkpoint().unwrap();
        journal
            .begin_stage(
                ORIGINAL,
                journal.revision().unwrap(),
                p::MaintenanceStage::Quiescing,
                p::MaintenanceActorClass::OwnerReauthenticated,
                5,
            )
            .unwrap();
        let current = journal.checkpoint().unwrap();
        Self {
            _root: root,
            path,
            journal,
            command,
            historical,
            current,
        }
    }

    fn evidence(&self) -> p::LegacyReleaseRecoveryEvidence {
        let data = data_identity();
        p::LegacyReleaseRecoveryEvidence {
            schema_version: p::SchemaVersion(1),
            status: p::LegacyReleaseRecoveryEvidenceStatus::Verified,
            installation_id: INSTALL.into(),
            owner: p::OwnerPrincipalRef(OWNER.into()),
            original_operation_id: ORIGINAL.into(),
            original_command_digest: self
                .command
                .bound_digest(&p::MaintenanceReferences {
                    schema_version: p::SchemaVersion(1),
                    original_release_ref: "0.1.0".into(),
                    target_release_ref: "0.2.0".into(),
                    data_candidate_ref: None,
                    backup_ref: Some("backup:legacy".into()),
                })
                .unwrap(),
            original_release_ref: "0.1.0".into(),
            original_release_digest: hash("source-release"),
            historical_journal_head: self.historical.1.clone(),
            historical_journal_sequence: self.historical.0,
            historical_journal_history_digest: self.historical.2.clone(),
            historical_archive_digest: hash("historical-archive"),
            current_journal_head: self.current.1.clone(),
            current_journal_sequence: self.current.0,
            current_journal_history_digest: self.current.2.clone(),
            current_observation_digest: hash("current-observation"),
            historical_data_identity: data.clone(),
            current_data_identity: data,
            historical_worker_digest: hash("worker-stopped"),
            current_worker_digest: hash("worker-stopped"),
            historical_service_digest: hash("services-stopped"),
            current_service_digest: hash("services-stopped"),
            historical_config_digest: hash("config-files"),
            current_config_digest: hash("config-files"),
            historical_program_digest: hash("programs"),
            current_program_digest: hash("programs"),
            repair_package_digest: hash("repair-package"),
            reader_updater_compatibility_digest: hash("reader-updater"),
            reviewed_at: 6,
            reason_digest: None,
        }
    }

    fn request(&self) -> p::LegacyReleaseRecoveryReviewRequest {
        let evidence = self.evidence();
        p::LegacyReleaseRecoveryReviewRequest {
            schema_version: p::SchemaVersion(1),
            review_id: "review:legacy".into(),
            idempotency_key: "key:legacy-review".into(),
            expected_revision: self.current.0,
            owner: p::OwnerPrincipalRef(OWNER.into()),
            evidence_digest: evidence.digest().unwrap(),
            evidence,
            requested_deadline: 20_000,
        }
    }
}

fn data_identity() -> p::ProductReleaseDataIdentity {
    p::ProductReleaseDataIdentity {
        schema_version: p::SchemaVersion(1),
        config_schema_version: 1,
        database_schema_version: 9,
        owner_digest: p::sha256_content_digest(OWNER.as_bytes()),
        store_identity: hash("store"),
        schema_digest: hash("schema"),
        database_digest: hash("database"),
        event_count: 4,
        last_stream_seq: 4,
    }
}

fn stage(
    journal: &ProductJournal,
    id: &str,
    stage: p::MaintenanceStage,
    intent_at: p::Timestamp,
    receipt_at: p::Timestamp,
) {
    journal
        .begin_stage(
            id,
            journal.revision().unwrap(),
            stage,
            p::MaintenanceActorClass::OwnerReauthenticated,
            intent_at,
        )
        .unwrap();
    journal
        .finish_stage(
            id,
            journal.revision().unwrap(),
            stage,
            vec![hash("stage-receipt")],
            receipt_at,
        )
        .unwrap();
}

fn verification(
    request: &p::LegacyReleaseRecoveryReviewRequest,
    revision: u64,
    deadline: p::Timestamp,
) -> p::LegacyReleaseRecoveryReviewVerification {
    p::LegacyReleaseRecoveryReviewVerification {
        schema_version: p::SchemaVersion(1),
        review_id: request.review_id.clone(),
        request_digest: request.digest().unwrap(),
        expected_revision: revision,
        owner: request.owner.clone(),
        requested_deadline: deadline,
    }
}

#[test]
fn legacy_review_commits_without_touching_active_release_or_history_and_replays() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let before = bytes(&fixture.journal);
    let active_before = fixture.journal.active_release().unwrap();
    fixture
        .journal
        .begin_legacy_recovery_review(request.clone(), 10_001)
        .unwrap();
    let intent = fixture
        .journal
        .legacy_recovery_review_operation(&request.review_id)
        .unwrap();
    assert_eq!(intent.stage, p::LegacyReleaseRecoveryReviewStage::Intent);
    assert_eq!(
        fixture.journal.operation(ORIGINAL).unwrap().pending_stage,
        Some(p::MaintenanceStage::Quiescing)
    );
    assert_eq!(fixture.journal.active_release().unwrap(), active_before);
    assert!(bytes(&fixture.journal).starts_with(&before));

    fixture
        .journal
        .commit_legacy_recovery_review(
            verification(&request, intent.revision, 30_000),
            request.evidence_digest.clone(),
            10_002,
        )
        .unwrap();
    let committed = fixture
        .journal
        .legacy_recovery_review_operation(&request.review_id)
        .unwrap();
    assert_eq!(
        committed.stage,
        p::LegacyReleaseRecoveryReviewStage::Committed
    );
    assert!(committed.receipt_digest.is_some());
    let original = fixture.journal.operation(ORIGINAL).unwrap();
    assert_eq!(original.stage, p::MaintenanceStage::Aborted);
    assert_eq!(original.pending_stage, None);
    assert_eq!(fixture.journal.active_release().unwrap(), active_before);
    assert_eq!(original.revision, committed.revision);

    let list = fixture
        .journal
        .legacy_recovery_reviews(&request.owner)
        .unwrap();
    assert_eq!(list.reviews, vec![committed.clone()]);
    let committed_bytes = bytes(&fixture.journal);
    drop(fixture.journal);
    let replayed = ProductJournal::open(&fixture.path, INSTALL).unwrap();
    assert_eq!(
        replayed
            .legacy_recovery_review_operation(&request.review_id)
            .unwrap(),
        committed
    );
    assert_eq!(replayed.operation(ORIGINAL).unwrap(), original);
    assert_eq!(replayed.active_release().unwrap(), active_before);
    assert_eq!(bytes(&replayed), committed_bytes);

    let mut operation_collision = fixture.command.clone();
    operation_collision.operation_id = request.review_id.clone();
    operation_collision.idempotency_key = "key:future-operation".into();
    operation_collision.requested_deadline = 30_000;
    operation_collision.expected_revision = replayed.revision().unwrap();
    assert!(replayed
        .plan(
            operation_collision,
            p::MaintenanceActorClass::OwnerReauthenticated,
            p::MaintenanceReferences {
                schema_version: p::SchemaVersion(1),
                original_release_ref: "0.1.0".into(),
                target_release_ref: "0.2.0".into(),
                data_candidate_ref: None,
                backup_ref: Some("backup:legacy".into()),
            },
            10_003,
        )
        .is_err());
}

#[test]
fn legacy_review_rejects_missing_or_changed_evidence_without_appending() {
    let fixture = Fixture::new();
    let before = bytes(&fixture.journal);
    let base = fixture.request();
    for status in [
        p::LegacyReleaseRecoveryEvidenceStatus::Rejected,
        p::LegacyReleaseRecoveryEvidenceStatus::Unverifiable,
    ] {
        let mut request = base.clone();
        request.evidence.status = status;
        request.evidence_digest = request.evidence.digest().unwrap();
        assert!(fixture
            .journal
            .begin_legacy_recovery_review(request, 10_001)
            .is_err());
    }
    for change in 0..4 {
        let mut request = base.clone();
        match change {
            0 => request.evidence.current_journal_head = hash("wrong-head"),
            1 => request.evidence.original_command_digest = hash("wrong-command"),
            2 => request.evidence.current_data_identity.database_digest = hash("changed-db"),
            _ => request.evidence.original_release_ref = "0.1.1".into(),
        }
        request.evidence_digest = request.evidence.digest().unwrap();
        assert!(fixture
            .journal
            .begin_legacy_recovery_review(request, 10_001)
            .is_err());
    }
    assert_eq!(bytes(&fixture.journal), before);
    assert!(fixture
        .journal
        .release_recovery_candidate(ORIGINAL, 20_000)
        .is_err());
}

#[test]
fn legacy_review_idempotency_cross_domain_conflicts_and_cas_are_fail_closed() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let other = {
        let mut value = request.clone();
        value.review_id = "review:other".into();
        value.idempotency_key = "key:other".into();
        value
    };
    let candidates = [request.clone(), other.clone()];
    let handles = candidates
        .iter()
        .cloned()
        .map(|request| {
            let journal = fixture.journal.clone();
            std::thread::spawn(move || journal.begin_legacy_recovery_review(request, 10_001))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap().is_ok())
            .filter(|ok| *ok)
            .count(),
        1
    );
    let winner = if fixture
        .journal
        .legacy_recovery_review_operation(&request.review_id)
        .is_ok()
    {
        request.clone()
    } else {
        other
    };
    let saved = bytes(&fixture.journal);
    assert!(fixture
        .journal
        .begin_legacy_recovery_review(winner.clone(), 10_002)
        .is_ok());
    assert_eq!(bytes(&fixture.journal), saved);

    let mut changed = winner.clone();
    changed.evidence.current_observation_digest = hash("changed-observation");
    changed.evidence_digest = changed.evidence.digest().unwrap();
    assert!(fixture
        .journal
        .find_legacy_recovery_review(&changed)
        .is_err());

    let mut cross_domain = winner;
    cross_domain.review_id = "review:cross-domain".into();
    cross_domain.idempotency_key = "key:legacy-original".into();
    assert!(fixture
        .journal
        .begin_legacy_recovery_review(cross_domain, 10_002)
        .is_err());
}

#[test]
fn legacy_review_interruption_requires_explicit_same_review_verification() {
    let fixture = Fixture::new();
    let request = fixture.request();
    fixture
        .journal
        .begin_legacy_recovery_review(request.clone(), 10_001)
        .unwrap();
    let intent = fixture
        .journal
        .legacy_recovery_review_operation(&request.review_id)
        .unwrap();
    let before = bytes(&fixture.journal);
    let wrong = p::LegacyReleaseRecoveryReviewVerification {
        review_id: "review:replacement".into(),
        ..verification(&request, intent.revision, 30_000)
    };
    assert!(fixture
        .journal
        .commit_legacy_recovery_review(wrong, request.evidence_digest.clone(), 10_002)
        .is_err());
    assert_eq!(bytes(&fixture.journal), before);
    fixture
        .journal
        .commit_legacy_recovery_review(
            verification(&request, intent.revision, 30_000),
            request.evidence_digest.clone(),
            10_002,
        )
        .unwrap();
    let changed_evidence = hash("changed-evidence");
    assert!(fixture
        .journal
        .commit_legacy_recovery_review(
            verification(&request, intent.revision, 30_000),
            changed_evidence,
            10_003,
        )
        .is_err());
    let mut changed_revision = verification(&request, intent.revision, 30_000);
    changed_revision.expected_revision += 1;
    assert!(fixture
        .journal
        .commit_legacy_recovery_review(changed_revision, request.evidence_digest.clone(), 10_003,)
        .is_err());
}

#[test]
fn legacy_review_unknown_fact_is_not_skipped_by_replay() {
    let fixture = Fixture::new();
    let request = fixture.request();
    fixture
        .journal
        .begin_legacy_recovery_review(request, 10_001)
        .unwrap();
    let original = bytes(&fixture.journal);
    let path = fixture.path.clone();
    let Fixture {
        _root: root,
        journal,
        ..
    } = fixture;
    drop(journal);
    let mut value: serde_json::Value = serde_json::from_slice(
        original
            .split(|byte| *byte == b'\n')
            .rfind(|line| !line.is_empty())
            .unwrap(),
    )
    .unwrap();
    value["fact"]["record"] = "unknown_legacy_review_fact".into();
    let mut corrupted = original
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .take(7)
        .flat_map(|line| line.iter().copied().chain(std::iter::once(b'\n')))
        .collect::<Vec<_>>();
    serde_json::to_writer(&mut corrupted, &value).unwrap();
    corrupted.push(b'\n');
    std::fs::write(&path, corrupted).unwrap();
    assert!(ProductJournal::open(&path, INSTALL).is_err());
    drop(root);
}
