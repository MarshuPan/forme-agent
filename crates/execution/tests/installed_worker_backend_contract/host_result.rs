//! Real Host admission, AEAD result store and backend completion; only OS effects are fixtures.
use super::*;
use forme_execution::installed::{
    DesktopHelperAdapter, HostAuth, HostCredentials, IsolatedExecutorHost, ProductHostBroker,
    SystemServiceManager, UnavailableWorkerIsolation, WorkerExecutorHost,
};

struct Helper {
    calls: Arc<AtomicUsize>,
    claims_missing_output: bool,
}
impl DesktopHelperAdapter for Helper {
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
            peer_ref: Some("peer:fixture".into()),
            receipt_ref: None,
            reason_ref: None,
            observed_at: at,
        })
    }
    fn execute(
        &self,
        request: &p::DesktopActionRequest,
        _intent: &p::ActionIntent,
    ) -> p::Result<p::DesktopActionReceipt> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(p::DesktopActionReceipt {
            schema_version: p::SchemaVersion(1),
            job_id: request.job_id.clone(),
            permit_ref: request.permit_ref.clone(),
            action_ref: request.binding.action_ref.clone(),
            plan_digest: request.binding.plan_digest.clone(),
            nonce: request.binding.nonce.clone(),
            normalized_action_digest: request.normalized_action_digest.clone(),
            outcome: p::DesktopActionOutcome::Committed,
            result_ref: self
                .claims_missing_output
                .then(|| "result:untransferred".into()),
            observed_at: now(),
        })
    }
}
struct HostTransport {
    host: WorkerExecutorHost<UnavailableWorkerIsolation, Helper>,
    auth: HostAuth,
    lose_dispatch_response: bool,
}
impl WorkerBrokerTransport for HostTransport {
    fn register_desktop_pending(
        &self,
        request: p::DesktopPendingAction,
    ) -> p::Result<p::DesktopPendingAction> {
        self.host
            .register_desktop_pending(&self.auth, request, now())
    }
    fn desktop_pending_consent(&self, pending: &str) -> p::Result<p::DesktopConsentView> {
        self.host
            .desktop_pending_consent(&self.auth, pending, now())
    }
    fn acquire_desktop_grant(
        &self,
        request: p::DesktopGrantRequest,
    ) -> p::Result<p::DesktopGrantView> {
        self.host.acquire_desktop_grant(&self.auth, request, now())
    }
    fn desktop_grant(&self, operation: &str) -> p::Result<p::DesktopGrantView> {
        self.host.desktop_grant(&self.auth, operation, now())
    }
    fn dispatch(&self, request: p::WorkerDispatchRequest) -> p::Result<p::WorkerObservation> {
        let observation = self.host.dispatch_with_execution(
            &self.auth,
            request.job,
            request.permit,
            request.execution,
            now(),
        )?;
        if self.lose_dispatch_response {
            Err(p::Error("fixture lost completed dispatch response".into()))
        } else {
            Ok(observation)
        }
    }
    fn probe(&self, id: &str) -> p::Result<p::WorkerObservation> {
        self.host.probe(&self.auth, id, now())
    }
    fn cancel(&self, id: &str) -> p::Result<p::WorkerCancelReceipt> {
        self.host.cancel(&self.auth, id, now())
    }
    fn result_manifest(&self, id: &str) -> p::Result<p::WorkerResultManifest> {
        self.host.result_manifest(&self.auth, id, now())
    }
    fn read_result(&self, request: p::WorkerResultRead) -> p::Result<p::WorkerResultChunk> {
        self.host.read_result(&self.auth, request, now())
    }
}
#[test]
fn desktop_no_output_completion_flows_through_real_host_while_missing_body_stays_unknown() {
    for (claims_missing_output, lose_dispatch_response) in
        [(false, false), (true, false), (false, true)]
    {
        let root = tempfile::tempdir().unwrap();
        let token = "fixture-agent-token-for-real-desktop-result-store";
        let broker = ProductHostBroker::open_with_service_manager(
            &root.path().join("host.jsonl"),
            "installation:fixture",
            HostCredentials::new(
                "fixture-maintenance-token-for-real-desktop-result-store".into(),
                Some(token.into()),
            )
            .unwrap(),
            p::ProductTarget::WindowsX86_64,
            "0.0.1".into(),
            SystemServiceManager,
        )
        .unwrap();
        let auth = broker.authenticate_agent(token).unwrap();
        let authority = Arc::new(
            DesktopAuthority::open(
                &root.path().join("authority.jsonl"),
                "installation:fixture".into(),
                p::VerifiedPrincipal("owner:worker-backend".into()),
                p::AuthorityEpoch(1),
                Arc::new(Directory),
            )
            .unwrap(),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let host = WorkerExecutorHost::open_with_adapters(
            &root.path().join("worker.jsonl"),
            p::canonical_digest(&token).unwrap(),
            p::AuthorityEpoch(1),
            [p::Scope("workspace:worker-backend".into())]
                .into_iter()
                .collect(),
            UnavailableWorkerIsolation,
            Helper {
                calls: calls.clone(),
                claims_missing_output,
            },
        )
        .unwrap()
        .with_desktop_authority(authority.clone());
        let transport = Arc::new(HostTransport {
            host,
            auth,
            lose_dispatch_response,
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
        let plan = computer_plan(&backend, "action:real-host-result");
        let approval = p::ApprovalId("approval:real-host-result".into());
        let pending = backend
            .register_desktop_pending(
                &plan,
                p::RunId("run:real-host-result".into()),
                approval.clone(),
                p::Version(1),
                p::Version(1),
                now() + 60_000,
            )
            .unwrap();
        let catalog = authority.sessions(now()).unwrap();
        authority
            .consent(
                p::DesktopConsentRequest {
                    schema_version: p::SchemaVersion(1),
                    operation_id: "consent:real-host-result".into(),
                    expected_revision: catalog.catalog_revision,
                    pending_ref: pending.pending_ref,
                    catalog_revision: catalog.catalog_revision,
                    catalog_digest: catalog.catalog_digest,
                    session_ref: catalog.entries[0].session_ref.clone(),
                    expires_at: now() + 30_000,
                },
                now(),
            )
            .unwrap();
        let plan = backend
            .prepare_desktop(plan.with_approval(approval))
            .unwrap();
        let expected = if claims_missing_output {
            ActionStatus::Unknown
        } else {
            ActionStatus::Completed
        };
        for attempt in 0..2 {
            let result = backend
                .execute(plan.clone(), &EventSink::default(), CancelToken::default())
                .unwrap();
            assert_eq!(
                result.status,
                if attempt == 0 && lose_dispatch_response {
                    ActionStatus::Unknown
                } else {
                    expected
                }
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let manifest = transport
            .result_manifest(&format!("worker-job:{}", plan.digest.0))
            .unwrap();
        assert_eq!(
            manifest.status,
            if claims_missing_output {
                p::WorkerResultStatus::Missing
            } else {
                p::WorkerResultStatus::Empty
            }
        );
        if !claims_missing_output {
            assert_eq!(manifest.encoding, p::WorkerResultEncoding::Utf8);
            assert_eq!(manifest.content_digest, Some(p::sha256_content_digest(b"")));
            assert_eq!(manifest.bytes, 0);
            assert!(manifest
                .result_ref
                .as_deref()
                .unwrap()
                .starts_with("desktop-empty:"));
        }
    }
}
