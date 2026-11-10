use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forme_execution::installed::{WorkerActionBackend, WorkerBrokerTransport};
use forme_execution::{
    ActionBackend, ActionStatus, BackendKind, CancelToken, EventSink, OutputBudget,
};
use forme_protocol as p;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureMode {
    None,
    Dispatch,
    LostDispatchResponse,
    Delayed,
    Manifest,
    ResultRef,
    ReceiptRef,
    OverBudget,
    Unknown,
    Pending,
    Empty,
    SplitUtf8,
    InvalidUtf8,
    BinaryNonUtf8,
    BinaryUtf8,
    BinaryEmpty,
    ChunkDigest,
    ChunkManifest,
    ChunkOffset,
    ChunkOverflow,
    EarlyEnd,
    LateEnd,
    EmptyChunk,
    TooManyChunks,
    LateResult,
}

#[derive(Default)]
struct FakeState {
    dispatches: usize,
    probes: usize,
    reads: usize,
    cancelled_jobs: Vec<String>,
    request: Option<p::WorkerDispatchRequest>,
    manifest: Option<p::WorkerResultManifest>,
}

struct FakeTransport {
    mode: FailureMode,
    state: Mutex<FakeState>,
    cancel_on_dispatch: Mutex<Option<CancelToken>>,
}

impl FakeTransport {
    fn new(mode: FailureMode) -> Self {
        Self {
            mode,
            state: Mutex::new(FakeState::default()),
            cancel_on_dispatch: Mutex::new(None),
        }
    }

    fn snapshot(&self) -> (usize, usize, Vec<String>) {
        let state = self.state.lock().unwrap();
        (state.dispatches, state.probes, state.cancelled_jobs.clone())
    }
}

impl WorkerBrokerTransport for FakeTransport {
    fn dispatch(&self, request: p::WorkerDispatchRequest) -> p::Result<p::WorkerObservation> {
        if self.mode == FailureMode::Delayed {
            std::thread::sleep(Duration::from_millis(25));
        }
        request.validate_at(now())?;
        let mut manifest = result_manifest(&request, payload(self.mode))?;
        if matches!(
            self.mode,
            FailureMode::BinaryNonUtf8 | FailureMode::BinaryUtf8 | FailureMode::BinaryEmpty
        ) {
            manifest.encoding = p::WorkerResultEncoding::Binary;
            manifest.manifest_digest = manifest.expected_digest()?;
        }
        let mut state = self.state.lock().unwrap();
        state.dispatches += 1;
        state.request = Some(request.clone());
        state.manifest = Some(manifest);
        if let Some(token) = self.cancel_on_dispatch.lock().unwrap().as_ref() {
            token.cancel();
        }
        if matches!(
            self.mode,
            FailureMode::Dispatch | FailureMode::LostDispatchResponse
        ) {
            return Err(p::Error("dispatch response unavailable after send".into()));
        }
        Ok(self.observation(&request.job))
    }

    fn probe(&self, job_id: &str) -> p::Result<p::WorkerObservation> {
        let mut state = self.state.lock().unwrap();
        state.probes += 1;
        if self.mode == FailureMode::Dispatch {
            return Err(p::Error("original job receipt unavailable".into()));
        }
        let request = state
            .request
            .as_ref()
            .filter(|request| request.job.job_id == job_id)
            .cloned()
            .ok_or_else(|| p::Error("fake job is unknown".into()))?;
        Ok(self.observation(&request.job))
    }

    fn cancel(&self, job_id: &str) -> p::Result<p::WorkerCancelReceipt> {
        let mut state = self.state.lock().unwrap();
        let request = state
            .request
            .as_ref()
            .filter(|request| request.job.job_id == job_id)
            .cloned()
            .ok_or_else(|| p::Error("fake job is unknown".into()))?;
        state.cancelled_jobs.push(job_id.to_owned());
        Ok(p::WorkerCancelReceipt {
            schema_version: p::SchemaVersion(1),
            job_id: request.job.job_id,
            plan_digest: request.job.plan_digest,
            nonce: request.job.nonce,
            outcome: p::WorkerOutcome::Cancelled,
            observed_at: now(),
            receipt_ref: Some("receipt:cancel".into()),
            reason_ref: None,
        })
    }

