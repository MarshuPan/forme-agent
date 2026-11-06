use super::*;
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[test]
fn registry_guard_holds_actual_exclusion_until_callback_returns() {
    let registry = Arc::new(Mutex::new(BTreeMap::<String, ()>::new()));
    let observer = registry.clone();
    let (entered, seen) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        with_empty_registry(&observer, &mut || {
            assert!(observer.try_lock().is_err());
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            Err(p::Error("fixture callback error".into()))
        })
    });
    seen.recv_timeout(Duration::from_secs(5)).unwrap();
    let contender = registry.clone();
    let (done, observed) = mpsc::channel();
    let admission = std::thread::spawn(move || {
        contender.lock().unwrap().insert("new-worker".into(), ());
        done.send(()).unwrap();
    });
    assert!(observed.recv_timeout(Duration::from_millis(40)).is_err());
    release.send(()).unwrap();
    assert_eq!(
        thread.join().unwrap().unwrap_err().0,
        "fixture callback error"
    );
    admission.join().unwrap();
    assert!(with_empty_registry(&registry, &mut || panic!("child is registered")).is_err());
    registry.lock().unwrap().clear();
    with_empty_registry(&registry, &mut || Ok(())).unwrap();
}

#[test]
fn nonempty_or_poisoned_resource_registry_never_invokes_recovery() {
    let registry = Arc::new(Mutex::new(BTreeMap::from([("retained".into(), ())])));
    assert!(with_empty_registry(&registry, &mut || panic!("still occupied")).is_err());
    assert_eq!(registry.lock().unwrap().len(), 1);
    registry.lock().unwrap().clear();
    let poison = registry.clone();
    assert!(std::thread::spawn(move || {
        let _guard = poison.lock().unwrap();
        panic!("fixture poisons resource custody");
    })
    .join()
    .is_err());
    assert!(with_empty_registry(&registry, &mut || panic!("poisoned registry")).is_err());
}
