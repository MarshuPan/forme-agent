//! Real MCP effects survive lost responses; the original run must wait for adjudication.
use forme_capabilities::{McpAllowlist, StdioMcpRegistry, StdioMcpServer};
use forme_execution::{ExecutionBackendRegistry, McpBackend, OutputBudget};
use forme_harness::{
    AgentHarness, GatewayControl, GovernanceConfig, HarnessConfig, ReactiveHarness,
};
use forme_loop::LoopState;
use forme_models::{
    Cost, ModelCapability, ModelOutput, ModelProfile, ModelProvider, ModelRequest, ModelResponse,
    ModelStrength, ModelToolCall, RateLimit, ScriptedModelProvider, Url,
};
use forme_policy::{
    ActionMatcher, ArgMatcher, DelegationGrant, DelegationSubject, PolicyLayer, PolicyLayerSource,
    PolicyRule,
};
use forme_protocol as p;
use forme_store::{
    EventStore, FileContentKeyResolver, SqliteEventStore, StoreContentProtection, StoreOptions,
};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    directory: PathBuf,
    registry: Arc<StdioMcpRegistry>,
    intent: p::ActionIntent,
    envelope: p::AutonomyEnvelope,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "forme-mcp-unknown-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&directory).unwrap();
        let provider = p::ProviderId("mcp-effect-test".into());
        let capability = p::CapabilityRef("mcp:mcp-effect-test:write_marker".into());
        let permission = p::PermissionRef("execute".into());
        let envelope = p::AutonomyEnvelope {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("workspace:default".into()),
            capability: p::CapabilitySet {
                schema_version: p::SchemaVersion(1),
                capabilities: vec![capability],
                permissions: vec![permission],
            },
            action_type: vec![p::ActionType::Execute],
            risk_limit: p::Risk::Low,
            approval_rule: p::ApprovalRule::Allow,
            budget: p::Budget("bounded-mcp-test".into()),
            timebox: p::Timebox {
                schema_version: p::SchemaVersion(1),
                starts_at: now().saturating_sub(1_000),
                expires_at: now().saturating_add(120_000),
                max_turns: 4,
            },
            rollback: p::RollbackReq {
                schema_version: p::SchemaVersion(1),
                required: false,
                boundary: None,
            },
        };
        let server = StdioMcpServer {
            schema_version: p::SchemaVersion(1),
            provider_id: provider.clone(),
            command: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            args: vec![
                "--exact".into(),
                "mcp_effect_fixture_child".into(),
                "--nocapture".into(),
                "--test-threads=1".into(),
            ],
            env: vec![
                ("FORME_HARNESS_MCP_MODE".into(), mode.into()),
                (
                    "FORME_HARNESS_MCP_EFFECT".into(),
                    directory.join("effect").to_string_lossy().into_owned(),
                ),
            ],
            allowlist: McpAllowlist {
                schema_version: p::SchemaVersion(1),
                tools: vec!["write_marker".into()],
                resources: Vec::new(),
            },
            timeout: p::DurationMs(1_500),
            declared_capabilities: envelope.capability.clone(),
        };
        let registry = Arc::new(StdioMcpRegistry::with_servers(vec![server]).unwrap());
        registry.configure(provider.clone()).unwrap();
        registry.enable(provider.clone()).unwrap();
        registry
            .bind_trust(
                provider.clone(),
                p::TrustTier::ApprovedSource,
                p::Actor::Owner,
            )
            .unwrap();
        registry.grant(provider.clone(), envelope.clone()).unwrap();
        let discovery = registry.refresh(provider).unwrap();
        let tool = registry
            .resolve_schema(discovery.tools[0].tool_ref.clone())
            .unwrap();
        let intent = registry
            .try_prepare_call(tool.tool_ref, serde_json::json!({}))
            .unwrap();
        Self {
            directory,
            registry,
            intent,
            envelope,
        }
    }

    fn open(&self) -> SqliteEventStore {
        let protection = StoreContentProtection::new(
            &p::VerifiedPrincipal("owner:mcp-test".into()),
            self.envelope.scope.clone(),
            Arc::new(FileContentKeyResolver::new(self.directory.join("keys")).unwrap()),
        )
        .unwrap();
        SqliteEventStore::open_with_content_protection(
            self.directory.join("events.sqlite"),
            StoreOptions::default(),
            protection,
        )
        .unwrap()
    }

    fn harness(&self, store: SqliteEventStore) -> (Arc<ReactiveHarness>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = CountModel {
            calls: calls.clone(),
            inner: ScriptedModelProvider::new(
                profile(),
                vec![
                    response(ModelOutput::Tool(Box::new(ModelToolCall {
                        schema_version: p::SchemaVersion(1),
                        call_id: p::ToolCallId("marker-call".into()),
                        tool: p::ToolRef("write_marker".into()),
                        arguments: serde_json::json!({}),
                        intent: Some(self.intent.clone()),
                    }))),
                    response(ModelOutput::Final(
                        "a verified tool result may continue".into(),
                    )),
                ],
            )
            .unwrap(),
        };
        let backends = Arc::new(ExecutionBackendRegistry::default());
        backends
            .register(Arc::new(
                McpBackend::new(OutputBudget::truncate_at(4_096), p::DurationMs(1_500)).unwrap(),
            ))
            .unwrap();
        let p::ActionParameters::Mcp { server, tool, .. } = &self.intent.parameters else {
            unreachable!()
        };
        let governance = GovernanceConfig {
            schema_version: p::SchemaVersion(1),
            layers: vec![PolicyLayer {
                schema_version: p::SchemaVersion(1),
                source: PolicyLayerSource::User,
                rules: vec![PolicyRule {
                    schema_version: p::SchemaVersion(1),
                    matcher: ActionMatcher {
                        backend: Some(p::BackendKind::Mcp),
                        capability: Some(self.intent.capability_ref.clone()),
                        action_type: Some(p::ActionType::Execute),
                        parameters: ArgMatcher::McpTarget {
                            server: server.clone(),
                            tool: tool.clone(),
                        },
                    },
                    effect: p::PolicyDecision::Allow,
                    scope: self.intent.scope.clone(),
                }],
            }],
            visible_capabilities: vec![self.intent.capability_ref.clone()],
            granted_permissions: self.intent.requested_permissions.clone(),
            allowed_scopes: vec![self.intent.scope.clone()],
            mcp_allowlist: vec![format!("{}/{}", server.0, tool.0)],
            sandbox_available: true,
            delegation: Some(DelegationGrant {
                schema_version: p::SchemaVersion(1),
                subject: DelegationSubject::Owner,
                envelope: self.envelope.clone(),
                granted_by: p::Actor::Owner,
                audit_ref: p::EventId("delegation:mcp-effect-test".into()),
            }),
            envelope: Some(self.envelope.clone()),
            ..GovernanceConfig::default()
        };
        (
            Arc::new(
                ReactiveHarness::new(
                    store,
                    Arc::new(model),
                    backends,
                    governance,
                    HarnessConfig::for_model(&profile()),
                )
                .unwrap()
                .with_capability_rechecker(self.registry.clone()),
            ),
            calls,
        )
    }

    fn assert_one_effect(&self) {
        assert_eq!(
            std::fs::read(self.directory.join("effect")).unwrap(),
            b"effect\n"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }
}

