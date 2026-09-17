use std::sync::Arc;

use forme_capabilities::{
    Capability, CapabilityDescriptor, CapabilityRegistry, Hook, HookPoint, HookTransform,
    InMemoryCapabilityRegistry, LocalPlugin, LocalPluginRegistry, McpAllowlist,
    PluginConfigContribution, PluginContribution, PluginManifest, PluginReloadOutcome,
    PluginRuntimeState, SkillBody, SkillDefinition, SkillMetadata, StdioMcpServer,
};
use forme_policy::{
    ActionMatcher, ArgMatcher, DefaultPolicyEngine, PolicyContext, PolicyEngine, PolicyLayer,
    PolicyLayerSource, PolicyRule,
};
use forme_protocol as p;

#[test]
fn s5_manifest_contributions_are_governed_and_disabled_plugins_leave_the_toolset() {
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let plugin = plugin();
    let id = plugin.id();
    let registry =
        LocalPluginRegistry::with_plugins(vec![plugin], Arc::clone(&capabilities)).unwrap();
    registry.configure(id.clone()).unwrap();
    registry.enable(id.clone()).unwrap();
    registry
        .bind_trust(id.clone(), p::TrustTier::ApprovedSource, p::Actor::Owner)
        .unwrap();
    registry.grant(id.clone(), envelope()).unwrap();

    let context = resolve_context(id.clone());
    let activation = registry.activate(&context).unwrap();
    assert_eq!(activation.skills.len(), 1);
    assert_eq!(activation.mcp_servers.len(), 1);
    assert_eq!(activation.config.len(), 1);
    let toolset = CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context).unwrap();
    assert!(toolset
        .items
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:tool".into())));
    assert!(toolset
        .items
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:skill".into())));
    assert!(toolset
        .items
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:hook".into())));
    assert!(!toolset
        .items
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:nested-mcp".into())));

    assert_eq!(
        capabilities
            .take_events()
            .iter()
            .map(p::EventPayload::kind)
            .collect::<Vec<_>>(),
        vec![
            p::EventKind::CapabilityIndexed,
            p::EventKind::CapabilityIndexed,
            p::EventKind::CapabilityIndexed,
            p::EventKind::ToolsetResolved,
        ]
    );

    let events = registry.take_events();
    assert_eq!(
        events.iter().map(p::EventPayload::kind).collect::<Vec<_>>(),
        vec![
            p::EventKind::PluginContributionRegistered,
            p::EventKind::PluginToggled,
            p::EventKind::PluginToggled,
        ]
    );
    let registered = events
        .iter()
        .find_map(|event| match event {
            p::EventPayload::PluginContributionRegistered(payload) => Some(payload),
            _ => None,
        })
        .unwrap();
    assert_eq!(registered.contributions.len(), 5);
    assert!(events
        .iter()
        .any(|event| matches!(event, p::EventPayload::PluginToggled(_))));

    registry.disable(id).unwrap();
    assert!(
        CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context)
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        registry
            .take_events()
            .iter()
            .map(p::EventPayload::kind)
            .collect::<Vec<_>>(),
        vec![p::EventKind::PluginToggled]
    );
}

#[test]
fn s5_plugin_hook_can_only_propose_and_the_result_is_rechecked_by_policy() {
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let plugin = plugin();
    let id = plugin.id();
    let registry = LocalPluginRegistry::with_plugins(vec![plugin], capabilities).unwrap();
    registry.configure(id.clone()).unwrap();
    registry.enable(id.clone()).unwrap();
    registry
        .bind_trust(id.clone(), p::TrustTier::ApprovedSource, p::Actor::Owner)
        .unwrap();
    registry.grant(id.clone(), envelope()).unwrap();
    registry.activate(&resolve_context(id)).unwrap();

    let proposal = registry
        .apply_hook(p::HookRef("plugin:hook".into()), shell_intent("git"))
        .unwrap();
    assert_eq!(proposal.requires_policy_recheck, p::RequiredTrue);
    let p::ActionParameters::Shell { program, .. } = &proposal.intent.parameters else {
        panic!("hook fixture must remain a shell proposal");
    };
    assert_eq!(program, "blocked-program");

    let engine = DefaultPolicyEngine;
    let evaluation = engine.evaluate_detailed(&policy_context(&engine), &proposal.intent);
    assert_eq!(evaluation.decision, p::PolicyDecision::Deny);
    assert!(matches!(
        evaluation.event_payload(),
        p::EventPayload::ToolPolicyEvaluated(_)
    ));
}

#[test]
fn s37_plugin_reload_is_atomic_and_runtime_failure_is_isolated() {
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let alpha = plugin();
    let beta = beta_plugin();
    let alpha_id = alpha.id();
    let beta_id = beta.id();
    let registry =
        LocalPluginRegistry::with_plugins(vec![alpha.clone(), beta], Arc::clone(&capabilities))
            .unwrap();
    let envelope = lifecycle_envelope();
    for id in [&alpha_id, &beta_id] {
        registry.configure(id.clone()).unwrap();
        registry.enable(id.clone()).unwrap();
        registry
            .bind_trust(id.clone(), p::TrustTier::ApprovedSource, p::Actor::Owner)
            .unwrap();
        registry.grant(id.clone(), envelope.clone()).unwrap();
    }
    let context = lifecycle_context();
    registry.activate(&context).unwrap();
    let initial = registry.status(&alpha_id).unwrap();
    assert_eq!(initial.state, PluginRuntimeState::Active);
    assert!(initial.generation > 0);

    let mut replacement = alpha;
    replacement.manifest.manifest_ref = p::PluginManifestRef("manifest-local-v2".into());
    let PluginContribution::Tool(tool) = &mut replacement.manifest.contributions[0] else {
        panic!("alpha fixture starts with a tool contribution");
    };
    tool.id = p::CapabilityRef("plugin:tool-v2".into());
    tool.capability = Capability::Tool(p::ToolRef("plugin-tool-v2".into()));
    let report = registry.reload(replacement.clone(), &context);
    assert_eq!(report.outcome, PluginReloadOutcome::Activated);
    assert!(report.generation > initial.generation);
    let toolset = CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context).unwrap();
    assert!(toolset
        .items
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:tool-v2".into())));
    assert!(toolset
        .items
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:beta-tool".into())));
    assert!(!toolset
        .items
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:tool".into())));

    let mut conflicting = replacement.clone();
    let PluginContribution::Hook(hook) = &mut conflicting.manifest.contributions[3] else {
        panic!("alpha fixture keeps its hook contribution at index three");
    };
    hook.id = p::HookRef("plugin:beta-hook".into());
    let retained_conflict = registry.reload(conflicting, &context);
    assert_eq!(
        retained_conflict.outcome,
        PluginReloadOutcome::RetainedPrevious
    );
    assert_eq!(retained_conflict.generation, report.generation);
    let after_conflict = CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context)
        .unwrap()
        .items;
    assert!(after_conflict
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:hook".into())));
    assert_eq!(
        after_conflict
            .iter()
            .filter(|item| item.id == p::CapabilityRef("plugin:beta-hook".into()))
            .count(),
        1
    );

    let mut invalid = replacement;
    invalid.manifest.manifest_ref = p::PluginManifestRef("manifest-local-invalid".into());
    invalid.manifest.contributions.clear();
    let retained = registry.reload(invalid, &context);
    assert_eq!(retained.outcome, PluginReloadOutcome::RetainedPrevious);
    assert_eq!(retained.generation, report.generation);
    assert!(retained.failure.is_some());
    let after_failed_reload =
        CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context).unwrap();
    assert!(after_failed_reload
        .items
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:tool-v2".into())));
    assert!(!after_failed_reload
        .items
        .iter()
        .any(|item| item.id == p::CapabilityRef("plugin:tool".into())));

    registry
        .mark_failed(alpha_id.clone(), "fixture plugin process stopped".into())
        .unwrap();
    let failed = registry.status(&alpha_id).unwrap();
    assert_eq!(failed.state, PluginRuntimeState::Failed);
    assert!(failed.last_failure.is_some());
    assert!(registry
        .apply_hook(p::HookRef("plugin:hook".into()), shell_intent("git"))
        .is_err());
    let isolated = CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context).unwrap();
    assert_eq!(
        isolated
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>(),
        vec![
            p::CapabilityRef("plugin:beta-hook".into()),
            p::CapabilityRef("plugin:beta-tool".into()),
        ]
    );
    assert_eq!(
        registry.status(&beta_id).unwrap().state,
        PluginRuntimeState::Active
    );

    let events = registry.take_events();
    assert!(events.iter().any(|event| matches!(
        event,
        p::EventPayload::PluginContributionRegistered(payload)
            if payload.manifest == p::PluginManifestRef("manifest-local-v2".into())
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        p::EventPayload::PluginToggled(payload)
            if payload.plugin == alpha_id && !payload.enabled
    )));
    let capability_events = capabilities.take_events();
    assert!(capability_events
        .iter()
        .any(|event| matches!(event, p::EventPayload::CapabilityIndexed(_))));
    assert!(capability_events
        .iter()
        .any(|event| matches!(event, p::EventPayload::ToolsetResolved(_))));
}

fn plugin() -> LocalPlugin {
    LocalPlugin {
        schema_version: p::SchemaVersion(1),
        manifest: PluginManifest {
            schema_version: p::SchemaVersion(1),
            id: p::PluginRef("plugin-local".into()),
            manifest_ref: p::PluginManifestRef("manifest-local".into()),
            trust: p::TrustTier::Untrusted,
            contributions: vec![
                PluginContribution::Tool(CapabilityDescriptor {
                    schema_version: p::SchemaVersion(1),
                    id: p::CapabilityRef("plugin:tool".into()),
                    capability: Capability::Tool(p::ToolRef("plugin-tool".into())),
                    scope: p::Scope("workspace:alpha".into()),
                    permissions: vec![p::PermissionRef("execute".into())],
                    risk: Some(p::Risk::Low),
                    enabled: true,
                }),
                PluginContribution::Skill(SkillDefinition {
                    schema_version: p::SchemaVersion(1),
                    metadata: SkillMetadata {
                        schema_version: p::SchemaVersion(1),
                        id: p::SkillRef("plugin:skill".into()),
                        summary: "local plugin skill".into(),
                        scope: p::Scope("workspace:alpha".into()),
                        version: p::Version(1),
                        trust: p::TrustTier::ApprovedSource,
                    },
                    body: SkillBody("plugin skill body".into()),
                }),
                PluginContribution::McpServer(StdioMcpServer {
                    schema_version: p::SchemaVersion(1),
                    provider_id: p::ProviderId("plugin-mcp".into()),
                    command: "nested-mcp".into(),
                    args: Vec::new(),
                    env: Vec::new(),
                    allowlist: McpAllowlist {
                        schema_version: p::SchemaVersion(1),
                        tools: Vec::new(),
                        resources: Vec::new(),
                    },
                    timeout: p::DurationMs(100),
                    declared_capabilities: p::CapabilitySet {
                        schema_version: p::SchemaVersion(1),
                        capabilities: vec![p::CapabilityRef("plugin:nested-mcp".into())],
                        permissions: vec![p::PermissionRef("execute".into())],
                    },
                }),
                PluginContribution::Hook(Hook {
                    schema_version: p::SchemaVersion(1),
                    id: p::HookRef("plugin:hook".into()),
                    point: HookPoint::BeforePolicy,
                    transform: HookTransform::ReplaceShellProgram("blocked-program".into()),
                }),
                PluginContribution::Config(PluginConfigContribution {
                    schema_version: p::SchemaVersion(1),
                    key: "plugin.local.mode".into(),
                    value: serde_json::json!("bounded"),
                }),
            ],
        },
    }
}

fn beta_plugin() -> LocalPlugin {
    LocalPlugin {
        schema_version: p::SchemaVersion(1),
        manifest: PluginManifest {
            schema_version: p::SchemaVersion(1),
            id: p::PluginRef("plugin-beta".into()),
            manifest_ref: p::PluginManifestRef("manifest-beta".into()),
            trust: p::TrustTier::Untrusted,
            contributions: vec![
                PluginContribution::Tool(CapabilityDescriptor {
                    schema_version: p::SchemaVersion(1),
                    id: p::CapabilityRef("plugin:beta-tool".into()),
                    capability: Capability::Tool(p::ToolRef("plugin-beta-tool".into())),
                    scope: p::Scope("workspace:alpha".into()),
                    permissions: vec![p::PermissionRef("execute".into())],
                    risk: Some(p::Risk::Low),
                    enabled: true,
                }),
                PluginContribution::Hook(Hook {
                    schema_version: p::SchemaVersion(1),
                    id: p::HookRef("plugin:beta-hook".into()),
                    point: HookPoint::BeforeExecution,
                    transform: HookTransform::Identity,
                }),
            ],
        },
    }
}

fn lifecycle_context() -> p::ResolveContext {
    p::ResolveContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session:m1c-plugin".into()),
        toolset: p::ToolsetRef("toolset:m1c-plugin".into()),
        envelope: lifecycle_envelope(),
        policy_allowed_providers: vec![
            p::ProviderId("plugin-local".into()),
            p::ProviderId("plugin-beta".into()),
        ],
        policy_allowed_capabilities: vec![
            p::CapabilityRef("plugin:tool".into()),
            p::CapabilityRef("plugin:tool-v2".into()),
            p::CapabilityRef("plugin:skill".into()),
            p::CapabilityRef("plugin:hook".into()),
            p::CapabilityRef("plugin:beta-hook".into()),
            p::CapabilityRef("plugin:beta-tool".into()),
        ],
    }
}

fn lifecycle_envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:alpha".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![
                p::CapabilityRef("plugin:tool".into()),
                p::CapabilityRef("plugin:tool-v2".into()),
                p::CapabilityRef("plugin:skill".into()),
                p::CapabilityRef("plugin:hook".into()),
                p::CapabilityRef("plugin:nested-mcp".into()),
                p::CapabilityRef("plugin:beta-hook".into()),
                p::CapabilityRef("plugin:beta-tool".into()),
            ],
            permissions: vec![p::PermissionRef("execute".into())],
        },
        action_type: vec![p::ActionType::Execute],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("plugin-m1c-budget".into()),
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

