//! Native discovery plus read-only current-source checks. No installed claim.
use super::*;
use crate::installed::{HostCredentials, HostManagement, ProductHostBroker, SystemServiceManager};

fn broker(f: &Fixture) -> ProductHostBroker {
    ProductHostBroker::open_with_service_manager_and_config(
        &f._temp.path().join("source-host.jsonl"),
        "install:config-test",
        HostCredentials::new("m".repeat(32), Some("a".repeat(32)))
            .unwrap()
            .with_web_configuration(Some("w".repeat(32)))
            .unwrap(),
        p::ProductTarget::WindowsX86_64,
        "fixture-release".into(),
        Some(f.config.clone()),
        SystemServiceManager,
    )
    .unwrap()
    .with_mcp_programs(f.programs.clone())
    .unwrap()
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_source_verification_is_read_only_authenticated_and_reopens() {
    let f = Fixture::new("normal", 10_000);
    let broker = broker(&f);
    let auth = broker.authenticate_agent(&"a".repeat(32)).unwrap();
    let worker = host(&f);
    let original = discover(&f, &worker, &auth);
    let journal = f._temp.path().join("call-worker.jsonl");
    drop(worker);
    let before = fs::read(&journal).unwrap();
    assert!(broker.verify_mcp_source(&auth, &original).is_err());
    let worker = host(&f);
    assert_eq!(
        broker.verify_mcp_source(&auth, &original).unwrap(),
        f.settings
    );
    for wrong in [
        broker.authenticate(&"m".repeat(32)).unwrap(),
        broker
            .authenticate_web_configuration(&"w".repeat(32))
            .unwrap(),
    ] {
        assert!(broker.verify_mcp_source(&wrong, &original).is_err());
    }
    let mut substituted = original.clone();
    substituted.produced_at += 1;
    substituted.manifest_digest = substituted.expected_digest().unwrap();
    assert!(broker.verify_mcp_source(&auth, &substituted).is_err());
    drop(worker);
    assert_eq!(fs::read(&journal).unwrap(), before);
    assert!(broker.verify_mcp_source(&auth, &original).is_err());
    let reopened = host(&f);
    assert_eq!(
        broker.verify_mcp_source(&auth, &original).unwrap(),
        f.settings
    );
    f.revoke();
    assert!(broker.verify_mcp_source(&auth, &original).is_err());
    assert_eq!(
        reopened
            .result_manifest(&auth, &f.request.job.job_id, recovery_timestamp())
            .unwrap(),
        original
    );
    assert!(!read_body(&reopened, &auth, &f.request).1.is_empty());
    drop(reopened);
    assert_eq!(fs::read(&journal).unwrap(), before);
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_source_current_settings_and_newer_unknown_block_old_original() {
    let f = Fixture::new("normal", 10_000);
    let broker = broker(&f);
    let auth = broker.authenticate_agent(&"a".repeat(32)).unwrap();
    let worker = host(&f);
    let original = discover(&f, &worker, &auth);
    assert_eq!(
        broker.verify_mcp_source(&auth, &original).unwrap(),
        f.settings
    );
    let mut newer = request(&f.registration, &f.settings);
    newer.job.job_id = "job:source-pending".into();
    newer.job.permit_ref = "permit:source-pending".into();
    newer.job.nonce = p::Nonce("nonce:source-pending".into());
    newer.permit.job_id = newer.job.job_id.clone();
    newer.permit.permit_ref = newer.job.permit_ref.clone();
    newer.permit.nonce = newer.job.nonce.clone();
    worker
        .journal
        .admit_with_execution(
            newer.job.clone(),
            newer.permit.clone(),
            newer.execution.as_ref(),
            newer.permit.issued_at,
        )
        .unwrap();
    assert!(broker.verify_mcp_source(&auth, &original).is_err());
    worker
        .journal
        .mark_unknown(
            &newer.job.job_id,
            "discovery:unknown".into(),
            recovery_timestamp(),
        )
        .unwrap();
    assert!(broker.verify_mcp_source(&auth, &original).is_err());
    drop(worker);
    let before = fs::read(f._temp.path().join("call-worker.jsonl")).unwrap();
    let reopened = host(&f);
    assert!(broker.verify_mcp_source(&auth, &original).is_err());
    drop(reopened);
    assert_eq!(
        fs::read(f._temp.path().join("call-worker.jsonl")).unwrap(),
        before
    );
    assert!(!directory(&f, &newer).exists());
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_source_disabled_configuration_rejects_current_metadata() {
    let f = Fixture::new("normal", 10_000);
    let broker = broker(&f);
    let auth = broker.authenticate_agent(&"a".repeat(32)).unwrap();
    let worker = host(&f);
    let original = discover(&f, &worker, &auth);
    assert_eq!(
        broker.verify_mcp_source(&auth, &original).unwrap(),
        f.settings
    );
    let mut disabled = f.settings.clone();
    disabled.enabled = false;
    config_tests::stage_mcp_fixture(&mut f.config.lock().unwrap(), disabled);
    assert!(broker.verify_mcp_source(&auth, &original).is_err());
    assert_eq!(
        worker
            .result_manifest(&auth, &f.request.job.job_id, recovery_timestamp())
            .unwrap(),
        original
    );
}
