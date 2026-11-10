use super::*;
use forme_harness::GatewayControl;

fn mcp() -> p::ProductMcpServerConfig {
    p::ProductMcpServerConfig {
        schema_version: p::SchemaVersion(1),
        server_ref: "fixture-discovery".into(),
        program_ref: format!("mcp-program:{}", "b".repeat(64)),
        scope: p::Scope("workspace:managed-test".into()),
        enabled: true,
        trust: p::ProductSkillTrust::OwnerInput,
        timeout: p::DurationMs(1000),
        output_budget: 8192,
        allowlist: p::ProductMcpAllowlist {
            schema_version: p::SchemaVersion(1),
            tools: vec!["fixture.read".into()],
            resources: vec![],
        },
    }
}

fn configure(temp: &Temporary) -> CoreServiceConfig {
    let mut config = temp.configured();
    config.brain.managed_capabilities = Some(p::ProductManagedCapabilitySettings {
        schema_version: p::SchemaVersion(1),
        mcp_servers: vec![mcp()],
        plugins: vec![],
        publisher_keys: vec![],
        computer: Default::default(),
    });
    config
}

#[test]
fn managed_mcp_mapping_carries_only_declared_scope_budget_and_owner_grant() {
    let temp = Temporary::new();
    let mut config = configure(&temp);
    for (enabled, trust, eligible) in [
        (true, p::ProductSkillTrust::OwnerInput, true),
        (false, p::ProductSkillTrust::OwnerInput, false),
        (true, p::ProductSkillTrust::Untrusted, false),
    ] {
        let settings = &mut config
            .brain
            .managed_capabilities
            .as_mut()
            .unwrap()
            .mcp_servers[0];
        settings.enabled = enabled;
        settings.trust = trust;
        let runtime = config.brain.runtime_config().unwrap();
        let entry = &runtime.capabilities.mcp[0];
        assert_eq!(entry.enabled, enabled);
        assert_eq!(entry.server.command, mcp().program_ref);
        assert!(
            entry.server.args.is_empty()
                && entry.server.env.is_empty()
                && entry.inherit_env.is_empty()
        );
        assert_eq!(entry.grant.is_some(), eligible);
        if let Some(grant) = &entry.grant {
            assert_eq!(grant.scope, mcp().scope);
            assert_eq!(grant.approval_rule, p::ApprovalRule::Ask);
            assert_eq!(
                grant.capability.capabilities,
                vec![p::CapabilityRef(
                    "mcp:fixture-discovery:fixture.read".into()
                )]
            );
        }
    }
}

#[test]
fn managed_production_discovery_requires_native_identity_or_restores_original_pending() {
    let temp = Temporary::new();
    let mut config = configure(&temp);
    let secret = temp.0.join("worker.token");
    std::fs::write(&secret, "repository-worker-token-0123456789abcdef").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(windows)]
    let endpoint = serde_json::json!({"transport":"windows_named_pipe",
        "name":format!(r"\\.\pipe\forme-agent-execution-v1-fixture-{}", std::process::id())});
    #[cfg(not(windows))]
    let endpoint = serde_json::json!({"transport":"loopback_tcp", "address":"127.0.0.1:9"});
    config.brain.worker_binding = Some(
        serde_json::from_value(serde_json::json!({
            "schema_version":1, "endpoint":endpoint, "credential_path":secret,
            "authority_epoch":1, "sandbox_profile_ref":p::MANAGED_MCP_PROGRAM_PROFILE,
            "backend_kinds":[p::BackendKind::Mcp]
        }))
        .unwrap(),
    );
    let runtime = ProductionRuntimeBuilder::from_managed_config(config.brain.clone())
        .unwrap()
        .build()
        .unwrap();
    let session = p::SessionId("session:managed-discovery".into());
    let settings = mcp();
    let result = runtime.harness().discover_managed_mcp(
        &p::VerifiedPrincipal(config.brain.owner.0.clone()),
        session.clone(),
        p::IdempotencyKey("command:managed-discovery".into()),
        p::ManagedMcpDiscoverySpec {
            schema_version: p::SchemaVersion(1),
            server: p::McpServerRef(settings.server_ref.clone()),
            program_ref: settings.program_ref.clone(),
            registration_digest: p::SchemaDigest(format!("sha256:{}", "b".repeat(64))),
            settings_digest: p::managed_mcp_settings_digest(&settings).unwrap(),
            timeout: settings.timeout,
            output_budget_bytes: settings.output_budget,
        },
    );
    // Ordinary Windows test processes cannot impersonate the installed Brain
    // Service SID. Its rejected native binding must never fall back locally.
    if cfg!(windows) {
        assert!(result.is_err());
        assert!(runtime
            .harness()
            .pending_approvals(session.clone())
            .unwrap()
            .is_empty());
        assert!(runtime.harness().list_runs().unwrap().is_empty());
        return;
    }
    let run = result.unwrap();
    let pending = runtime
        .harness()
        .pending_approvals(session.clone())
        .unwrap();
    assert_eq!(pending.len(), 1);
    let page = |runtime: &ProductionRuntime| {
        runtime
            .harness()
            .stream_event_page(p::EventCursor {
                schema_version: p::SchemaVersion(1),
                run: run.clone(),
                after_stream_seq: 0,
            })
            .unwrap()
            .events
    };
    let before = page(&runtime);
    assert!(before.iter().all(|e| !matches!(
        e.kind,
        p::EventKind::ActionStarted | p::EventKind::ModelCallStarted
    )));
    assert!(before.iter().any(|e| matches!(&e.payload, p::EventPayload::CompetenceGateEvaluated(v) if v.max_level == p::InterventionLevel::L0Observe)));
    drop(runtime);
    let reopened = ProductionRuntimeBuilder::from_managed_config(config.brain)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        reopened.harness().pending_approvals(session).unwrap(),
        pending
    );
    assert_eq!(page(&reopened), before);
}