fn resolve_context(id: p::PluginRef) -> p::ResolveContext {
    p::ResolveContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session-1".into()),
        toolset: p::ToolsetRef("toolset-plugin".into()),
        envelope: envelope(),
        policy_allowed_providers: vec![p::ProviderId(id.0)],
        policy_allowed_capabilities: vec![
            p::CapabilityRef("plugin:tool".into()),
            p::CapabilityRef("plugin:skill".into()),
            p::CapabilityRef("plugin:hook".into()),
        ],
    }
}

fn envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:alpha".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![
                p::CapabilityRef("plugin:tool".into()),
                p::CapabilityRef("plugin:skill".into()),
                p::CapabilityRef("plugin:hook".into()),
                p::CapabilityRef("plugin:nested-mcp".into()),
            ],
            permissions: vec![p::PermissionRef("execute".into())],
        },
        action_type: vec![p::ActionType::Execute],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("plugin-budget".into()),
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

fn shell_intent(program: &str) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("plugin-hook-intent".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("inspect repository".into()),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("plugin:tool".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace:alpha".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: program.into(),
            args: vec!["status".into()],
            cwd: Some("C:/workspace/alpha".into()),
            network: false,
        },
        requested_permissions: vec![p::PermissionRef("execute".into())],
        requested_at: 10,
        estimated_output_bytes: 512,
        estimated_duration: p::DurationMs(100),
    }
}

fn policy_context(engine: &DefaultPolicyEngine) -> PolicyContext {
    PolicyContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session-1".into()),
        toolset: p::ToolsetRef("toolset-plugin".into()),
        policy: engine.merge(&[PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::Managed,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                matcher: ActionMatcher {
                    parameters: ArgMatcher::Any,
                    ..ActionMatcher::default()
                },
                effect: p::PolicyDecision::Allow,
                scope: p::Scope("workspace:alpha".into()),
            }],
        }]),
        visible_capabilities: vec![p::CapabilityRef("plugin:tool".into())],
        granted_permissions: vec![p::PermissionRef("execute".into())],
        allowed_scopes: vec![p::Scope("workspace:alpha".into())],
        shell_allowlist: vec!["git".into()],
        file_roots: vec!["C:/workspace/alpha".into()],
        mcp_allowlist: Vec::new(),
        external: forme_policy::ExternalPolicyLimits::default(),
        network_allowed: false,
        sandbox_available: true,
        delegation: None,
        envelope: None,
    }
}
