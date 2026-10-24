//! Real encrypted data/schema/backup evidence; not native service-upgrade evidence.
use super::{installed_restore::RestoreFixture, *};
use crate::offline_data::{OfflineCoreMaintenance, OfflineReleaseWorker};

#[path = "installed_release/live_probe.rs"]
mod live_probe;

fn request(f: &RestoreFixture) -> p::ProductReleaseDataRequest {
    p::ProductReleaseDataRequest {
        schema_version: p::SchemaVersion(1),
        release: p::ProductReleaseRequest {
            schema_version: p::SchemaVersion(1),
            install_ref: f.request.install_ref.clone(),
            operation_id: "release:offline-fixture".into(),
            command_digest: p::canonical_digest(&"command").unwrap(),
            change: p::ProductReleaseChange::Upgrade,
            original_release_ref: "0.1.0".into(),
            original_release_digest: p::canonical_digest(&"old").unwrap(),
            target_release_ref: "0.2.0".into(),
            target_release_digest: p::canonical_digest(&"new").unwrap(),
            backup_ref: "backup:release-fixture".into(),
        },
        owner: f.request.owner.clone(),
        expected_config_revision: f.request.expected_config_revision,
    }
}
fn step(
    request: &p::ProductReleaseDataRequest,
    step: p::ProductReleaseDataStep,
) -> p::ProductReleaseDataStepRequest {
    p::ProductReleaseDataStepRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: request.release.operation_id.clone(),
        command_digest: request.release.command_digest.clone(),
        step,
    }
}

#[test]
fn release_data_observes_real_schema_holds_store_and_creates_verified_backup_once() {
    let f = RestoreFixture::new();
    let request = request(&f);
    let before = fs::read(&f.f.config.store_path).unwrap();
    let mut worker = OfflineReleaseWorker::open(&f.manifest, request.clone()).unwrap();
    let p::ProductReleaseDataResult::Inspected { identity } = worker.inspected().unwrap().result
    else {
        panic!()
    };
    assert_eq!(identity.config_schema_version, 1);
    assert!(
        identity.database_schema_version > 1,
        "must observe SQLite, not DTO schema v1"
    );
    assert!(
        OfflineCoreMaintenance::open(&f.core, None).is_err(),
        "the source must remain exclusively fenced"
    );
    let backup = worker
        .step(step(&request, p::ProductReleaseDataStep::Backup))
        .unwrap();
    let p::ProductReleaseDataResult::BackedUp {
        identity: backed_up,
        backup_ref,
        backup_digest,
    } = backup.result
    else {
        panic!()
    };
    assert_eq!(backed_up, identity);
    assert_eq!(backup_ref, request.release.backup_ref);
    let paths: Vec<_> = fs::read_dir(f.f.root.join("backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("release-")
                && p.extension().is_some_and(|e| e == "db")
        })
        .collect();
    assert_eq!(paths.len(), 1);
    let verified = OfflineCoreMaintenance::open(&f.core, Some(&paths[0])).unwrap();
    assert_eq!(verified.identity().store_identity, identity.store_identity);
    assert_eq!(verified.identity().event_count, identity.event_count);
    assert_eq!(
        p::canonical_digest(&verified.verify().unwrap().manifest).unwrap(),
        backup_digest
    );
    drop(verified);
    assert!(worker
        .step(step(&request, p::ProductReleaseDataStep::Backup))
        .is_err());
    worker
        .step(step(&request, p::ProductReleaseDataStep::Release))
        .unwrap();
    assert!(
        OfflineCoreMaintenance::open(&f.core, None).is_ok(),
        "release receipt follows lock release"
    );
    assert!(worker.inspected().is_err());
    assert_eq!(
        fs::read(&f.f.config.store_path).unwrap(),
        before,
        "release data lane must not mutate the brain"
    );
    assert_eq!(
        f.f.calls.load(Ordering::SeqCst),
        0,
        "offline lane must not call a model"
    );
}

#[test]
fn release_data_rejects_wrong_owner_revision_missing_keys_and_cross_operation_frames() {
    let f = RestoreFixture::new();
    let base = request(&f);
    for change in 0..3 {
        let mut bad = base.clone();
        match change {
            0 => bad.expected_config_revision += 1,
            1 => bad.owner = p::OwnerPrincipalRef("owner:other".into()),
            _ => bad.release.install_ref = "install:other".into(),
        }
        assert!(OfflineReleaseWorker::open(&f.manifest, bad).is_err());
    }
    let mut worker = OfflineReleaseWorker::open(&f.manifest, base.clone()).unwrap();
    let mut wrong = step(&base, p::ProductReleaseDataStep::Backup);
    wrong.operation_id = "release:different".into();
    assert!(worker.step(wrong).is_err());
    drop(worker);
    let keys = f.f.config.store_path.with_extension("content-keys");
    let retained = keys.with_extension("retained-fixture-keys");
    fs::rename(&keys, &retained).unwrap();
    assert!(OfflineReleaseWorker::open(&f.manifest, base).is_err());
    assert!(!keys.exists(), "missing keys must not be regenerated");
    fs::rename(retained, keys).unwrap();
}
