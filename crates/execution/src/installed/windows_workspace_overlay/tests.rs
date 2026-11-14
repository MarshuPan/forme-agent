#![forbid(unsafe_code)]

use std::fs;

use super::super::windows_appcontainer_support::recovery_timestamp;
use super::*;

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    run: PathBuf,
    job: p::WorkerJob,
    permit: p::WorkerPermit,
    execution: p::WorkerExecutionSpec,
    now: p::Timestamp,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("original");
        let run = temporary.path().join("run");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&run).unwrap();
        fs::write(root.join("existing"), b"before").unwrap();
        let root = fs::canonicalize(root).unwrap();
        let run = fs::canonicalize(run).unwrap();
        let now = recovery_timestamp();
        let intent = p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId("overlay-action".into()),
            source: p::Source::Internal,
            goal: p::GoalRef("overlay-goal".into()),
            backend_hint: p::BackendKind::Shell,
            capability_ref: p::CapabilityRef("shell:overlay".into()),
            action_type: p::ActionType::Execute,
            scope: p::Scope("workspace:overlay".into()),
            risk_hint: p::Risk::Low,
            expected_effect: p::ExpectedEffect::Internal,
            rollback_expectation: p::RollbackBoundary("no-blind-rollback".into()),
            parameters: p::ActionParameters::Shell {
                program: "registered-fixture".into(),
                args: vec![],
                cwd: Some(wire_path(&root)),
                network: false,
            },
            requested_permissions: vec![],
            requested_at: now,
            estimated_output_bytes: 4096,
            estimated_duration: p::DurationMs(10_000),
        };
        let job = p::WorkerJob {
            schema_version: p::SchemaVersion(1),
            job_id: "overlay-job".into(),
            owner_ref: p::VerifiedPrincipal("owner:overlay".into()),
            action_ref: intent.intent_id.clone(),
            affected_resources: p::affected_resources_for(&intent),
            execution_plan_ref: "overlay-plan".into(),
            plan_digest: p::PlanDigest(p::canonical_digest(&intent).unwrap().0),
            authority_epoch: p::AuthorityEpoch(1),
            permit_ref: "overlay-permit".into(),
            sandbox_profile_ref: "sandbox:overlay".into(),
            workspace_scope: intent.scope.clone(),
            deadline: now + 60_000,
            nonce: p::Nonce("overlay-nonce".into()),
            cancel_ref: "overlay-cancel".into(),
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
            issued_at: now,
        };
        let execution = p::WorkerExecutionSpec {
            schema_version: p::SchemaVersion(1),
            plan_digest: job.plan_digest.clone(),
            intent: intent.clone(),
            backend: intent.backend_hint,
            scope: intent.scope.clone(),
            affected_resources: job.affected_resources.clone(),
            normalized_action_digest: p::normalized_action_digest(&intent).unwrap(),
            timeout: p::DurationMs(10_000),
            output_budget_bytes: 4096,
            desktop_session: None,
            input_grants: Vec::new(),
        };
        Self {
            _temporary: temporary,
            root,
            run,
            job,
            permit,
            execution,
            now,
        }
    }

    fn file_action(&mut self, operation: p::FileOperation, content: Option<Vec<u8>>) {
        self.execution.intent.backend_hint = p::BackendKind::File;
        self.execution.backend = p::BackendKind::File;
        self.execution.intent.parameters = p::ActionParameters::File {
            operation,
            path: wire_path(&self.root.join("existing")),
            content,
        };
        self.rebind();
    }

    fn rebind(&mut self) {
        self.execution.affected_resources = p::affected_resources_for(&self.execution.intent);
        self.execution.normalized_action_digest =
            p::normalized_action_digest(&self.execution.intent).unwrap();
        self.job.affected_resources = self.execution.affected_resources.clone();
        self.permit.affected_resources = self.job.affected_resources.clone();
    }

    fn prepare(&self) -> PreparedOverlay {
        PreparedOverlay::prepare(
            &self.run,
            &self.root,
            &self.job,
            &self.permit,
            Some(&self.execution),
            self.now,
        )
        .unwrap()
    }
}

