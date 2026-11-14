//! Real authority/journal/AEAD store; the image source and OS catalog are fixtures.
use super::*;
use crate::installed::desktop_output_fixture as fixture;
use crate::installed::{
    DesktopAuthority, DesktopSessionCatalogProvider, HostCredentials, HostManagement,
    ObservedDesktopSession, ProductHostBroker, SystemServiceManager,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct Directory(ObservedDesktopSession);
impl DesktopSessionCatalogProvider for Directory {
    fn sessions(&self) -> p::Result<Vec<ObservedDesktopSession>> {
        Ok(vec![self.0.clone()])
    }
    fn recheck(&self, value: &ObservedDesktopSession) -> p::Result<()> {
        if value == &self.0 {
            Ok(())
        } else {
            Err(p::Error("fixture changed session".into()))
        }
    }
}
struct Helper {
    calls: Arc<AtomicUsize>,
    mode: u8,
    authority: Arc<DesktopAuthority>,
}
impl DesktopHelperAdapter for Helper {
    fn supports_output(&self) -> bool {
        self.mode != 6
    }
    fn observe(
        &self,
        binding: &p::DesktopSessionBinding,
        at: p::Timestamp,
    ) -> p::Result<p::DesktopSessionObservation> {
        Ok(p::DesktopSessionObservation {
            schema_version: p::SchemaVersion(1),
            helper_ref: binding.helper_ref.clone(),
            grant_ref: Some(binding.grant_ref.clone()),
            state: p::DesktopSessionState::Ready,
            liveness: p::DesktopSessionLiveness::Live,
            os_user_ref: Some(binding.os_user_ref.clone()),
            os_session_ref: Some(binding.os_session_ref.clone()),
            session_generation: Some(binding.session_generation),
            platform: binding.platform,
            desktop_ref: Some(binding.desktop_ref.clone()),
            peer_ref: Some("peer:original-fixture".into()),
            receipt_ref: None,
            reason_ref: None,
            observed_at: at,
        })
    }
    fn execute(
        &self,
        _request: &p::DesktopActionRequest,
        _intent: &p::ActionIntent,
    ) -> p::Result<p::DesktopActionReceipt> {
        panic!("screenshot cannot fall back to the metadata-only helper")
    }
    fn execute_with_output(
        &self,
        request: &p::DesktopActionRequest,
        _intent: &p::ActionIntent,
        sink: &mut dyn DesktopOutputSink,
    ) -> p::Result<p::DesktopActionReceipt> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let bytes = if self.mode == 1 {
            b"not-a-png".to_vec()
        } else {
            fixture::png()
        };
        let header = fixture::header(sink.transfer(), &bytes);
        sink.begin(header.clone())?;
        if self.mode == 2 {
            sink.write_chunk(fixture::chunk(&header, &bytes[..4], 0), &bytes[..4])?;
            return Err(p::Error("fixture truncated transfer".into()));
        }
        sink.write_chunk(fixture::chunk(&header, &bytes, 0), &bytes)?;
        let mut finish = fixture::finish(request, &header);
        finish.receipt.observed_at = now();
        if self.mode == 3 {
            finish.receipt.observed_at = request.deadline + 1;
        }
        let receipt = finish.receipt.clone();
        sink.finish(finish)?;
        if self.mode == 4 {
            return Err(p::Error("fixture lost original finish response".into()));
        }
        if self.mode == 5 {
            let consent = self
                .authority
                .consent_operation("consent:host-output", now())
                .unwrap();
            self.authority
                .revoke(
                    p::DesktopRevokeRequest {
                        schema_version: p::SchemaVersion(1),
                        operation_id: "revoke:host-output".into(),
                        expected_revision: self.authority.sessions(now()).unwrap().catalog_revision,
                        consent_ref: consent.consent_ref,
                    },
                    now(),
                )
                .unwrap();
        }
        Ok(receipt)
    }
}
fn now() -> p::Timestamp {
    crate::installed::desktop_helper::now_ms().unwrap()
}
struct Fixture {
    host: WorkerExecutorHost<UnavailableWorkerIsolation, Helper>,
    auth: HostAuth,
    maintenance: HostAuth,
    job: p::WorkerJob,
    permit: p::WorkerPermit,
    execution: p::WorkerExecutionSpec,
    calls: Arc<AtomicUsize>,
    root: tempfile::TempDir,
}
impl Fixture {
    fn new(mode: u8) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (job, permit, request, intent, _) = fixture::task();
        let binding = &request.binding;
        let directory = Arc::new(Directory(ObservedDesktopSession {
            schema_version: p::SchemaVersion(1),
            helper_ref: binding.helper_ref.clone(),
            platform: binding.platform,
            label: "Original output fixture".into(),
            os_user_ref: binding.os_user_ref.clone(),
            os_session_ref: binding.os_session_ref.clone(),
            session_generation: binding.session_generation,
            desktop_ref: binding.desktop_ref.clone(),
            capabilities: binding.capabilities.clone(),
        }));
        let authority = Arc::new(
            DesktopAuthority::open(
                &root.path().join("authority.jsonl"),
                "installation:host-output".into(),
                job.owner_ref.clone(),
                job.authority_epoch,
                directory,
            )
            .unwrap(),
        );
        let token = "fixture-output-agent-credential-original-value";
        let broker = ProductHostBroker::open_with_service_manager(
            &root.path().join("host.jsonl"),
            "installation:host-output",
            HostCredentials::new(
                "fixture-output-host-credential-original-value".into(),
                Some(token.into()),
            )
            .unwrap(),
            p::ProductTarget::WindowsX86_64,
            "0.0.1".into(),
            SystemServiceManager,
        )
        .unwrap();
        let auth = broker.authenticate_agent(token).unwrap();
        let maintenance = broker
            .authenticate("fixture-output-host-credential-original-value")
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let host = WorkerExecutorHost::open_with_adapters(
            &root.path().join("worker.jsonl"),
            p::canonical_digest(&token).unwrap(),
            job.authority_epoch,
            [job.workspace_scope.clone()].into_iter().collect(),
            UnavailableWorkerIsolation,
            Helper {
                calls: calls.clone(),
                mode,
                authority: authority.clone(),
            },
        )
        .unwrap()
        .with_desktop_authority(authority.clone());
        let mut execution = p::WorkerExecutionSpec {
            schema_version: p::SchemaVersion(1),
            plan_digest: job.plan_digest.clone(),
            backend: p::BackendKind::Computer,
            scope: intent.scope.clone(),
            affected_resources: job.affected_resources.clone(),
            normalized_action_digest: request.normalized_action_digest,
            intent,
            timeout: p::DurationMs(30_000),
            output_budget_bytes: 4096,
            desktop_session: None,
            input_grants: Vec::new(),
        };
        let pending = p::DesktopPendingAction {
            schema_version: p::SchemaVersion(1),
            pending_ref: "pending:host-output".into(),
            owner_ref: job.owner_ref.clone(),
            authority_epoch: job.authority_epoch,
            run_ref: p::RunId("run:host-output".into()),
            approval_ref: p::ApprovalId("approval:host-output".into()),
            action_ref: job.action_ref.clone(),
            plan_digest: job.plan_digest.clone(),
            normalized_action_digest: execution.normalized_action_digest.clone(),
            affected_resources: job.affected_resources.clone(),
            capability: p::DesktopCapability::Screenshot,
            policy_version: p::Version(1),
            tool_schema_version: p::Version(1),
            expires_at: job.deadline,
        };
        host.register_desktop_pending(&auth, pending, now())
            .unwrap();
        let catalog = authority.sessions(now()).unwrap();
        let consent = authority
            .consent(
                p::DesktopConsentRequest {
                    schema_version: p::SchemaVersion(1),
                    operation_id: "consent:host-output".into(),
                    expected_revision: catalog.catalog_revision,
                    pending_ref: "pending:host-output".into(),
                    catalog_revision: catalog.catalog_revision,
                    catalog_digest: catalog.catalog_digest,
                    session_ref: catalog.entries[0].session_ref.clone(),
                    expires_at: job.deadline,
                },
                now(),
            )
            .unwrap();
        let grant = host
            .acquire_desktop_grant(
                &auth,
                p::DesktopGrantRequest {
                    schema_version: p::SchemaVersion(1),
                    operation_id: "issue:host-output".into(),
                    consent_ref: consent.consent_ref,
                    job: job.clone(),
                    permit: permit.clone(),
                    execution: execution.clone(),
                },
                now(),
            )
            .unwrap();
        execution.desktop_session = Some(grant.binding);
        Self {
            host,
            auth,
            maintenance,
            job,
            permit,
            execution,
            calls,
            root,
        }
    }
    fn dispatch(&self) -> p::Result<p::WorkerObservation> {
        self.host.dispatch_with_execution(
            &self.auth,
            self.job.clone(),
            self.permit.clone(),
            Some(self.execution.clone()),
            now(),
        )
    }
}
#[test]
fn original_png_is_encrypted_and_retrievable_once_after_real_host_completion() {
    let f = Fixture::new(0);
    assert_eq!(f.dispatch().unwrap().state, p::WorkerState::Completed);
    assert_eq!(f.dispatch().unwrap().state, p::WorkerState::Completed);
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    assert!(f
        .host
        .desktop_output(&f.maintenance, &f.job.job_id, now())
        .is_err());
    assert!(f
        .host
        .desktop_output(&f.auth, "job:foreign", now())
        .is_err());
    let manifest = f
        .host
        .result_manifest(&f.auth, &f.job.job_id, now())
        .unwrap();
    let descriptor = f
        .host
        .desktop_output(&f.auth, &f.job.job_id, now())
        .unwrap();
    descriptor.validate_for(&manifest).unwrap();
    let chunk = f
        .host
        .read_result(
            &f.auth,
            p::WorkerResultRead {
                schema_version: p::SchemaVersion(1),
                binding: manifest.binding.clone(),
                manifest_digest: manifest.manifest_digest.clone(),
                offset: 0,
                max_bytes: 4096,
            },
            now(),
        )
        .unwrap();
    assert_eq!(chunk.bytes, fixture::png());
    assert_eq!(
        descriptor.content_digest,
        p::sha256_content_digest(&chunk.bytes)
    );
    let root = f.root.path().join("worker.results");
    assert_eq!(
        std::fs::read_dir(root.join("desktop-pending"))
            .unwrap()
            .count(),
        0
    );
    let data = root.join(format!("data-{}.bin", &manifest.manifest_digest.0[7..]));
    let encrypted = std::fs::read(data).unwrap();
    assert!(!encrypted
        .windows(chunk.bytes.len())
        .any(|window| window == chunk.bytes));
}
#[test]
fn corrupt_truncated_late_lost_or_revoked_output_stays_unknown_without_second_capture() {
    for mode in 1..=5 {
        let f = Fixture::new(mode);
        assert!(f.dispatch().is_err(), "mode {mode}");
        assert_eq!(
            f.host.probe(&f.auth, &f.job.job_id, now()).unwrap().state,
            p::WorkerState::Unknown
        );
        assert_eq!(f.dispatch().unwrap().state, p::WorkerState::Unknown);
        assert_eq!(f.calls.load(Ordering::SeqCst), 1);
        assert!(f
            .host
            .desktop_output(&f.auth, &f.job.job_id, now())
            .is_err());
        assert!(
            std::fs::read_dir(f.root.path().join("worker.results/desktop-pending"))
                .unwrap()
                .count()
                >= 2
        );
    }
}
#[test]
fn absent_output_support_fails_before_job_start_or_capture() {
    let f = Fixture::new(6);
    assert!(f.dispatch().is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(f
        .host
        .journal
        .existing_observation(&f.job.job_id, now())
        .unwrap()
        .is_none());
    assert!(!f
        .root
        .path()
        .join("worker.results/desktop-pending")
        .exists());
}
