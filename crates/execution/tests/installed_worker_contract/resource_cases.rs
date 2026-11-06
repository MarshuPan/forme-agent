//! A settled worker log does not replace adapter-owned resource observations.
use super::*;
use std::collections::BTreeSet;
use std::sync::{mpsc, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
struct RetainedResource {
    occupied: Arc<Mutex<bool>>,
}

impl WorkerIsolation for RetainedResource {
    fn with_quiescent_resources(&self, action: &mut dyn FnMut() -> p::Result<()>) -> p::Result<()> {
        let occupied = self.occupied.lock().unwrap();
        if *occupied {
            return Err(p::Error("fixture resource is still retained".into()));
        }
        let result = action();
        drop(occupied);
        result
    }

    fn admit(&self, _: &p::WorkerJob, _: &p::WorkerPermit) -> p::Result<()> {
        *self.occupied.lock().unwrap() = true;
        Ok(())
    }

    fn cancel(&self, _: &p::WorkerJob) -> p::Result<p::WorkerCancelReceipt> {
        panic!("this fixture observes completion only")
    }

    fn observe(
        &self,
        job: &p::WorkerJob,
        now: p::Timestamp,
    ) -> p::Result<Option<p::WorkerReceipt>> {
        Ok(Some(p::WorkerReceipt {
            schema_version: p::SchemaVersion(1),
            receipt_ref: "receipt:retained-resource".into(),
            job_id: job.job_id.clone(),
            plan_digest: job.plan_digest.clone(),
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            observed_outcome: p::WorkerOutcome::Completed,
            safe_result_ref: None,
            ground_truth_refs: vec![],
            started_at: 10,
            finished_at: Some(now),
        }))
    }
}

#[test]
fn missing_resource_observer_rejects_even_an_empty_worker_journal() {
    let temp = Temp::new();
    let worker = WorkerExecutorHost::open(
        &temp.worker_journal(),
        p::canonical_digest(&AGENT_TOKEN).unwrap(),
        p::AuthorityEpoch(1),
        BTreeSet::from([p::Scope("workspace:worker-test".into())]),
    )
    .unwrap();
    let length = fs::metadata(temp.worker_journal()).unwrap().len();
    let mut callbacks = 0;
    assert!(worker
        .with_quiescent_workers(&mut |_| {
            callbacks += 1;
            Ok(())
        })
        .is_err());
    assert_eq!(callbacks, 0);
    assert_eq!(fs::metadata(temp.worker_journal()).unwrap().len(), length);
}

#[test]
fn settled_journal_cannot_hide_adapter_resources_or_rewrite_their_state() {
    let temp = Temp::new();
    let (_, _, auth) = auths(&temp);
    let isolation = RetainedResource::default();
    let worker = host(&temp, isolation.clone());
    let (job, permit) = make_job(1000);
    worker.dispatch(&auth, job.clone(), permit, 10).unwrap();
    assert_eq!(
        worker.probe(&auth, &job.job_id, 20).unwrap().state,
        p::WorkerState::Completed
    );
    let length = fs::metadata(temp.worker_journal()).unwrap().len();
    assert!(worker
        .with_quiescent_workers(&mut |_| panic!("resource is still occupied"))
        .is_err());
    assert!(*isolation.occupied.lock().unwrap());
    assert_eq!(fs::metadata(temp.worker_journal()).unwrap().len(), length);
    *isolation.occupied.lock().unwrap() = false;
    let mut callbacks = 0;
    worker
        .with_quiescent_workers(&mut |_| {
            assert!(isolation.occupied.try_lock().is_err());
            callbacks += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(callbacks, 1);
    assert_eq!(fs::metadata(temp.worker_journal()).unwrap().len(), length);
}

#[test]
fn recovery_callback_holds_adapter_exclusion_and_returns_the_original_error() {
    let temp = Temp::new();
    let isolation = RetainedResource::default();
    let worker = Arc::new(host(&temp, isolation.clone()));
    let observer = worker.clone();
    let (entered, seen) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        observer.with_quiescent_workers(&mut |_| {
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            Err(p::Error("fixture recovery declined".into()))
        })
    });
    seen.recv_timeout(Duration::from_secs(5)).unwrap();
    let contender = isolation.clone();
    let (done, observed) = mpsc::channel();
    let (job, permit) = make_job(1000);
    let admission = std::thread::spawn(move || {
        // Direct adapter access also waits for the resource guard, independent
        // of the Host operations mutex used by ordinary dispatch.
        contender.admit(&job, &permit).unwrap();
        done.send(()).unwrap();
    });
    assert!(observed.recv_timeout(Duration::from_millis(40)).is_err());
    release.send(()).unwrap();
    assert_eq!(
        thread.join().unwrap().unwrap_err().0,
        "fixture recovery declined"
    );
    admission.join().unwrap();
    observed.recv_timeout(Duration::from_secs(5)).unwrap();
    *isolation.occupied.lock().unwrap() = false;
    worker.with_quiescent_workers(&mut |_| Ok(())).unwrap();
}
