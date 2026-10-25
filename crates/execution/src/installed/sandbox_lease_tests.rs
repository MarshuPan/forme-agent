use std::fs;
use std::io::Write;
use std::path::PathBuf;

use forme_protocol as p;
use tempfile::TempDir;

use super::{
    SandboxAclReadback, SandboxFileIdentity, SandboxLeaseBinding, SandboxLeaseJournal,
    SandboxProcessTreeTermination, SandboxTargetAccess, JOURNAL_FILE,
};

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    job_directory: PathBuf,
    journal: SandboxLeaseJournal,
    lease_id: String,
    grant: super::SandboxAclMutation,
}

fn setup() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("runtime");
    let job_directory = root.join("job-1");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&job_directory).unwrap();
    fs::create_dir(&workspace).unwrap();
    let identity = SandboxFileIdentity::new(1, [1; 16]).unwrap();
    let journal = SandboxLeaseJournal::open(&root.join(JOURNAL_FILE))
        .unwrap_or_else(|error| panic!("open {}: {error}", root.display()));
    let binding = SandboxLeaseBinding::new(
        "job-1",
        p::canonical_digest(&"plan").unwrap(),
        "forme-job-1",
        &job_directory,
        identity.clone(),
    )
    .unwrap();
    let lease_id = journal.begin_lease(binding).unwrap();
    let grant = journal
        .begin_acl_grant(
            &lease_id,
            &workspace,
            SandboxFileIdentity::new(2, [2; 16]).unwrap(),
            SandboxTargetAccess::WorkspaceReadWrite,
        )
        .unwrap();
    Fixture {
        _temp: temp,
        root,
        job_directory,
        journal,
        lease_id,
        grant,
    }
}

fn acl_readback(mutation: &super::SandboxAclMutation, effective: bool) -> SandboxAclReadback {
    SandboxAclReadback {
        schema_version: p::SchemaVersion(1),
        operation_id: mutation.operation_id.clone(),
        target_file_identity: mutation.target_file_identity.clone(),
        effective_acl_digest: p::canonical_digest(&("acl", effective)).unwrap(),
        target_access_effective: effective,
    }
}

fn termination() -> SandboxProcessTreeTermination {
    SandboxProcessTreeTermination {
        schema_version: p::SchemaVersion(1),
        job_id: "job-1".into(),
        plan_digest: p::canonical_digest(&"plan").unwrap(),
        appcontainer_identity: "forme-job-1".into(),
        root_exit_observed: true,
        active_processes: 0,
        observed_at: 100,
    }
}

#[test]
fn incomplete_acl_intent_survives_restart_as_pending_cleanup() {
    let fixture = setup();
    assert!(fixture
        .journal
        .record_acl_readback(
            &fixture.grant,
            SandboxAclReadback {
                target_file_identity: SandboxFileIdentity::new(3, [3; 16]).unwrap(),
                ..acl_readback(&fixture.grant, true)
            },
        )
        .is_err());
    drop(fixture.journal);

    let journal = SandboxLeaseJournal::open(&fixture.root.join(JOURNAL_FILE)).unwrap();
    let pending = journal.pending_cleanup().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].lease_id, fixture.lease_id);
    assert_eq!(pending[0].acl_mutations.len(), 1);
    assert!(pending[0].acl_mutations[0].readback.is_none());
    assert!(pending[0].process_tree_termination.is_none());
    assert!(journal.verify_startup_recovery_complete().is_err());
}

