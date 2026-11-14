use forme_protocol as p;

fn request(now: p::Timestamp) -> p::WorkerDispatchRequest {
    let plan = p::PlanDigest(p::canonical_digest(&"worker-plan").unwrap().0);
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:worker".into()),
        source: p::Source::Internal,
        goal: p::GoalRef("goal:worker".into()),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("shell:worker".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:one".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: "worker".into(),
            args: Vec::new(),
            cwd: None,
            network: false,
        },
        requested_permissions: Vec::new(),
        requested_at: now - 1,
        estimated_output_bytes: 1_024,
        estimated_duration: p::DurationMs(1_000),
    };
    let resources = p::affected_resources_for(&intent);
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: "job:worker".into(),
        owner_ref: p::VerifiedPrincipal("owner:worker".into()),
        action_ref: intent.intent_id.clone(),
        affected_resources: resources.clone(),
        execution_plan_ref: "plan:worker".into(),
        plan_digest: plan.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: "permit:worker".into(),
        sandbox_profile_ref: "sandbox:worker".into(),
        workspace_scope: p::Scope("workspace:one".into()),
        deadline: now + 30_000,
        nonce: p::Nonce("nonce:worker".into()),
        cancel_ref: "cancel:worker".into(),
    };
    let permit = p::WorkerPermit {
        schema_version: p::SchemaVersion(1),
        permit_ref: job.permit_ref.clone(),
        job_id: job.job_id.clone(),
        owner_ref: job.owner_ref.clone(),
        action_ref: job.action_ref.clone(),
        affected_resources: resources.clone(),
        plan_digest: plan.clone(),
        authority_epoch: job.authority_epoch,
        workspace_scope: job.workspace_scope.clone(),
        sandbox_profile_ref: job.sandbox_profile_ref.clone(),
        deadline: job.deadline,
        nonce: job.nonce.clone(),
        issued_at: now - 1,
    };
    let execution = p::WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: plan,
        intent: intent.clone(),
        backend: p::BackendKind::Shell,
        scope: job.workspace_scope.clone(),
        affected_resources: resources,
        normalized_action_digest: p::normalized_action_digest(&intent).unwrap(),
        timeout: p::DurationMs(5_000),
        output_budget_bytes: 4 * 1024,
        desktop_session: None,
        input_grants: Vec::new(),
    };
    p::WorkerDispatchRequest {
        schema_version: p::SchemaVersion(1),
        job,
        permit,
        execution: Some(execution),
    }
}

#[test]
fn typed_execution_spec_is_bound_to_job_and_permit() {
    let valid = request(10_000);
    valid.validate_at(10_001).unwrap();

    let mut tampered = valid.clone();
    tampered.execution.as_mut().unwrap().backend = p::BackendKind::File;
    assert!(tampered.validate_at(10_001).is_err());

    let mut mismatched_permit = valid;
    mismatched_permit.permit.plan_digest =
        p::PlanDigest(p::canonical_digest(&"different-plan").unwrap().0);
    assert!(mismatched_permit.validate_at(10_001).is_err());
}

#[test]
fn typed_execution_spec_rejects_expiry_and_unbounded_payload() {
    let valid = request(10_000);
    assert!(valid.validate_at(40_000).is_err());

    let mut oversized = request(10_000);
    let intent = oversized.execution.as_mut().unwrap();
    if let p::ActionParameters::Shell { args, .. } = &mut intent.intent.parameters {
        args.push("x".repeat(1_100_000));
    }
    intent.normalized_action_digest = p::normalized_action_digest(&intent.intent).unwrap();
    assert!(oversized.validate_at(10_001).is_err());
}

#[test]
fn dispatch_wire_accepts_legacy_request_without_optional_execution() {
    let request = request(10_000);
    let mut value = serde_json::to_value(request).unwrap();
    value.as_object_mut().unwrap().remove("execution");
    let decoded: p::WorkerDispatchRequest = serde_json::from_value(value).unwrap();
    assert!(decoded.execution.is_none());
}

#[test]
fn transport_delay_preserves_the_fixed_timeout_and_never_extends_the_deadline() {
    let mut request = request(10_000);
    request.permit.issued_at = 10_000;
    request.job.deadline = 15_000;
    request.permit.deadline = 15_000;
    let original = request.clone();
    request.validate_at(10_025).unwrap();
    request.validate_at(14_999).unwrap();
    assert_eq!(request, original);
    assert!(request.validate_at(15_000).is_err());
    request.execution.as_mut().unwrap().timeout = p::DurationMs(5_001);
    assert!(request.validate_at(10_025).is_err());
}

#[test]
fn legacy_worker_wire_digests_remain_exact() {
    let req = request(10_000);
    assert_eq!(
        p::sha256_content_digest(&serde_json::to_vec(&req.job).unwrap()).0,
        "sha256:888f5b0126e1d9f3580e807daf31a64f7229dfd157a193c324e6fca2cd288ed7"
    );
    assert_eq!(
        p::sha256_content_digest(&serde_json::to_vec(&req.permit).unwrap()).0,
        "sha256:255007e89edfdb4d840ca25f06269686e34dbc0401ff7407bfef3dbe29a87117"
    );
    assert_eq!(
        p::sha256_content_digest(&serde_json::to_vec(&req.execution.as_ref().unwrap()).unwrap()).0,
        "sha256:099a982cf81b54ba0324b9c207989cdd03ddb149697b970fde1e4c2772785c3d"
    );
}