#[test]
fn lost_mcp_response_suspends_before_model_continuation_and_survives_restart() {
    assert_unknown_lifecycle("lost-response", false);
}

#[test]
fn invalid_mcp_response_suspends_before_model_continuation_and_survives_restart() {
    assert_unknown_lifecycle("invalid-response", false);
}

#[test]
fn cancel_after_mcp_effect_preserves_unknown_instead_of_aborting_run() {
    assert_unknown_lifecycle("lost-response", true);
}

fn assert_unknown_lifecycle(mode: &str, cancel_after_effect: bool) {
    let fixture = Fixture::new(mode);
    let store = fixture.open();
    let (harness, calls) = fixture.harness(store.clone());
    let request = request();
    let run = harness.clone().start_run(request.clone()).unwrap();
    if cancel_after_effect {
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::fs::read(fixture.directory.join("effect"))
            .ok()
            .as_deref()
            != Some(b"effect\n")
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        fixture.assert_one_effect();
        harness.cancel(run.clone()).unwrap();
    }
    let waited = harness.wait(run.clone());
    harness
        .drain(p::SessionId(request.session.0.clone()))
        .unwrap();
    fixture.assert_one_effect();
    assert_eq!(
        harness.run_summary(run.clone()).unwrap().status,
        p::RunStatus::Waiting,
        "an unobserved real effect must suspend the original run; wait={waited:?}"
    );
    assert!(waited.is_err());
    assert!(matches!(
        harness.state(run.clone()).unwrap(),
        LoopState::Suspended(_)
    ));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "Unknown must not enter another model turn"
    );
    assert_unresolved(&store, &run);
    harness.cancel(run.clone()).unwrap();
    assert_eq!(
        harness.run_summary(run.clone()).unwrap().status,
        p::RunStatus::Waiting,
        "a later cancel cannot adjudicate an already uncertain effect"
    );
    assert_unresolved(&store, &run);
    let before = events(&store, &run);
    assert_eq!(harness.submit_run(request.clone()).unwrap(), run);
    assert_eq!(events(&store, &run), before);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(harness);
    drop(store);

    let store = fixture.open();
    let (harness, restarted_calls) = fixture.harness(store.clone());
    assert_eq!(
        harness.run_summary(run.clone()).unwrap().status,
        p::RunStatus::Waiting
    );
    assert!(matches!(
        harness.state(run.clone()).unwrap(),
        LoopState::Suspended(_)
    ));
    assert_unresolved(&store, &run);
    let before = events(&store, &run);
    harness.recover_unknown_outcomes().unwrap();
    assert_eq!(harness.submit_run(request).unwrap(), run);
    assert_eq!(events(&store, &run), before);
    assert_eq!(restarted_calls.load(Ordering::SeqCst), 0);
    fixture.assert_one_effect();

    let command = p::RunControl::ResolveToolOutcome {
        command: p::IdempotencyKey("owner-confirm-original-effect".into()),
        resolution: p::ToolOutcomeResolution {
            schema_version: p::SchemaVersion(1),
            action: fixture.intent.intent_id.clone(),
            outcome: p::ToolOutcomeKind::Completed,
            result_ref: Some(p::ActionResultRef("owner-verified-original-marker".into())),
            failure_ref: None,
            reason: None,
            summary: "owner independently checked original marker".into(),
        },
    };
    harness.control(run.clone(), command.clone()).unwrap();
    let completed = events(&store, &run);
    harness.control(run.clone(), command).unwrap();
    assert_eq!(events(&store, &run), completed);
    assert_eq!(
        harness.wait(run.clone()).unwrap().status,
        p::RunStatus::Complete
    );
    assert_eq!(restarted_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        completed
            .iter()
            .filter(|e| e.kind == p::EventKind::ActionStarted)
            .count(),
        1
    );
    assert_eq!(
        completed
            .iter()
            .filter(|e| e.kind == p::EventKind::ActionCompleted)
            .count(),
        1
    );
    fixture.assert_one_effect();
}

