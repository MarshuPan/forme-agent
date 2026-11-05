use super::*;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;

const INSTALL: &str = "install:legacy-review";
const ORIGINAL: &str = "upgrade:legacy";
const OWNER: &str = "owner:legacy";

#[path = "source_tests.rs"]
mod source_tests;

fn hash(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}

fn journal_bytes(journal: &ProductJournal) -> Vec<u8> {
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
    current: (u64, p::SchemaDigest),
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
                references(),
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
                command_digest: operation.command_digest,
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
            .prepare_release_takeover(journal.revision().unwrap(), takeover, 4)
            .unwrap();
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
            current: (current.0, current.1),
        }
    }

    fn review(&self) -> p::SignedLegacyReleaseReview {
        let binding = p::LegacyReleaseReviewBinding {
            schema_version: p::SchemaVersion(1),
            review_id: "review:legacy".into(),
            install_ref: INSTALL.into(),
            owner: p::OwnerPrincipalRef(OWNER.into()),
            original_operation_id: ORIGINAL.into(),
            original_command_digest: self
                .journal
                .command(ORIGINAL)
                .unwrap()
                .bound_digest(&references())
                .unwrap(),
            original_release_ref: "0.1.0".into(),
            original_release_digest: hash("source-release"),
            config_revision: 1,
            config_digest: hash("config"),
            journal_seq: self.current.0,
            journal_head: self.current.1.clone(),
            expires_at: 20_000,
        };
        let binding_digest = binding.digest().unwrap();
        let evidence = p::LegacyReleaseEvidenceKind::ALL
            .into_iter()
            .map(|kind| p::LegacyReleaseEvidence {
                schema_version: p::SchemaVersion(1),
                kind,
                artifact_digest: hash("artifact"),
                binding_digest: binding_digest.clone(),
                observation_digest: hash("observation"),
            })
            .collect();
        let review = p::LegacyReleaseReview {
            schema_version: p::SchemaVersion(1),
            binding,
            evidence,
            status: p::LegacyReleaseReviewStatus::Verified,
            issues: Vec::new(),
        };
        p::SignedLegacyReleaseReview {
            schema_version: p::SchemaVersion(1),
            review,
            signature: vec![7; 64],
        }
    }

    fn request(&self, review: &p::SignedLegacyReleaseReview) -> p::LegacyReleaseRecoveryRequest {
        p::LegacyReleaseRecoveryRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: "recover:legacy".into(),
            idempotency_key: "key:legacy-recovery".into(),
            expected_revision: self.current.0,
            owner: p::OwnerPrincipalRef(OWNER.into()),
            review_id: review.review.binding.review_id.clone(),
            review_digest: review.review_digest().unwrap(),
            requested_deadline: 19_000,
        }
    }
}

