use super::*;
use forme_execution::installed::{WorkerActionBackend, WorkerBrokerTransport};
use forme_execution::{ActionBackend, CancelToken, EventSink, OutputBudget};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Clone)]
pub(super) struct Original {
    pub request: p::WorkerDispatchRequest,
    pub observation: p::WorkerObservation,
    pub manifest: p::WorkerResultManifest,
    pub bytes: Vec<u8>,
}
#[derive(Default)]
pub(super) struct Transport {
    original: Mutex<Option<Original>>,
    pub calls: AtomicUsize,
}
impl Transport {
    pub fn original(&self) -> Original {
        self.original.lock().unwrap().clone().unwrap()
    }
}
impl WorkerBrokerTransport for Transport {
    fn dispatch(&self, request: p::WorkerDispatchRequest) -> p::Result<p::WorkerObservation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let execution = request.execution.as_ref().unwrap();
        let p::ActionParameters::McpDiscovery(spec) = &execution.intent.parameters else {
            panic!()
        };
        let mut catalog = p::ManagedMcpDiscoveryCatalogV1 {
            schema_version: p::SchemaVersion(1),
            spec: spec.clone(),
            tools: vec![p::ManagedMcpDiscoveryToolV1 {
                schema_version: p::SchemaVersion(1),
                name: "fixture.read".into(),
                description: "untrusted fixture description".into(),
                input_schema: json!({"type":"object"}),
                schema_digest: p::canonical_digest(&json!({"type":"object"})).unwrap(),
            }],
            resources: vec![],
            catalog_digest: p::SchemaDigest(String::new()),
        };
        catalog.catalog_digest = catalog.computed_digest().unwrap();
        let bytes = serde_json::to_vec(&catalog).unwrap();
        let observation = p::WorkerObservation {
            schema_version: p::SchemaVersion(1),
            job_id: request.job.job_id.clone(),
            plan_digest: request.job.plan_digest.clone(),
            authority_epoch: request.job.authority_epoch,
            nonce: request.job.nonce.clone(),
            state: p::WorkerState::Completed,
            outcome: Some(p::WorkerOutcome::Completed),
            receipt_ref: Some("receipt:mcp-original".into()),
            safe_result_ref: Some("result:mcp-original".into()),
            ground_truth_refs: vec!["evidence:mcp-original".into()],
            observed_at: now(),
            reason_ref: None,
        };
        let mut manifest = p::WorkerResultManifest {
            schema_version: p::SchemaVersion(1),
            manifest_digest: p::SchemaDigest(String::new()),
            binding: p::WorkerResultBinding {
                schema_version: p::SchemaVersion(1),
                installation_ref: "installation:mcp-original".into(),
                job_id: request.job.job_id.clone(),
                owner_ref: request.job.owner_ref.clone(),
                action_ref: request.job.action_ref.clone(),
                workspace_scope: request.job.workspace_scope.clone(),
                plan_digest: request.job.plan_digest.clone(),
                normalized_action_digest: Some(execution.normalized_action_digest.clone()),
                permit_ref: request.permit.permit_ref.clone(),
                permit_digest: p::canonical_digest(&request.permit).unwrap(),
                authority_epoch: request.job.authority_epoch,
                nonce: request.job.nonce.clone(),
                receipt_ref: observation.receipt_ref.clone(),
                output_budget_bytes: execution.output_budget_bytes,
            },
            result_ref: observation.safe_result_ref.clone(),
            status: p::WorkerResultStatus::Available,
            outcome: observation.outcome,
            bytes: bytes.len() as u64,
            content_digest: Some(p::sha256_content_digest(&bytes)),
            encoding: p::WorkerResultEncoding::Utf8,
            truncated: false,
            produced_at: observation.observed_at,
            evidence_refs: observation.ground_truth_refs.clone(),
        };
        manifest.manifest_digest = manifest.expected_digest().unwrap();
        *self.original.lock().unwrap() = Some(Original {
            request,
            observation: observation.clone(),
            manifest,
            bytes,
        });
        Ok(observation)
    }
    fn probe(&self, job: &str) -> p::Result<p::WorkerObservation> {
        let original = self.original();
        assert_eq!(job, original.request.job.job_id);
        Ok(original.observation)
    }
    fn cancel(&self, _: &str) -> p::Result<p::WorkerCancelReceipt> {
        Err(p::Error("fixture has completed".into()))
    }
    fn result_manifest(&self, job: &str) -> p::Result<p::WorkerResultManifest> {
        let original = self.original();
        assert_eq!(job, original.request.job.job_id);
        Ok(original.manifest)
    }
    fn read_result(&self, request: p::WorkerResultRead) -> p::Result<p::WorkerResultChunk> {
        let original = self.original();
        assert_eq!(request.manifest_digest, original.manifest.manifest_digest);
        assert_eq!(request.binding, original.manifest.binding);
        let next = (request.offset + request.max_bytes).min(original.bytes.len() as u64);
        let bytes = original.bytes[request.offset as usize..next as usize].to_vec();
        Ok(p::WorkerResultChunk {
            schema_version: p::SchemaVersion(1),
            manifest_digest: request.manifest_digest,
            offset: request.offset,
            next_offset: next,
            bytes: bytes.clone(),
            chunk_digest: p::sha256_content_digest(&bytes),
            finished: next == original.bytes.len() as u64,
        })
    }
}
pub(super) fn intent() -> p::ActionIntent {
    let mut intent = crate::production_artifact_store::worker_fixture::intent();
    intent.backend_hint = p::BackendKind::Mcp;
    intent.source = p::Source::OwnerControl;
    intent.expected_effect = p::ExpectedEffect::Outward;
    intent.capability_ref = p::CapabilityRef("mcp-discovery:fixture-material".into());
    intent.parameters = p::ActionParameters::McpDiscovery(p::ManagedMcpDiscoverySpec {
        schema_version: p::SchemaVersion(1),
        server: p::McpServerRef("fixture-material".into()),
        program_ref: "program:fixture-material".into(),
        registration_digest: p::canonical_digest(&"registration").unwrap(),
        settings_digest: p::canonical_digest(&"settings").unwrap(),
        timeout: p::DurationMs(1000),
        output_budget_bytes: 8192,
    });
    intent
}
pub(super) fn backend(
    consumer: Arc<dyn ManagedMcpDiscoveryConsumer>,
) -> (WorkerActionBackend, Arc<Transport>) {
    let transport = Arc::new(Transport::default());
    let worker = WorkerActionBackend::new(
        p::BackendKind::Mcp,
        transport.clone(),
        owner(),
        p::AuthorityEpoch(1),
        p::MANAGED_MCP_PROGRAM_PROFILE,
        OutputBudget::strict(8192),
        p::DurationMs(1000),
    )
    .unwrap()
    .with_discovery_consumer(consumer);
    (worker, transport)
}
struct Capture;
impl ManagedMcpDiscoveryConsumer for Capture {
    fn persist_discovery(
        &self,
        _: &p::WorkerDispatchRequest,
        _: &p::WorkerObservation,
        _: &p::WorkerResultManifest,
        _: &[u8],
    ) -> p::Result<ArtifactRecord> {
        Err(p::Error(
            "fixture intentionally interrupted before Core persistence".into(),
        ))
    }
}
pub(super) fn original() -> Original {
    let (worker, transport) = backend(Arc::new(Capture));
    let plan = worker.plan(&intent()).unwrap();
    assert_eq!(
        worker
            .execute(plan, &EventSink::default(), CancelToken::default())
            .unwrap()
            .status,
        forme_execution::ActionStatus::Unknown
    );
    transport.original()
}
pub(super) fn save(f: &Fixture, original: &Original) -> p::Result<ArtifactRecord> {
    f.sink.persist_discovery(
        &original.request,
        &original.observation,
        &original.manifest,
        &original.bytes,
    )
}
pub(super) fn id(original: &Original) -> p::ContentRef {
    content_ref(&managed_mcp_material_object(&original.request, &original.manifest).unwrap())
}