#[test]
fn cleanup_requires_tree_termination_and_exact_acl_revocation() {
    let fixture = setup();
    let proof = termination();
    assert!(fixture
        .journal
        .begin_acl_grant(
            &fixture.lease_id,
            PathBuf::from(&fixture.grant.target_path).as_path(),
            fixture.grant.target_file_identity.clone(),
            SandboxTargetAccess::WorkspaceReadWrite,
        )
        .is_err());
    assert!(fixture
        .journal
        .begin_acl_revoke(&fixture.lease_id, &fixture.grant.operation_id)
        .is_err());
    fixture
        .journal
        .record_acl_readback(&fixture.grant, acl_readback(&fixture.grant, true))
        .unwrap();
    fixture
        .journal
        .record_acl_readback(&fixture.grant, acl_readback(&fixture.grant, true))
        .unwrap();
    fixture
        .journal
        .record_process_tree_terminated(&fixture.lease_id, proof)
        .unwrap();
    let revoke = fixture
        .journal
        .begin_acl_revoke(&fixture.lease_id, &fixture.grant.operation_id)
        .unwrap();
    assert!(fixture
        .journal
        .begin_acl_revoke(&fixture.lease_id, &fixture.grant.operation_id)
        .is_err());
    assert!(fixture
        .journal
        .begin_appcontainer_removal(&fixture.lease_id)
        .is_err());
    fixture
        .journal
        .record_acl_readback(&revoke, acl_readback(&revoke, false))
        .unwrap();
    fixture
        .journal
        .begin_appcontainer_removal(&fixture.lease_id)
        .unwrap();
    fixture
        .journal
        .record_appcontainer_removed(&fixture.lease_id, "forme-job-1", true)
        .unwrap();
    fixture
        .journal
        .begin_job_directory_removal(&fixture.lease_id)
        .unwrap();
    let late_proof = super::SandboxWritebackReceipt {
        schema_version: p::SchemaVersion(1),
        job_id: "job-1".into(),
        plan_digest: p::canonical_digest(&"plan").unwrap(),
        intent_digest: p::canonical_digest(&"intent").unwrap(),
        baseline_digest: p::canonical_digest(&"baseline").unwrap(),
        result_digest: p::canonical_digest(&"result").unwrap(),
    };
    let path = fixture.root.join(JOURNAL_FILE);
    let before = fs::metadata(&path).unwrap().len();
    assert!(fixture
        .journal
        .record_writeback_receipt(&fixture.lease_id, late_proof.clone())
        .is_err());
    assert_eq!(fs::metadata(&path).unwrap().len(), before);
    fs::remove_dir(&fixture.job_directory).unwrap();
    fixture
        .journal
        .record_job_directory_removed(&fixture.lease_id)
        .unwrap();
    assert!(fixture
        .journal
        .record_writeback_receipt(&fixture.lease_id, late_proof)
        .is_err());
    fixture
        .journal
        .record_cleanup_receipt(&fixture.lease_id)
        .unwrap();
    fixture
        .journal
        .record_cleanup_receipt(&fixture.lease_id)
        .unwrap();
    assert!(fixture.journal.pending_cleanup().unwrap().is_empty());
    fixture.journal.verify_startup_recovery_complete().unwrap();
}

#[test]
fn cleanup_receipt_and_projection_survive_reopen() {
    let fixture = setup();
    fixture
        .journal
        .record_acl_readback(&fixture.grant, acl_readback(&fixture.grant, true))
        .unwrap();
    fixture
        .journal
        .record_process_tree_terminated(&fixture.lease_id, termination())
        .unwrap();
    let revoke = fixture
        .journal
        .begin_acl_revoke(&fixture.lease_id, &fixture.grant.operation_id)
        .unwrap();
    fixture
        .journal
        .record_acl_readback(&revoke, acl_readback(&revoke, false))
        .unwrap();
    fixture
        .journal
        .begin_appcontainer_removal(&fixture.lease_id)
        .unwrap();
    fixture
        .journal
        .record_appcontainer_removed(&fixture.lease_id, "forme-job-1", true)
        .unwrap();
    fixture
        .journal
        .begin_job_directory_removal(&fixture.lease_id)
        .unwrap();
    fs::remove_dir(&fixture.job_directory).unwrap();
    fixture
        .journal
        .record_job_directory_removed(&fixture.lease_id)
        .unwrap();
    fixture
        .journal
        .record_cleanup_receipt(&fixture.lease_id)
        .unwrap();
    drop(fixture.journal);

    let journal = SandboxLeaseJournal::open(&fixture.root.join(JOURNAL_FILE)).unwrap();
    assert!(journal.pending_cleanup().unwrap().is_empty());
}

#[test]
fn second_writer_and_torn_journal_fail_closed() {
    let fixture = setup();
    assert!(SandboxLeaseJournal::open(&fixture.root.join(JOURNAL_FILE)).is_err());
    drop(fixture.journal);
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(fixture.root.join(JOURNAL_FILE))
        .unwrap();
    file.write_all(b"{partial").unwrap();
    file.sync_all().unwrap();
    assert!(SandboxLeaseJournal::open(&fixture.root.join(JOURNAL_FILE)).is_err());
}

