//! Real Host journal, encrypted original discovery, and native call sessions.
use super::*;
use crate::installed::{HostAuth, IsolatedExecutorHost, WorkerExecutorHost};

fn auth(fixture: &Fixture) -> HostAuth {
    let broker = crate::installed::ProductHostBroker::open_with_service_manager(
        &fixture._temp.path().join("call-host.jsonl"),
        "install:config-test",
        crate::installed::HostCredentials::new("m".repeat(32), Some("a".repeat(32))).unwrap(),
        p::ProductTarget::WindowsX86_64,
        "fixture-release".into(),
        crate::installed::SystemServiceManager,
    )
    .unwrap();
    broker.authenticate_agent(&"a".repeat(32)).unwrap()
}
fn host(fixture: &Fixture) -> WorkerExecutorHost {
    WorkerExecutorHost::open(
        &fixture._temp.path().join("call-worker.jsonl"),
        p::canonical_digest(&"a".repeat(32)).unwrap(),
        p::AuthorityEpoch(1),
        [fixture.settings.scope.clone()].into_iter().collect(),
    )
    .unwrap()
    .with_managed_mcp_programs(fixture.programs.clone(), fixture.runtime.join("calls"))
    .unwrap()
}
fn directory(fixture: &Fixture, request: &p::WorkerDispatchRequest) -> PathBuf {
    let root = fs::canonicalize(fixture.runtime.join("calls")).unwrap();
    root.join(
        profile_name_for(
            root.to_str().unwrap(),
            &request.job.job_id,
            &request.job.plan_digest,
        )
        .unwrap(),
    )
}
fn dispatch(
    host: &WorkerExecutorHost,
    auth: &HostAuth,
    req: &p::WorkerDispatchRequest,
) -> p::Result<p::WorkerObservation> {
    host.dispatch_with_execution(
        auth,
        req.job.clone(),
        req.permit.clone(),
        req.execution.clone(),
        recovery_timestamp(),
    )
}
fn finish(
    host: &WorkerExecutorHost,
    auth: &HostAuth,
    req: &p::WorkerDispatchRequest,
) -> p::WorkerObservation {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let value = host
            .probe(auth, &req.job.job_id, recovery_timestamp())
            .unwrap();
        if !matches!(
            value.state,
            p::WorkerState::Pending | p::WorkerState::Running
        ) {
            return value;
        }
        assert!(Instant::now() < end, "original call did not settle");
        std::thread::sleep(Duration::from_millis(3));
    }
}
fn discover(
    fixture: &Fixture,
    host: &WorkerExecutorHost,
    auth: &HostAuth,
) -> p::WorkerResultManifest {
    assert_eq!(
        dispatch(host, auth, &fixture.request).unwrap().state,
        p::WorkerState::Running
    );
    if matches!(fixture_mode(fixture).as_str(), "hold" | "call-no-read") {
        let dir = directory(fixture, &fixture.request);
        wait_file(&dir.join("workspace/started.pid"));
        fs::write(dir.join("workspace/continue"), []).unwrap();
    }
    assert_eq!(
        finish(host, auth, &fixture.request).state,
        p::WorkerState::Completed
    );
    host.result_manifest(auth, &fixture.request.job.job_id, recovery_timestamp())
        .unwrap()
}
fn fixture_mode(fixture: &Fixture) -> String {
    let bytes = fs::read(fixture._temp.path().join("programs/sources.json")).unwrap();
    serde_json::from_slice::<Value>(&bytes).unwrap()["sources"][0]["args"][0]
        .as_str()
        .unwrap()
        .into()
}
fn call(
    fixture: &Fixture,
    suffix: &str,
    change: impl FnOnce(&mut p::ActionIntent),
) -> p::WorkerDispatchRequest {
    use crate::{DefaultExecutionPlanner, ExecutionPlanner, OutputBudget};
    let mut req = request(&fixture.registration, &fixture.settings);
    let mut intent = req.execution.take().unwrap().intent;
    intent.intent_id = p::ActionId(format!("action:{suffix}"));
    intent.capability_ref = p::CapabilityRef("mcp:fixture:native:fixture.inspect".into());
    intent.parameters = p::ActionParameters::Mcp {
        server: p::McpServerRef(fixture.settings.server_ref.clone()),
        tool: p::ToolRef("fixture.inspect".into()),
        arguments: json!({"value":7}),
        schema_digest: Some(
            p::canonical_digest(&json!({"type":"object","properties":{}})).unwrap(),
        ),
        transport: p::McpTransport::Stdio,
        stdio: p::McpStdioSpec {
            schema_version: p::SchemaVersion(1),
            command: fixture.settings.program_ref.clone(),
            args: vec![],
            env: vec![],
        },
        timeout: fixture.settings.timeout,
    };
    change(&mut intent);
    let plan = DefaultExecutionPlanner::new(
        OutputBudget::strict(fixture.settings.output_budget),
        fixture.settings.timeout,
    )
    .plan(&intent)
    .unwrap();
    req.job.job_id = format!("job:{suffix}");
    req.job.action_ref = intent.intent_id.clone();
    req.job.affected_resources = plan.affected_resources.clone();
    req.job.plan_digest = plan.digest.clone();
    req.job.permit_ref = format!("permit:{suffix}");
    req.job.nonce = p::Nonce(format!("nonce:{suffix}"));
    req.permit.job_id = req.job.job_id.clone();
    req.permit.action_ref = intent.intent_id.clone();
    req.permit.affected_resources = plan.affected_resources.clone();
    req.permit.plan_digest = plan.digest.clone();
    req.permit.permit_ref = req.job.permit_ref.clone();
    req.permit.nonce = req.job.nonce.clone();
    req.execution = Some(p::WorkerExecutionSpec {
        schema_version: p::SchemaVersion(1),
        plan_digest: plan.digest,
        intent,
        backend: plan.backend,
        scope: plan.scope,
        affected_resources: plan.affected_resources,
        normalized_action_digest: plan.normalized_action_digest,
        timeout: plan.timeout,
        output_budget_bytes: plan.budget.max_bytes,
        desktop_session: None,
    });
    req
}
fn read_body(
    host: &WorkerExecutorHost,
    auth: &HostAuth,
    req: &p::WorkerDispatchRequest,
) -> (p::WorkerResultManifest, Vec<u8>) {
    let manifest = host
        .result_manifest(auth, &req.job.job_id, recovery_timestamp())
        .unwrap();
    let chunk = host
        .read_result(
            auth,
            p::WorkerResultRead {
                schema_version: p::WORKER_RESULT_SCHEMA_VERSION,
                binding: manifest.binding.clone(),
                manifest_digest: manifest.manifest_digest.clone(),
                offset: 0,
                max_bytes: 131_072,
            },
            recovery_timestamp(),
        )
        .unwrap();
    chunk.validate().unwrap();
    assert!(chunk.finished);
    assert_eq!(
        manifest.content_digest,
        Some(p::sha256_content_digest(&chunk.bytes))
    );
    (manifest, chunk.bytes)
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_managed_mcp_call_reuses_completed_original_after_host_restart() {
    let fixture = Fixture::new("normal", 10_000);
    let auth = auth(&fixture);
    let first = host(&fixture);
    discover(&fixture, &first, &auth);
    drop(first);
    let restarted = host(&fixture);
    let req = call(&fixture, "restart-call", |_| {});
    assert!(!directory(&fixture, &req).exists());
    assert_eq!(
        dispatch(&restarted, &auth, &req).unwrap().state,
        p::WorkerState::Running
    );
    let terminal = finish(&restarted, &auth, &req);
    assert_eq!(terminal.state, p::WorkerState::Completed);
    let (_, bytes) = read_body(&restarted, &auth, &req);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<String>>(body["content"][0]["text"].as_str().unwrap()).unwrap(),
        ["initialize", "notifications/initialized", "tools/call"]
    );
    assert_eq!(body["structuredContent"]["arguments"], json!({"value":7}));
    assert!(!directory(&fixture, &req).exists());
    assert_eq!(
        dispatch(&restarted, &auth, &req).unwrap().receipt_ref,
        terminal.receipt_ref
    );
    drop(restarted);
    let facts = fs::read_to_string(fixture._temp.path().join("call-worker.jsonl")).unwrap();
    assert_eq!(
        facts
            .lines()
            .filter(
                |line| serde_json::from_str::<Value>(line).unwrap()["fact"]["record"] == "started"
            )
            .count(),
        2
    );
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_managed_mcp_call_rejects_missing_original_schema_and_configuration_drift() {
    let fixture = Fixture::new("normal", 10_000);
    let auth = auth(&fixture);
    let host = host(&fixture);
    let original = discover(&fixture, &host, &auth);
    for mode in ["schema", "args", "env", "path", "tool"] {
        let req = call(&fixture, mode, |intent| {
            let p::ActionParameters::Mcp {
                schema_digest,
                stdio,
                tool,
                ..
            } = &mut intent.parameters
            else {
                unreachable!()
            };
            match mode {
                "schema" => *schema_digest = Some(p::canonical_digest(&json!({})).unwrap()),
                "args" => stdio.args.push("unregistered".into()),
                "env" => stdio.env.push(("HOST_SECRET".into(), "fixture".into())),
                "path" => stdio.command = "C:\\outside.exe".into(),
                "tool" => tool.0 = "fixture.denied".into(),
                _ => unreachable!(),
            }
        });
        assert!(dispatch(&host, &auth, &req).is_err(), "{mode}");
        assert!(!directory(&fixture, &req).exists());
    }
    let root = fixture._temp.path().join("call-worker.results");
    for kind in ["manifest", "data"] {
        let suffix = original.manifest_digest.0.strip_prefix("sha256:").unwrap();
        let file = root.join(format!(
            "{kind}-{suffix}.{}",
            if kind == "manifest" { "json" } else { "bin" }
        ));
        let bytes = fs::read(&file).unwrap();
        fs::remove_file(&file).unwrap();
        let req = call(&fixture, kind, |_| {});
        assert!(dispatch(&host, &auth, &req).is_err());
        assert!(!directory(&fixture, &req).exists());
        fs::write(file, bytes).unwrap();
    }
    let mut changed = fixture.settings.clone();
    changed.allowlist.tools.clear();
    config_tests::stage_mcp_fixture(&mut fixture.config.lock().unwrap(), changed);
    let req = call(&fixture, "config-drift", |_| {});
    assert!(dispatch(&host, &auth, &req).is_err());
    assert!(!directory(&fixture, &req).exists());
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_managed_mcp_call_errors_keep_trusted_failure_or_original_unknown() {
    for mode in [
        "call-tool-error",
        "call-error",
        "call-malformed",
        "call-tail",
        "call-nonzero",
    ] {
        let fixture = Fixture::new(mode, 10_000);
        let auth = auth(&fixture);
        let host = host(&fixture);
        discover(&fixture, &host, &auth);
        let req = call(&fixture, mode, |_| {});
        dispatch(&host, &auth, &req).unwrap();
        let dir = directory(&fixture, &req);
        wait_file(&dir.join("workspace/effect.json"));
        assert_eq!(
            methods(&dir),
            ["initialize", "notifications/initialized", "tools/call"]
        );
        let terminal = finish(&host, &auth, &req);
        let known = matches!(mode, "call-tool-error" | "call-error");
        assert_eq!(
            terminal.state,
            if known {
                p::WorkerState::Failed
            } else {
                p::WorkerState::Unknown
            },
            "{mode}"
        );
        if known {
            assert!(!read_body(&host, &auth, &req).1.is_empty());
        }
        assert!(!dir.exists());
        assert_eq!(dispatch(&host, &auth, &req).unwrap().state, terminal.state);
    }
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_managed_mcp_call_rechecks_actual_write_and_preserves_result_after_revoke() {
    for mode in ["hold", "call-held-result"] {
        let fixture = Fixture::new(mode, 10_000);
        let auth = auth(&fixture);
        let host = host(&fixture);
        discover(&fixture, &host, &auth);
        let req = call(&fixture, mode, |_| {});
        dispatch(&host, &auth, &req).unwrap();
        let dir = directory(&fixture, &req);
        wait_file(&dir.join(if mode == "hold" {
            "workspace/started.pid"
        } else {
            "workspace/effect.json"
        }));
        if mode == "hold" {
            let until = Instant::now() + Duration::from_secs(2);
            while methods(&dir).is_empty() {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        fixture.revoke();
        fs::write(dir.join("workspace/continue"), []).unwrap();
        // Capture actual writes before the Host seals and removes the sandbox.
        std::thread::sleep(Duration::from_millis(30));
        let sent = methods(&dir);
        let terminal = finish(&host, &auth, &req);
        if mode == "hold" {
            assert_eq!(sent, ["initialize"]);
            assert_eq!(terminal.state, p::WorkerState::Failed);
            let (manifest, bytes) = read_body(&host, &auth, &req);
            assert_eq!(manifest.status, p::WorkerResultStatus::Empty);
            assert!(bytes.is_empty());
        } else {
            assert_eq!(
                sent,
                ["initialize", "notifications/initialized", "tools/call"]
            );
            assert_eq!(terminal.state, p::WorkerState::Completed);
            assert!(!read_body(&host, &auth, &req).1.is_empty());
        }
        assert_eq!(
            dispatch(&host, &auth, &req).unwrap().receipt_ref,
            terminal.receipt_ref
        );
    }
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_managed_mcp_call_cancel_after_effect_or_blocked_write_is_unknown_once() {
    for mode in ["call-hang", "call-no-read"] {
        let fixture = Fixture::new(mode, 10_000);
        let auth = auth(&fixture);
        let host = host(&fixture);
        discover(&fixture, &host, &auth);
        let req = call(&fixture, mode, |intent| {
            if mode == "call-no-read" {
                let p::ActionParameters::Mcp { arguments, .. } = &mut intent.parameters else {
                    unreachable!()
                };
                *arguments = json!({"value":"x".repeat(65_536)});
            }
        });
        dispatch(&host, &auth, &req).unwrap();
        let dir = directory(&fixture, &req);
        if mode == "call-hang" {
            wait_file(&dir.join("workspace/effect.json"));
        } else {
            wait_file(&dir.join("workspace/started.pid"));
            let until = Instant::now() + Duration::from_secs(2);
            while methods(&dir).len() < 2 {
                assert!(Instant::now() < until);
                std::thread::sleep(Duration::from_millis(2));
            }
            // The child remains alive and never reads the call; a large pipe
            // write must be interrupted by the independent original Job guard.
            std::thread::sleep(Duration::from_millis(40));
        }
        let receipt = host
            .cancel(&auth, &req.job.job_id, recovery_timestamp())
            .unwrap();
        assert_eq!(receipt.outcome, p::WorkerOutcome::Unknown);
        assert!(!dir.exists());
        let replay = dispatch(&host, &auth, &req).unwrap();
        assert_eq!(replay.state, p::WorkerState::Unknown);
        assert!(!directory(&fixture, &req).exists());
    }
}

#[test]
#[ignore = "explicit native fixture; Host component, not installed acceptance"]
fn native_managed_mcp_new_pending_discovery_shadows_old_completed_metadata() {
    let fixture = Fixture::new("normal", 10_000);
    let auth = auth(&fixture);
    let host = host(&fixture);
    discover(&fixture, &host, &auth);
    let mut newer = request(&fixture.registration, &fixture.settings);
    newer.job.job_id = "job:newer-pending".into();
    newer.job.permit_ref = "permit:newer-pending".into();
    newer.job.nonce = p::Nonce("nonce:newer-pending".into());
    newer.permit.job_id = newer.job.job_id.clone();
    newer.permit.permit_ref = newer.job.permit_ref.clone();
    newer.permit.nonce = newer.job.nonce.clone();
    host.journal
        .admit_with_execution(
            newer.job.clone(),
            newer.permit.clone(),
            newer.execution.as_ref(),
            newer.permit.issued_at,
        )
        .unwrap();
    let req = call(&fixture, "stale-discovery", |_| {});
    assert!(dispatch(&host, &auth, &req).is_err());
    assert!(!directory(&fixture, &req).exists());
    host.journal
        .mark_unknown(
            &newer.job.job_id,
            "discovery:unknown".into(),
            recovery_timestamp(),
        )
        .unwrap();
    assert!(dispatch(&host, &auth, &req).is_err());
    assert!(!directory(&fixture, &req).exists());
}
