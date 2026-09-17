use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_protocol as p;

const TOKEN: &str = "gateway-e2e-token-0123456789abcdef0123456789abcdef";

#[test]
fn s23_http_web_surface_uses_real_gateway_harness_and_event_stream() {
    let model_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let model_address = model_listener.local_addr().unwrap();
    let model_server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = model_listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let request = read_request(&mut stream);
            assert!(request.starts_with("POST /v1/chat/completions HTTP/"));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer gateway-e2e-model-secret"));
            let body = r#"{"choices":[{"message":{"content":"gateway e2e answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            stream.flush().unwrap();
            let _ = stream.shutdown(Shutdown::Both);
        }
    });

    let gateway_probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let gateway_address = gateway_probe.local_addr().unwrap();
    drop(gateway_probe);
    let tag = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let store = std::env::temp_dir().join(format!("forme-gateway-e2e-{tag}.db"));
    let mut gatewayd = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_forme-gatewayd"))
            .env("FORME_MODEL_BASE_URL", format!("http://{model_address}/v1"))
            .env("FORME_MODEL_NAME", "gateway-e2e-model")
            .env("FORME_MODEL_API_KEY", "gateway-e2e-model-secret")
            .env("FORME_MODEL_TIMEOUT_MS", "5000")
            .env("FORME_STORE_PATH", &store)
            .env("FORME_GATEWAY_BIND", "127.0.0.1")
            .env("FORME_GATEWAY_PORT", gateway_address.port().to_string())
            .env("FORME_GATEWAY_TOKEN", TOKEN)
            .env("FORME_GATEWAY_MAX_BODY_BYTES", "1024")
            .env("FORME_OWNER_ID", "owner:gateway-e2e")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    wait_until_ready(&mut gatewayd.0, gateway_address);

    let index = http(gateway_address, "GET", "/", &[], None);
    assert_status(&index, 200);
    assert!(response_body(&index).contains("forme control"));
    assert!(response_body(&index).contains("data-view=\"timeline\""));

    let unauthenticated = http(gateway_address, "GET", "/v1/profile", &[], None);
    assert_status(&unauthenticated, 401);
    let profile_response = http(
        gateway_address,
        "GET",
        "/v1/profile",
        &[authorization()],
        None,
    );
    assert_status(&profile_response, 200);
    assert!(!response_body(&profile_response).contains(TOKEN));
    let profile =
        serde_json::from_str::<p::ControlProfile>(response_body(&profile_response)).unwrap();
    assert_eq!(
        profile.owner,
        p::VerifiedPrincipal("owner:gateway-e2e".into())
    );

    let unauthenticated_evolution = http(
        gateway_address,
        "GET",
        "/v1/evolution/auto-activation",
        &[],
        None,
    );
    assert_status(&unauthenticated_evolution, 401);
    let evolution_snapshot = http(
        gateway_address,
        "GET",
        "/v1/evolution/snapshot?scope=workspace:gateway-e2e",
        &[authorization()],
        None,
    );
    assert_status(&evolution_snapshot, 400);
    assert!(response_body(&evolution_snapshot).contains("no active strategy projection"));
    let pause_body = serde_json::json!({"schema_version": 1, "paused": true}).to_string();
    let pause_without_csrf = http(
        gateway_address,
        "POST",
        "/v1/evolution/auto-activation",
        &[authorization(), ("Content-Type", "application/json".into())],
        Some(&pause_body),
    );
    assert_status(&pause_without_csrf, 403);
    let paused = http(
        gateway_address,
        "POST",
        "/v1/evolution/auto-activation",
        &mutation_headers(gateway_address),
        Some(&pause_body),
    );
    assert_status(&paused, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(response_body(&paused)).unwrap()["paused"],
        true
    );
    let paused_read = http(
        gateway_address,
        "GET",
        "/v1/evolution/auto-activation",
        &[authorization()],
        None,
    );
    assert_status(&paused_read, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(response_body(&paused_read)).unwrap()["paused"],
        true
    );

    let request = p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef("session:gateway-e2e".into()),
        agent_profile: p::AgentProfileRef("agent:forme-local".into()),
        input: p::RunInput("Does the governed HTTP path work?".into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey("gateway-e2e-run".into())),
    };
    let body = serde_json::to_string(&request).unwrap();
    let oversized = http(
        gateway_address,
        "POST",
        "/v1/runs",
        &[
            authorization(),
            ("Content-Type", "application/json".into()),
            ("Origin", format!("http://{gateway_address}")),
            ("x-forme-csrf", "1".into()),
        ],
        Some(&"x".repeat(2048)),
    );
    assert_status(&oversized, 413);
    let missing_csrf = http(
        gateway_address,
        "POST",
        "/v1/runs",
        &[authorization(), ("Content-Type", "application/json".into())],
        Some(&body),
    );
    assert_status(&missing_csrf, 403);

    let origin = format!("http://{gateway_address}");
    let created = http(
        gateway_address,
        "POST",
        "/v1/runs",
        &[
            authorization(),
            ("Content-Type", "application/json".into()),
            ("Origin", origin),
            ("x-forme-csrf", "1".into()),
        ],
        Some(&body),
    );
    assert_status(&created, 202);
    let run = serde_json::from_str::<p::RunId>(response_body(&created)).unwrap();

    let summary = wait_for_terminal_summary(gateway_address, &run);
    assert_eq!(summary.status, p::RunStatus::Complete);

    let runs = http(gateway_address, "GET", "/v1/runs", &[authorization()], None);
    assert_status(&runs, 200);
    let summaries = serde_json::from_str::<Vec<p::RunSummary>>(response_body(&runs)).unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].run, run);
    assert_eq!(summaries[0], summary);

    let event_path = format!("/v1/runs/{}/events?after=0", run.0);
    let events_response = http(
        gateway_address,
        "GET",
        &event_path,
        &[authorization()],
        None,
    );
    assert_status(&events_response, 200);
    assert!(events_response
        .to_ascii_lowercase()
        .contains("content-type: text/event-stream"));
    let events = parse_sse(response_body(&events_response));
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        vec![
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::GoalFramed,
            p::EventKind::ResourcePlanned,
            p::EventKind::DoneContractSet,
            p::EventKind::AutonomyEnvelopeSet,
            p::EventKind::DecisionTraceRecorded,
            p::EventKind::TurnStarted,
            p::EventKind::ContextBuildStarted,
            p::EventKind::ContextBuildFinished,
            p::EventKind::ModelCallStarted,
            p::EventKind::ModelCallDelta,
            p::EventKind::ModelCallFinished,
            p::EventKind::OutputClassified,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::TurnComplete,
            p::EventKind::RunComplete,
        ]
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.stream_seq)
            .collect::<Vec<_>>(),
        (1..=events.len() as u64).collect::<Vec<_>>()
    );

    let reconnect_path = format!("/v1/runs/{}/events?after=5", run.0);
    let reconnect_a = http(
        gateway_address,
        "GET",
        &reconnect_path,
        &[authorization()],
        None,
    );
    let reconnect_b = http(
        gateway_address,
        "GET",
        &reconnect_path,
        &[authorization()],
        None,
    );
    assert_eq!(response_body(&reconnect_a), response_body(&reconnect_b));
    assert_eq!(
        parse_sse(response_body(&reconnect_a))
            .iter()
            .map(|event| event.stream_seq)
            .collect::<Vec<_>>(),
        (6..=events.len() as u64).collect::<Vec<_>>()
    );
    let last_event_id = http(
        gateway_address,
        "GET",
        &format!("/v1/runs/{}/events", run.0),
        &[authorization(), ("Last-Event-ID", format!("{}|5", run.0))],
        None,
    );
    assert_eq!(response_body(&last_event_id), response_body(&reconnect_a));

    let trace = http(
        gateway_address,
        "GET",
        &format!("/v1/runs/{}/trace", run.0),
        &[authorization()],
        None,
    );
    assert_status(&trace, 200);
    let trace = serde_json::from_str::<p::TraceView>(response_body(&trace)).unwrap();
    assert_eq!(trace.events, events);

    let approvals = http(
        gateway_address,
        "GET",
        "/v1/sessions/session:gateway-e2e/approvals",
        &[authorization()],
        None,
    );
    assert_status(&approvals, 200);
    assert_eq!(response_body(&approvals), "[]");

    let wrong_principal_control = p::RunControl::ResolveApproval(p::ApprovalDecision {
        schema_version: p::SchemaVersion(1),
        approval_id: p::ApprovalId("approval:not-pending".into()),
        outcome: p::ApprovalOutcome::Granted,
        approver: p::VerifiedPrincipal("owner:other".into()),
        bound_plan_digest: p::PlanDigest("digest:not-pending".into()),
        policy_version: p::Version(1),
        tool_schema_version: p::Version(1),
        nonce: p::Nonce("nonce:wrong-principal".into()),
        use_by: now_ms().saturating_add(30_000),
    });
    let wrong_principal = http(
        gateway_address,
        "POST",
        &format!("/v1/runs/{}/control", run.0),
        &mutation_headers(gateway_address),
        Some(&serde_json::to_string(&wrong_principal_control).unwrap()),
    );
    assert_status(&wrong_principal, 403);

    let missing_candidate = p::CandidateReviewCommand {
        schema_version: p::SchemaVersion(1),
        run: run.clone(),
        candidate: p::CandidateId("candidate:not-present".into()),
        expected_state: p::CandidateReviewState::Candidate,
        decision: p::CandidateReviewDecision::Promote,
        actor: p::Actor::Owner,
        evidence: vec![p::EvidenceRef(events[0].event_id.0.clone())],
        retraction: None,
    };
    let missing_review = http(
        gateway_address,
        "POST",
        "/v1/candidates/candidate:not-present/review",
        &mutation_headers(gateway_address),
        Some(&serde_json::to_string(&missing_candidate).unwrap()),
    );
    assert_status(&missing_review, 400);
    let trace_after_rejected_mutations = http(
        gateway_address,
        "GET",
        &format!("/v1/runs/{}/trace", run.0),
        &[authorization()],
        None,
    );
    assert_eq!(
        serde_json::from_str::<p::TraceView>(response_body(&trace_after_rejected_mutations))
            .unwrap(),
        trace
    );

    let cases_response = http(
        gateway_address,
        "GET",
        "/v1/evals/cases",
        &[authorization()],
        None,
    );
    assert_status(&cases_response, 200);
    let eval_case = serde_json::from_str::<Vec<p::ManualEvalCase>>(response_body(&cases_response))
        .unwrap()
        .into_iter()
        .find(|case| case.kind == p::GoldenTaskKind::FinalOnly)
        .unwrap();
    let eval_request = p::ManualEvalRequest {
        schema_version: p::SchemaVersion(1),
        case: eval_case,
        profile: p::EvalProfile {
            schema_version: p::SchemaVersion(1),
            eval_ref: p::EvalRef("eval:gateway-e2e".into()),
            model: profile.gateway.model,
            policy: profile.gateway.policy,
            toolset: profile.gateway.toolset,
            workspace: profile.gateway.workspace,
            event_schema: p::SchemaVersion(1),
            replay_snapshot: p::ReplaySnapshotRef("snapshot:gateway-e2e".into()),
        },
    };
    let eval_response = http(
        gateway_address,
        "POST",
        "/v1/evals/run",
        &mutation_headers(gateway_address),
        Some(&serde_json::to_string(&eval_request).unwrap()),
    );
    assert_status(&eval_response, 200);
    let report =
        serde_json::from_str::<p::ManualEvalReport>(response_body(&eval_response)).unwrap();
    assert_eq!(report.outcome, p::VerificationOutcome::Pass);
    assert!(!report.trace_refs.is_empty());
    let exported = http(
        gateway_address,
        "GET",
        "/v1/evals/eval:gateway-e2e",
        &[authorization()],
        None,
    );
    assert_status(&exported, 200);
    assert_eq!(
        serde_json::from_str::<p::ManualEvalReport>(response_body(&exported)).unwrap(),
        report
    );

    let evolution_run = p::RunId("run:gateway-e2e:evolution-candidate".into());
    let untrusted_candidate = p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId("candidate:gateway-e2e:loop:v2".into()),
        domain: p::StrategyDomain::Loop,
        scope: p::Scope("workspace:gateway-e2e".into()),
        target_tier: p::StabilityTier::Stable,
        proposed_version: p::StrategyVersionRef("loop:gateway-e2e:v2".into()),
        baseline: p::StrategyVersionRef("loop:gateway-e2e:v1".into()),
        spec_ref: p::ContentRef("content:loop:gateway-e2e:v2".into()),
        spec_digest: p::SchemaDigest("digest:loop:gateway-e2e:v2".into()),
        evidence: vec![p::EvidenceRef("evidence:gateway-e2e".into())],
        provenance: p::Provenance {
            source: p::Source::Communication,
            actor: p::Actor::External(p::ParticipantId("external:spoof".into())),
            trust_tier: p::TrustTier::Untrusted,
            caused_by: None,
        },
        impact: p::EvolutionImpact::Cautious,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef("loop:gateway-e2e:v1".into()),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    };
    let candidate_body = serde_json::json!({
        "schema_version": 1,
        "run": evolution_run,
        "candidate": untrusted_candidate
    })
    .to_string();
    let candidate_response = http(
        gateway_address,
        "POST",
        "/v1/evolution/candidates",
        &mutation_headers(gateway_address),
        Some(&candidate_body),
    );
    assert_status(&candidate_response, 201);
    let candidate_events = http(
        gateway_address,
        "GET",
        "/v1/runs/run:gateway-e2e:evolution-candidate/events?after=0",
        &[authorization()],
        None,
    );
    assert_status(&candidate_events, 200);
    let candidate_events = parse_sse(response_body(&candidate_events));
    let p::EventPayload::CandidateCreated(candidate) = &candidate_events[0].payload else {
        panic!("evolution candidate event was not recorded")
    };
    let stamped = candidate.strategy_candidate.as_ref().unwrap();
    assert_eq!(stamped.provenance.actor, p::Actor::Owner);
    assert_eq!(stamped.provenance.trust_tier, p::TrustTier::OwnerInput);

    stop(&mut gatewayd.0);
    model_server.join().unwrap();
    remove_sqlite_files(&store);
}

