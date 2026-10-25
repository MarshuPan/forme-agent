#![forbid(unsafe_code)]

use super::super::sandbox_lease::{
    SandboxLeaseBinding, SandboxLeaseJournal, SandboxProcessTreeTermination,
};
use super::*;

#[test]
fn failed_file_opens_never_become_owned_handles() {
    assert!(owned_handle(null_mut(), "invalid").is_err());
    assert!(owned_handle(INVALID_HANDLE_VALUE, "invalid").is_err());
    let directory = tempfile::tempdir().unwrap();
    assert!(file_identity(&directory.path().join("missing")).is_err());
}

#[test]
fn real_acl_grant_and_revoke_match_the_lease_readback_contract() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    let run = runtime.join("job");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    let plan = p::canonical_digest(&"acl-native-roundtrip").unwrap();
    let identity = super::super::windows_appcontainer_support::profile_name_for(
        runtime.to_str().unwrap(),
        "job-acl",
        &p::PlanDigest(plan.0.clone()),
    )
    .unwrap();
    // Deriving a SID does not create a profile or change any real workspace.
    let profile = resolve_profile(&identity).unwrap();
    let journal = SandboxLeaseJournal::open(&runtime.join("sandbox-leases.jsonl")).unwrap();
    let lease = journal
        .begin_lease(
            SandboxLeaseBinding::new(
                "job-acl",
                plan,
                identity,
                &run,
                file_identity(&run).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let grant = journal
        .begin_acl_grant(
            &lease,
            &workspace,
            file_identity(&workspace).unwrap(),
            SandboxTargetAccess::WorkspaceReadWrite,
        )
        .unwrap();
    let granted = apply_acl(&grant, &profile).unwrap();
    assert!(granted.target_access_effective);
    journal.record_acl_readback(&grant, granted).unwrap();
    journal
        .record_process_tree_terminated(
            &lease,
            SandboxProcessTreeTermination {
                schema_version: p::SchemaVersion(1),
                job_id: grant.job_id.clone(),
                plan_digest: grant.plan_digest.clone(),
                appcontainer_identity: grant.appcontainer_identity.clone(),
                root_exit_observed: true,
                active_processes: 0,
                observed_at: 1,
            },
        )
        .unwrap();
    let revoke = journal
        .begin_acl_revoke(&lease, &grant.operation_id)
        .unwrap();
    let revoked = apply_acl(&revoke, &profile).unwrap();
    assert!(!revoked.target_access_effective);
    journal.record_acl_readback(&revoke, revoked).unwrap();
    assert!(
        !apply_acl(&revoke, &profile)
            .unwrap()
            .target_access_effective
    );
}

#[test]
fn low_labels_are_only_applied_to_a_private_test_copy() {
    let temporary = tempfile::tempdir().unwrap();
    let original = temporary.path().join("original");
    let copy = temporary.path().join("copy");
    std::fs::create_dir(&original).unwrap();
    std::fs::create_dir(&copy).unwrap();
    std::fs::write(copy.join("data"), b"private").unwrap();
    let identity = file_identity(&original).unwrap();
    set_low_integrity(&copy).unwrap();
    set_low_integrity(&copy.join("data")).unwrap();
    assert_eq!(file_identity(&original).unwrap(), identity);
    assert_eq!(std::fs::read(copy.join("data")).unwrap(), b"private");
}