#[test]
fn hash_chain_tampering_is_rejected() {
    let fixture = setup();
    drop(fixture.journal);
    let path = fixture.root.join(JOURNAL_FILE);
    let contents = fs::read_to_string(&path).unwrap();
    let mut lines = contents.lines();
    let first = lines.next().unwrap();
    let mut envelope: serde_json::Value = serde_json::from_str(first).unwrap();
    envelope["digest"] = serde_json::Value::String(format!("sha256:{}", "0".repeat(64)));
    let mut modified = serde_json::to_string(&envelope).unwrap();
    modified.push('\n');
    for line in lines {
        modified.push_str(line);
        modified.push('\n');
    }
    fs::write(&path, modified).unwrap();
    assert!(SandboxLeaseJournal::open(&path).is_err());
}

#[test]
fn appcontainer_identity_and_cleanup_directory_are_bound_to_lease() {
    let fixture = setup();
    assert!(fixture
        .journal
        .record_appcontainer_removed(&fixture.lease_id, "other-job", true)
        .is_err());
    assert!(fixture
        .journal
        .record_job_directory_removed(&fixture.lease_id)
        .is_err());
}

#[test]
fn active_leases_cannot_share_job_directory_path_or_identity() {
    let fixture = setup();
    let same_path = SandboxLeaseBinding::new(
        "job-2",
        p::canonical_digest(&"plan-2").unwrap(),
        "forme-job-2",
        &fixture.job_directory,
        SandboxFileIdentity::new(2, [2; 16]).unwrap(),
    )
    .unwrap();
    assert!(fixture.journal.begin_lease(same_path).is_err());

    let other_directory = fixture.root.join("job-2");
    fs::create_dir(&other_directory).unwrap();
    let same_identity = SandboxLeaseBinding::new(
        "job-2",
        p::canonical_digest(&"plan-2").unwrap(),
        "forme-job-2",
        &other_directory,
        SandboxFileIdentity::new(1, [1; 16]).unwrap(),
    )
    .unwrap();
    assert!(fixture.journal.begin_lease(same_identity).is_err());
    assert_eq!(fixture.journal.pending_cleanup().unwrap().len(), 1);
}

#[test]
fn writeback_evidence_is_bound_to_terminal_job_and_replays_without_duplication() {
    let fixture = setup();
    let proof = super::SandboxWritebackReceipt {
        schema_version: p::SchemaVersion(1),
        job_id: "job-1".into(),
        plan_digest: p::canonical_digest(&"plan").unwrap(),
        intent_digest: p::canonical_digest(&"intent").unwrap(),
        baseline_digest: p::canonical_digest(&"baseline").unwrap(),
        result_digest: p::canonical_digest(&"result").unwrap(),
    };
    let path = fixture.root.join(JOURNAL_FILE);
    let before = fs::metadata(&path).unwrap().len();
    assert!(fixture
        .journal
        .record_writeback_receipt(&fixture.lease_id, proof.clone())
        .is_err());
    assert_eq!(fs::metadata(&path).unwrap().len(), before);
    fixture
        .journal
        .record_process_tree_terminated(&fixture.lease_id, termination())
        .unwrap();
    let mut foreign = proof.clone();
    foreign.job_id = "job-2".into();
    assert!(fixture
        .journal
        .record_writeback_receipt(&fixture.lease_id, foreign)
        .is_err());
    fixture
        .journal
        .record_writeback_receipt(&fixture.lease_id, proof.clone())
        .unwrap();
    let saved = fs::metadata(&path).unwrap().len();
    fixture
        .journal
        .record_writeback_receipt(&fixture.lease_id, proof.clone())
        .unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), saved);
    let mut conflicting = proof.clone();
    conflicting.result_digest = p::canonical_digest(&"other-result").unwrap();
    assert!(fixture
        .journal
        .record_writeback_receipt(&fixture.lease_id, conflicting)
        .is_err());
    drop(fixture.journal);
    let journal = SandboxLeaseJournal::open(&path).unwrap();
    assert_eq!(
        journal.writeback_receipt(&fixture.lease_id).unwrap(),
        Some(proof)
    );
    assert_eq!(fs::metadata(&path).unwrap().len(), saved);
}