#[test]
fn http_submit_accepts_before_the_background_model_call_completes() {
    let model_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let model_address = model_listener.local_addr().unwrap();
    let (model_started_tx, model_started_rx) = std::sync::mpsc::channel();
    let (release_model_tx, release_model_rx) = std::sync::mpsc::channel();
    let model_server = std::thread::spawn(move || {
        let (mut stream, _) = model_listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let request = read_request(&mut stream);
        assert!(request.starts_with("POST /v1/chat/completions HTTP/"));
        model_started_tx.send(()).unwrap();
        let _ = release_model_rx.recv_timeout(Duration::from_secs(3));
        let body = r#"{"choices":[{"message":{"content":"background complete"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        stream.flush().unwrap();
    });

    let gateway_probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let gateway_address = gateway_probe.local_addr().unwrap();
    drop(gateway_probe);
    let store = std::env::temp_dir().join(format!("forme-gateway-async-{}.db", now_ms()));
    let mut gatewayd = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_forme-gatewayd"))
            .env("FORME_MODEL_BASE_URL", format!("http://{model_address}/v1"))
            .env("FORME_MODEL_NAME", "gateway-async-model")
            .env("FORME_MODEL_API_KEY", "gateway-async-model-secret")
            .env("FORME_MODEL_TIMEOUT_MS", "5000")
            .env("FORME_STORE_PATH", &store)
            .env("FORME_GATEWAY_BIND", "127.0.0.1")
            .env("FORME_GATEWAY_PORT", gateway_address.port().to_string())
            .env("FORME_GATEWAY_TOKEN", TOKEN)
            .env("FORME_OWNER_ID", "owner:gateway-async")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    wait_until_ready(&mut gatewayd.0, gateway_address);

    let request = p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef("session:gateway-async".into()),
        agent_profile: p::AgentProfileRef("agent:forme-local".into()),
        input: p::RunInput("Complete after the accepted response.".into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey("gateway-async-run".into())),
    };
    let started_at = Instant::now();
    let created = http(
        gateway_address,
        "POST",
        "/v1/runs",
        &mutation_headers(gateway_address),
        Some(&serde_json::to_string(&request).unwrap()),
    );
    assert_status(&created, 202);
    assert!(
        started_at.elapsed() < Duration::from_secs(1),
        "accepted response waited for the model call"
    );
    let run = serde_json::from_str::<p::RunId>(response_body(&created)).unwrap();
    model_started_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let running = http(
        gateway_address,
        "GET",
        &format!("/v1/runs/{}", run.0),
        &[authorization()],
        None,
    );
    assert_status(&running, 200);
    assert!(!matches!(
        serde_json::from_str::<p::RunSummary>(response_body(&running))
            .unwrap()
            .status,
        p::RunStatus::Complete
            | p::RunStatus::Aborted
            | p::RunStatus::Failed
            | p::RunStatus::Limited
    ));

    release_model_tx.send(()).unwrap();
    assert_eq!(
        wait_for_terminal_summary(gateway_address, &run).status,
        p::RunStatus::Complete
    );
    stop(&mut gatewayd.0);
    model_server.join().unwrap();
    remove_sqlite_files(&store);
}

