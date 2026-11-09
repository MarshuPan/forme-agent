use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use forme_capabilities::{
    CapabilityRegistry, InMemoryCapabilityRegistry, McpAllowlist, McpErrorClass, McpRegistry,
    StdioMcpRegistry, StdioMcpServer,
};
use forme_protocol as p;

fn transport_test() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct ScratchDirectory(std::path::PathBuf);

impl ScratchDirectory {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "forme-mcp-discovery-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn s3_stdio_discovery_allowlist_prepare_call_and_disable_are_governed() {
    let _serial = transport_test();
    let server = server("success", 1_000);
    let id = server.id();
    let registry = activate(server, envelope());

    let tools = McpRegistry::discover(&registry, id.clone()).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "read_note");
    assert!(tools[0].input_schema.is_none());
    assert!(tools[0].schema_digest.is_none());
    let discovery = registry.discovery(&id).unwrap();
    assert_eq!(discovery.resources.len(), 1);
    assert_eq!(discovery.resources[0].uri, "note://allowed");
    assert!(matches!(
        registry.take_events().as_slice(),
        [p::EventPayload::McpDiscovered(_)]
    ));

    assert!(registry
        .try_prepare_call(
            tools[0].tool_ref.clone(),
            serde_json::json!({ "path": "notes/today.md" }),
        )
        .is_err());
    let selected = registry.resolve_schema(tools[0].tool_ref.clone()).unwrap();
    assert!(selected.input_schema.is_some());
    assert!(selected.schema_digest.is_some());
    let intent = registry
        .try_prepare_call(
            tools[0].tool_ref.clone(),
            serde_json::json!({ "path": "notes/today.md" }),
        )
        .unwrap();
    assert_eq!(intent.backend_hint, p::BackendKind::Mcp);
    assert_eq!(intent.capability_ref, capability_ref());
    let p::ActionParameters::Mcp {
        server,
        tool,
        schema_digest,
        transport,
        stdio,
        ..
    } = intent.parameters
    else {
        panic!("MCP prepare_call must produce normalized MCP parameters");
    };
    assert_eq!(server, p::McpServerRef(id.0.clone()));
    assert_eq!(tool, p::ToolRef("read_note".into()));
    assert!(schema_digest.is_some());
    assert_eq!(transport, p::McpTransport::Stdio);
    assert!(!stdio.command.is_empty());

    let capabilities = InMemoryCapabilityRegistry::default();
    let context = resolve_context(id.clone());
    registry.index_discovered(&capabilities, &context).unwrap();
    assert_eq!(
        CapabilityRegistry::resolve_toolset(&capabilities, &context)
            .unwrap()
            .items
            .len(),
        1
    );
    let capability_events = capabilities.take_events();
    assert_eq!(
        capability_events
            .iter()
            .map(p::EventPayload::kind)
            .collect::<Vec<_>>(),
        vec![
            p::EventKind::CapabilityIndexed,
            p::EventKind::ToolsetResolved,
        ]
    );

    registry.disable(id.clone()).unwrap();
    registry.index_discovered(&capabilities, &context).unwrap();
    assert!(CapabilityRegistry::resolve_toolset(&capabilities, &context)
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn s3_discovery_classifies_timeout_schema_and_server_errors() {
    let _serial = transport_test();
    for (mode, timeout_ms, expected) in [
        ("timeout", 150, McpErrorClass::Timeout),
        ("server", 1_000, McpErrorClass::ServerError),
    ] {
        let server = server(mode, timeout_ms);
        let id = server.id();
        let registry = activate(server, envelope());
        let failure = registry.discover_detailed(id).unwrap_err();
        assert_eq!(failure.class, expected, "mode={mode}");
    }
    let server = server("schema", 1_000);
    let id = server.id();
    let registry = activate(server, envelope());
    let tools = registry.discover_detailed(id).unwrap().tools;
    let failure = registry
        .resolve_schema_detailed(tools[0].tool_ref.clone())
        .unwrap_err();
    assert_eq!(failure.class, McpErrorClass::SchemaMismatch);
}

#[test]
fn mcp_discovery_rejects_oversized_frames_without_publishing_metadata() {
    let _serial = transport_test();
    let server = server("oversized-frame", 5_000);
    let id = server.id();
    let registry = activate(server, envelope());
    let error = registry.discover_detailed(id.clone()).unwrap_err();
    assert_eq!(error.class, McpErrorClass::SchemaMismatch);
    assert!(error.detail.contains("wire frame bound"));
    assert!(registry.discovery(&id).is_none());
    assert!(registry.take_events().is_empty());
}

#[test]
fn mcp_discovery_retains_original_io_and_never_publishes_uncertain_metadata() {
    let _serial = transport_test();
    let directory = ScratchDirectory::new();
    let ready = directory.path().join("discovery-ready");
    let done = directory.path().join("holder-done");
    let mut source = server("held-pipe", 500);
    source.env.push((
        "FORME_MCP_READY".into(),
        ready.to_string_lossy().into_owned(),
    ));
    source.env.push((
        "FORME_MCP_HOLDER_DONE".into(),
        done.to_string_lossy().into_owned(),
    ));
    let original_id = source.id();
    let original = activate(source, envelope());
    let started = Instant::now();
    let error = original.discover_detailed(original_id.clone()).unwrap_err();
    assert!(ready.exists());
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(error.class, McpErrorClass::ServerError);
    assert!(error.detail.contains("cleanup is unverified"));
    assert!(original.discovery(&original_id).is_none());
    assert!(original.take_events().is_empty());

    let source = server("success", 1_000);
    let id = source.id();
    let next = activate(source, envelope());
    let blocked = next.discover_detailed(id.clone()).unwrap_err();
    assert!(blocked.detail.contains("original cleanup"));
    assert!(next.discovery(&id).is_none());
    assert!(next.take_events().is_empty());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(done.exists());
    loop {
        match next.discover_detailed(id.clone()) {
            Ok(discovery) => {
                assert_eq!(discovery.tools.len(), 1);
                break;
            }
            Err(error)
                if error.detail.contains("original cleanup") && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("new discovery after original cleanup: {}", error.detail),
        }
    }
    assert!(matches!(
        next.take_events().as_slice(),
        [p::EventPayload::McpDiscovered(_)]
    ));
    assert!(original.discovery(&original_id).is_none());
    assert!(original.take_events().is_empty());
}

#[test]
fn mcp_fixture_child() {
    let Ok(mode) = std::env::var("FORME_MCP_FIXTURE_MODE") else {
        return;
    };
    if mode == "pipe-holder" {
        std::thread::sleep(Duration::from_secs(4));
        std::fs::write(std::env::var_os("FORME_MCP_HOLDER_DONE").unwrap(), b"done").unwrap();
        return;
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout).unwrap();
    stdout.flush().unwrap();
    for line in stdin.lock().lines() {
        let line = line.unwrap();
        let Ok(request) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(method) = request.get("method").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let id = request.get("id").cloned();
        if method == "notifications/initialized" {
            continue;
        }
        if mode == "timeout" {
            std::thread::sleep(Duration::from_secs(2));
            return;
        }
        if method == "tools/list" && mode == "oversized-frame" {
            let _ = stdout.write_all(&vec![b'x'; 8 * 1_024 * 1_024 + 1]);
            let _ = stdout.flush();
            return;
        }
        let response = match method {
            "initialize" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "serverInfo": { "name": "forme-fixture", "version": "1" }
                }
            }),
            "tools/list" if mode == "server" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32001, "message": "fixture failure" }
            }),
            "tools/list" if mode == "schema" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": [{ "name": "read_note" }] }
            }),
            "tools/list" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [
                        { "name": "read_note", "inputSchema": { "type": "object" } },
                        { "name": "delete_note", "inputSchema": { "type": "object" } }
                    ]
                }
            }),
            "resources/list" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "resources": [
                        { "uri": "note://allowed", "name": "allowed" },
                        { "uri": "note://hidden", "name": "hidden" }
                    ]
                }
            }),
            _ => continue,
        };
        writeln!(stdout, "{response}").unwrap();
        stdout.flush().unwrap();
        if method == "initialize" && mode == "held-pipe" {
            let mut holder = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "mcp_fixture_child",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env_clear()
                .env("FORME_MCP_FIXTURE_MODE", "pipe-holder")
                .env(
                    "FORME_MCP_HOLDER_DONE",
                    std::env::var_os("FORME_MCP_HOLDER_DONE").unwrap(),
                )
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            std::fs::write(std::env::var_os("FORME_MCP_READY").unwrap(), b"ready").unwrap();
            holder.wait().unwrap();
            return;
        }
        if method == "resources/list" || (method == "tools/list" && mode == "server") {
            return;
        }
    }
}