#[test]
fn confirmed_mcp_success_and_protocol_failure_continue_normally() {
    for mode in ["success", "server-error"] {
        let fixture = Fixture::new(mode);
        let store = fixture.open();
        let (harness, calls) = fixture.harness(store.clone());
        let run = harness.submit_run(request()).unwrap();
        assert_eq!(
            harness.wait(run.clone()).unwrap().status,
            p::RunStatus::Complete
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        fixture.assert_one_effect();
        let events = events(&store, &run);
        assert!(!events
            .iter()
            .any(|e| e.kind == p::EventKind::ActionOutcomeUnknown));
        let expected = if mode == "success" {
            p::EventKind::ActionCompleted
        } else {
            p::EventKind::ActionFailed
        };
        assert_eq!(events.iter().filter(|e| e.kind == expected).count(), 1);
    }
}

fn assert_unresolved(store: &SqliteEventStore, run: &p::RunId) {
    let events = events(store, run);
    for kind in [
        p::EventKind::ActionStarted,
        p::EventKind::ActionOutcomeUnknown,
        p::EventKind::RunWaiting,
    ] {
        assert_eq!(
            events.iter().filter(|e| e.kind == kind).count(),
            1,
            "{kind:?}"
        );
    }
    assert!(!events.iter().any(|e| matches!(
        e.kind,
        p::EventKind::ActionCompleted
            | p::EventKind::ActionFailed
            | p::EventKind::ActionCancelled
            | p::EventKind::RunComplete
            | p::EventKind::RunFailed
            | p::EventKind::RunAborted
    )));
}

fn events(store: &SqliteEventStore, run: &p::RunId) -> Vec<p::Event> {
    store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
}

fn request() -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef("session:mcp-unknown".into()),
        agent_profile: p::AgentProfileRef("agent:mcp-test".into()),
        input: p::RunInput("write one marker".into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey("command:original-marker".into())),
    }
}