fn references() -> p::MaintenanceReferences {
    p::MaintenanceReferences {
        schema_version: p::SchemaVersion(1),
        original_release_ref: "0.1.0".into(),
        target_release_ref: "0.2.0".into(),
        data_candidate_ref: None,
        backup_ref: Some("backup:legacy".into()),
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
    request: &p::LegacyReleaseRecoveryRequest,
    revision: u64,
    deadline: p::Timestamp,
) -> p::LegacyReleaseRecoveryVerification {
    p::LegacyReleaseRecoveryVerification {
        schema_version: p::SchemaVersion(1),
        operation_id: request.operation_id.clone(),
        request_digest: request.digest().unwrap(),
        expected_revision: revision,
        owner: request.owner.clone(),
        requested_deadline: deadline,
    }
}

#[test]
fn verified_legacy_recovery_is_atomic_and_replays_without_touching_active_release() {
    let fixture = Fixture::new();
    let review = fixture.review();
    let request = fixture.request(&review);
    let before = journal_bytes(&fixture.journal);
    let active = fixture.journal.active_release().unwrap();
    fixture
        .journal
        .begin_legacy_recovery(request.clone(), review, 10_001)
        .unwrap();
    assert!(journal_bytes(&fixture.journal).starts_with(&before));
    let intent = fixture
        .journal
        .legacy_recovery_operation(&request.operation_id)
        .unwrap();
    assert_eq!(intent.stage, p::LegacyReleaseRecoveryStage::Intent);
    assert_eq!(
        fixture.journal.operation(ORIGINAL).unwrap().pending_stage,
        Some(p::MaintenanceStage::Quiescing)
    );

    fixture
        .journal
        .commit_legacy_recovery(
            verification(&request, intent.revision, 19_500),
            hash("final-observation"),
            10_002,
        )
        .unwrap();
    let committed = fixture
        .journal
        .legacy_recovery_operation(&request.operation_id)
        .unwrap();
    assert_eq!(committed.stage, p::LegacyReleaseRecoveryStage::Committed);
    assert!(committed.receipt_digest.is_some());
    assert_eq!(
        fixture.journal.operation(ORIGINAL).unwrap().stage,
        p::MaintenanceStage::Aborted
    );
    assert_eq!(fixture.journal.active_release().unwrap(), active);

    let committed_bytes = journal_bytes(&fixture.journal);
    drop(fixture.journal);
    let replayed = ProductJournal::open(&fixture.path, INSTALL).unwrap();
    assert_eq!(journal_bytes(&replayed), committed_bytes);
    assert_eq!(
        replayed
            .legacy_recovery_operation(&request.operation_id)
            .unwrap(),
        committed
    );
    assert_eq!(
        replayed
            .legacy_recoveries(&request.owner)
            .unwrap()
            .operations,
        vec![committed]
    );
}

#[test]
fn non_verified_or_drifted_reviews_are_rejected_without_append() {
    let fixture = Fixture::new();
    let review = fixture.review();
    let request = fixture.request(&review);
    let before = journal_bytes(&fixture.journal);
    for status in [
        p::LegacyReleaseReviewStatus::Rejected,
        p::LegacyReleaseReviewStatus::Unverifiable,
    ] {
        let mut changed = review.clone();
        changed.review.status = status;
        changed.review.issues = vec!["issue:missing".into()];
        assert!(fixture
            .journal
            .begin_legacy_recovery(request.clone(), changed, 10_001)
            .is_err());
    }
    let mut changed_request = request.clone();
    changed_request.review_digest = hash("other-review");
    assert!(fixture
        .journal
        .begin_legacy_recovery(changed_request, review.clone(), 10_001)
        .is_err());
    let mut changed_review = review.clone();
    changed_review.review.binding.config_digest = hash("drifted-config");
    assert!(fixture
        .journal
        .begin_legacy_recovery(request, changed_review, 10_001)
        .is_err());
    assert_eq!(journal_bytes(&fixture.journal), before);
}

#[test]
fn recovery_cas_and_cross_domain_collisions_allow_only_one_intent() {
    let fixture = Fixture::new();
    let review = fixture.review();
    let request = fixture.request(&review);
    let other = {
        let mut value = request.clone();
        value.operation_id = "recover:other".into();
        value.idempotency_key = "key:other".into();
        value
    };
    let handles = [request.clone(), other.clone()]
        .into_iter()
        .map(|request| {
            let journal = fixture.journal.clone();
            let review = review.clone();
            std::thread::spawn(move || journal.begin_legacy_recovery(request, review, 10_001))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|result| result.is_ok())
            .count(),
        1
    );
    let winner = fixture
        .journal
        .legacy_recoveries(&request.owner)
        .unwrap()
        .operations[0]
        .clone();
    assert_eq!(
        fixture
            .journal
            .find_legacy_recovery(&winner.request)
            .unwrap(),
        Some(winner.clone())
    );
    let saved = journal_bytes(&fixture.journal);
    assert!(fixture
        .journal
        .begin_legacy_recovery(winner.request.clone(), review.clone(), 10_002)
        .is_ok());
    assert_eq!(journal_bytes(&fixture.journal), saved);

    let mut collision = request.clone();
    collision.operation_id = "recover:cross".into();
    collision.idempotency_key = "key:legacy-original".into();
    assert!(fixture
        .journal
        .begin_legacy_recovery(collision, review, 10_002)
        .is_err());
}

#[test]
fn interrupted_recovery_requires_same_request_and_explicit_verification() {
    let fixture = Fixture::new();
    let review = fixture.review();
    let request = fixture.request(&review);
    fixture
        .journal
        .begin_legacy_recovery(request.clone(), review, 10_001)
        .unwrap();
    let intent = fixture
        .journal
        .legacy_recovery_operation(&request.operation_id)
        .unwrap();
    let before = journal_bytes(&fixture.journal);
    let mut wrong = verification(&request, intent.revision, 19_500);
    wrong.operation_id = "recover:replacement".into();
    assert!(fixture
        .journal
        .commit_legacy_recovery(wrong, hash("final"), 10_002)
        .is_err());
    assert_eq!(journal_bytes(&fixture.journal), before);
    fixture
        .journal
        .commit_legacy_recovery(
            verification(&request, intent.revision, 19_500),
            hash("final"),
            10_002,
        )
        .unwrap();
}

#[test]
fn schema_three_is_not_read_by_a_v2_reader_and_unknown_fact_fails_closed() {
    let fixture = Fixture::new();
    let review = fixture.review();
    let request = fixture.request(&review);
    fixture
        .journal
        .begin_legacy_recovery(request, review, 10_001)
        .unwrap();
    let bytes = journal_bytes(&fixture.journal);
    let path = fixture.path.clone();
    drop(fixture.journal);
    assert!(ProductJournal::open_with_maximum_schema(&path, INSTALL, 2).is_err());

    let mut records: Vec<serde_json::Value> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    records.last_mut().unwrap()["fact"]["record"] = "unrecognized_legacy_fact".into();
    let mut corrupted = Vec::new();
    for record in records {
        serde_json::to_writer(&mut corrupted, &record).unwrap();
        corrupted.push(b'\n');
    }
    std::fs::write(&path, corrupted).unwrap();
    assert!(ProductJournal::open(&path, INSTALL).is_err());
}

#[test]
fn legacy_authorization_waits_for_expired_takeover_and_cannot_outlive_review() {
    let fixture = Fixture::new();
    let review = fixture.review();
    let request = fixture.request(&review);
    let before = journal_bytes(&fixture.journal);
    assert!(fixture
        .journal
        .begin_legacy_recovery(request.clone(), review.clone(), 9_999)
        .is_err());
    assert_eq!(journal_bytes(&fixture.journal), before);
    fixture
        .journal
        .begin_legacy_recovery(request.clone(), review, 10_001)
        .unwrap();
    let intent = fixture
        .journal
        .legacy_recovery_operation(&request.operation_id)
        .unwrap();
    let before = journal_bytes(&fixture.journal);
    for (at, deadline) in [
        (10_002, 20_001),
        (20_000, 20_001),
        (10_000, 19_500),
        (10_002, 130_003),
    ] {
        assert!(
            fixture
                .journal
                .commit_legacy_recovery(
                    verification(&request, intent.revision, deadline),
                    hash("final"),
                    at,
                )
                .is_err(),
            "at={at}, deadline={deadline}"
        );
        assert_eq!(journal_bytes(&fixture.journal), before);
    }
    // A fresh verification window may outlive the request, within the report.
    fixture
        .journal
        .commit_legacy_recovery(
            verification(&request, intent.revision, 20_000),
            hash("final"),
            19_001,
        )
        .unwrap();
    assert_eq!(
        fixture
            .journal
            .legacy_recovery_operation(&request.operation_id)
            .unwrap()
            .stage,
        p::LegacyReleaseRecoveryStage::Committed
    );
}

#[test]
fn legacy_source_binding_is_checked_even_when_changed_report_digests_are_consistent() {
    for mode in 0..7 {
        let fixture = Fixture::new();
        let mut review = fixture.review();
        match mode {
            0 => review.review.binding.owner = p::OwnerPrincipalRef("owner:foreign".into()),
            1 => review.review.binding.install_ref = "install:foreign".into(),
            2 => review.review.binding.original_command_digest = hash("foreign-command"),
            3 => review.review.binding.config_digest = hash("drifted-config"),
            4 => review.review.binding.original_release_digest = hash("foreign-release"),
            5 => review.review.binding.journal_head = hash("foreign-head"),
            _ => review.review.binding.journal_seq += 1,
        }
        let binding = review.review.binding.digest().unwrap();
        for item in &mut review.review.evidence {
            item.binding_digest = binding.clone();
        }
        let mut request = fixture.request(&review);
        request.owner = review.review.binding.owner.clone();
        let before = journal_bytes(&fixture.journal);
        assert!(
            fixture
                .journal
                .begin_legacy_recovery(request, review, 10_001)
                .is_err(),
            "mode {mode}"
        );
        assert_eq!(journal_bytes(&fixture.journal), before);
    }
}