    fn result_manifest(&self, job_id: &str) -> p::Result<p::WorkerResultManifest> {
        let state = self.state.lock().unwrap();
        let request = state
            .request
            .as_ref()
            .filter(|request| request.job.job_id == job_id)
            .ok_or_else(|| p::Error("fake job is unknown".into()))?;
        let mut manifest = state
            .manifest
            .clone()
            .ok_or_else(|| p::Error("fake result is missing".into()))?;
        if self.mode == FailureMode::Manifest {
            manifest.binding.job_id = "worker-job:tampered".into();
        }
        if self.mode == FailureMode::ResultRef {
            manifest.result_ref = Some("result:foreign".into());
        }
        if self.mode == FailureMode::ReceiptRef {
            manifest.binding.receipt_ref = Some("receipt:foreign".into());
        }
        if self.mode == FailureMode::OverBudget {
            manifest.bytes = manifest.binding.output_budget_bytes + 1;
        }
        // Correct digests cannot excuse binding substitutions.
        manifest.manifest_digest = manifest.expected_digest()?;
        if request.job.job_id != manifest.binding.job_id && self.mode != FailureMode::Manifest {
            return Err(p::Error("fake result binding is inconsistent".into()));
        }
        Ok(manifest)
    }

    fn read_result(&self, request: p::WorkerResultRead) -> p::Result<p::WorkerResultChunk> {
        if self.mode == FailureMode::LateResult {
            std::thread::sleep(Duration::from_millis(1_100));
        }
        let mut state = self.state.lock().unwrap();
        state.reads += 1;
        let manifest = state
            .manifest
            .as_ref()
            .ok_or_else(|| p::Error("fake result is missing".into()))?;
        let payload = payload(self.mode);
        let start = usize::try_from(request.offset).unwrap();
        let end = if matches!(
            self.mode,
            FailureMode::SplitUtf8 | FailureMode::TooManyChunks
        ) {
            (start + 1).min(payload.len())
        } else if self.mode == FailureMode::EarlyEnd {
            1
        } else {
            payload.len()
        };
        let bytes = if self.mode == FailureMode::EmptyChunk {
            Vec::new()
        } else {
            payload[start..end].to_vec()
        };
        let offset = match self.mode {
            FailureMode::ChunkOffset => request.offset.saturating_add(1),
            FailureMode::ChunkOverflow => u64::MAX,
            _ => request.offset,
        };
        let digest = if self.mode == FailureMode::ChunkDigest {
            p::sha256_content_digest(b"tampered")
        } else {
            p::sha256_content_digest(&bytes)
        };
        Ok(p::WorkerResultChunk {
            schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
            manifest_digest: if self.mode == FailureMode::ChunkManifest {
                p::sha256_content_digest(b"foreign manifest")
            } else {
                manifest.manifest_digest.clone()
            },
            offset,
            next_offset: offset.wrapping_add(bytes.len() as u64),
            bytes,
            chunk_digest: digest,
            finished: match self.mode {
                FailureMode::EmptyChunk | FailureMode::LateEnd => false,
                FailureMode::EarlyEnd => true,
                _ => end == payload.len(),
            },
        })
    }
}

impl FakeTransport {
    fn observation(&self, job: &p::WorkerJob) -> p::WorkerObservation {
        let mut observation = terminal_observation(job);
        if self.mode == FailureMode::Pending {
            observation.state = p::WorkerState::Running;
            observation.outcome = None;
            observation.receipt_ref = None;
            observation.safe_result_ref = None;
        } else if self.mode == FailureMode::Unknown {
            observation.state = p::WorkerState::Unknown;
            observation.outcome = Some(p::WorkerOutcome::Unknown);
        }
        observation
    }
}

fn payload(mode: FailureMode) -> &'static [u8] {
    match mode {
        FailureMode::Empty | FailureMode::BinaryEmpty => b"",
        FailureMode::SplitUtf8 => "跨块结果\n".as_bytes(),
        FailureMode::InvalidUtf8 | FailureMode::BinaryNonUtf8 => &[0xff, 0xfe],
        FailureMode::TooManyChunks => &[b'x'; 4097],
        _ => b"worker result\n",
    }
}

fn backend(mode: FailureMode) -> (WorkerActionBackend, Arc<FakeTransport>) {
    let transport = Arc::new(FakeTransport::new(mode));
    let backend = WorkerActionBackend::new(
        BackendKind::Shell,
        transport.clone(),
        p::VerifiedPrincipal("owner:worker-backend".into()),
        p::AuthorityEpoch(1),
        "sandbox:worker-backend",
        OutputBudget::strict(8 * 1024),
        p::DurationMs(1_000),
    )
    .unwrap();
    (backend, transport)
}

