//! Real journal/replay/CAS boundaries; never native installation evidence.
use super::*;
use std::sync::Arc;

fn hash(value: &str) -> p::SchemaDigest {
    p::sha256_content_digest(value.as_bytes())
}
const ORIGINAL: &str = "upgrade:interrupted";
const OWNER: p::MaintenanceActorClass = p::MaintenanceActorClass::OwnerReauthenticated;

struct Fixture {
    root: tempfile::TempDir,
    journal: Arc<ProductJournal>,
    takeover: p::ProductReleaseTakeover,
    command: p::MaintenanceCommand,
    references: p::MaintenanceReferences,
}
impl Fixture {
    fn new(baseline: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let journal = Arc::new(
            ProductJournal::open(&root.path().join("journal"), "install:recovery").unwrap(),
        );
        journal
            .record_installation_verification_with_active_release(
                hash("installation"),
                hash("source"),
                Some(("0.1.0", &hash("source"))),
            )
            .unwrap();
        let command = p::MaintenanceCommand {
            schema_version: p::SchemaVersion(1),
            operation_id: ORIGINAL.into(),
            idempotency_key: "key:original".into(),
            expected_revision: journal.revision().unwrap(),
            actor_proof: p::MaintenanceProofRef {
                schema_version: p::SchemaVersion(1),
                proof_ref: "proof:original".into(),
            },
            kind: p::MaintenanceKind::Upgrade {
                release_id: "0.2.0".into(),
                release_digest: hash("target"),
                backup_ref: "backup:original".into(),
            },
            requested_deadline: 10_000,
        };
        let references = p::MaintenanceReferences {
            schema_version: p::SchemaVersion(1),
            original_release_ref: "0.1.0".into(),
            target_release_ref: "0.2.0".into(),
            backup_ref: Some("backup:original".into()),
            data_candidate_ref: None,
        };
        let op = journal
            .plan(command.clone(), OWNER, references.clone(), 1)
            .unwrap();
        journal
            .begin_stage(
                ORIGINAL,
                journal.revision().unwrap(),
                p::MaintenanceStage::Validated,
                OWNER,
                2,
            )
            .unwrap();
        journal
            .finish_stage(
                ORIGINAL,
                journal.revision().unwrap(),
                p::MaintenanceStage::Validated,
                vec![hash("validated")],
                3,
            )
            .unwrap();
        let takeover = p::ProductReleaseTakeover {
            schema_version: p::SchemaVersion(1),
            release: p::ProductReleaseRequest {
                schema_version: p::SchemaVersion(1),
                install_ref: "install:recovery".into(),
                operation_id: ORIGINAL.into(),
                command_digest: op.command_digest,
                change: p::ProductReleaseChange::Upgrade,
                original_release_ref: "0.1.0".into(),
                original_release_digest: hash("source"),
                target_release_ref: "0.2.0".into(),
                target_release_digest: hash("target"),
                backup_ref: "backup:original".into(),
            },
            installation_digest: hash("installation"),
            owner: p::OwnerPrincipalRef("owner:recovery".into()),
            expected_config_revision: 1,
            expected_config_digest: hash("config"),
            deadline: 10_000,
            readiness_nonce: "a".repeat(64),
        };
        journal
            .prepare_release_takeover(journal.revision().unwrap(), takeover.clone(), 4)
            .unwrap();
        if baseline {
            journal
                .prepare_release_recovery_baseline(
                    journal.revision().unwrap(),
                    source_baseline(&takeover),
                )
                .unwrap();
        }
        journal
            .begin_stage(
                ORIGINAL,
                journal.revision().unwrap(),
                p::MaintenanceStage::Quiescing,
                OWNER,
                6,
            )
            .unwrap();
        Self {
            root,
            journal,
            takeover,
            command,
            references,
        }
    }
    fn request(&self) -> p::ProductReleaseRecoveryRequest {
        let candidate = self
            .journal
            .release_recovery_candidate(ORIGINAL, 10_001)
            .unwrap();
        p::ProductReleaseRecoveryRequest {
            schema_version: p::SchemaVersion(1),
            operation_id: "recover:original".into(),
            idempotency_key: "key:recovery".into(),
            expected_revision: candidate.product_revision,
            owner: candidate.owner.clone(),
            decision: p::ProductReleaseRecoveryDecision::AbortBeforeSwitch,
            candidate,
            requested_deadline: 30_000,
        }
    }
    fn bytes(&self) -> Vec<u8> {
        let mut writer = self.journal.writer.lock().unwrap();
        writer.file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        writer.file.read_to_end(&mut bytes).unwrap();
        bytes
    }
}
fn source_baseline(t: &p::ProductReleaseTakeover) -> p::ProductReleaseRecoveryBaseline {
    p::ProductReleaseRecoveryBaseline {
        schema_version: p::SchemaVersion(1),
        operation_id: ORIGINAL.into(),
        command_digest: t.release.command_digest.clone(),
        takeover_digest: t.digest().unwrap(),
        source_images_digest: hash("source-images"),
        service_configuration_digest: hash("services"),
        config_files_digest: hash("configuration"),
        worker_state_digest: hash("workers"),
        captured_at: 5,
        data_identity: p::ProductReleaseDataIdentity {
            schema_version: p::SchemaVersion(1),
            config_schema_version: 1,
            database_schema_version: 1,
            owner_digest: p::sha256_content_digest(t.owner.0.as_bytes()),
            store_identity: hash("store"),
            schema_digest: hash("schema"),
            database_digest: hash("database"),
            event_count: 3,
            last_stream_seq: 3,
        },
    }
}
fn commit(
    j: &ProductJournal,
    r: &p::ProductReleaseRecoveryRequest,
    at: p::Timestamp,
) -> p::Result<()> {
    j.commit_release_recovery(
        &r.operation_id,
        j.revision()?,
        r.digest()?,
        r.candidate.evidence_digest.clone(),
        40_000,
        at,
    )
}

