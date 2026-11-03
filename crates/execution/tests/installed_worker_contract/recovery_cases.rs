//! The release observer shares the actual worker admission lock and watermark.
use super::*;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn release_recovery_worker_running_and_restart_unknown_never_prove_quiescence() {
    let temp = Temp::new();
    let (_broker, _, auth) = auths(&temp);
    let worker = host(&temp, FakeIsolation::default());
    let mut callbacks = 0;
    worker
        .with_quiescent_workers(&mut |_| {
            callbacks += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(callbacks, 1);
    let (job, permit) = make_job(1000);
    assert_eq!(
        worker.dispatch(&auth, job, permit, 2).unwrap().state,
        p::WorkerState::Running
    );
    assert!(worker
        .with_quiescent_workers(&mut |_| {
            callbacks += 1;
            Ok(())
        })
        .is_err());
    drop(worker);
    let recovered = host(&temp, FakeIsolation::default());
    assert_eq!(
        recovered.probe(&auth, "job-1", 3).unwrap().state,
        p::WorkerState::Unknown
    );
    assert!(recovered
        .with_quiescent_workers(&mut |_| {
            callbacks += 1;
            Ok(())
        })
        .is_err());
    assert_eq!(callbacks, 1);
}

#[test]
fn release_recovery_worker_observation_holds_admission_exclusion_through_its_callback() {
    let temp = Temp::new();
    let (_broker, _, auth) = auths(&temp);
    let isolation = FakeIsolation::default();
    let admitted = isolation.admitted.clone();
    let worker = Arc::new(host(&temp, isolation));
    let observer = worker.clone();
    let (locked, lock_seen) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        observer.with_quiescent_workers(&mut |_| {
            locked.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(())
        })
    });
    lock_seen.recv_timeout(Duration::from_secs(3)).unwrap();
    let dispatcher = worker.clone();
    let (job, permit) = make_job(1000);
    let (done, received) = mpsc::channel();
    let dispatch = std::thread::spawn(move || {
        let result = dispatcher.dispatch(&auth, job, permit, 2);
        done.send(()).unwrap();
        result
    });
    assert!(received.recv_timeout(Duration::from_millis(40)).is_err());
    assert_eq!(admitted.load(Ordering::SeqCst), 0);
    release.send(()).unwrap();
    thread.join().unwrap().unwrap();
    assert_eq!(
        dispatch.join().unwrap().unwrap().state,
        p::WorkerState::Running
    );
    assert_eq!(admitted.load(Ordering::SeqCst), 1);
}
