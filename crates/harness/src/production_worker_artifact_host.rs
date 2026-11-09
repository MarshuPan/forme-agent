//! Real Host, authority, result store and core driver; only the OS source is a fixture.
use super::*;
use forme_execution::{
    installed::*, ActionBackend, CancelToken, EventSink, ExecutionPlan, OutputBudget,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

struct Sessions;
impl DesktopSessionCatalogProvider for Sessions {
    fn sessions(&self) -> p::Result<Vec<ObservedDesktopSession>> {
        Ok(vec![ObservedDesktopSession {
            schema_version: p::SchemaVersion(1),
            helper_ref: "helper:worker-material".into(),
            platform: p::DesktopPlatform::Windows,
            label: "Self-authored image source".into(),
            os_user_ref: "user:worker-material".into(),
            os_session_ref: "os-session:worker-material".into(),
            session_generation: 1,
            desktop_ref: "desktop:worker-material".into(),
            capabilities: [p::DesktopCapability::Screenshot].into_iter().collect(),
        }])
    }
    fn recheck(&self, value: &ObservedDesktopSession) -> p::Result<()> {
        if self.sessions()?.contains(value) {
            Ok(())
        } else {
            Err(p::Error("fixture session changed".into()))
        }
    }
}
struct Helper(Arc<AtomicUsize>);
impl DesktopHelperAdapter for Helper {
    fn supports_output(&self) -> bool {
        true
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
            peer_ref: Some("peer:worker-material".into()),
            receipt_ref: None,
            reason_ref: None,
            observed_at: at,
        })
    }
    fn execute(
        &self,
        _: &p::DesktopActionRequest,
        _: &p::ActionIntent,
    ) -> p::Result<p::DesktopActionReceipt> {
        panic!("screenshot must use the original-byte output exchange")
    }
    fn execute_with_output(
        &self,
        request: &p::DesktopActionRequest,
        _: &p::ActionIntent,
        sink: &mut dyn DesktopOutputSink,
    ) -> p::Result<p::DesktopActionReceipt> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let bytes = png();
        let transfer = sink.transfer().clone();
        let header = p::DesktopOutputHeader {
            schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
            binding: transfer.binding.clone(),
            transfer_ref: transfer.transfer_ref.clone(),
            media_type: "image/png".into(),
            bytes: bytes.len() as u64,
            content_digest: p::sha256_content_digest(&bytes),
            width: 2,
            height: 1,
        };
        sink.begin(header.clone())?;
        // Exercise multiple raw chunks, including a non-UTF8 prefix split.
        let mut offset = 0;
        for part in bytes.chunks(19) {
            let next = offset + part.len() as u64;
            sink.write_chunk(
                p::DesktopOutputChunkHeader {
                    schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
                    binding_digest: transfer.binding.digest()?,
                    transfer_ref: transfer.transfer_ref.clone(),
                    offset,
                    bytes: part.len() as u32,
                    chunk_digest: p::sha256_content_digest(part),
                    next_offset: next,
                    finished: next == header.bytes,
                },
                part,
            )?;
            offset = next;
        }
        let receipt = p::DesktopActionReceipt {
            schema_version: p::SchemaVersion(1),
            job_id: request.job_id.clone(),
            permit_ref: request.permit_ref.clone(),
            action_ref: request.binding.action_ref.clone(),
            plan_digest: request.binding.plan_digest.clone(),
            nonce: request.binding.nonce.clone(),
            normalized_action_digest: request.normalized_action_digest.clone(),
            outcome: p::DesktopActionOutcome::Committed,
            result_ref: Some(transfer.transfer_ref.clone()),
            observed_at: now(),
        };
        sink.finish(p::DesktopOutputFinish {
            schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
            binding_digest: transfer.binding.digest()?,
            transfer_ref: transfer.transfer_ref,
            header_digest: header.digest()?,
            bytes: header.bytes,
            content_digest: header.content_digest,
            receipt: receipt.clone(),
        })?;
        Ok(receipt)
    }
}
pub(super) struct Transport {
    host: WorkerExecutorHost<UnavailableWorkerIsolation, Helper>,
    auth: HostAuth,
    pub mode: AtomicUsize,
    pub dispatches: AtomicUsize,
    pub captures: Arc<AtomicUsize>,
    job: Mutex<Option<String>>,
}
impl WorkerBrokerTransport for Transport {
    fn register_desktop_pending(
        &self,
        r: p::DesktopPendingAction,
    ) -> p::Result<p::DesktopPendingAction> {
        self.host.register_desktop_pending(&self.auth, r, now())
    }
    fn desktop_pending_consent(&self, id: &str) -> p::Result<p::DesktopConsentView> {
        self.host.desktop_pending_consent(&self.auth, id, now())
    }
    fn acquire_desktop_grant(&self, r: p::DesktopGrantRequest) -> p::Result<p::DesktopGrantView> {
        self.host.acquire_desktop_grant(&self.auth, r, now())
    }
    fn desktop_grant(&self, id: &str) -> p::Result<p::DesktopGrantView> {
        self.host.desktop_grant(&self.auth, id, now())
    }
    fn dispatch(&self, r: p::WorkerDispatchRequest) -> p::Result<p::WorkerObservation> {
        self.dispatches.fetch_add(1, Ordering::SeqCst);
        *self.job.lock().unwrap() = Some(r.job.job_id.clone());
        let result =
            self.host
                .dispatch_with_execution(&self.auth, r.job, r.permit, r.execution, now())?;
        if self.mode.load(Ordering::SeqCst) == 1 {
            Err(p::Error("fixture lost completed response".into()))
        } else {
            Ok(result)
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
    fn read_result(&self, r: p::WorkerResultRead) -> p::Result<p::WorkerResultChunk> {
        let mut chunk = self.host.read_result(&self.auth, r, now())?;
        if self.mode.load(Ordering::SeqCst) == 5 {
            chunk.bytes[0] ^= 1;
        }
        Ok(chunk)
    }
    fn desktop_output(&self, id: &str) -> p::Result<p::DesktopOutputDescriptor> {
        let mut d = self.host.desktop_output(&self.auth, id, now())?;
        match self.mode.load(Ordering::SeqCst) {
            2 => d.binding.grant_ref = "grant:substituted".into(),
            3 => d.width = 3,
            4 => d.binding.deadline = 1,
            _ => (),
        }
        Ok(d)
    }
}
impl Transport {
    pub fn original(&self) -> p::WorkerArtifactBinding {
        let id = self.job.lock().unwrap().clone().unwrap();
        p::WorkerArtifactBinding {
            schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
            manifest: self.host.result_manifest(&self.auth, &id, now()).unwrap(),
            descriptor: self.host.desktop_output(&self.auth, &id, now()).unwrap(),
        }
    }
}
pub(super) struct HostFixture {
    pub backend: WorkerActionBackend,
    pub transport: Arc<Transport>,
    authority: Arc<DesktopAuthority>,
    _root: Arc<Directory>,
}
impl HostFixture {
    pub fn new(f: &Fixture, artifacts: Option<Arc<dyn ArtifactStore>>) -> Self {
        let token = "original-test-worker-material-agent-credential";
        let broker = ProductHostBroker::open_with_service_manager(
            &f.root.0.join("host.jsonl"),
            "installation:worker-material",
            HostCredentials::new(
                "original-test-worker-material-maintenance-credential".into(),
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
                &f.root.0.join("desktop.jsonl"),
                "installation:worker-material".into(),
                owner(),
                p::AuthorityEpoch(1),
                Arc::new(Sessions),
            )
            .unwrap(),
        );
        let captures = Arc::new(AtomicUsize::new(0));
        let host = WorkerExecutorHost::open_with_adapters(
            &f.root.0.join("worker.jsonl"),
            p::canonical_digest(&token).unwrap(),
            p::AuthorityEpoch(1),
            [scope()].into_iter().collect(),
            UnavailableWorkerIsolation,
            Helper(captures.clone()),
        )
        .unwrap()
        .with_desktop_authority(authority.clone());
        let transport = Arc::new(Transport {
            host,
            auth,
            mode: AtomicUsize::new(0),
            dispatches: AtomicUsize::new(0),
            captures,
            job: Mutex::new(None),
        });
        let backend = WorkerActionBackend::new(
            p::BackendKind::Computer,
            transport.clone(),
            owner(),
            p::AuthorityEpoch(1),
            "sandbox:worker-material",
            OutputBudget::strict(4096),
            p::DurationMs(30_000),
        )
        .unwrap();
        let backend = match artifacts {
            Some(sink) => backend.with_artifact_store(sink),
            None => backend,
        };
        Self {
            backend,
            transport,
            authority,
            _root: f.root.clone(),
        }
    }
    pub fn prepare(&self) -> ExecutionPlan {
        let plan = self.backend.plan(&intent()).unwrap();
        let approval = p::ApprovalId("approval:worker-material".into());
        let pending = self
            .backend
            .register_desktop_pending(
                &plan,
                p::RunId("run:worker-material".into()),
                approval.clone(),
                p::Version(1),
                p::Version(1),
                now() + 60_000,
            )
            .unwrap();
        let catalog = self.authority.sessions(now()).unwrap();
        self.authority
            .consent(
                p::DesktopConsentRequest {
                    schema_version: p::SchemaVersion(1),
                    operation_id: "consent:worker-material".into(),
                    expected_revision: catalog.catalog_revision,
                    pending_ref: pending.pending_ref,
                    catalog_revision: catalog.catalog_revision,
                    catalog_digest: catalog.catalog_digest,
                    session_ref: catalog.entries[0].session_ref.clone(),
                    expires_at: pending.expires_at,
                },
                now(),
            )
            .unwrap();
        self.backend
            .prepare_desktop(plan.with_approval(approval))
            .unwrap()
    }
    pub fn execute(&self, plan: &ExecutionPlan, sink: &EventSink) -> forme_execution::ActionResult {
        self.backend
            .execute(plan.clone(), sink, CancelToken::default())
            .unwrap()
    }
}