#[test]
fn release_recovery_preserves_failure_history_and_atomically_aborts_without_changing_active_release(
) {
    let f = Fixture::new(true);
    f.journal
        .require_recovery(
            ORIGINAL,
            f.journal.revision().unwrap(),
            hash("original-failure"),
            7,
        )
        .unwrap();
    let before = f.bytes();
    let request = f.request();
    f.journal
        .begin_release_recovery(request.clone(), 10_001)
        .unwrap();
    assert!(f.journal.release_is_unresolved().unwrap());
    assert_eq!(
        f.journal
            .release_recovery_operation(&request.operation_id)
            .unwrap()
            .stage,
        p::ProductReleaseRecoveryStage::Intent
    );
    assert_eq!(
        f.journal.operation(ORIGINAL).unwrap().stage,
        p::MaintenanceStage::RecoveryRequired
    );
    commit(&f.journal, &request, 10_002).unwrap();
    let decision = f
        .journal
        .release_recovery_operation(&request.operation_id)
        .unwrap();
    let original = f.journal.operation(ORIGINAL).unwrap();
    assert_eq!(original.stage, p::MaintenanceStage::Aborted);
    assert_eq!(original.pending_stage, None);
    assert_eq!(original.failure_ref, Some(hash("original-failure").0));
    assert_eq!(decision.stage, p::ProductReleaseRecoveryStage::Committed);
    assert!(decision.receipt_digest.is_some());
    assert_eq!(original.revision, decision.revision);
    assert_eq!(f.journal.active_release().unwrap().unwrap().0, "0.1.0");
    assert!(!f.journal.release_is_unresolved().unwrap());
    assert!(f.bytes().starts_with(&before));
    assert_eq!(
        f.journal
            .plan(f.command.clone(), OWNER, f.references.clone(), 50_000)
            .unwrap()
            .stage,
        p::MaintenanceStage::Aborted
    );
    assert!(f
        .journal
        .begin_stage(
            ORIGINAL,
            decision.revision,
            p::MaintenanceStage::Quiescing,
            OWNER,
            50_001
        )
        .is_err());
    assert!(f
        .journal
        .require_recovery(ORIGINAL, decision.revision, hash("late"), 50_001)
        .is_err());
    assert!(commit(&f.journal, &request, 10_003).is_err());
    assert_eq!(
        f.journal.find_release_recovery(&request).unwrap(),
        Some(decision)
    );
}

