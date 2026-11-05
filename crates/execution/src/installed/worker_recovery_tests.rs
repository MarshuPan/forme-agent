//! Original temporary journals; corruption is injected through their locked handle.
use super::*;

struct Fixture {
    journal: WorkerJournal,
    _temp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let journal = WorkerJournal::open(&temp.path().join("worker.jsonl")).unwrap();
        Self {
            journal,
            _temp: temp,
        }
    }

    fn cancelled(&self, suffix: &str) {
        let (job, permit) = job(suffix);
        self.journal.admit(job.clone(), permit, 2).unwrap();
        self.journal
            .record_cancel_receipt(
                p::WorkerCancelReceipt {
                    schema_version: p::SchemaVersion(1),
                    job_id: job.job_id.clone(),
                    plan_digest: job.plan_digest.clone(),
                    nonce: job.nonce.clone(),
                    outcome: p::WorkerOutcome::Cancelled,
                    observed_at: 3,
                    receipt_ref: Some(format!("receipt:{suffix}")),
                    reason_ref: None,
                },
                3,
            )
            .unwrap();
    }

    fn bytes(&self) -> Vec<u8> {
        let mut writer = self.journal.writer.lock().unwrap();
        let position = writer.file.stream_position().unwrap();
        writer.file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        writer.file.read_to_end(&mut bytes).unwrap();
        writer.file.seek(SeekFrom::Start(position)).unwrap();
        bytes
    }

    fn replace(&self, bytes: &[u8]) {
        let mut writer = self.journal.writer.lock().unwrap();
        writer.file.seek(SeekFrom::Start(0)).unwrap();
        writer.file.write_all(bytes).unwrap();
        writer.file.set_len(bytes.len() as u64).unwrap();
        writer.file.sync_all().unwrap();
    }

    fn assert_poisoned_without_repair(&self, expected: &[u8]) {
        assert!(self.journal.quiescent_digest().is_err());
        assert!(self.journal.writer.lock().unwrap().poisoned);
        let (job, permit) = job("after-corruption");
        assert!(self.journal.admit(job, permit, 4).is_err());
        assert_eq!(self.bytes(), expected);
    }
}

fn job(suffix: &str) -> (p::WorkerJob, p::WorkerPermit) {
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: format!("job:{suffix}"),
        owner_ref: p::VerifiedPrincipal("owner:original-worker-evidence".into()),
        action_ref: p::ActionId(format!("action:{suffix}")),
        affected_resources: vec![p::ResourceRef("workspace:evidence".into())],
        execution_plan_ref: format!("plan:{suffix}"),
        plan_digest: p::PlanDigest(p::canonical_digest(&suffix).unwrap().0),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: format!("permit:{suffix}"),
        sandbox_profile_ref: "sandbox:evidence".into(),
        workspace_scope: p::Scope("workspace:evidence".into()),
        deadline: 1000,
        nonce: p::Nonce(format!("nonce:{suffix}")),
        cancel_ref: format!("cancel:{suffix}"),
    };
    let permit = p::WorkerPermit {
        schema_version: p::SchemaVersion(1),
        permit_ref: job.permit_ref.clone(),
        job_id: job.job_id.clone(),
        owner_ref: job.owner_ref.clone(),
        action_ref: job.action_ref.clone(),
        affected_resources: job.affected_resources.clone(),
        plan_digest: job.plan_digest.clone(),
        authority_epoch: job.authority_epoch,
        workspace_scope: job.workspace_scope.clone(),
        sandbox_profile_ref: job.sandbox_profile_ref.clone(),
        deadline: job.deadline,
        nonce: job.nonce.clone(),
        issued_at: 1,
    };
    (job, permit)
}

