use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Directory {
    live: AtomicBool,
    reads: AtomicUsize,
}
impl Directory {
    fn new() -> Self {
        Self {
            live: AtomicBool::new(true),
            reads: AtomicUsize::new(0),
        }
    }
    fn entry() -> ObservedDesktopSession {
        ObservedDesktopSession {
            schema_version: p::SchemaVersion(1),
            helper_ref: "helper:desktop".into(),
            platform: p::DesktopPlatform::Windows,
            label: "Fixture desktop".into(),
            os_user_ref: "os-user:fixture".into(),
            os_session_ref: "os-session:7".into(),
            session_generation: 4,
            desktop_ref: "desktop:windows-default".into(),
            capabilities: [p::DesktopCapability::PointerClick].into_iter().collect(),
        }
    }
}
impl DesktopSessionCatalogProvider for Directory {
    fn sessions(&self) -> p::Result<Vec<ObservedDesktopSession>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(if self.live.load(Ordering::SeqCst) {
            vec![Self::entry()]
        } else {
            vec![]
        })
    }
    fn recheck(&self, original: &ObservedDesktopSession) -> p::Result<()> {
        if !self.live.load(Ordering::SeqCst) || *original != Self::entry() {
            return Err(invalid());
        }
        Ok(())
    }
}
struct Fixture {
    root: std::path::PathBuf,
    provider: Arc<Directory>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "forme-desktop-authority-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        Self {
            root,
            provider: Arc::new(Directory::new()),
        }
    }
    fn path(&self) -> std::path::PathBuf {
        self.root.join("desktop.jsonl")
    }
    fn open(&self) -> DesktopAuthority {
        DesktopAuthority::open(
            &self.path(),
            "installation:fixture".into(),
            p::VerifiedPrincipal("owner:worker".into()),
            p::AuthorityEpoch(1),
            self.provider.clone(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn make_job(deadline: p::Timestamp) -> (p::WorkerJob, p::WorkerPermit) {
    let plan = p::PlanDigest(p::canonical_digest(&"worker-plan").unwrap().0);
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: "job-1".into(),
        owner_ref: p::VerifiedPrincipal("owner:worker".into()),
        action_ref: p::ActionId("action-1".into()),
        affected_resources: vec![p::ResourceRef("workspace:one".into())],
        execution_plan_ref: "plan-ref-1".into(),
        plan_digest: plan.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: "permit-1".into(),
        sandbox_profile_ref: "sandbox:strict".into(),
        workspace_scope: p::Scope("workspace:one".into()),
        deadline,
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
        plan_digest: plan,
        authority_epoch: job.authority_epoch,
        workspace_scope: job.workspace_scope.clone(),
        sandbox_profile_ref: job.sandbox_profile_ref.clone(),
        deadline,
        nonce: job.nonce.clone(),
        issued_at: 1,
    };
    (job, permit)
}

fn desktop_execution(
    job: &mut p::WorkerJob,
    permit: &mut p::WorkerPermit,
) -> p::WorkerExecutionSpec {
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: job.action_ref.clone(),
        source: p::Source::Internal,
        goal: p::GoalRef("goal:desktop".into()),
        backend_hint: p::BackendKind::Computer,
        capability_ref: p::CapabilityRef("computer:desktop".into()),
        action_type: p::ActionType::Execute,
        scope: job.workspace_scope.clone(),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Computer(p::ComputerActionSpec {
            schema_version: p::SchemaVersion(1),
            driver: p::ProviderId("driver:desktop-fixture".into()),
            surface: p::SurfaceRef("surface:desktop".into()),
            bounds: p::CoordinateBounds {
                schema_version: p::SchemaVersion(1),
                min_x: 0,
                min_y: 0,
                max_x_exclusive: 800,
                max_y_exclusive: 600,
            },
            operation: p::ComputerOperation::Click {
                x: 20,
                y: 30,
                button: p::PointerButton::Primary,
            },
            artifact_scope: job.workspace_scope.clone(),
        }),
        requested_permissions: Vec::new(),
        requested_at: 2,
        estimated_output_bytes: 128,
        estimated_duration: p::DurationMs(50),
    };
    let resources = p::affected_resources_for(&intent);
    job.affected_resources = resources.clone();
    permit.affected_resources = resources.clone();
    p::WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: job.plan_digest.clone(),
        intent: intent.clone(),
        backend: p::BackendKind::Computer,
        scope: job.workspace_scope.clone(),
        affected_resources: resources,
        normalized_action_digest: p::normalized_action_digest(&intent).unwrap(),
        timeout: p::DurationMs(100),
        output_budget_bytes: 4 * 1024,
        desktop_session: Some(p::DesktopSessionBinding {
            schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
            helper_ref: "helper:desktop".into(),
            grant_ref: "grant:desktop".into(),
            owner_ref: job.owner_ref.clone(),
            os_user_ref: "os-user:desktop".into(),
            os_session_ref: "os-session:desktop".into(),
            session_generation: 4,
            platform: p::DesktopPlatform::Windows,
            desktop_ref: "desktop:primary".into(),
            action_ref: job.action_ref.clone(),
            plan_digest: job.plan_digest.clone(),
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            capabilities: [p::DesktopCapability::PointerClick].into_iter().collect(),
            issued_at: 1,
            expires_at: 900,
        }),
    }
}

