#![cfg(windows)]

use std::path::Path;

use forme_protocol as p;

use super::*;

#[test]
fn profile_identity_is_deterministic_and_plan_bound() {
    let plan = p::PlanDigest("sha256:plan-a".into());
    let first = profile_name_for(r"C:\forme\runtime", "job-a", &plan).unwrap();
    let second = profile_name_for(r"C:\forme\runtime", "job-a", &plan).unwrap();
    let other_plan = profile_name_for(
        r"C:\forme\runtime",
        "job-a",
        &p::PlanDigest("sha256:plan-b".into()),
    )
    .unwrap();
    assert_eq!(first, second);
    assert_ne!(first, other_plan);
    assert!(first.starts_with("forme-job-"));
    assert!(first.len() <= 64);
    assert!(first
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-'));
}

#[test]
fn job_object_name_is_closed_and_bound_to_the_appcontainer_identity() {
    let identity = "forme-job-0123456789abcdef0123456789abcdef0123456789abcdef";
    assert_eq!(
        job_object_name(identity).unwrap(),
        format!("Local\\ForMeWorker-{identity}")
    );
    assert!(job_object_name("forme-job-../foreign").is_err());
}

#[test]
fn environment_block_is_deterministic_and_scoped() {
    let block = environment_block(
        Path::new(r"C:\forme\run\job.json"),
        Path::new(r"C:\forme\run\permit.json"),
        Path::new(r"C:\forme\workspace"),
        Some(Path::new(r"C:\forme\run\execution.json")),
        "workspace:demo",
        42,
    )
    .expect("valid Windows paths should encode");
    let text = String::from_utf16_lossy(&block);
    assert!(text.contains("FORME_WORKER_WORKSPACE_SCOPE=workspace:demo\0"));
    assert!(text.contains("FORME_WORKER_DEADLINE=42\0"));
    assert!(text.ends_with("\0\0"));
}

#[test]
fn an_unowned_lease_blocks_new_admission_without_sandbox_side_effects() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let isolation = WindowsAppContainerWorkerIsolation::open(
        directory.path().join("runtime"),
        WorkerLaunchRegistry::default(),
        BTreeMap::from([("workspace:one".into(), workspace)]),
    )
    .unwrap();
    let run_dir = isolation.runtime_root.join("interrupted-job");
    std::fs::create_dir(&run_dir).unwrap();
    let (job, permit) = fixture_job();
    isolation
        .journal
        .begin_lease(
            SandboxLeaseBinding::new(
                "old-job",
                p::SchemaDigest(job.plan_digest.0.clone()),
                isolation.profile_name(&job).unwrap(),
                &run_dir,
                acl::file_identity(&run_dir).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let journal = isolation.runtime_root.join("sandbox-leases.jsonl");
    let before = std::fs::metadata(&journal).unwrap().len();
    for _ in 0..2 {
        let error = isolation.admit_at(&job, &permit, 10).unwrap_err();
        assert!(error.0.contains("recovery is required"));
        assert!(isolation.children.lock().unwrap().is_empty());
        assert_eq!(std::fs::metadata(&journal).unwrap().len(), before);
        assert_eq!(isolation.journal.pending_cleanup().unwrap().len(), 1);
    }
}

#[test]
fn startup_recovery_revokes_an_unacknowledged_grant_and_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    let run_dir = runtime.join("interrupted-job");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    let identity = profile_name_for(
        runtime.to_str().unwrap(),
        "old-job",
        &fixture_job().0.plan_digest,
    )
    .unwrap();
    let profile = acl::resolve_profile(&identity).unwrap();
    let journal_path = runtime.join("sandbox-leases.jsonl");
    let journal = SandboxLeaseJournal::open(&journal_path).unwrap();
    let lease = journal
        .begin_lease(
            SandboxLeaseBinding::new(
                "old-job",
                p::SchemaDigest(fixture_job().0.plan_digest.0),
                identity,
                &run_dir,
                acl::file_identity(&run_dir).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    let mutation = journal
        .begin_acl_grant(
            &lease,
            &workspace,
            acl::file_identity(&workspace).unwrap(),
            SandboxTargetAccess::WorkspaceReadWrite,
        )
        .unwrap();
    // Simulate the crash gap after a real DACL grant but before its receipt.
    assert!(
        acl::apply_acl(&mutation, &profile)
            .unwrap()
            .target_access_effective
    );
    drop(journal);
    let journal = SandboxLeaseJournal::open(&journal_path).unwrap();
    recovery::recover_pending(&journal).unwrap();
    assert!(!run_dir.exists());
    assert!(workspace.is_dir());
    assert!(journal.pending_cleanup().unwrap().is_empty());
    drop(journal);
    let before = std::fs::read(&journal_path).unwrap();
    let journal = SandboxLeaseJournal::open(&journal_path).unwrap();
    recovery::recover_pending(&journal).unwrap();
    drop(journal);
    assert_eq!(std::fs::read(&journal_path).unwrap(), before);
}

fn fixture_job() -> (p::WorkerJob, p::WorkerPermit) {
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: "job-1".into(),
        owner_ref: p::VerifiedPrincipal("owner:appcontainer-test".into()),
        action_ref: p::ActionId("action-1".into()),
        affected_resources: vec![p::ResourceRef("workspace:one".into())],
        execution_plan_ref: "plan-1".into(),
        plan_digest: p::PlanDigest(p::canonical_digest(&"appcontainer-plan").unwrap().0),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: "permit-1".into(),
        sandbox_profile_ref: "sandbox:strict".into(),
        workspace_scope: p::Scope("workspace:one".into()),
        deadline: 100,
        nonce: p::Nonce("nonce-1".into()),
        cancel_ref: "cancel-1".into(),
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
#[ignore = "requires an explicit native AppContainer integration environment"]
fn existing_profile_is_rejected_before_acl_or_process_setup() {
    let name = "forme-contract-existing-profile";
    let profile = acl::create_profile(name).expect("integration host can create profile");
    assert!(acl::create_profile(name).is_err());
    assert!(acl::delete_profile(&profile).expect("owned profile can be removed"));
}