#[test]
fn receipt_confirmation_rechecks_original_bytes_and_refuses_a_missing_durable_receipt() {
    let f = Fixture::new();
    let (job, permit) = job("receipt-origin");
    f.journal.admit(job.clone(), permit, 2).unwrap();
    f.journal.start(&job.job_id, 3).unwrap();
    let prefix = f.bytes();
    let receipt = p::WorkerReceipt {
        schema_version: p::SchemaVersion(1),
        receipt_ref: "receipt:original".into(),
        job_id: job.job_id.clone(),
        plan_digest: job.plan_digest.clone(),
        authority_epoch: job.authority_epoch,
        nonce: job.nonce.clone(),
        observed_outcome: p::WorkerOutcome::Completed,
        safe_result_ref: None,
        ground_truth_refs: Vec::new(),
        started_at: 3,
        finished_at: Some(4),
    };
    f.journal.record_receipt(receipt.clone(), 4).unwrap();
    let original = f.bytes();
    assert_eq!(
        f.journal.confirm_receipt(&receipt, 5).unwrap().state,
        p::WorkerState::Completed
    );
    assert_eq!(f.bytes(), original);
    // A still-valid old prefix has no persisted receipt, even though memory does.
    f.replace(&prefix);
    assert!(f.journal.confirm_receipt(&receipt, 6).is_err());
    assert!(f.journal.writer.lock().unwrap().poisoned);
    assert!(f.journal.confirm_receipt(&receipt, 7).is_err());
    f.assert_poisoned_without_repair(&prefix);
}

#[test]
fn quiescent_digest_rejects_valid_truncation_against_live_writer() {
    let f = Fixture::new();
    f.cancelled("first");
    let prefix = f.bytes();
    f.cancelled("second");
    f.replace(&prefix);
    f.assert_poisoned_without_repair(&prefix);
}

#[test]
fn original_worker_recheck_preserves_bytes_cursor_digest_and_os_lock() {
    let f = Fixture::new();
    assert_eq!(
        f.journal.quiescent_digest().unwrap(),
        p::canonical_digest(&(
            "forme-worker-quiescent-v1",
            0_u64,
            p::canonical_digest(&"forme-worker-journal-v1").unwrap(),
        ))
        .unwrap()
    );
    assert!(f.bytes().is_empty());
    f.cancelled("unchanged");
    let original = f.bytes();
    let expected = {
        let mut writer = f.journal.writer.lock().unwrap();
        writer.file.seek(SeekFrom::Start(17)).unwrap();
        p::canonical_digest(&(
            "forme-worker-quiescent-v1",
            writer.projection.sequence,
            writer.projection.head().unwrap(),
        ))
        .unwrap()
    };
    assert_eq!(f.journal.quiescent_digest().unwrap(), expected);
    assert_eq!(f.journal.quiescent_digest().unwrap(), expected);
    assert_eq!(f.bytes(), original);
    assert_eq!(
        f.journal
            .writer
            .lock()
            .unwrap()
            .file
            .stream_position()
            .unwrap(),
        17
    );
    assert!(WorkerJournal::open(&f._temp.path().join("worker.jsonl")).is_err());
    f.cancelled("subsequent-admission");
    assert_ne!(f.journal.quiescent_digest().unwrap(), expected);
    let final_bytes = f.bytes();
    drop(f.journal);
    let reopened = WorkerJournal::open(&f._temp.path().join("worker.jsonl")).unwrap();
    assert!(reopened.quiescent_digest().is_ok());
    drop(reopened);
    assert_eq!(
        std::fs::read(f._temp.path().join("worker.jsonl")).unwrap(),
        final_bytes
    );
}

#[test]
fn original_worker_recheck_rejects_same_sequence_with_another_valid_chain() {
    let f = Fixture::new();
    f.cancelled("original");
    let other = Fixture::new();
    other.cancelled("another");
    let bytes = other.bytes();
    f.replace(&bytes);
    f.assert_poisoned_without_repair(&bytes);
}

#[test]
fn original_worker_recheck_rejects_an_untracked_valid_terminal_suffix() {
    let f = Fixture::new();
    f.cancelled("original");
    let original = f.bytes();
    let other = Fixture::new();
    other.cancelled("original");
    assert_eq!(other.bytes(), original);
    other.cancelled("untracked");
    let bytes = other.bytes();
    f.replace(&bytes);
    f.assert_poisoned_without_repair(&bytes);
}

#[test]
fn original_worker_recheck_rejects_truncation_to_empty_after_real_history() {
    let f = Fixture::new();
    f.cancelled("original");
    f.replace(&[]);
    f.assert_poisoned_without_repair(&[]);
}

