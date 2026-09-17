use forme_protocol as p;

fn event(run: &p::RunId, seq: u64) -> p::Event {
    let mut event = p::Event::new(
        p::EventId(format!("event:{seq}")),
        run.clone(),
        None,
        p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 0 }),
        p::SchemaVersion(1),
        100,
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        },
    );
    event.stream_seq = seq;
    event
}

#[test]
fn m1_control_objects_round_trip_with_schema_versions() {
    let surface = p::SurfaceProfile {
        schema_version: p::SchemaVersion(1),
        surface: p::SurfaceRef("surface:local-web".into()),
        kind: p::SurfaceKind::LocalWeb,
        trust: p::TrustTier::OwnerInput,
        scope: p::Scope("workspace:default".into()),
    };
    let gateway = p::GatewayProfile {
        schema_version: p::SchemaVersion(1),
        surface: surface.surface.clone(),
        policy: p::PolicyProfileRef("policy:default".into()),
        model: p::ModelProfileRef("model:default".into()),
        toolset: p::ToolsetRef("toolset:default".into()),
        workspace: p::WorkspaceRef("workspace:default".into()),
    };
    surface.validate().unwrap();
    gateway.validate().unwrap();
    for value in [
        serde_json::to_value(&surface).unwrap(),
        serde_json::to_value(&gateway).unwrap(),
    ] {
        assert_eq!(value["schema_version"], 1);
    }
    assert_eq!(
        serde_json::from_value::<p::SurfaceProfile>(serde_json::to_value(surface.clone()).unwrap())
            .unwrap(),
        surface
    );
    assert_eq!(
        serde_json::from_value::<p::GatewayProfile>(serde_json::to_value(gateway.clone()).unwrap())
            .unwrap(),
        gateway
    );

    let approval = p::PendingApproval {
        schema_version: p::SchemaVersion(1),
        run: p::RunId("run:approval".into()),
        session: p::SessionId("session:approval".into()),
        approval_id: p::ApprovalId("approval:1".into()),
        action_summary: "write a governed file".into(),
        risk_level: p::Risk::Medium,
        scope: p::Scope("workspace:default".into()),
        requested_permissions: vec![p::PermissionRef("file.write".into())],
        affected_resources: vec![p::ResourceRef("file:README.md".into())],
        rollback_boundary: p::RollbackBoundary("revert file".into()),
        expires_at: 200,
        choices: vec![
            p::ApprovalChoice("grant-once".into()),
            p::ApprovalChoice("deny".into()),
        ],
        plan_digest: p::PlanDigest("digest:1".into()),
        policy_version: p::Version(1),
        tool_schema_version: p::Version(1),
    };
    approval.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<p::PendingApproval>(serde_json::to_value(&approval).unwrap())
            .unwrap(),
        approval
    );
}

#[test]
fn event_page_requires_one_strict_run_ordered_cursor_slice() {
    let run = p::RunId("run:m1-page".into());
    let page = p::EventPage {
        schema_version: p::SchemaVersion(1),
        run: run.clone(),
        after_stream_seq: 3,
        snapshot_upper_bound: 5,
        events: vec![event(&run, 4), event(&run, 5)],
    };
    page.validate().unwrap();

    let mut duplicate = page.clone();
    duplicate.events[1].stream_seq = 4;
    assert!(duplicate.validate().is_err());

    let mut wrong_run = page;
    wrong_run.events[0].run_id = p::RunId("run:other".into());
    assert!(wrong_run.validate().is_err());

    let mut gap = p::EventPage {
        schema_version: p::SchemaVersion(1),
        run: run.clone(),
        after_stream_seq: 3,
        snapshot_upper_bound: 5,
        events: vec![event(&run, 5)],
    };
    assert!(gap.validate().is_err());
    gap.events.clear();
    assert!(gap.validate().is_err());

    assert!(p::EventCursor {
        schema_version: p::SchemaVersion(1),
        run,
        after_stream_seq: u64::MAX,
    }
    .validate()
    .is_err());
}