struct CountModel {
    inner: ScriptedModelProvider,
    calls: Arc<AtomicUsize>,
}
impl p::ExternalProvider for CountModel {
    fn kind(&self) -> p::ProviderKind {
        p::ExternalProvider::kind(&self.inner)
    }
    fn id(&self) -> p::ProviderId {
        p::ExternalProvider::id(&self.inner)
    }
    fn declared_capabilities(&self) -> p::CapabilitySet {
        p::ExternalProvider::declared_capabilities(&self.inner)
    }
    fn trust_default(&self) -> p::TrustTier {
        p::ExternalProvider::trust_default(&self.inner)
    }
}
impl p::ModelProvider for CountModel {}
impl ModelProvider for CountModel {
    fn call(&self, request: ModelRequest) -> p::Result<ModelResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.call(request)
    }
    fn profile(&self) -> ModelProfile {
        self.inner.profile()
    }
}

fn profile() -> ModelProfile {
    ModelProfile {
        schema_version: p::SchemaVersion(1),
        provider: p::ProviderId("mcp-unknown-model".into()),
        model: "bounded-test".into(),
        base_url: Url::parse("https://mcp.invalid/v1").unwrap(),
        capability: ModelCapability {
            schema_version: p::SchemaVersion(1),
            context_window: 16_384,
            tool_use: true,
            strength: ModelStrength::Standard,
        },
        cost: Cost {
            schema_version: p::SchemaVersion(1),
            input_microunits_per_million: 1,
            output_microunits_per_million: 1,
        },
        rate_limit: RateLimit {
            schema_version: p::SchemaVersion(1),
            requests_per_minute: 60,
            tokens_per_minute: 100_000,
        },
        credential_ref: p::CredentialRef("secret:mcp-test".into()),
    }
}

fn response(output: ModelOutput) -> ModelResponse {
    ModelResponse {
        schema_version: p::SchemaVersion(1),
        output,
        usage: p::ModelUsage {
            input_tokens: 4,
            output_tokens: 2,
        },
        finish_reason: p::FinishReason("test".into()),
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn mcp_effect_fixture_child() {
    let Ok(mode) = std::env::var("FORME_HARNESS_MCP_MODE") else {
        return;
    };
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout).unwrap();
    stdout.flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        let Ok(request) = serde_json::from_str::<serde_json::Value>(&line.unwrap()) else {
            continue;
        };
        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let result = match method {
            "initialize" => serde_json::json!({"protocolVersion":"2025-06-18", "capabilities":{}}),
            "tools/list" => serde_json::json!({"tools":[{"name":"write_marker",
                "description":"write a test marker", "inputSchema":{"type":"object"}}]}),
            "resources/list" => serde_json::json!({"resources":[]}),
            "tools/call" => {
                let effect = std::env::var("FORME_HARNESS_MCP_EFFECT").unwrap();
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(effect)
                    .unwrap();
                file.write_all(b"effect\n").unwrap();
                file.sync_all().unwrap();
                if mode == "lost-response" {
                    std::thread::sleep(Duration::from_secs(5));
                    return;
                }
                if mode == "invalid-response" || mode == "server-error" {
                    let reply = if mode == "invalid-response" {
                        serde_json::json!({"jsonrpc":"2.0", "id":id})
                    } else {
                        serde_json::json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32000, "message":"fixture failure"}})
                    };
                    writeln!(stdout, "{reply}").unwrap();
                    stdout.flush().unwrap();
                    return;
                }
                serde_json::json!({"content":[{"type":"text", "text":"marker written"}], "isError":false})
            }
            _ => continue,
        };
        let reply = serde_json::json!({"jsonrpc":"2.0", "id":id, "result":result});
        writeln!(stdout, "{reply}").unwrap();
        stdout.flush().unwrap();
        if method == "tools/call" || method == "resources/list" {
            return;
        }
    }
}