#[test]
fn original_worker_recheck_rejects_corruption_closed_schema_and_torn_records() {
    for mode in 0..7 {
        let f = Fixture::new();
        f.cancelled("original");
        let original = f.bytes();
        let mut bytes = original.clone();
        match mode {
            0 => {
                let offset = bytes.len() - 3;
                bytes[offset] ^= 1;
            }
            1 => {
                bytes.pop();
            }
            2 => bytes.push(b'\n'),
            _ => {
                let first_end = bytes.iter().position(|byte| *byte == b'\n').unwrap();
                let mut first: serde_json::Value =
                    serde_json::from_slice(&bytes[..first_end]).unwrap();
                match mode {
                    3 => first["schema_version"] = serde_json::json!(2),
                    4 => first["unexpected"] = serde_json::json!(true),
                    5 => first["worker_seq"] = serde_json::json!(2),
                    _ => first["fact"]["at"] = serde_json::json!(9),
                }
                bytes = serde_json::to_vec(&first).unwrap();
                bytes.extend_from_slice(&original[first_end..]);
            }
        }
        f.replace(&bytes);
        let position = f
            .journal
            .writer
            .lock()
            .unwrap()
            .file
            .stream_position()
            .unwrap();
        f.assert_poisoned_without_repair(&bytes);
        assert_eq!(
            f.journal
                .writer
                .lock()
                .unwrap()
                .file
                .stream_position()
                .unwrap(),
            position,
            "mode {mode}"
        );
    }
}

#[test]
fn original_worker_recheck_enforces_record_and_total_bounds_without_repair() {
    let f = Fixture::new();
    let bytes = vec![b' '; MAX_RECORD_BYTES + 1];
    f.replace(&bytes);
    f.assert_poisoned_without_repair(&bytes);

    let f = Fixture::new();
    {
        let writer = f.journal.writer.lock().unwrap();
        writer.file.set_len(MAX_JOURNAL_BYTES + 1).unwrap();
        writer.file.sync_all().unwrap();
    }
    assert!(f.journal.quiescent_digest().is_err());
    let mut writer = f.journal.writer.lock().unwrap();
    assert!(writer.poisoned);
    assert_eq!(writer.file.metadata().unwrap().len(), MAX_JOURNAL_BYTES + 1);
    assert_eq!(writer.file.stream_position().unwrap(), 0);
}

#[test]
fn original_worker_recheck_does_not_poison_valid_unsettled_outcomes() {
    for mode in 0..4 {
        let f = Fixture::new();
        let (pending, permit) = job("unsettled");
        f.journal.admit(pending.clone(), permit, 2).unwrap();
        match mode {
            1 => {
                f.journal.start(&pending.job_id, 3).unwrap();
            }
            2 => {
                f.journal.request_cancel(&pending.job_id, 3).unwrap();
            }
            3 => {
                f.journal
                    .mark_unknown(&pending.job_id, "reason:unknown".into(), 3)
                    .unwrap();
            }
            _ => {}
        }
        let original = f.bytes();
        assert!(f.journal.quiescent_digest().is_err(), "mode {mode}");
        assert!(!f.journal.writer.lock().unwrap().poisoned, "mode {mode}");
        assert_eq!(f.bytes(), original);
        let (next, permit) = job("following-valid-admission");
        assert!(f.journal.admit(next, permit, 4).is_ok());
    }
}

#[test]
fn original_worker_recheck_accepts_only_known_terminal_receipts() {
    for outcome in [
        p::WorkerOutcome::Completed,
        p::WorkerOutcome::Failed,
        p::WorkerOutcome::Cancelled,
        p::WorkerOutcome::Unknown,
    ] {
        let f = Fixture::new();
        let (job, permit) = job("receipt");
        f.journal.admit(job.clone(), permit, 2).unwrap();
        f.journal.start(&job.job_id, 3).unwrap();
        f.journal
            .record_receipt(
                p::WorkerReceipt {
                    schema_version: p::SchemaVersion(1),
                    receipt_ref: "receipt:terminal".into(),
                    job_id: job.job_id,
                    plan_digest: job.plan_digest,
                    authority_epoch: job.authority_epoch,
                    nonce: job.nonce,
                    observed_outcome: outcome,
                    safe_result_ref: None,
                    ground_truth_refs: Vec::new(),
                    started_at: 3,
                    finished_at: Some(4),
                },
                4,
            )
            .unwrap();
        assert_eq!(
            f.journal.quiescent_digest().is_ok(),
            outcome != p::WorkerOutcome::Unknown,
        );
        assert!(!f.journal.writer.lock().unwrap().poisoned);
    }
}

#[test]
fn original_worker_recheck_failure_cannot_be_cleared_by_replacing_old_bytes() {
    let f = Fixture::new();
    f.cancelled("original");
    let original = f.bytes();
    f.replace(&[]);
    assert!(f.journal.quiescent_digest().is_err());
    f.replace(&original);
    f.assert_poisoned_without_repair(&original);
}