fn plan(backend: &WorkerActionBackend, action: &str) -> forme_execution::ExecutionPlan {
    backend
        .plan(&p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId(action.into()),
            source: p::Source::Internal,
            goal: p::GoalRef("verify managed worker backend".into()),
            backend_hint: BackendKind::Shell,
            capability_ref: p::CapabilityRef("capability:worker-backend".into()),
            action_type: p::ActionType::Execute,
            scope: p::Scope("workspace:worker-backend".into()),
            risk_hint: p::Risk::Low,
            expected_effect: p::ExpectedEffect::Internal,
            rollback_expectation: p::RollbackBoundary("none".into()),
            parameters: p::ActionParameters::Shell {
                program: "worker-fixture".into(),
                args: Vec::new(),
                cwd: None,
                network: false,
            },
            requested_permissions: vec![p::PermissionRef("permission:execute".into())],
            requested_at: now(),
            estimated_output_bytes: 128,
            estimated_duration: p::DurationMs(100),
        })
        .unwrap()
}

fn computer_plan(backend: &WorkerActionBackend, action: &str) -> forme_execution::ExecutionPlan {
    backend
        .plan(&p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId(action.into()),
            source: p::Source::Internal,
            goal: p::GoalRef("verify desktop grant forwarding".into()),
            backend_hint: BackendKind::Computer,
            capability_ref: p::CapabilityRef("capability:desktop-forwarding".into()),
            action_type: p::ActionType::Execute,
            scope: p::Scope("workspace:worker-backend".into()),
            risk_hint: p::Risk::Low,
            expected_effect: p::ExpectedEffect::Internal,
            rollback_expectation: p::RollbackBoundary("none".into()),
            parameters: p::ActionParameters::Computer(p::ComputerActionSpec {
                schema_version: p::SchemaVersion(1),
                driver: p::ProviderId("driver:desktop-forwarding".into()),
                surface: p::SurfaceRef("surface:desktop-forwarding".into()),
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
                artifact_scope: p::Scope("workspace:worker-backend".into()),
            }),
            requested_permissions: Vec::new(),
            requested_at: now(),
            estimated_output_bytes: 128,
            estimated_duration: p::DurationMs(100),
        })
        .unwrap()
}

fn desktop_binding(plan: &forme_execution::ExecutionPlan) -> p::DesktopSessionBinding {
    let issued_at = now();
    p::DesktopSessionBinding {
        schema_version: p::DESKTOP_SESSION_SCHEMA_VERSION,
        helper_ref: "helper:desktop-forwarding".into(),
        grant_ref: "grant:desktop-forwarding".into(),
        owner_ref: p::VerifiedPrincipal("owner:worker-backend".into()),
        os_user_ref: "os-user:desktop-forwarding".into(),
        os_session_ref: "os-session:desktop-forwarding".into(),
        session_generation: 1,
        platform: p::DesktopPlatform::Windows,
        desktop_ref: "desktop:primary".into(),
        action_ref: plan.intent.intent_id.clone(),
        plan_digest: plan.digest.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        nonce: p::Nonce(format!("worker-nonce:{}", plan.digest.0)),
        capabilities: [p::DesktopCapability::PointerClick].into_iter().collect(),
        issued_at,
        expires_at: issued_at + 5_000,
    }
}

fn terminal_observation(job: &p::WorkerJob) -> p::WorkerObservation {
    p::WorkerObservation {
        schema_version: p::SchemaVersion(1),
        job_id: job.job_id.clone(),
        plan_digest: job.plan_digest.clone(),
        authority_epoch: job.authority_epoch,
        nonce: job.nonce.clone(),
        state: p::WorkerState::Completed,
        outcome: Some(p::WorkerOutcome::Completed),
        receipt_ref: Some("receipt:worker".into()),
        safe_result_ref: Some("result:worker".into()),
        ground_truth_refs: vec!["evidence:worker".into()],
        observed_at: now(),
        reason_ref: None,
    }
}