fn wire_path(path: &Path) -> String {
    path.to_str()
        .unwrap()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/")
}

#[test]
fn copy_is_private_and_verified_writeback_handles_creates_edits_and_deletes() {
    let f = Fixture::new();
    fs::create_dir(f.root.join("old-dir")).unwrap();
    fs::write(f.root.join("old-dir/old-file"), b"removed").unwrap();
    let overlay = f.prepare();
    let workspace = overlay.workspace();
    fs::write(workspace.join("existing"), b"after").unwrap();
    fs::remove_file(workspace.join("old-dir/old-file")).unwrap();
    fs::remove_dir(workspace.join("old-dir")).unwrap();
    fs::create_dir(workspace.join("new-dir")).unwrap();
    fs::write(workspace.join("new-dir/result"), b"new").unwrap();
    assert_eq!(fs::read(f.root.join("existing")).unwrap(), b"before");
    assert!(!f.root.join("new-dir").exists());
    assert_eq!(
        overlay.complete(&f.job, f.now).unwrap(),
        Completion::Committed
    );
    assert_eq!(fs::read(f.root.join("existing")).unwrap(), b"after");
    assert_eq!(fs::read(f.root.join("new-dir/result")).unwrap(), b"new");
    assert!(!f.root.join("old-dir").exists());
    verify_cleanup_allowed(&f.run).unwrap();
    assert!(overlay.complete(&f.job, f.now).is_err());
}

#[test]
fn owner_changes_reject_the_whole_writeback_without_an_intent() {
    let f = Fixture::new();
    let overlay = f.prepare();
    fs::write(overlay.workspace().join("existing"), b"worker").unwrap();
    fs::write(overlay.workspace().join("new"), b"worker").unwrap();
    fs::write(f.root.join("existing"), b"owner").unwrap();
    assert_eq!(
        overlay.complete(&f.job, f.now).unwrap(),
        Completion::Rejected
    );
    assert_eq!(fs::read(f.root.join("existing")).unwrap(), b"owner");
    assert!(!f.root.join("new").exists());
    assert!(!f.run.join(INTENT).exists());
}

#[test]
fn exact_file_action_rejects_parameter_substitution_and_sibling_changes() {
    let mut f = Fixture::new();
    f.file_action(p::FileOperation::Write, Some(b"approved".to_vec()));
    let overlay = f.prepare();
    fs::write(overlay.workspace().join("existing"), b"unapproved").unwrap();
    assert_eq!(
        overlay.complete(&f.job, f.now).unwrap(),
        Completion::Rejected
    );
    fs::write(overlay.workspace().join("existing"), b"approved").unwrap();
    fs::write(overlay.workspace().join("sibling"), b"unexpected").unwrap();
    assert_eq!(
        overlay.complete(&f.job, f.now).unwrap(),
        Completion::Rejected
    );
    fs::remove_file(overlay.workspace().join("sibling")).unwrap();
    assert_eq!(
        overlay.complete(&f.job, f.now).unwrap(),
        Completion::Committed
    );
    assert_eq!(fs::read(f.root.join("existing")).unwrap(), b"approved");
}

#[test]
fn file_read_cannot_write_and_exact_delete_is_supported() {
    let mut f = Fixture::new();
    f.file_action(p::FileOperation::Read, None);
    let overlay = f.prepare();
    fs::write(overlay.workspace().join("existing"), b"forbidden").unwrap();
    assert_eq!(
        overlay.complete(&f.job, f.now).unwrap(),
        Completion::Rejected
    );
    let mut delete = Fixture::new();
    delete.file_action(p::FileOperation::Delete, None);
    let overlay = delete.prepare();
    fs::remove_file(overlay.workspace().join("existing")).unwrap();
    assert_eq!(
        overlay.complete(&delete.job, delete.now).unwrap(),
        Completion::Committed
    );
    assert!(!delete.root.join("existing").exists());
}

