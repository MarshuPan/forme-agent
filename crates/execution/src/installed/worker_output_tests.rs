use super::*;
use crate::installed::desktop_helper::worker_receipt;
use crate::installed::desktop_output_fixture as fixture;
use crate::installed::worker_results::WorkerResultStore;

struct Fixture {
    root: tempfile::TempDir,
    journal: WorkerJournal,
    job: p::WorkerJob,
    permit: p::WorkerPermit,
    execution: p::WorkerExecutionSpec,
    descriptor: p::DesktopOutputDescriptor,
    manifest: p::WorkerResultManifest,
    receipt: p::WorkerReceipt,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let journal = WorkerJournal::open(&root.path().join("worker.jsonl")).unwrap();
        let (job, permit, request, intent, transfer) = fixture::task();
        let execution = p::WorkerExecutionSpec {
            schema_version: p::SchemaVersion(1),
            backend: p::BackendKind::Computer,
            scope: intent.scope.clone(),
            plan_digest: job.plan_digest.clone(),
            affected_resources: job.affected_resources.clone(),
            normalized_action_digest: request.normalized_action_digest.clone(),
            intent,
            timeout: p::DurationMs(60_000),
            output_budget_bytes: 4096,
            desktop_session: Some(request.binding.clone()),
        };
        let now = permit.issued_at;
        journal
            .admit_with_desktop_output(
                job.clone(),
                permit.clone(),
                &execution,
                transfer.clone(),
                now,
            )
            .unwrap();
        let observation = p::DesktopSessionObservation {
            schema_version: p::SchemaVersion(1),
            helper_ref: request.binding.helper_ref.clone(),
            grant_ref: Some(request.binding.grant_ref.clone()),
            state: p::DesktopSessionState::Ready,
            liveness: p::DesktopSessionLiveness::Live,
            os_user_ref: Some(request.binding.os_user_ref.clone()),
            os_session_ref: Some(request.binding.os_session_ref.clone()),
            session_generation: Some(request.binding.session_generation),
            platform: request.binding.platform,
            desktop_ref: Some(request.binding.desktop_ref.clone()),
            peer_ref: Some("peer:fixture".into()),
            receipt_ref: None,
            reason_ref: None,
            observed_at: now,
        };
        journal
            .record_desktop_observation(&job.job_id, request.binding.clone(), observation, now)
            .unwrap();
        journal.start(&job.job_id, now).unwrap();
        let bytes = fixture::png();
        let header = fixture::header(&transfer, &bytes);
        let mut receipt =
            worker_receipt(&request, fixture::finish(&request, &header).receipt, now).unwrap();
        let binding = journal
            .result_binding(
                &job.job_id,
                transfer.binding.installation_ref.clone(),
                Some(receipt.receipt_ref.clone()),
            )
            .unwrap();
        let store = WorkerResultStore::open(&root.path().join("results")).unwrap();
        let manifest = store
            .seal_desktop_output(binding, &bytes, receipt.ground_truth_refs.clone(), now)
            .unwrap();
        receipt.safe_result_ref = manifest.result_ref.clone();
        let descriptor = p::DesktopOutputDescriptor {
            schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
            binding: transfer.binding.clone(),
            manifest_digest: manifest.manifest_digest.clone(),
            content_digest: header.content_digest,
            bytes: header.bytes,
            media_type: header.media_type,
            width: header.width,
            height: header.height,
        };
        Self {
            root,
            journal,
            job,
            permit,
            execution,
            descriptor,
            manifest,
            receipt,
        }
    }
    fn bytes(&self) -> Vec<u8> {
        journal_bytes(&self.journal)
    }
    fn complete(&self) {
        self.journal
            .record_result_manifest(self.manifest.clone())
            .unwrap();
        self.journal
            .record_desktop_output(self.descriptor.clone())
            .unwrap();
        self.journal
            .record_receipt(self.receipt.clone(), self.permit.issued_at + 1)
            .unwrap();
    }
}
#[test]
fn output_completion_requires_original_manifest_then_descriptor() {
    let f = Fixture::new();
    assert!(f
        .journal
        .record_receipt(f.receipt.clone(), f.permit.issued_at + 1)
        .is_err());
    assert!(f
        .journal
        .record_desktop_output(f.descriptor.clone())
        .is_err());
    f.journal
        .record_result_manifest(f.manifest.clone())
        .unwrap();
    assert!(f
        .journal
        .record_receipt(f.receipt.clone(), f.permit.issued_at + 1)
        .is_err());
    let mut wrong = f.descriptor.clone();
    wrong.binding.session_generation += 1;
    assert!(f.journal.record_desktop_output(wrong).is_err());
    f.journal
        .record_desktop_output(f.descriptor.clone())
        .unwrap();
    let mut wrong = f.receipt.clone();
    wrong.safe_result_ref = Some("result:other".into());
    assert!(f
        .journal
        .record_receipt(wrong, f.permit.issued_at + 1)
        .is_err());
    f.journal
        .record_receipt(f.receipt.clone(), f.permit.issued_at + 1)
        .unwrap();
    assert_eq!(
        f.journal.desktop_output(&f.job.job_id).unwrap(),
        f.descriptor
    );
}
#[test]
fn output_metadata_query_is_read_only_and_rechecks_original_locked_journal() {
    let f = Fixture::new();
    f.complete();
    let original = f.bytes();
    for _ in 0..3 {
        assert_eq!(
            f.journal.desktop_output(&f.job.job_id).unwrap(),
            f.descriptor
        );
    }
    assert_eq!(f.bytes(), original);
    let mut corrupt = original.clone();
    corrupt[1] = b'X';
    {
        let mut writer = f.journal.writer.lock().unwrap();
        writer.file.seek(SeekFrom::Start(0)).unwrap();
        writer.file.write_all(&corrupt).unwrap();
        writer.file.sync_all().unwrap();
    }
    assert!(f.journal.desktop_output(&f.job.job_id).is_err());
    assert!(f.journal.writer.lock().unwrap().poisoned);
    assert_eq!(f.bytes(), corrupt);
}
#[test]
fn every_output_crash_cut_stays_unknown_and_original_job_never_becomes_fresh() {
    for cut in 0..4 {
        let f = Fixture::new();
        if cut >= 1 {
            f.journal
                .record_result_manifest(f.manifest.clone())
                .unwrap();
        }
        if cut >= 2 {
            f.journal
                .record_desktop_output(f.descriptor.clone())
                .unwrap();
        }
        if cut == 3 {
            f.journal
                .record_receipt(f.receipt.clone(), f.permit.issued_at + 1)
                .unwrap();
        }
        let original = f.bytes();
        let Fixture {
            root,
            journal,
            job,
            permit,
            execution,
            descriptor,
            receipt,
            ..
        } = f;
        drop(journal);
        let journal = WorkerJournal::open(&root.path().join("worker.jsonl")).unwrap();
        let (fresh, observation) = journal
            .admit_with_execution(
                job.clone(),
                permit.clone(),
                Some(&execution),
                permit.issued_at + 2,
            )
            .unwrap();
        assert!(!fresh);
        assert_eq!(
            observation.state,
            if cut == 3 {
                p::WorkerState::Completed
            } else {
                p::WorkerState::Unknown
            }
        );
        let after = journal_bytes(&journal);
        assert!(after.starts_with(&original));
        if cut < 3 {
            assert!(journal
                .record_receipt(receipt, permit.issued_at + 2)
                .is_err());
        }
        if cut >= 2 {
            assert_eq!(journal.desktop_output(&job.job_id).unwrap(), descriptor);
        }
    }
}
#[test]
fn schema_one_prefix_is_byte_stable_and_v2_history_cannot_downgrade() {
    let f = Fixture::new();
    f.complete();
    let bytes = f.bytes();
    let mut envelopes: Vec<WorkerEnvelope> = bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert!(envelopes
        .iter()
        .all(|entry| entry.schema_version == p::SchemaVersion(2)));
    let mut projection = WorkerProjection::default();
    for entry in &envelopes[..envelopes.len() - 1] {
        projection.fold(entry).unwrap();
    }
    let last = envelopes.last_mut().unwrap();
    last.schema_version = p::SchemaVersion(1);
    last.digest = WorkerEnvelope::calculate(
        last.schema_version,
        last.worker_seq,
        &last.previous,
        &last.fact,
    )
    .unwrap();
    assert!(projection.fold(last).is_err());
    let fact = WorkerFact::Admitted {
        job: Box::new(f.job.clone()),
        permit: Box::new(f.permit.clone()),
        at: f.permit.issued_at,
        execution_binding: None,
        output_transfer: None,
    };
    let serialized = serde_json::to_value(&fact).unwrap();
    assert!(serialized.get("output_transfer").is_none());
    let previous = p::canonical_digest(&"forme-worker-journal-v1").unwrap();
    let digest = p::canonical_digest(&(p::SchemaVersion(1), 1u64, &previous, &fact)).unwrap();
    let mut old = WorkerProjection::default();
    old.fold(&WorkerEnvelope {
        schema_version: p::SchemaVersion(1),
        worker_seq: 1,
        previous,
        fact,
        digest,
    })
    .unwrap();
}

fn journal_bytes(journal: &WorkerJournal) -> Vec<u8> {
    let mut writer = journal.writer.lock().unwrap();
    let position = writer.file.stream_position().unwrap();
    writer.file.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    writer.file.read_to_end(&mut bytes).unwrap();
    writer.file.seek(SeekFrom::Start(position)).unwrap();
    bytes
}