#[test]
fn release_recovery_intent_survives_restart_queries_are_pure_and_only_explicit_verification_commits(
) {
    let f = Fixture::new(true);
    let request = f.request();
    f.journal
        .begin_release_recovery(request.clone(), 10_001)
        .unwrap();
    let before = f.bytes();
    let Fixture { root, journal, .. } = f;
    drop(journal);
    let reopened = ProductJournal::open(&root.path().join("journal"), "install:recovery").unwrap();
    assert!(reopened.release_is_unresolved().unwrap());
    assert_eq!(
        reopened
            .find_release_recovery(&request)
            .unwrap()
            .unwrap()
            .stage,
        p::ProductReleaseRecoveryStage::Intent
    );
    let list = reopened.release_recoveries(&request.owner).unwrap();
    assert_eq!(list.operations.len(), 1);
    assert_eq!(list.operations[0].request, request);
    let mut file = reopened.writer.lock().unwrap();
    file.file.seek(SeekFrom::Start(0)).unwrap();
    let mut after = Vec::new();
    file.file.read_to_end(&mut after).unwrap();
    drop(file);
    assert_eq!(before, after);
    // Old authorization is no longer valid. A separately authenticated new
    // verification supplies its own window without rewriting the old request.
    assert!(reopened
        .commit_release_recovery(
            &request.operation_id,
            list.product_revision,
            request.digest().unwrap(),
            request.candidate.evidence_digest.clone(),
            request.requested_deadline,
            31_000
        )
        .is_err());
    commit(&reopened, &request, 31_000).unwrap();
    assert_eq!(
        reopened
            .release_recovery_operation(&request.operation_id)
            .unwrap()
            .request,
        request
    );
}

#[test]
fn release_recovery_cas_race_and_cross_domain_keys_allow_exactly_one_decision() {
    let f = Fixture::new(true);
    let request = f.request();
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let journal = f.journal.clone();
            let request = request.clone();
            std::thread::spawn(move || journal.begin_release_recovery(request, 10_001))
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(Result::is_ok)
            .count(),
        1
    );
    let intent = f
        .journal
        .release_recovery_operation(&request.operation_id)
        .unwrap();
    assert_eq!(
        f.journal.find_release_recovery(&request).unwrap(),
        Some(intent.clone())
    );
    let mut changed = request.clone();
    changed.requested_deadline += 1;
    assert!(f.journal.find_release_recovery(&changed).is_err());
    changed = request.clone();
    changed.idempotency_key = f.command.idempotency_key.clone();
    assert!(f.journal.find_release_recovery(&changed).is_err());
    changed = request.clone();
    changed.operation_id = ORIGINAL.into();
    assert!(f.journal.find_release_recovery(&changed).is_err());
    changed = request.clone();
    changed.operation_id = "recover:replacement".into();
    changed.idempotency_key = "key:replacement".into();
    changed.expected_revision = intent.revision;
    changed.candidate.product_revision = intent.revision;
    assert!(f.journal.begin_release_recovery(changed, 10_002).is_err());
    assert_eq!(f.journal.revision().unwrap(), intent.revision);
}

