//! Receipt requests confirm Host observations; they never produce those facts.
use super::*;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone)]
pub(super) struct ObservedIsolation {
    pub(super) platform: FakeIsolation,
    probes: Arc<AtomicU64>,
    seals: Arc<AtomicU64>,
    outcome: p::WorkerOutcome,
    fail_cleanup: bool,
}

impl Default for ObservedIsolation {
    fn default() -> Self {
        Self {
            platform: FakeIsolation::default(),
            probes: Arc::new(AtomicU64::new(0)),
            seals: Arc::new(AtomicU64::new(0)),
            outcome: p::WorkerOutcome::Completed,
            fail_cleanup: false,
        }
    }
}

impl ObservedIsolation {
    pub(super) fn receipt(&self, job: &p::WorkerJob) -> p::WorkerReceipt {
        p::WorkerReceipt {
            schema_version: p::SchemaVersion(1),
            receipt_ref: "receipt:platform-exit".into(),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            observed_outcome: self.outcome,
            safe_result_ref: Some("result:platform-exit".into()),
            ground_truth_refs: vec!["ground-truth:observed-exit".into()],
            started_at: 10,
            finished_at: Some(20),
        }
    }

    fn effects(&self) -> [u64; 4] {
        [
            self.platform.admitted.load(Ordering::SeqCst),
            self.probes.load(Ordering::SeqCst),
            self.seals.load(Ordering::SeqCst),
            self.platform.cancelled.load(Ordering::SeqCst),
        ]
    }
}

impl WorkerIsolation for ObservedIsolation {
    fn admit(&self, job: &p::WorkerJob, permit: &p::WorkerPermit) -> p::Result<()> {
        self.platform.admit(job, permit)
    }

    fn cancel(&self, job: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
        self.platform.cancel(job)
    }

    fn observe(&self, job: &p::WorkerJob, _: p::Timestamp) -> p::Result<Option<p::WorkerReceipt>> {
        self.probes.fetch_add(1, Ordering::SeqCst);
        Ok(Some(self.receipt(job)))
    }

    fn result_sealed(&self, _: &p::WorkerJob, _: p::Timestamp) -> p::Result<()> {
        self.seals.fetch_add(1, Ordering::SeqCst);
        if self.fail_cleanup {
            return Err(p::Error("fixture cleanup is incomplete".into()));
        }
        Ok(())
    }
}

fn files(root: &Path) -> BTreeMap<PathBuf, (u64, Option<Vec<u8>>)> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(files(&path));
        } else {
            // Windows locks journal bytes to their original writer. Check
            // zero appends here; the journal unit tests check original bytes
            // through that locked handle without bypassing its OS lock.
            let bytes = if path.extension().is_some_and(|ext| ext == "jsonl") {
                None
            } else {
                Some(fs::read(&path).unwrap())
            };
            result.insert(path.clone(), (fs::metadata(path).unwrap().len(), bytes));
        }
    }
    result
}

#[test]
fn caller_receipt_cannot_probe_seal_or_complete_an_unobserved_job() {
    let temp = Temp::new();
    let (_broker, maintenance, agent) = auths(&temp);
    let isolation = ObservedIsolation::default();
    let worker = host(&temp, isolation.clone());
    let (job, permit) = make_job(100);
    worker.dispatch(&agent, job.clone(), permit, 10).unwrap();
    let before = files(&temp.0);
    let receipt = isolation.receipt(&job);
    assert!(worker
        .record_receipt(&maintenance, receipt.clone(), 21)
        .is_err());
    assert!(worker.record_receipt(&agent, receipt, 21).is_err());
    assert_eq!(isolation.effects(), [1, 0, 0, 0]);
    assert_eq!(files(&temp.0), before);
    assert!(worker
        .with_quiescent_workers(&mut |_| panic!("not settled"))
        .is_err());
    // Only the explicit platform observation now seals and persists a receipt.
    assert_eq!(
        worker.probe(&agent, &job.job_id, 22).unwrap().state,
        p::WorkerState::Completed
    );
    assert_eq!(isolation.effects(), [1, 1, 1, 0]);
}