#[test]
fn jobs_api_persists_across_daemon_restart_and_cancel_prevents_a_run() {
    let tag = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let store = std::env::temp_dir().join(format!("forme-gateway-jobs-{tag}.db"));

    let first_address = free_address();
    let mut first = spawn_jobs_gateway(first_address, &store);
    wait_until_ready(&mut first.0, first_address);

    let missing_csrf = scheduled_reminder("intention:jobs-no-csrf", now_ms() + 60_000);
    let denied = http(
        first_address,
        "POST",
        "/v1/jobs",
        &[authorization(), ("Content-Type", "application/json".into())],
        Some(&serde_json::to_string(&missing_csrf).unwrap()),
    );
    assert_status(&denied, 403);

    let due = scheduled_reminder("intention:jobs-due", now_ms() + 50);
    let created = http(
        first_address,
        "POST",
        "/v1/jobs",
        &mutation_headers(first_address),
        Some(&serde_json::to_string(&due).unwrap()),
    );
    assert_status(&created, 202);
    assert_eq!(
        serde_json::from_str::<p::IntentionId>(response_body(&created)).unwrap(),
        due.intention.id
    );
    let completed = wait_for_job_state(first_address, &due.intention.id, p::IntentionState::Done);
    assert_eq!(completed.run_status, Some(p::RunStatus::Complete));
    let due_run = completed.run.unwrap();
    let due_events = http(
        first_address,
        "GET",
        &format!("/v1/runs/{}/events?after=0", due_run.0),
        &[authorization()],
        None,
    );
    assert_status(&due_events, 200);
    let due_kinds = parse_sse(response_body(&due_events))
        .into_iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    for required in [
        p::EventKind::RunAccepted,
        p::EventKind::ProactiveProposalEmitted,
        p::EventKind::ToolCallProposed,
        p::EventKind::ToolPolicyEvaluated,
        p::EventKind::ActionPlanned,
        p::EventKind::ActionStarted,
        p::EventKind::ActionCompleted,
        p::EventKind::RunComplete,
    ] {
        assert!(due_kinds.contains(&required), "missing {required:?}");
    }

    let after_restart = scheduled_reminder("intention:jobs-restart", now_ms() + 1_500);
    let persisted = http(
        first_address,
        "POST",
        "/v1/jobs",
        &mutation_headers(first_address),
        Some(&serde_json::to_string(&after_restart).unwrap()),
    );
    assert_status(&persisted, 202);
    assert_eq!(
        wait_for_job_state(
            first_address,
            &after_restart.intention.id,
            p::IntentionState::Pending,
        )
        .run,
        None
    );
    stop(&mut first.0);

    let second_address = free_address();
    let mut second = spawn_jobs_gateway(second_address, &store);
    wait_until_ready(&mut second.0, second_address);
    let recovered = wait_for_job_state(
        second_address,
        &after_restart.intention.id,
        p::IntentionState::Done,
    );
    assert_eq!(recovered.run_status, Some(p::RunStatus::Complete));

    let cancelled = scheduled_reminder("intention:jobs-cancelled", now_ms() + 60_000);
    let cancel_created = http(
        second_address,
        "POST",
        "/v1/jobs",
        &mutation_headers(second_address),
        Some(&serde_json::to_string(&cancelled).unwrap()),
    );
    assert_status(&cancel_created, 202);
    let cancel_response = http(
        second_address,
        "POST",
        &format!("/v1/jobs/{}/cancel", cancelled.intention.id.0),
        &mutation_headers(second_address),
        Some(""),
    );
    assert_status(&cancel_response, 204);
    let cancelled_job = wait_for_job_state(
        second_address,
        &cancelled.intention.id,
        p::IntentionState::Cancelled,
    );
    assert_eq!(cancelled_job.run, None);
    assert_eq!(cancelled_job.run_status, None);

    stop(&mut second.0);
    remove_sqlite_files(&store);
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        stop(&mut self.0);
    }
}