fn request() -> p::DesktopGrantRequest {
    let (mut job, mut permit) = make_job(10_000);
    let mut execution = desktop_execution(&mut job, &mut permit);
    execution.desktop_session = None;
    p::DesktopGrantRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: "issue:one".into(),
        consent_ref: "consent:unset".into(),
        job,
        permit,
        execution,
    }
}
fn pending(request: &p::DesktopGrantRequest) -> p::DesktopPendingAction {
    p::DesktopPendingAction {
        schema_version: p::SchemaVersion(1),
        pending_ref: "pending:one".into(),
        owner_ref: request.job.owner_ref.clone(),
        authority_epoch: request.job.authority_epoch,
        run_ref: p::RunId("run:one".into()),
        approval_ref: p::ApprovalId("approval:one".into()),
        action_ref: request.job.action_ref.clone(),
        plan_digest: request.job.plan_digest.clone(),
        normalized_action_digest: request.execution.normalized_action_digest.clone(),
        affected_resources: request.job.affected_resources.clone(),
        capability: p::DesktopCapability::PointerClick,
        policy_version: p::Version(1),
        tool_schema_version: p::Version(1),
        expires_at: 9000,
    }
}
fn consent(
    authority: &DesktopAuthority,
    request: &p::DesktopGrantRequest,
) -> p::DesktopConsentRequest {
    authority.register_pending(pending(request), 10).unwrap();
    let catalog = authority.sessions(20).unwrap();
    p::DesktopConsentRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: "consent:one".into(),
        expected_revision: catalog.catalog_revision,
        pending_ref: "pending:one".into(),
        catalog_revision: catalog.catalog_revision,
        catalog_digest: catalog.catalog_digest,
        session_ref: catalog.entries[0].session_ref.clone(),
        expires_at: 8000,
    }
}
fn prepared(authority: &DesktopAuthority) -> (p::DesktopGrantRequest, p::DesktopConsentView) {
    let mut request = request();
    let consent = consent(authority, &request);
    let view = authority.consent(consent, 30).unwrap();
    request.consent_ref = view.consent_ref.clone();
    (request, view)
}
#[test]
fn original_consent_and_grant_survive_restart_and_consume_exactly_once() {
    let f = Fixture::new();
    let authority = f.open();
    let (request, consent) = prepared(&authority);
    let grant = authority.issue(request.clone(), 40).unwrap();
    assert_eq!(grant.status, p::DesktopGrantStatus::Issued);
    assert_eq!(authority.issue(request.clone(), 50).unwrap(), grant);
    let length = std::fs::metadata(f.path()).unwrap().len();
    drop(authority);
    let authority = f.open();
    assert_eq!(
        authority.consent_operation("consent:one", 50).unwrap(),
        consent
    );
    assert_eq!(authority.grant_operation("issue:one", 50).unwrap(), grant);
    assert_eq!(std::fs::metadata(f.path()).unwrap().len(), length);
    let mut execution = request.execution.clone();
    execution.desktop_session = Some(grant.binding.clone());
    let calls = AtomicUsize::new(0);
    authority
        .consume(&request.job, &request.permit, &execution, 60, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    assert!(authority
        .consume(&request.job, &request.permit, &execution, 61, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .is_err());
    drop(authority);
    let authority = f.open();
    assert_eq!(
        authority.grant_operation("issue:one", 70).unwrap().status,
        p::DesktopGrantStatus::Consumed
    );
    assert!(authority
        .consume(&request.job, &request.permit, &execution, 71, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn consumed_intent_with_no_worker_receipt_cannot_reenter_after_a_crash() {
    let f = Fixture::new();
    let authority = f.open();
    let (request, _) = prepared(&authority);
    let grant = authority.issue(request.clone(), 40).unwrap();
    let mut execution = request.execution.clone();
    execution.desktop_session = Some(grant.binding);
    let calls = AtomicUsize::new(0);
    assert!(authority
        .consume::<()>(&request.job, &request.permit, &execution, 50, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(invalid())
        })
        .is_err());
    drop(authority);
    let authority = f.open();
    assert!(authority
        .consume(&request.job, &request.permit, &execution, 60, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .is_err());
    let mut changed = request.clone();
    changed.operation_id = "issue:replacement".into();
    assert!(authority.issue(changed, 60).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
#[test]
fn foreign_or_changed_pending_catalog_and_consent_have_zero_new_facts() {
    let f = Fixture::new();
    let authority = f.open();
    let request = request();
    let consent = consent(&authority, &request);
    let length = std::fs::metadata(f.path()).unwrap().len();
    for index in 0..7 {
        let mut bad = consent.clone();
        match index {
            0 => bad.expected_revision += 1,
            1 => bad.catalog_digest = p::canonical_digest(&"other").unwrap(),
            2 => bad.session_ref = "os-session:7".into(),
            3 => bad.pending_ref = "pending:other".into(),
            4 => bad.expires_at = 9100,
            5 => bad.schema_version = p::SchemaVersion(2),
            _ => bad.catalog_revision += 1,
        }
        assert!(authority.consent(bad, 30).is_err());
        assert_eq!(std::fs::metadata(f.path()).unwrap().len(), length);
    }
    let mut foreign = pending(&request);
    foreign.pending_ref = "pending:foreign".into();
    foreign.owner_ref = p::VerifiedPrincipal("owner:foreign".into());
    assert!(authority.register_pending(foreign, 30).is_err());
    let mut changed = pending(&request);
    changed.policy_version = p::Version(2);
    assert!(authority.register_pending(changed, 30).is_err());
    assert!(authority.consent(consent.clone(), 30_020).is_err());
    drop(authority);
    let authority = f.open();
    assert!(authority.consent(consent, 30).is_err());
    assert_eq!(std::fs::metadata(f.path()).unwrap().len(), length);
}
#[test]
fn grant_cannot_substitute_action_parameters_nonce_owner_or_prior_binding() {
    let f = Fixture::new();
    let authority = f.open();
    let (request, _) = prepared(&authority);
    let grant = authority.issue(request.clone(), 40).unwrap();
    let length = std::fs::metadata(f.path()).unwrap().len();
    for index in 0..6 {
        let mut bad = request.clone();
        match index {
            0 => bad.job.nonce = p::Nonce("nonce:other".into()),
            1 => bad.execution.output_budget_bytes += 1,
            2 => bad.job.owner_ref = p::VerifiedPrincipal("owner:foreign".into()),
            3 => bad.execution.desktop_session = Some(grant.binding.clone()),
            4 => bad.execution.normalized_action_digest = p::canonical_digest(&"other").unwrap(),
            _ => bad.operation_id = "issue:other".into(),
        }
        assert!(authority.issue(bad, 50).is_err());
        assert_eq!(std::fs::metadata(f.path()).unwrap().len(), length);
    }
    let mut execution = request.execution.clone();
    let mut forged = grant.binding.clone();
    forged.session_generation += 1;
    execution.desktop_session = Some(forged);
    assert!(authority
        .consume::<()>(&request.job, &request.permit, &execution, 50, || panic!(
            "forged grant admitted"
        ))
        .is_err());
    assert_eq!(std::fs::metadata(f.path()).unwrap().len(), length);
}
#[test]
fn revocation_and_session_loss_stop_new_admission_without_clearing_original_grant() {
    let f = Fixture::new();
    let authority = f.open();
    let (request, consent) = prepared(&authority);
    f.provider.live.store(false, Ordering::SeqCst);
    assert!(authority.issue(request.clone(), 40).is_err());
    f.provider.live.store(true, Ordering::SeqCst);
    let grant = authority.issue(request.clone(), 40).unwrap();
    let mut execution = request.execution.clone();
    execution.desktop_session = Some(grant.binding.clone());
    f.provider.live.store(false, Ordering::SeqCst);
    assert!(authority
        .consume::<()>(&request.job, &request.permit, &execution, 50, || panic!(
            "locked desktop admitted"
        ))
        .is_err());
    f.provider.live.store(true, Ordering::SeqCst);
    let revoke = p::DesktopRevokeRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: "revoke:one".into(),
        expected_revision: grant.revision,
        consent_ref: consent.consent_ref,
    };
    let view = authority.revoke(revoke.clone(), 60).unwrap();
    assert_eq!(view.status, p::DesktopConsentStatus::Revoked);
    assert_eq!(authority.revoke(revoke, 70).unwrap(), view);
    assert!(authority
        .consume::<()>(&request.job, &request.permit, &execution, 70, || panic!(
            "revoked grant admitted"
        ))
        .is_err());
    drop(authority);
    let authority = f.open();
    assert_eq!(
        authority.grant_operation("issue:one", 80).unwrap().status,
        p::DesktopGrantStatus::Revoked
    );
}
#[test]
fn journal_has_one_writer_and_refuses_truncation_identity_and_hash_changes() {
    let f = Fixture::new();
    let authority = f.open();
    let (_request, _) = prepared(&authority);
    assert!(DesktopAuthority::open(
        &f.path(),
        "installation:fixture".into(),
        p::VerifiedPrincipal("owner:worker".into()),
        p::AuthorityEpoch(1),
        f.provider.clone()
    )
    .is_err());
    drop(authority);
    let bytes = std::fs::read(f.path()).unwrap();
    assert!(DesktopAuthority::open(
        &f.path(),
        "installation:other".into(),
        p::VerifiedPrincipal("owner:worker".into()),
        p::AuthorityEpoch(1),
        f.provider.clone()
    )
    .is_err());
    for changed in [
        bytes[..bytes.len() - 1].to_vec(),
        String::from_utf8(bytes.clone())
            .unwrap()
            .replace("owner:worker", "owner:forged")
            .into_bytes(),
    ] {
        std::fs::write(f.path(), &changed).unwrap();
        assert!(DesktopAuthority::open(
            &f.path(),
            "installation:fixture".into(),
            p::VerifiedPrincipal("owner:worker".into()),
            p::AuthorityEpoch(1),
            f.provider.clone()
        )
        .is_err());
        assert_eq!(std::fs::read(f.path()).unwrap(), changed);
    }
}
#[test]
fn closed_requests_reject_os_identity_fields_and_new_schema_before_authority_writes() {
    let f = Fixture::new();
    let authority = f.open();
    let request = request();
    let consent = consent(&authority, &request);
    let mut wire = serde_json::to_value(consent).unwrap();
    wire["os_user_ref"] = "os-user:forged".into();
    assert!(serde_json::from_value::<p::DesktopConsentRequest>(wire).is_err());
    let mut bad = request.clone();
    bad.schema_version = p::SchemaVersion(2);
    assert!(bad.validate_at(30).is_err());
    let mut catalog = authority.sessions(20).unwrap();
    catalog.entries[0].label.push('x');
    assert!(catalog.validate_at(21).is_err());
    let mut bad_pending = pending(&request);
    bad_pending
        .affected_resources
        .push(bad_pending.affected_resources[0].clone());
    assert!(bad_pending.validate().is_err());
}

#[test]
fn refresh_recovers_original_consent_and_prepared_reads_never_write_or_issue() {
    let f = Fixture::new();
    let authority = f.open();
    let mut pending = pending(&request());
    pending.pending_ref =
        p::desktop_pending_ref(&pending.approval_ref, &pending.plan_digest).unwrap();
    let reference = pending.pending_ref.clone();
    authority.register_pending(pending, 10).unwrap();
    let length = std::fs::metadata(f.path()).unwrap().len();
    let original = authority.consent_operation(&reference, 20).unwrap();
    assert_eq!(original.status, p::DesktopConsentStatus::Prepared);
    assert_eq!(authority.pending_consent(&reference, 21).unwrap(), original);
    assert_eq!(std::fs::metadata(f.path()).unwrap().len(), length);
    assert_eq!(f.provider.reads.load(Ordering::SeqCst), 0);
    assert!(authority
        .consent_operation("desktop-pending:unknown", 20)
        .is_err());
    drop(authority);
    let authority = f.open();
    assert_eq!(authority.pending_consent(&reference, 22).unwrap(), original);
    let catalog = authority.sessions(30).unwrap();
    let request = p::DesktopConsentRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: original.operation_id.clone(),
        expected_revision: catalog.catalog_revision,
        pending_ref: reference.clone(),
        catalog_revision: catalog.catalog_revision,
        catalog_digest: catalog.catalog_digest,
        session_ref: catalog.entries[0].session_ref.clone(),
        expires_at: 8000,
    };
    let committed = authority.consent(request.clone(), 40).unwrap();
    drop(authority); // Simulate losing the successful POST response, then reloading.
    let authority = f.open();
    let length = std::fs::metadata(f.path()).unwrap().len();
    assert_eq!(
        authority.consent_operation(&reference, 50).unwrap(),
        committed
    );
    assert_eq!(
        authority
            .consent_operation(&original.operation_id, 50)
            .unwrap(),
        committed
    );
    assert_eq!(authority.consent(request.clone(), 50).unwrap(), committed);
    let mut replacement = request;
    replacement.operation_id = "consent:refresh-replacement".into();
    assert!(authority.consent(replacement, 50).is_err());
    assert_eq!(
        authority
            .consent_operation(&reference, 9500)
            .unwrap()
            .status,
        p::DesktopConsentStatus::Expired
    );
    assert_eq!(std::fs::metadata(f.path()).unwrap().len(), length);
    assert_eq!(f.provider.reads.load(Ordering::SeqCst), 1);
}
