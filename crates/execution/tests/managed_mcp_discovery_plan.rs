use forme_execution::{
    ActionBackend, CancelToken, DefaultExecutionPlanner, EventSink, ExecutionPlan,
    ExecutionPlanner, McpBackend, OutputBudget,
};
use forme_protocol as p;

fn planner() -> DefaultExecutionPlanner {
    DefaultExecutionPlanner::new(OutputBudget::strict(8192), p::DurationMs(5000))
}
fn dispatch(plan: &ExecutionPlan) -> p::WorkerDispatchRequest {
    let job = p::WorkerJob {
        schema_version: p::SchemaVersion(1),
        job_id: "job:discover".into(),
        owner_ref: p::VerifiedPrincipal("owner:test".into()),
        action_ref: plan.intent.intent_id.clone(),
        affected_resources: plan.affected_resources.clone(),
        execution_plan_ref: "plan:discover".into(),
        plan_digest: plan.digest.clone(),
        authority_epoch: p::AuthorityEpoch(1),
        permit_ref: "permit:discover".into(),
        sandbox_profile_ref: "sandbox:discovery".into(),
        workspace_scope: plan.scope.clone(),
        deadline: 20_000,
        nonce: p::Nonce("nonce:discovery".into()),
        cancel_ref: "cancel:discovery".into(),
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
        issued_at: 10_000,
    };
    let execution = p::WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: plan.digest.clone(),
        intent: plan.intent.clone(),
        backend: plan.backend,
        scope: plan.scope.clone(),
        affected_resources: plan.affected_resources.clone(),
        normalized_action_digest: plan.normalized_action_digest.clone(),
        timeout: plan.timeout,
        output_budget_bytes: plan.budget.max_bytes,
        desktop_session: None,
    };
    p::WorkerDispatchRequest {
        schema_version: p::SchemaVersion(1),
        job,
        permit,
        execution: Some(execution),
    }
}
#[test]
fn discovery_plan_and_worker_preserve_exact_request_and_resource_identity() {
    let i = intent();
    let plan = planner().plan(&i).unwrap();
    plan.validate_digest().unwrap();
    assert_eq!(plan.intent, i);
    assert_eq!(
        plan.affected_resources,
        vec![
            p::ResourceRef("mcp-discovery:fixture:catalog".into()),
            p::ResourceRef("program-registration:program:fixture".into())
        ]
    );
    let req = dispatch(&plan);
    req.validate_at(10_010).unwrap();
    for n in 0..7 {
        let mut changed = plan.clone();
        let p::ActionParameters::McpDiscovery(spec) = &mut changed.intent.parameters else {
            unreachable!()
        };
        match n {
            0 => spec.schema_version.0 = 2,
            1 => spec.server.0 = "fixture:other".into(),
            2 => spec.program_ref = "program:other".into(),
            3 => spec.registration_digest = p::canonical_digest(&"other").unwrap(),
            4 => spec.settings_digest = p::canonical_digest(&"other").unwrap(),
            5 => spec.timeout.0 += 1,
            _ => spec.output_budget_bytes += 1,
        }
        assert!(changed.validate_digest().is_err(), "mutation {n}");
        let mut substituted = req.clone();
        substituted.execution.as_mut().unwrap().intent = changed.intent.clone();
        assert!(substituted.validate_at(10_010).is_err(), "mutation {n}");
        changed.refresh_digest().unwrap();
        assert_ne!(changed.digest, plan.digest);
        let mut replaced = dispatch(&changed);
        replaced.permit = req.permit.clone();
        assert!(replaced.validate_at(10_010).is_err());
    }
}
#[test]
fn discovery_cannot_be_planned_as_truncatable_low_risk_or_internal() {
    let i = intent();
    for (budget, timeout) in [
        (OutputBudget::truncate_at(8192), 5000),
        (OutputBudget::strict(4095), 5000),
        (OutputBudget::strict(8192), 999),
    ] {
        assert!(DefaultExecutionPlanner::new(budget, p::DurationMs(timeout))
            .plan(&i)
            .is_err());
    }
    for n in 0..6 {
        let mut changed = i.clone();
        match n {
            0 => changed.risk_hint = p::Risk::Low,
            1 => changed.expected_effect = p::ExpectedEffect::Internal,
            2 => changed.source = p::Source::UserTurn,
            3 => changed.backend_hint = p::BackendKind::Shell,
            4 => changed.action_type = p::ActionType::Observe,
            _ => changed.capability_ref.0 = "mcp:echo".into(),
        }
        assert!(planner().plan(&changed).is_err());
        let mut request = dispatch(&planner().plan(&i).unwrap());
        let execution = request.execution.as_mut().unwrap();
        execution.intent = changed;
        execution.normalized_action_digest =
            p::normalized_action_digest(&execution.intent).unwrap();
        assert!(request.validate_at(10_010).is_err());
    }
}
#[test]
fn discovery_never_reaches_in_process_or_remote_execution() {
    let i = intent();
    let plan = planner().plan(&i).unwrap();
    let backend = McpBackend::new(OutputBudget::strict(8192), p::DurationMs(5000)).unwrap();
    assert!(backend.plan(&i).is_err());
    let sink = EventSink::default();
    assert!(backend
        .execute(plan.clone(), &sink, CancelToken::default())
        .is_err());
    assert!(sink.events().is_empty());
    let mut remote = p::RemoteOperation {
        schema_version: p::SchemaVersion(1),
        backend: p::BackendKind::Mcp,
        parameters: i.parameters,
        capability: i.capability_ref,
        scope: i.scope,
        action_type: i.action_type,
        expected_effect: i.expected_effect,
        rollback_boundary: i.rollback_expectation,
        credential_slot: None,
        digest: p::SchemaDigest(String::new()),
    };
    remote.refresh_digest().unwrap();
    assert!(remote.validate().is_err());
}
#[test]
fn worker_checks_remaining_original_deadline_and_output_limits() {
    let req = dispatch(&planner().plan(&intent()).unwrap());
    req.validate_at(19_000).unwrap();
    assert!(req.validate_at(19_001).is_err());
    let mut small = req.clone();
    small.execution.as_mut().unwrap().output_budget_bytes = 4095;
    assert!(small.validate_at(10_010).is_err());
    let mut short = req;
    short.execution.as_mut().unwrap().timeout.0 = 999;
    assert!(short.validate_at(10_010).is_err());
}

fn intent() -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:discover".into()),
        source: p::Source::OwnerControl,
        goal: p::GoalRef("goal:discover".into()),
        backend_hint: p::BackendKind::Mcp,
        capability_ref: p::CapabilityRef("mcp-discovery:fixture:catalog".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:test".into()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::McpDiscovery(p::ManagedMcpDiscoverySpec {
            schema_version: p::SchemaVersion(1),
            server: p::McpServerRef("fixture:catalog".into()),
            program_ref: "program:fixture".into(),
            registration_digest: p::canonical_digest(&"registration").unwrap(),
            settings_digest: p::canonical_digest(&"settings").unwrap(),
            timeout: p::DurationMs(1000),
            output_budget_bytes: 4096,
        }),
        requested_permissions: vec![p::PermissionRef("mcp:discovery".into())],
        requested_at: 10_000,
        estimated_output_bytes: 4096,
        estimated_duration: p::DurationMs(1000),
    }
}