fn result_manifest(
    request: &p::WorkerDispatchRequest,
    bytes: &[u8],
) -> p::Result<p::WorkerResultManifest> {
    let execution = request
        .execution
        .as_ref()
        .ok_or_else(|| p::Error("fake dispatch needs an execution spec".into()))?;
    let mut manifest = p::WorkerResultManifest {
        schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
        manifest_digest: p::SchemaDigest(String::new()),
        binding: p::WorkerResultBinding {
            schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
            installation_ref: "install:worker-backend".into(),
            job_id: request.job.job_id.clone(),
            owner_ref: request.job.owner_ref.clone(),
            action_ref: request.job.action_ref.clone(),
            workspace_scope: request.job.workspace_scope.clone(),
            plan_digest: request.job.plan_digest.clone(),
            normalized_action_digest: Some(execution.normalized_action_digest.clone()),
            permit_ref: request.permit.permit_ref.clone(),
            permit_digest: p::canonical_digest(&request.permit)?,
            authority_epoch: request.job.authority_epoch,
            nonce: request.job.nonce.clone(),
            receipt_ref: Some("receipt:worker".into()),
            output_budget_bytes: execution.output_budget_bytes,
        },
        result_ref: Some("result:worker".into()),
        status: if bytes.is_empty() {
            p::WorkerResultStatus::Empty
        } else {
            p::WorkerResultStatus::Available
        },
        outcome: Some(p::WorkerOutcome::Completed),
        bytes: bytes.len() as u64,
        content_digest: Some(p::sha256_content_digest(bytes)),
        encoding: p::WorkerResultEncoding::Utf8,
        truncated: false,
        produced_at: now(),
        evidence_refs: vec!["evidence:worker".into()],
    };
    manifest.manifest_digest = manifest.expected_digest()?;
    Ok(manifest)
}

fn now() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as p::Timestamp
}

fn assert_unknown(sink: &EventSink) {
    let events = sink.events();
    assert!(events
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_))));
    assert!(!events.iter().any(|event| matches!(
        event,
        p::EventPayload::ActionCompleted(_)
            | p::EventPayload::ActionOutputDelta(_)
            | p::EventPayload::ActionCancelled(_)
    )));
    assert!(!events.iter().any(|event| matches!(event,
        p::EventPayload::CapabilityEvidenceRecorded(e) if e.outcome.0 == "success")));
}

#[test]
fn managed_worker_completes_and_reuses_original_job_without_redispatch() {
    let (backend, transport) = backend(FailureMode::None);
    let plan = plan(&backend, "worker-backend-success");
    let sink = EventSink::default();

    let first = backend
        .execute(plan.clone(), &sink, CancelToken::default())
        .unwrap();
    assert_eq!(first.status, ActionStatus::Completed);
    assert_eq!(first.output, "worker result\n");
    assert!(sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionCompleted(_))));
    assert!(!sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_))));

    let second = backend
        .execute(plan, &sink, CancelToken::default())
        .unwrap();
    assert_eq!(second.status, ActionStatus::Completed);
    let (dispatches, probes, cancelled) = transport.snapshot();
    assert_eq!(dispatches, 1);
    assert_eq!(probes, 1);
    assert!(cancelled.is_empty());
}

#[test]
fn dispatch_error_becomes_unknown_and_never_creates_a_replacement_job() {
    let (backend, transport) = backend(FailureMode::Dispatch);
    let plan = plan(&backend, "worker-backend-dispatch-error");
    let sink = EventSink::default();

    let first = backend
        .execute(plan.clone(), &sink, CancelToken::default())
        .unwrap();
    let second = backend
        .execute(plan, &sink, CancelToken::default())
        .unwrap();
    assert_eq!(first.status, ActionStatus::Unknown);
    assert_eq!(second.status, ActionStatus::Unknown);
    let (dispatches, probes, _) = transport.snapshot();
    assert_eq!(dispatches, 1);
    assert_eq!(probes, 1);
    assert_unknown(&sink);
}

#[test]
fn binary_manifests_cannot_become_text_output_or_a_completed_action() {
    for mode in [
        FailureMode::BinaryUtf8,
        FailureMode::BinaryEmpty,
        FailureMode::BinaryNonUtf8,
    ] {
        let (backend, transport) = backend(mode);
        let plan = plan(&backend, &format!("worker-backend-encoding-{mode:?}"));
        for _ in 0..2 {
            let sink = EventSink::default();
            let result = backend
                .execute(plan.clone(), &sink, CancelToken::default())
                .unwrap();
            assert_eq!(result.status, ActionStatus::Unknown, "{mode:?}");
            assert!(result.output.is_empty());
            assert_unknown(&sink);
        }
        let state = transport.state.lock().unwrap();
        let manifest = state.manifest.as_ref().unwrap();
        manifest.validate().unwrap();
        assert_eq!(manifest.encoding, p::WorkerResultEncoding::Binary);
        assert_eq!(state.dispatches, 1);
        assert_eq!(state.probes, 1);
        assert_eq!(state.reads, 0);
        assert!(state.cancelled_jobs.is_empty());
    }
}

