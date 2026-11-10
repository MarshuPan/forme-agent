//! Lock contention must consume the original budget and remain cancellable.
use super::*;

fn fixture() -> (tempfile::TempDir, Arc<InstalledManagedMcpPrograms>) {
    let (temp, _, configuration) = crate::installed::product_config::tests::fixture();
    let programs = InstalledManagedMcpPrograms::new(
        temp.path().join("programs.jsonl"),
        "install:config-test".into(),
        p::AuthorityEpoch(1),
        Arc::new(configuration),
        temp.path().join("sources"),
        vec![p::Scope("workspace:test".into())],
        vec![],
    )
    .unwrap();
    (temp, Arc::new(programs))
}
#[test]
fn managed_mcp_configuration_wait_cancels_without_waiting_for_maintenance() {
    let (_temp, programs) = fixture();
    let locked = programs.configuration.lock().unwrap();
    let token = crate::CancelToken::default();
    let waiter_token = token.clone();
    let waiter = programs.clone();
    let (ready, begun) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        ready.send(()).unwrap();
        waiter
            .configuration_until(Instant::now() + Duration::from_secs(20), &waiter_token)
            .is_err()
    });
    begun.recv_timeout(Duration::from_secs(1)).unwrap();
    token.cancel();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !thread.is_finished() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(thread.join().unwrap());
    assert!(!programs.journal.exists());
    drop(locked);
}
#[test]
fn managed_mcp_configuration_wait_does_not_renew_original_deadline() {
    let (_temp, programs) = fixture();
    let locked = programs.configuration.lock().unwrap();
    let started = Instant::now();
    assert!(programs
        .configuration_until(
            started + Duration::from_millis(30),
            &crate::CancelToken::default()
        )
        .is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(!programs.journal.exists());
    drop(locked);
    assert!(programs
        .configuration_until(started, &crate::CancelToken::default())
        .is_err());
}
