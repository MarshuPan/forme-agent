use super::*;
use forme_execution::installed::{
    DesktopAuthority, DesktopSessionCatalogProvider, ObservedDesktopSession,
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Directory;
impl DesktopSessionCatalogProvider for Directory {
    fn sessions(&self) -> p::Result<Vec<ObservedDesktopSession>> {
        Ok(vec![ObservedDesktopSession {
            schema_version: p::SchemaVersion(1),
            helper_ref: "helper:fixture".into(),
            platform: p::DesktopPlatform::Windows,
            label: "Fixture desktop".into(),
            os_user_ref: "os-user:fixture".into(),
            os_session_ref: "os-session:fixture".into(),
            session_generation: 1,
            desktop_ref: "desktop:fixture".into(),
            capabilities: [p::DesktopCapability::PointerClick].into_iter().collect(),
        }])
    }
    fn recheck(&self, original: &ObservedDesktopSession) -> p::Result<()> {
        if self.sessions()?.contains(original) {
            Ok(())
        } else {
            Err(p::Error("fixture session changed".into()))
        }
    }
}
struct Transport {
    authority: Arc<DesktopAuthority>,
    inner: FakeTransport,
    issues: AtomicUsize,
    reads: AtomicUsize,
    lost_issue: bool,
    unavailable_reads: bool,
}
impl WorkerBrokerTransport for Transport {
    fn register_desktop_pending(
        &self,
        request: p::DesktopPendingAction,
    ) -> p::Result<p::DesktopPendingAction> {
        self.authority.register_pending(request, now())
    }
    fn desktop_pending_consent(&self, pending_ref: &str) -> p::Result<p::DesktopConsentView> {
        self.authority.pending_consent(pending_ref, now())
    }
    fn acquire_desktop_grant(
        &self,
        request: p::DesktopGrantRequest,
    ) -> p::Result<p::DesktopGrantView> {
        self.issues.fetch_add(1, Ordering::SeqCst);
        let view = self.authority.issue(request, now())?;
        if self.lost_issue {
            Err(p::Error("fixture lost grant response".into()))
        } else {
            Ok(view)
        }
    }
    fn desktop_grant(&self, operation: &str) -> p::Result<p::DesktopGrantView> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.unavailable_reads {
            return Err(p::Error("fixture query unavailable".into()));
        }
        self.authority.grant_operation(operation, now())
    }
    fn dispatch(&self, request: p::WorkerDispatchRequest) -> p::Result<p::WorkerObservation> {
        request.validate_at(now())?;
        let view = self.authority.grant_operation(
            &format!("desktop-issue:{}", request.job.plan_digest.0),
            now(),
        )?;
        if view.status != p::DesktopGrantStatus::Issued
            || request
                .execution
                .as_ref()
                .and_then(|s| s.desktop_session.as_ref())
                != Some(&view.binding)
        {
            return Err(p::Error("fixture rejects inactive grant".into()));
        }
        self.inner.dispatch(request)
    }

    fn probe(&self, job_id: &str) -> p::Result<p::WorkerObservation> {
        self.inner.probe(job_id)
    }
    fn cancel(&self, job_id: &str) -> p::Result<p::WorkerCancelReceipt> {
        self.inner.cancel(job_id)
    }
    fn result_manifest(&self, job_id: &str) -> p::Result<p::WorkerResultManifest> {
        self.inner.result_manifest(job_id)
    }
    fn read_result(&self, request: p::WorkerResultRead) -> p::Result<p::WorkerResultChunk> {
        self.inner.read_result(request)
    }
}
struct Fixture {
    backend: WorkerActionBackend,
    transport: Arc<Transport>,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new(lost_issue: bool, unavailable_reads: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "forme-desktop-backend-{}-{}.jsonl",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let authority = Arc::new(
            DesktopAuthority::open(
                &path,
                "installation:backend-fixture".into(),
                p::VerifiedPrincipal("owner:worker-backend".into()),
                p::AuthorityEpoch(1),
                Arc::new(Directory),
            )
            .unwrap(),
        );
        let transport = Arc::new(Transport {
            authority,
            inner: FakeTransport::new(FailureMode::None),
            issues: AtomicUsize::new(0),
            reads: AtomicUsize::new(0),
            lost_issue,
            unavailable_reads,
        });
        let backend = WorkerActionBackend::new(
            BackendKind::Computer,
            transport.clone(),
            p::VerifiedPrincipal("owner:worker-backend".into()),
            p::AuthorityEpoch(1),
            "sandbox:desktop-fixture",
            OutputBudget::strict(8192),
            p::DurationMs(10_000),
        )
        .unwrap();
        Self {
            backend,
            transport,
            path,
        }
    }
    fn pending(&self) -> forme_execution::ExecutionPlan {
        let plan = computer_plan(&self.backend, "action:desktop-preflight");
        self.backend
            .register_desktop_pending(
                &plan,
                p::RunId("run:desktop".into()),
                p::ApprovalId("approval:desktop".into()),
                p::Version(1),
                p::Version(1),
                now() + 60_000,
            )
            .unwrap();
        plan.with_approval(p::ApprovalId("approval:desktop".into()))
    }
    fn consent(&self, plan: &forme_execution::ExecutionPlan) -> p::DesktopConsentView {
        let catalog = self.transport.authority.sessions(now()).unwrap();
        self.transport
            .authority
            .consent(
                p::DesktopConsentRequest {
                    schema_version: p::SchemaVersion(1),
                    operation_id: "consent:backend".into(),
                    expected_revision: catalog.catalog_revision,
                    pending_ref: p::desktop_pending_ref(
                        plan.approval_ref.as_ref().unwrap(),
                        &plan.digest,
                    )
                    .unwrap(),
                    catalog_revision: catalog.catalog_revision,
                    catalog_digest: catalog.catalog_digest,
                    session_ref: catalog.entries[0].session_ref.clone(),
                    expires_at: now() + 30_000,
                },
                now(),
            )
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // The journal's OS lock is intentionally retained until its handles drop.
        let _ = std::fs::remove_file(&self.path);
    }
}
#[test]
fn desktop_preflight_waits_for_real_consent_then_dispatches_once() {
    let f = Fixture::new(false, false);
    let plan = f.pending();
    assert!(f.backend.require_desktop_consent(&plan).is_err());
    assert!(f.backend.prepare_desktop(plan.clone()).is_err());
    assert_eq!(f.transport.issues.load(Ordering::SeqCst), 0);
    f.consent(&plan);
    let bound = f.backend.prepare_desktop(plan.clone()).unwrap();
    assert_eq!(bound.digest, plan.digest);
    assert_eq!(f.transport.inner.snapshot().0, 0);
    let result = f
        .backend
        .execute(bound.clone(), &EventSink::default(), CancelToken::default())
        .unwrap();
    assert_eq!(result.status, ActionStatus::Completed);
    f.backend
        .execute(bound, &EventSink::default(), CancelToken::default())
        .unwrap();
    assert_eq!(f.transport.inner.snapshot(), (1, 1, vec![]));
    assert_eq!(f.transport.issues.load(Ordering::SeqCst), 1);
}
#[test]
fn desktop_lost_issue_response_recovers_only_by_original_query() {
    let f = Fixture::new(true, false);
    let plan = f.pending();
    f.consent(&plan);
    let bound = f.backend.prepare_desktop(plan).unwrap();
    assert_eq!(f.transport.issues.load(Ordering::SeqCst), 1);
    assert_eq!(f.transport.reads.load(Ordering::SeqCst), 1);
    assert_eq!(f.transport.inner.snapshot().0, 0);
    f.backend
        .execute(bound, &EventSink::default(), CancelToken::default())
        .unwrap();
    assert_eq!(f.transport.inner.snapshot().0, 1);
}
#[test]
fn desktop_unknown_issue_never_reissues_or_dispatches() {
    let f = Fixture::new(true, true);
    let plan = f.pending();
    f.consent(&plan);
    for _ in 0..2 {
        assert!(f.backend.prepare_desktop(plan.clone()).is_err());
    }
    assert_eq!(f.transport.issues.load(Ordering::SeqCst), 1);
    assert_eq!(f.transport.reads.load(Ordering::SeqCst), 2);
    assert_eq!(f.transport.inner.snapshot().0, 0);
}
#[test]
fn desktop_revoked_after_issue_cannot_be_admitted() {
    let f = Fixture::new(false, false);
    let plan = f.pending();
    let consent = f.consent(&plan);
    let bound = f.backend.prepare_desktop(plan).unwrap();
    let catalog = f.transport.authority.sessions(now()).unwrap();
    f.transport
        .authority
        .revoke(
            p::DesktopRevokeRequest {
                schema_version: p::SchemaVersion(1),
                operation_id: "revoke:backend".into(),
                expected_revision: catalog.catalog_revision,
                consent_ref: consent.consent_ref,
            },
            now(),
        )
        .unwrap();
    let result = f
        .backend
        .execute(bound.clone(), &EventSink::default(), CancelToken::default())
        .unwrap();
    assert_eq!(result.status, ActionStatus::Unknown);
    f.backend
        .execute(bound, &EventSink::default(), CancelToken::default())
        .unwrap();
    assert_eq!(f.transport.inner.snapshot().0, 0);
    assert_eq!(f.transport.issues.load(Ordering::SeqCst), 1);
}

#[path = "host_result.rs"]
mod host_result;