#[test]
fn receipt_confirmation_rejects_every_changed_field_without_side_effects() {
    let temp = Temp::new();
    let (_broker, maintenance, agent) = auths(&temp);
    let isolation = ObservedIsolation::default();
    let worker = host(&temp, isolation.clone());
    let (job, permit) = make_job(100);
    worker.dispatch(&agent, job.clone(), permit, 10).unwrap();
    worker.probe(&agent, &job.job_id, 20).unwrap();
    let receipt = isolation.receipt(&job);
    let before = files(&temp.0);
    assert!(worker
        .record_receipt(&maintenance, receipt.clone(), 21)
        .is_err());
    type Mutation = fn(&mut p::WorkerReceipt);
    let mutations: &[(&str, Mutation)] = &[
        ("schema", |r| r.schema_version = p::SchemaVersion(2)),
        ("reference", |r| r.receipt_ref = "receipt:other".into()),
        ("job", |r| r.job_id = "job:other".into()),
        ("plan", |r| {
            r.plan_digest = p::PlanDigest(p::canonical_digest(&"other").unwrap().0)
        }),
        ("epoch", |r| r.authority_epoch = p::AuthorityEpoch(2)),
        ("nonce", |r| r.nonce = p::Nonce("nonce:other".into())),
        ("outcome", |r| r.observed_outcome = p::WorkerOutcome::Failed),
        ("result", |r| {
            r.safe_result_ref = Some("result:other".into())
        }),
        ("absent result", |r| r.safe_result_ref = None),
        ("evidence", |r| {
            r.ground_truth_refs.push("ground-truth:extra".into())
        }),
        ("absent evidence", |r| r.ground_truth_refs.clear()),
        ("start", |r| r.started_at += 1),
        ("finish", |r| r.finished_at = Some(21)),
        ("absent finish", |r| r.finished_at = None),
    ];
    for (label, mutate) in mutations {
        let mut changed = receipt.clone();
        mutate(&mut changed);
        assert!(
            worker.record_receipt(&agent, changed, 21).is_err(),
            "{label}"
        );
        assert_eq!(files(&temp.0), before, "{label}");
        assert_eq!(isolation.effects(), [1, 1, 1, 0], "{label}");
    }
    // Rejection does not poison a valid writer or replace its original receipt.
    assert_eq!(
        worker.record_receipt(&agent, receipt, 22).unwrap().state,
        p::WorkerState::Completed
    );
    assert_eq!(files(&temp.0), before);
}

#[test]
fn persisted_receipt_confirmation_survives_restart_and_preserves_all_outcomes() {
    for (outcome, state) in [
        (p::WorkerOutcome::Completed, p::WorkerState::Completed),
        (p::WorkerOutcome::Failed, p::WorkerState::Failed),
        (p::WorkerOutcome::Cancelled, p::WorkerState::Cancelled),
        (p::WorkerOutcome::Unknown, p::WorkerState::Unknown),
    ] {
        let temp = Temp::new();
        let (_broker, _, agent) = auths(&temp);
        let isolation = ObservedIsolation {
            outcome,
            ..Default::default()
        };
        let (job, permit) = make_job(100);
        let receipt = isolation.receipt(&job);
        {
            let worker = host(&temp, isolation.clone());
            worker.dispatch(&agent, job.clone(), permit, 10).unwrap();
            assert_eq!(worker.probe(&agent, &job.job_id, 20).unwrap().state, state);
        }
        let worker = host(&temp, isolation.clone());
        let before = files(&temp.0);
        for now in [21, 101] {
            let observed = worker.record_receipt(&agent, receipt.clone(), now).unwrap();
            observed.validate().unwrap();
            assert_eq!(observed.state, state);
            assert_eq!(observed.outcome, Some(outcome));
            assert_eq!(observed.receipt_ref.as_ref(), Some(&receipt.receipt_ref));
        }
        let mut callbacks = 0;
        let settled = worker.with_quiescent_workers(&mut |_| {
            callbacks += 1;
            Ok(())
        });
        assert_eq!(settled.is_ok(), outcome != p::WorkerOutcome::Unknown);
        assert_eq!(callbacks, u64::from(outcome != p::WorkerOutcome::Unknown));
        assert_eq!(files(&temp.0), before);
        assert_eq!(isolation.effects(), [1, 1, 1, 0]);
    }
}

#[test]
fn cleanup_failure_or_restart_unknown_cannot_be_laundered_by_a_receipt() {
    for fail_cleanup in [false, true] {
        let temp = Temp::new();
        let (_broker, _, agent) = auths(&temp);
        let isolation = ObservedIsolation {
            fail_cleanup,
            ..Default::default()
        };
        let (job, permit) = make_job(100);
        let receipt = isolation.receipt(&job);
        {
            let worker = host(&temp, isolation.clone());
            worker.dispatch(&agent, job.clone(), permit, 10).unwrap();
            if fail_cleanup {
                assert!(worker.probe(&agent, &job.job_id, 20).is_err());
            }
        }
        let worker = host(&temp, isolation.clone());
        let before = files(&temp.0);
        let effects = isolation.effects();
        assert!(worker.record_receipt(&agent, receipt, 21).is_err());
        assert_eq!(
            worker.probe(&agent, &job.job_id, 22).unwrap().state,
            p::WorkerState::Unknown
        );
        assert!(worker
            .with_quiescent_workers(&mut |_| panic!("unknown"))
            .is_err());
        assert_eq!(files(&temp.0), before);
        assert_eq!(isolation.effects(), effects);
    }
}

#[test]
fn concurrent_receipt_confirmations_cannot_repeat_observation_or_sealing() {
    let temp = Temp::new();
    let (_broker, _, agent) = auths(&temp);
    let isolation = ObservedIsolation::default();
    let worker = host(&temp, isolation.clone());
    let (job, permit) = make_job(100);
    worker.dispatch(&agent, job.clone(), permit, 10).unwrap();
    worker.probe(&agent, &job.job_id, 20).unwrap();
    let receipt = isolation.receipt(&job);
    let before = files(&temp.0);
    std::thread::scope(|scope| {
        let handles = (0..12)
            .map(|_| scope.spawn(|| worker.record_receipt(&agent, receipt.clone(), 21).unwrap()))
            .collect::<Vec<_>>();
        for handle in handles {
            assert_eq!(handle.join().unwrap().state, p::WorkerState::Completed);
        }
    });
    assert_eq!(files(&temp.0), before);
    assert_eq!(isolation.effects(), [1, 1, 1, 0]);
}