#[test]
fn candidate_review_is_owner_bound_and_approval_is_plan_bound() {
    let review = p::CandidateReviewCommand {
        schema_version: p::SchemaVersion(1),
        run: p::RunId("run:review".into()),
        candidate: p::CandidateId("candidate:1".into()),
        expected_state: p::CandidateReviewState::Candidate,
        decision: p::CandidateReviewDecision::Promote,
        actor: p::Actor::Owner,
        evidence: vec![p::EvidenceRef("owner:confirmed".into())],
        retraction: None,
    };
    review.validate().unwrap();
    let mut external = review;
    external.actor = p::Actor::External(p::ParticipantId("outside".into()));
    assert!(external.validate().is_err());

    let decision = p::ApprovalDecision {
        schema_version: p::SchemaVersion(1),
        approval_id: p::ApprovalId("approval:1".into()),
        outcome: p::ApprovalOutcome::Granted,
        approver: p::VerifiedPrincipal("owner".into()),
        bound_plan_digest: p::PlanDigest("digest:1".into()),
        policy_version: p::Version(1),
        tool_schema_version: p::Version(1),
        nonce: p::Nonce("nonce:1".into()),
        use_by: 200,
    };
    decision.validate().unwrap();
    let encoded = serde_json::to_value(&decision).unwrap();
    assert_eq!(encoded["bound_plan_digest"], "digest:1");
    assert_eq!(
        serde_json::from_value::<p::ApprovalDecision>(encoded).unwrap(),
        decision
    );
}

#[test]
fn control_boundaries_reject_unknown_enums_empty_scope_and_invalid_retraction() {
    assert!(serde_json::from_str::<p::SurfaceKind>("\"RemoteWeb\"").is_err());
    assert!(serde_json::from_str::<p::CandidateReviewDecision>("\"Confirm\"").is_err());

    let surface = p::SurfaceProfile {
        schema_version: p::SchemaVersion(1),
        surface: p::SurfaceRef("surface:local-web".into()),
        kind: p::SurfaceKind::LocalWeb,
        trust: p::TrustTier::OwnerInput,
        scope: p::Scope(String::new()),
    };
    assert!(surface.validate().is_err());

    let review = p::CandidateReviewCommand {
        schema_version: p::SchemaVersion(1),
        run: p::RunId("run:review".into()),
        candidate: p::CandidateId("candidate:1".into()),
        expected_state: p::CandidateReviewState::Promoted,
        decision: p::CandidateReviewDecision::Retract,
        actor: p::Actor::Owner,
        evidence: vec![p::EvidenceRef("failure:1".into())],
        retraction: None,
    };
    assert!(review.validate().is_err());
}

#[test]
fn notification_parameters_are_structured_and_never_embed_a_body() {
    let parameters = p::ActionParameters::Notification {
        surface: p::SurfaceRef("surface:local".into()),
        target: p::ParticipantId("owner".into()),
        title: "Approval required".into(),
        body_ref: p::ContentRef("content:event:42".into()),
    };
    let value = serde_json::to_value(parameters).unwrap();
    assert_eq!(value["Notification"]["body_ref"], "content:event:42");
    assert!(value["Notification"].get("body").is_none());
    assert_eq!(p::EventKind::ALL.len(), 99);
    assert_eq!(p::EventKind::ALL[85], p::EventKind::ComplianceCheckResult);
}

#[test]
fn m1_c_mcp_schema_digest_is_additive_and_old_parameters_remain_replayable() {
    let old = serde_json::json!({
        "Mcp": {
            "server": "mcp:notes",
            "tool": "read_note",
            "arguments": { "path": "notes/today.md" },
            "transport": "Stdio",
            "stdio": {
                "schema_version": 1,
                "command": "fixture",
                "args": [],
                "env": []
            },
            "timeout": 1000
        }
    });
    let decoded = serde_json::from_value::<p::ActionParameters>(old).unwrap();
    let p::ActionParameters::Mcp { schema_digest, .. } = decoded else {
        panic!("fixture must decode as MCP parameters");
    };
    assert!(schema_digest.is_none());

    let current = p::ActionParameters::Mcp {
        server: p::McpServerRef("mcp:notes".into()),
        tool: p::ToolRef("read_note".into()),
        arguments: serde_json::json!({ "path": "notes/today.md" }),
        schema_digest: Some(p::SchemaDigest("sha256:current".into())),
        transport: p::McpTransport::Stdio,
        stdio: p::McpStdioSpec {
            schema_version: p::SchemaVersion(1),
            command: "fixture".into(),
            args: Vec::new(),
            env: Vec::new(),
        },
        timeout: p::DurationMs(1_000),
    };
    assert_eq!(
        serde_json::from_value::<p::ActionParameters>(serde_json::to_value(&current).unwrap())
            .unwrap(),
        current
    );
}