fn authorization() -> (&'static str, String) {
    ("Authorization", format!("Bearer {TOKEN}"))
}

fn mutation_headers(address: SocketAddr) -> Vec<(&'static str, String)> {
    vec![
        authorization(),
        ("Content-Type", "application/json".into()),
        ("Origin", format!("http://{address}")),
        ("x-forme-csrf", "1".into()),
    ]
}

fn free_address() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    address
}

fn spawn_jobs_gateway(address: SocketAddr, store: &std::path::Path) -> ChildGuard {
    ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_forme-gatewayd"))
            .env("FORME_MODEL_BASE_URL", "http://127.0.0.1:9/v1")
            .env("FORME_MODEL_NAME", "gateway-jobs-model")
            .env("FORME_MODEL_API_KEY", "gateway-jobs-model-secret")
            .env("FORME_STORE_PATH", store)
            .env("FORME_GATEWAY_BIND", "127.0.0.1")
            .env("FORME_GATEWAY_PORT", address.port().to_string())
            .env("FORME_GATEWAY_TOKEN", TOKEN)
            .env("FORME_GATEWAY_MAX_BODY_BYTES", "8192")
            .env("FORME_GATEWAY_MAX_REQUESTS_PER_MINUTE", "1000")
            .env("FORME_OWNER_ID", "owner:gateway-jobs")
            .env("FORME_SCHEDULER_ENABLED", "true")
            .env("FORME_SCHEDULER_TICK_MS", "20")
            .env("FORME_SCHEDULER_LEASE_MS", "200")
            .env("FORME_SCHEDULER_MAX_CLAIMS", "4")
            .env("FORME_NOTIFICATION_ENABLED", "true")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
}