#[test]
fn manifest_and_chunk_tampering_are_unknown() {
    for mode in [
        FailureMode::Manifest,
        FailureMode::ResultRef,
        FailureMode::ReceiptRef,
        FailureMode::OverBudget,
        FailureMode::ChunkDigest,
        FailureMode::ChunkManifest,
        FailureMode::ChunkOffset,
        FailureMode::ChunkOverflow,
        FailureMode::EarlyEnd,
        FailureMode::LateEnd,
        FailureMode::EmptyChunk,
        FailureMode::InvalidUtf8,
        FailureMode::BinaryNonUtf8,
        FailureMode::Unknown,
    ] {
        let (backend, _) = backend(mode);
        let sink = EventSink::default();
        let result = backend
            .execute(
                plan(&backend, &format!("worker-backend-{mode:?}")),
                &sink,
                CancelToken::default(),
            )
            .unwrap();
        assert_eq!(result.status, ActionStatus::Unknown, "{mode:?}");
        assert_unknown(&sink);
    }
}

#[test]
fn cancel_targets_the_original_worker_job() {
    let (backend, transport) = backend(FailureMode::None);
    let plan = plan(&backend, "worker-backend-cancel");
    backend
        .execute(plan.clone(), &EventSink::default(), CancelToken::default())
        .unwrap();
    backend.cancel(plan.intent.intent_id).unwrap();

    let (_, _, cancelled) = transport.snapshot();
    assert_eq!(cancelled, vec!["worker-job:".to_owned() + &plan.digest.0]);
}

#[test]
fn transport_delay_empty_results_and_split_utf8_preserve_the_plan() {
    for mode in [
        FailureMode::Delayed,
        FailureMode::Empty,
        FailureMode::SplitUtf8,
    ] {
        let (backend, transport) = backend(mode);
        let plan = plan(&backend, &format!("worker-backend-valid-{mode:?}"));
        let result = backend
            .execute(plan.clone(), &EventSink::default(), CancelToken::default())
            .unwrap();
        assert_eq!(result.status, ActionStatus::Completed);
        assert_eq!(result.output.as_bytes(), payload(mode));
        let state = transport.state.lock().unwrap();
        let request = state.request.as_ref().unwrap();
        let spec = request.execution.as_ref().unwrap();
        assert_eq!(
            request.job.deadline - request.permit.issued_at,
            plan.timeout.0 as i64
        );
        assert_eq!(spec.timeout, plan.timeout);
        assert_eq!(spec.plan_digest, plan.digest);
        if mode == FailureMode::SplitUtf8 {
            assert!(state.reads > 1);
        }
    }
}

#[test]
fn caller_supplied_desktop_binding_cannot_bypass_host_grant_acquisition() {
    let transport = Arc::new(FakeTransport::new(FailureMode::None));
    let backend = WorkerActionBackend::new(
        BackendKind::Computer,
        transport.clone(),
        p::VerifiedPrincipal("owner:worker-backend".into()),
        p::AuthorityEpoch(1),
        "sandbox:worker-backend",
        OutputBudget::strict(8 * 1024),
        p::DurationMs(10_000),
    )
    .unwrap();
    let plan = computer_plan(&backend, "worker-backend-self-issued");
    let binding = desktop_binding(&plan);
    let bound = plan.with_desktop_session(binding).unwrap();
    let sink = EventSink::default();
    assert!(backend
        .execute(bound, &sink, CancelToken::default())
        .is_err());
    assert!(sink.events().is_empty());
    assert_eq!(transport.snapshot().0, 0);
}

#[path = "installed_worker_backend_contract/desktop.rs"]
mod desktop;

#[test]
fn a_lost_dispatch_response_can_only_read_the_original_terminal_receipt() {
    let (backend, transport) = backend(FailureMode::LostDispatchResponse);
    let plan = plan(&backend, "worker-backend-lost-response");
    let first_sink = EventSink::default();
    let first = backend
        .execute(plan.clone(), &first_sink, CancelToken::default())
        .unwrap();
    assert_eq!(first.status, ActionStatus::Unknown);
    assert_unknown(&first_sink);
    let result = backend
        .execute(plan, &EventSink::default(), CancelToken::default())
        .unwrap();
    assert_eq!(result.status, ActionStatus::Completed);
    assert_eq!(transport.snapshot(), (1, 1, vec![]));
}