fn server(mode: &str, timeout_ms: u64) -> StdioMcpServer {
    let executable = std::env::current_exe().unwrap();
    StdioMcpServer {
        schema_version: p::SchemaVersion(1),
        provider_id: p::ProviderId(format!("mcp-{mode}")),
        command: executable.to_string_lossy().into_owned(),
        args: vec![
            "--exact".into(),
            "mcp_fixture_child".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ],
        env: vec![("FORME_MCP_FIXTURE_MODE".into(), mode.into())],
        allowlist: McpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: vec!["read_note".into()],
            resources: vec!["note://allowed".into()],
        },
        timeout: p::DurationMs(timeout_ms),
        declared_capabilities: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![capability_ref()],
            permissions: vec![p::PermissionRef("execute".into())],
        },
    }
}

fn activate(server: StdioMcpServer, grant: p::AutonomyEnvelope) -> StdioMcpRegistry {
    let id = server.id();
    let registry = StdioMcpRegistry::with_servers(vec![server]).unwrap();
    registry.configure(id.clone()).unwrap();
    registry.enable(id.clone()).unwrap();
    registry
        .bind_trust(id.clone(), p::TrustTier::ApprovedSource, p::Actor::Owner)
        .unwrap();
    registry.grant(id, grant).unwrap();
    registry
}

fn resolve_context(id: p::ProviderId) -> p::ResolveContext {
    p::ResolveContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session-1".into()),
        toolset: p::ToolsetRef("toolset-mcp".into()),
        envelope: envelope(),
        policy_allowed_providers: vec![id],
        policy_allowed_capabilities: vec![capability_ref()],
    }
}

fn envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:alpha".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![capability_ref()],
            permissions: vec![p::PermissionRef("execute".into())],
        },
        action_type: vec![p::ActionType::Execute],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("mcp-budget".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 0,
            expires_at: i64::MAX,
            max_turns: 3,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

fn capability_ref() -> p::CapabilityRef {
    p::CapabilityRef("mcp:mcp-success:read_note".into())
}