fn scheduled_reminder(id: &str, due: p::Timestamp) -> p::ScheduleCommand {
    p::ScheduleCommand {
        schema_version: p::SchemaVersion(1),
        intention: p::ProspectiveIntention {
            schema_version: p::SchemaVersion(1),
            id: p::IntentionId(id.into()),
            source: p::IntentionSource::Commitment,
            trigger: p::IntentionTrigger::At(due),
            state: p::IntentionState::Pending,
            seed: p::SeedRef(format!("scheduled reminder {id}")),
            provenance: p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            expires_at: Some(due + 30_000),
        },
        session: p::SessionId(format!("session:{id}")),
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
                starts_at: due,
                expires_at: due + 60_000,
                max_turns: 2,
            },
            rollback: p::RollbackReq {
                schema_version: p::SchemaVersion(1),
                required: false,
                boundary: None,
            },
        },
        budget: p::Budget("units:1".into()),
    }
}

fn wait_for_job_state(
    address: SocketAddr,
    intention: &p::IntentionId,
    expected: p::IntentionState,
) -> p::ScheduledJob {
    for _ in 0..250 {
        let response = http(address, "GET", "/v1/jobs", &[authorization()], None);
        if response.starts_with("HTTP/1.1 200 ") {
            if let Some(job) =
                serde_json::from_str::<Vec<p::ScheduledJob>>(response_body(&response))
                    .unwrap()
                    .into_iter()
                    .find(|job| job.intention.id == *intention && job.intention.state == expected)
            {
                return job;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "job {} did not reach intention state {expected:?}",
        intention.0
    );
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn wait_for_terminal_summary(address: SocketAddr, run: &p::RunId) -> p::RunSummary {
    for _ in 0..200 {
        let response = http(
            address,
            "GET",
            &format!("/v1/runs/{}", run.0),
            &[authorization()],
            None,
        );
        if response.starts_with("HTTP/1.1 200 ") {
            let summary = serde_json::from_str::<p::RunSummary>(response_body(&response)).unwrap();
            if matches!(
                summary.status,
                p::RunStatus::Complete
                    | p::RunStatus::Aborted
                    | p::RunStatus::Failed
                    | p::RunStatus::Limited
            ) {
                return summary;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("run {} did not reach a terminal state", run.0);
}

fn wait_until_ready(child: &mut Child, address: SocketAddr) {
    for _ in 0..100 {
        if let Some(status) = child.try_wait().unwrap() {
            let mut stderr = String::new();
            if let Some(mut stream) = child.stderr.take() {
                let _ = stream.read_to_string(&mut stderr);
            }
            panic!("gateway exited before readiness ({status}): {stderr}");
        }
        if TcpStream::connect_timeout(&address, Duration::from_millis(40)).is_ok() {
            let health = http(address, "GET", "/healthz", &[], None);
            if health.starts_with("HTTP/1.1 200") {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    stop(child);
    panic!("gateway did not become ready");
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn http(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: Option<&str>,
) -> String {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let body = body.unwrap_or("");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )
    .unwrap();
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n").unwrap();
    }
    write!(stream, "\r\n{body}").unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn assert_status(response: &str, expected: u16) {
    assert!(
        response.starts_with(&format!("HTTP/1.1 {expected} ")),
        "unexpected response: {response}"
    );
}

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("")
}

fn parse_sse(body: &str) -> Vec<p::Event> {
    body.split("\n\n")
        .filter_map(|block| block.lines().find_map(|line| line.strip_prefix("data: ")))
        .map(|json| serde_json::from_str(json).unwrap())
        .collect()
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4_096];
    let mut expected = None;
    loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "client closed before sending a complete request");
        bytes.extend_from_slice(&buffer[..read]);
        if expected.is_none() {
            if let Some(header_end) = find_bytes(&bytes, b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::trim)
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                expected = Some(header_end + 4 + content_length);
            }
        }
        if expected.is_some_and(|length| bytes.len() >= length) {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn remove_sqlite_files(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
}