#[test]
fn release_recovery_rejects_missing_baseline_early_late_and_mismatched_evidence_without_appending()
{
    let legacy = Fixture::new(false);
    assert!(legacy
        .journal
        .release_recovery_candidate(ORIGINAL, 20_000)
        .is_err());
    let f = Fixture::new(true);
    assert!(f
        .journal
        .release_recovery_candidate(ORIGINAL, 9_999)
        .is_err());
    let request = f.request();
    let before = f.bytes();
    for mode in 0..7 {
        let mut r = request.clone();
        match mode {
            0 => r.owner = p::OwnerPrincipalRef("owner:foreign".into()),
            1 => r.candidate.config_digest = hash("drift"),
            2 => r.candidate.original_command_digest = hash("foreign-command"),
            3 => r.candidate.evidence_digest = hash("forged-evidence"),
            4 => r.requested_deadline = 10_001,
            5 => r.expected_revision += 1,
            _ => r.candidate.original_release_ref = "foreign".into(),
        }
        assert!(f.journal.begin_release_recovery(r, 10_001).is_err());
    }
    assert_eq!(before, f.bytes());
    f.journal
        .finish_stage(
            ORIGINAL,
            f.journal.revision().unwrap(),
            p::MaintenanceStage::Quiescing,
            vec![hash("quiesced")],
            8,
        )
        .unwrap();
    assert!(f
        .journal
        .release_recovery_candidate(ORIGINAL, 20_000)
        .is_err());
    f.journal
        .begin_stage(
            ORIGINAL,
            f.journal.revision().unwrap(),
            p::MaintenanceStage::BackedUp,
            OWNER,
            9,
        )
        .unwrap();
    assert!(f.journal.begin_release_recovery(request, 20_000).is_err());
}

#[test]
fn release_recovery_schema_floor_preserves_v1_hashes_and_rejects_a_validly_rehashed_downgrade() {
    let f = Fixture::new(true);
    let bytes = f.bytes();
    let mut envelopes: Vec<JournalEnvelope> = bytes
        .split(|b| *b == b'\n')
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_slice(s).unwrap())
        .collect();
    assert_eq!(envelopes[0].schema_version, p::SchemaVersion(1));
    let v2 = envelopes
        .iter()
        .position(|e| e.schema_version == p::SchemaVersion(2))
        .unwrap();
    assert!(v2 > 0);
    assert!(envelopes[v2..]
        .iter()
        .all(|e| e.schema_version == p::SchemaVersion(2)));
    let last = envelopes.last_mut().unwrap();
    last.schema_version = p::SchemaVersion(1);
    last.digest = JournalEnvelope::calculate(
        last.schema_version,
        last.product_seq,
        &last.previous,
        &last.fact,
    )
    .unwrap();
    let mut projection = Projection::default();
    for envelope in &envelopes[..envelopes.len() - 1] {
        projection.fold(envelope).unwrap();
    }
    assert!(projection.fold(envelopes.last().unwrap()).is_err());
    let original = f.journal.operation(ORIGINAL).unwrap();
    let Fixture { root, journal, .. } = f;
    drop(journal);
    let replay = ProductJournal::open(&root.path().join("journal"), "install:recovery").unwrap();
    assert_eq!(replay.operation(ORIGINAL).unwrap(), original);
}

#[test]
fn release_recovery_protocol_is_closed_and_binds_owner_revision_and_receipt() {
    let f = Fixture::new(true);
    let r = f.request();
    r.validate().unwrap();
    let mut wire = serde_json::to_value(&r).unwrap();
    wire["program"] = "untrusted-shell".into();
    assert!(serde_json::from_value::<p::ProductReleaseRecoveryRequest>(wire).is_err());
    let mut r2 = r.clone();
    r2.candidate.owner = p::OwnerPrincipalRef("owner:other".into());
    assert!(r2.validate().is_err());
    r2 = r.clone();
    r2.schema_version = p::SchemaVersion(2);
    assert!(r2.validate().is_err());
    let verification = p::ProductReleaseRecoveryVerification {
        schema_version: p::SchemaVersion(1),
        operation_id: r.operation_id.clone(),
        request_digest: r.digest().unwrap(),
        expected_revision: r.expected_revision + 1,
        owner: r.owner.clone(),
        requested_deadline: r.requested_deadline,
    };
    assert_ne!(verification.digest().unwrap(), r.digest().unwrap());
    source_baseline(&f.takeover)
        .validate_for(&f.takeover)
        .unwrap();
    assert!(!p::MaintenanceStage::Validated.permits(p::MaintenanceStage::Aborted, true));
    assert!(!p::MaintenanceStage::Aborted.permits(p::MaintenanceStage::Committed, true));
}