#[test]
fn token_cancellation_contacts_the_original_running_job() {
    let (backend, transport) = backend(FailureMode::Pending);
    let token = CancelToken::default();
    *transport.cancel_on_dispatch.lock().unwrap() = Some(token.clone());
    let plan = plan(&backend, "worker-backend-token-cancel");
    let sink = EventSink::default();
    let result = backend.execute(plan.clone(), &sink, token).unwrap();
    assert_eq!(result.status, ActionStatus::Cancelled);
    let (dispatches, probes, cancels) = transport.snapshot();
    assert_eq!((dispatches, probes), (1, 0));
    assert_eq!(cancels, vec![format!("worker-job:{}", plan.digest.0)]);
    assert!(sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionCancelled(_))));
    assert!(!sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionCompleted(_))));
}

#[test]
fn invalid_limits_and_changed_plan_never_dispatch() {
    for (bytes, timeout) in [
        (0, 1000),
        (p::WORKER_RESULT_MAX_BYTES + 1, 1000),
        (1024, 0),
        (1024, 600_001),
    ] {
        assert!(WorkerActionBackend::new(
            BackendKind::Shell,
            Arc::new(FakeTransport::new(FailureMode::None)),
            p::VerifiedPrincipal("owner:limits".into()),
            p::AuthorityEpoch(1),
            "sandbox:limits",
            OutputBudget::strict(bytes),
            p::DurationMs(timeout)
        )
        .is_err());
    }
    let (backend, transport) = backend(FailureMode::None);
    let mut plan = plan(&backend, "worker-backend-changed-plan");
    plan.timeout = p::DurationMs(500);
    plan.refresh_digest().unwrap();
    let sink = EventSink::default();
    assert!(backend
        .execute(plan, &sink, CancelToken::default())
        .is_err());
    assert!(sink.events().is_empty());
    assert_eq!(transport.snapshot(), (0, 0, vec![]));
}

#[test]
fn worker_file_mutations_never_claim_a_local_rollback_snapshot() {
    let (shell, transport) = backend(FailureMode::None);
    let worker = WorkerActionBackend::new(
        BackendKind::File,
        transport.clone(),
        p::VerifiedPrincipal("owner:worker-backend".into()),
        p::AuthorityEpoch(1),
        "sandbox:worker-backend",
        OutputBudget::strict(8 * 1024),
        p::DurationMs(1_000),
    )
    .unwrap();
    for operation in [p::FileOperation::Write, p::FileOperation::Delete] {
        let mut intent = plan(&shell, "file-without-rollback").intent;
        intent.backend_hint = BackendKind::File;
        intent.parameters = p::ActionParameters::File {
            operation,
            path: "fixture-project/file.txt".into(),
            content: (operation == p::FileOperation::Write).then(|| "new content".into()),
        };
        let mut planned = worker.plan(&intent).unwrap();
        assert_eq!(planned.rollback_boundary.0, "none");
        planned.validate_digest().unwrap();
        planned.rollback_boundary = p::RollbackBoundary("file-content-snapshot".into());
        planned.refresh_digest().unwrap();
        assert!(worker
            .execute(planned, &EventSink::default(), CancelToken::default())
            .is_err());
    }
    assert_eq!(transport.snapshot().0, 0);
}

#[test]
fn fragmented_or_late_results_stop_without_output_completion_or_redispatch() {
    for (mode, expected_reads) in [
        (FailureMode::TooManyChunks, 4096),
        (FailureMode::LateResult, 1),
    ] {
        let (backend, transport) = backend(mode);
        let sink = EventSink::default();
        let result = backend
            .execute(
                plan(&backend, &format!("worker-backend-bounded-{mode:?}")),
                &sink,
                CancelToken::default(),
            )
            .unwrap();
        assert_eq!(result.status, ActionStatus::Unknown);
        assert_unknown(&sink);
        assert_eq!(transport.snapshot(), (1, 0, vec![]));
        assert_eq!(transport.state.lock().unwrap().reads, expected_reads);
    }
}

#[path = "installed_worker_backend_contract/mcp.rs"]
mod mcp;
