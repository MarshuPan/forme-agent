use forme_protocol as p;

pub fn parts() -> (
    p::WorkerJob,
    p::WorkerPermit,
    p::DesktopActionRequest,
    p::ActionIntent,
) {
    let now = 1_800_000_000_000;
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:output".into()),
        source: p::Source::Internal,
        goal: p::GoalRef("goal:output".into()),
        backend_hint: p::BackendKind::Computer,
        capability_ref: p::CapabilityRef("computer:fixture".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:output".into()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        requested_permissions: vec![],
        requested_at: now,
        estimated_output_bytes: 4096,
        estimated_duration: p::DurationMs(1000),
        parameters: p::ActionParameters::Computer(p::ComputerActionSpec {
            schema_version: p::SchemaVersion(1),
            driver: p::ProviderId("driver:output".into()),
            surface: p::SurfaceRef("surface:fixture".into()),
            bounds: p::CoordinateBounds {
                schema_version: p::SchemaVersion(1),
                min_x: 0,
                min_y: 0,
                max_x_exclusive: 2,
                max_y_exclusive: 1,
            },
            operation: p::ComputerOperation::Screenshot,
            artifact_scope: p::Scope("workspace:output".into()),
        }),
    };
    let plan = p::PlanDigest(p::canonical_digest(&"output-plan").unwrap().0);
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: "job:output".into(),
        owner_ref: p::VerifiedPrincipal("owner:output".into()),
        action_ref: intent.intent_id.clone(),
        affected_resources: p::affected_resources_for(&intent),
        execution_plan_ref: "plan:output".into(),
        plan_digest: plan.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: "permit:output".into(),
        sandbox_profile_ref: "sandbox:output".into(),
        workspace_scope: intent.scope.clone(),
        deadline: now + 60_000,
        nonce: p::Nonce("nonce:output".into()),
        cancel_ref: "cancel:output".into(),
    };
    let permit = p::WorkerPermit {
        schema_version: p::SchemaVersion(1),
        permit_ref: job.permit_ref.clone(),
        job_id: job.job_id.clone(),
        owner_ref: job.owner_ref.clone(),
        action_ref: job.action_ref.clone(),
        affected_resources: job.affected_resources.clone(),
        plan_digest: plan.clone(),
        authority_epoch: job.authority_epoch,
        workspace_scope: job.workspace_scope.clone(),
        sandbox_profile_ref: job.sandbox_profile_ref.clone(),
        deadline: job.deadline,
        nonce: job.nonce.clone(),
        issued_at: now,
    };
    let request = p::DesktopActionRequest {
        schema_version: p::SchemaVersion(1),
        job_id: job.job_id.clone(),
        permit_ref: job.permit_ref.clone(),
        binding: p::DesktopSessionBinding {
            schema_version: p::SchemaVersion(1),
            helper_ref: "helper:output".into(),
            grant_ref: "grant:output".into(),
            owner_ref: job.owner_ref.clone(),
            os_user_ref: "user:output".into(),
            os_session_ref: "session:output".into(),
            session_generation: 1,
            platform: p::DesktopPlatform::Windows,
            desktop_ref: "desktop:output".into(),
            action_ref: job.action_ref.clone(),
            plan_digest: plan,
            authority_epoch: job.authority_epoch,
            nonce: job.nonce.clone(),
            capabilities: [p::DesktopCapability::Screenshot].into_iter().collect(),
            issued_at: now,
            expires_at: job.deadline,
        },
        normalized_action_digest: p::normalized_action_digest(&intent).unwrap(),
        capability: p::DesktopCapability::Screenshot,
        deadline: job.deadline,
    };
    (job, permit, request, intent)
}
pub fn request() -> (
    p::DesktopActionRequest,
    p::ActionIntent,
    p::DesktopOutputTransfer,
) {
    let (job, permit, request, intent) = parts();
    let binding = p::DesktopExchangeBinding::from_request(
        "installation:output".into(),
        &job,
        &permit,
        &request,
    )
    .unwrap();
    let transfer = p::DesktopOutputTransfer::new(binding, &intent, 4096).unwrap();
    (request, intent, transfer)
}
pub fn header(transfer: &p::DesktopOutputTransfer, bytes: &[u8]) -> p::DesktopOutputHeader {
    p::DesktopOutputHeader {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        binding: transfer.binding.clone(),
        transfer_ref: transfer.transfer_ref.clone(),
        media_type: "image/png".into(),
        bytes: bytes.len() as u64,
        content_digest: p::sha256_content_digest(bytes),
        width: transfer.width,
        height: transfer.height,
    }
}
pub fn chunk(
    header: &p::DesktopOutputHeader,
    bytes: &[u8],
    offset: u64,
) -> p::DesktopOutputChunkHeader {
    let next_offset = offset + bytes.len() as u64;
    p::DesktopOutputChunkHeader {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        binding_digest: header.binding.digest().unwrap(),
        transfer_ref: header.transfer_ref.clone(),
        offset,
        bytes: bytes.len() as u32,
        chunk_digest: p::sha256_content_digest(bytes),
        next_offset,
        finished: next_offset == header.bytes,
    }
}
pub fn finish(
    request: &p::DesktopActionRequest,
    header: &p::DesktopOutputHeader,
) -> p::DesktopOutputFinish {
    p::DesktopOutputFinish {
        schema_version: p::DESKTOP_OUTPUT_SCHEMA_VERSION,
        binding_digest: header.binding.digest().unwrap(),
        transfer_ref: header.transfer_ref.clone(),
        header_digest: header.digest().unwrap(),
        bytes: header.bytes,
        content_digest: header.content_digest.clone(),
        receipt: p::DesktopActionReceipt {
            schema_version: p::SchemaVersion(1),
            job_id: request.job_id.clone(),
            permit_ref: request.permit_ref.clone(),
            action_ref: request.binding.action_ref.clone(),
            plan_digest: request.binding.plan_digest.clone(),
            nonce: request.binding.nonce.clone(),
            normalized_action_digest: request.normalized_action_digest.clone(),
            outcome: p::DesktopActionOutcome::Committed,
            result_ref: Some(header.transfer_ref.clone()),
            observed_at: request.binding.issued_at,
        },
    }
}