#[test]
fn changed_binding_or_expired_permit_never_writes() {
    let f = Fixture::new();
    let overlay = f.prepare();
    fs::write(overlay.workspace().join("existing"), b"worker").unwrap();
    let mut changed = f.job.clone();
    changed.authority_epoch = p::AuthorityEpoch(2);
    assert_eq!(
        overlay.complete(&changed, f.now).unwrap(),
        Completion::Rejected
    );
    assert_eq!(
        overlay.complete(&f.job, f.job.deadline).unwrap(),
        Completion::Rejected
    );
    assert_eq!(fs::read(f.root.join("existing")).unwrap(), b"before");
    assert!(!f.run.join(INTENT).exists());
}

#[test]
fn original_identity_replacement_is_a_conflict_even_with_identical_content() {
    let f = Fixture::new();
    let overlay = f.prepare();
    fs::write(overlay.workspace().join("existing"), b"worker").unwrap();
    fs::rename(f.root.join("existing"), f.root.join("previous")).unwrap();
    fs::write(f.root.join("existing"), b"before").unwrap();
    fs::remove_file(f.root.join("previous")).unwrap();
    assert_eq!(
        overlay.complete(&f.job, f.now).unwrap(),
        Completion::Rejected
    );
    assert_eq!(fs::read(f.root.join("existing")).unwrap(), b"before");
}

#[test]
fn snapshot_rejects_hardlinks_and_oversized_files() {
    let f = Fixture::new();
    fs::hard_link(
        f.root.join("existing"),
        f._temporary.path().join("outside-link"),
    )
    .unwrap();
    assert!(PreparedOverlay::prepare(
        &f.run,
        &f.root,
        &f.job,
        &f.permit,
        Some(&f.execution),
        f.now
    )
    .is_err());
    let f = Fixture::new();
    File::create(f.root.join("too-large"))
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    assert!(PreparedOverlay::prepare(
        &f.run,
        &f.root,
        &f.job,
        &f.permit,
        Some(&f.execution),
        f.now
    )
    .is_err());
}

#[test]
fn original_handles_block_concurrent_write_and_ancestor_rename() {
    let f = Fixture::new();
    let pinned = Tree::open(&f.root, &Default::default()).unwrap();
    assert!(fs::write(f.root.join("existing"), b"race").is_err());
    assert!(fs::rename(&f.root, f._temporary.path().join("moved")).is_err());
    drop(pinned);
    fs::write(f.root.join("existing"), b"after release").unwrap();
}

#[test]
fn interrupted_writeback_preserves_intent_and_does_not_retry_partial_effects() {
    let f = Fixture::new();
    let overlay = f.prepare();
    fs::write(overlay.workspace().join("existing"), b"worker").unwrap();
    transaction::leave_intent_for_test(&overlay, &f.job, f.now);
    // Simulate an effect after durable intent and before its receipt.
    fs::write(f.root.join("existing"), b"partial").unwrap();
    assert!(verify_cleanup_allowed(&f.run).is_err());
    assert!(overlay.complete(&f.job, f.now).is_err());
    assert_eq!(fs::read(f.root.join("existing")).unwrap(), b"partial");
    assert!(f.run.join(BASELINE).exists());
    assert!(overlay.workspace().join("existing").exists());
}

#[test]
fn malformed_receipt_blocks_cleanup_instead_of_destroying_evidence() {
    let f = Fixture::new();
    let overlay = f.prepare();
    fs::write(overlay.workspace().join("existing"), b"worker").unwrap();
    assert_eq!(
        overlay.complete(&f.job, f.now).unwrap(),
        Completion::Committed
    );
    fs::write(f.run.join(RECEIPT), b"{}").unwrap();
    assert!(verify_cleanup_allowed(&f.run).is_err());
    assert!(f.run.join(INTENT).exists());
}

#[test]
fn missing_spec_network_or_foreign_cwd_cannot_create_a_copy() {
    let mut f = Fixture::new();
    assert!(PreparedOverlay::prepare(&f.run, &f.root, &f.job, &f.permit, None, f.now).is_err());
    if let p::ActionParameters::Shell { network, .. } = &mut f.execution.intent.parameters {
        *network = true;
    }
    f.rebind();
    assert!(PreparedOverlay::prepare(
        &f.run,
        &f.root,
        &f.job,
        &f.permit,
        Some(&f.execution),
        f.now
    )
    .is_err());
    assert!(!f.run.join("workspace").exists());
}