#[test]
fn scheduler_contract_is_versioned_bounded_and_round_trips() {
    let command = p::ScheduleCommand {
        schema_version: p::SchemaVersion(1),
        intention: p::ProspectiveIntention {
            schema_version: p::SchemaVersion(1),
            id: p::IntentionId("intention:reminder".into()),
            source: p::IntentionSource::Commitment,
            trigger: p::IntentionTrigger::At(150),
            state: p::IntentionState::Pending,
            seed: p::SeedRef("review the acceptance evidence".into()),
            provenance: p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            expires_at: Some(250),
        },
        session: p::SessionId("session:scheduled".into()),
        envelope: p::AutonomyEnvelope {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("workspace:default".into()),
            capability: p::CapabilitySet {
                schema_version: p::SchemaVersion(1),
                capabilities: vec![p::CapabilityRef("capability:local-notification".into())],
                permissions: vec![p::PermissionRef("permission:local-notification".into())],
            },
            action_type: vec![p::ActionType::Deliver],
            risk_limit: p::Risk::Low,
            approval_rule: p::ApprovalRule::Allow,
            budget: p::Budget("units:1".into()),
            timebox: p::Timebox {
                schema_version: p::SchemaVersion(1),
                starts_at: 100,
                expires_at: 300,
                max_turns: 1,
            },
            rollback: p::RollbackReq {
                schema_version: p::SchemaVersion(1),
                required: false,
                boundary: None,
            },
        },
        budget: p::Budget("units:1".into()),
    };
    command.validate().unwrap();
    let encoded = serde_json::to_value(&command).unwrap();
    assert_eq!(encoded["schema_version"], 1);
    assert_eq!(
        serde_json::from_value::<p::ScheduleCommand>(encoded).unwrap(),
        command
    );

    let config = p::SchedulerConfig {
        schema_version: p::SchemaVersion(1),
        tick: p::DurationMs(1_000),
        lease: p::DurationMs(30_000),
        max_claims_per_tick: 1,
    };
    config.validate().unwrap();
    let mut invalid = config;
    invalid.max_claims_per_tick = 0;
    assert!(invalid.validate().is_err());
    assert!(serde_json::from_str::<p::IntentionState>("\"Unknown\"").is_err());
    for check in [p::ConfigCheck::Scheduler, p::ConfigCheck::Notification] {
        let encoded = serde_json::to_value(check).unwrap();
        assert_eq!(
            serde_json::from_value::<p::ConfigCheck>(encoded).unwrap(),
            check
        );
    }
}

#[test]
fn manual_eval_contract_round_trips_a_frozen_profile_and_trace_report() {
    let case = p::ManualEvalCase {
        schema_version: p::SchemaVersion(1),
        case_ref: p::EvalCaseRef("case:final-only".into()),
        kind: p::GoldenTaskKind::FinalOnly,
        request: p::RunRequest {
            schema_version: p::SchemaVersion(1),
            source: p::Source::UserTurn,
            session: p::SessionRef("eval:final-only".into()),
            agent_profile: p::AgentProfileRef("agent:eval".into()),
            input: p::RunInput("return a deterministic answer".into()),
            budget: None,
            idempotency_key: Some(p::IdempotencyKey("eval:final-only:v1".into())),
        },
        workspace: p::WorkspaceRef("workspace:default".into()),
        done_contract: p::DoneContractRef("done:final-only".into()),
        allowed_capabilities: Vec::new(),
        policy: p::PolicyProfileRef("policy:default-deny".into()),
        rubric: p::RubricRef("rubric:verified-complete".into()),
        required_events: vec![
            p::EventKind::RunAccepted,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ],
        forbidden_events: vec![p::EventKind::CandidatePromoted],
    };
    case.validate().unwrap();
    let profile = p::EvalProfile {
        schema_version: p::SchemaVersion(1),
        eval_ref: p::EvalRef("eval:m1-a".into()),
        model: p::ModelProfileRef("model:test".into()),
        policy: case.policy.clone(),
        toolset: p::ToolsetRef("toolset:empty".into()),
        workspace: case.workspace.clone(),
        event_schema: p::SchemaVersion(1),
        replay_snapshot: p::ReplaySnapshotRef("snapshot:m1-a".into()),
    };
    let report = p::ManualEvalReport {
        schema_version: p::SchemaVersion(1),
        eval_ref: profile.eval_ref.clone(),
        case_ref: case.case_ref.clone(),
        run: p::RunId("run:eval".into()),
        trace_refs: vec![p::EventId("event:eval:1".into())],
        outcome: p::VerificationOutcome::Pass,
        rubric: case.rubric.clone(),
        snapshot: profile.replay_snapshot.clone(),
        profile,
    };
    report.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<p::ManualEvalReport>(serde_json::to_value(&report).unwrap())
            .unwrap(),
        report
    );
}
