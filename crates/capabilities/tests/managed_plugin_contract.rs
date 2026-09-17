use std::collections::BTreeMap;
use std::sync::Arc;

use forme_capabilities::{
    plugin_manifest_digest, Capability, CapabilityDescriptor, CapabilityRegistry, Hook, HookPoint,
    HookTransform, InMemoryCapabilityRegistry, LocalPlugin, LocalPluginRegistry,
    ManagedPluginSignatureVerifier, PluginContribution, PluginManifest, PluginReloadOutcome,
};
use forme_protocol as p;

struct BoundSignatureVerifier;

impl ManagedPluginSignatureVerifier for BoundSignatureVerifier {
    fn verify(&self, _plugin: &LocalPlugin, binding: &p::ManagedPluginBinding) -> p::Result<()> {
        let expected = format!(
            "signature:{}:{}",
            binding.source_digest.0, binding.manifest_digest.0
        );
        if binding.signature.0 == expected {
            Ok(())
        } else {
            Err(p::Error(
                "managed plugin signature is not bound to source and manifest".into(),
            ))
        }
    }
}

fn plugin(id: &str, tool: &str, hook: &str) -> LocalPlugin {
    LocalPlugin {
        schema_version: p::SchemaVersion(1),
        manifest: PluginManifest {
            schema_version: p::SchemaVersion(1),
            id: p::PluginRef(id.into()),
            manifest_ref: p::PluginManifestRef(format!("manifest:{id}:{tool}")),
            trust: p::TrustTier::Untrusted,
            contributions: vec![
                PluginContribution::Tool(CapabilityDescriptor {
                    schema_version: p::SchemaVersion(1),
                    id: p::CapabilityRef(tool.into()),
                    capability: Capability::Tool(p::ToolRef(format!("tool:{tool}"))),
                    scope: p::Scope("workspace:alpha".into()),
                    permissions: vec![p::PermissionRef("permission:read".into())],
                    risk: Some(p::Risk::Low),
                    enabled: true,
                }),
                PluginContribution::Hook(Hook {
                    schema_version: p::SchemaVersion(1),
                    id: p::HookRef(hook.into()),
                    point: HookPoint::BeforeExecution,
                    transform: HookTransform::Identity,
                }),
            ],
        },
    }
}

fn envelope() -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:alpha".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![
                p::CapabilityRef("capability:alpha-v1".into()),
                p::CapabilityRef("capability:alpha-v2".into()),
                p::CapabilityRef("capability:beta".into()),
                p::CapabilityRef("hook:alpha".into()),
                p::CapabilityRef("hook:beta".into()),
            ],
            permissions: vec![p::PermissionRef("permission:read".into())],
        },
        action_type: vec![p::ActionType::Analyze],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget("units:4".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: 1,
            expires_at: 1_000,
            max_turns: 4,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

fn context() -> p::ResolveContext {
    p::ResolveContext {
        schema_version: p::SchemaVersion(1),
        session: p::SessionId("session:m2-c-managed".into()),
        toolset: p::ToolsetRef("toolset:m2-c-managed".into()),
        envelope: envelope(),
        policy_allowed_providers: vec![
            p::ProviderId("plugin:alpha".into()),
            p::ProviderId("plugin:beta".into()),
        ],
        policy_allowed_capabilities: vec![
            p::CapabilityRef("capability:alpha-v1".into()),
            p::CapabilityRef("capability:alpha-v2".into()),
            p::CapabilityRef("capability:beta".into()),
            p::CapabilityRef("hook:alpha".into()),
            p::CapabilityRef("hook:beta".into()),
        ],
    }
}

fn binding(plugin: &LocalPlugin, source: &str) -> p::ManagedPluginBinding {
    let manifest_digest = plugin_manifest_digest(plugin);
    let source_digest = p::SchemaDigest(format!("sha256:source:{source}"));
    p::ManagedPluginBinding {
        schema_version: p::SchemaVersion(1),
        source: p::ManagedPluginSourceRef(source.into()),
        source_digest: source_digest.clone(),
        manifest_digest: manifest_digest.clone(),
        signature: p::PluginSignatureRef(format!(
            "signature:{}:{}",
            source_digest.0, manifest_digest.0
        )),
    }
}

fn policy(version: u32, denied: Vec<p::PluginRef>) -> p::ManagedPluginPolicy {
    p::ManagedPluginPolicy {
        schema_version: p::SchemaVersion(1),
        reference: p::ManagedPluginPolicyRef("managed-policy:plugins".into()),
        version: p::Version(version),
        allowed_plugins: vec![
            p::PluginRef("plugin:alpha".into()),
            p::PluginRef("plugin:beta".into()),
        ],
        denied_plugins: denied,
        allowed_sources: vec![p::ManagedPluginSourceRef("source:admin".into())],
        revoked_manifests: Vec::new(),
        require_signature: true,
    }
}

fn prepare_registry(
    alpha: &LocalPlugin,
    beta: &LocalPlugin,
) -> (Arc<InMemoryCapabilityRegistry>, LocalPluginRegistry) {
    let capabilities = Arc::new(InMemoryCapabilityRegistry::default());
    let registry = LocalPluginRegistry::with_plugins(
        vec![alpha.clone(), beta.clone()],
        Arc::clone(&capabilities),
    )
    .unwrap();
    for plugin in [alpha, beta] {
        let id = plugin.id();
        registry.configure(id.clone()).unwrap();
        registry.enable(id.clone()).unwrap();
        registry
            .bind_trust(id.clone(), p::TrustTier::ApprovedSource, p::Actor::Owner)
            .unwrap();
        registry.grant(id, envelope()).unwrap();
    }
    registry.take_events();
    (capabilities, registry)
}

#[test]
fn s51_managed_plugin_policy_verifies_then_switches_atomically_and_revokes_without_ghosts() {
    let alpha = plugin("plugin:alpha", "capability:alpha-v1", "hook:alpha");
    let beta = plugin("plugin:beta", "capability:beta", "hook:beta");
    let (capabilities, registry) = prepare_registry(&alpha, &beta);
    let mut bindings = BTreeMap::from([
        (alpha.id(), binding(&alpha, "source:admin")),
        (beta.id(), binding(&beta, "source:admin")),
    ]);
    let initial_policy = policy(1, vec![beta.id()]);
    registry
        .activate_managed(
            &initial_policy,
            &bindings,
            &BoundSignatureVerifier,
            &context(),
        )
        .unwrap();
    let initial_generation = registry.status(&alpha.id()).unwrap().generation;
    assert!(initial_generation > 0);
    let toolset = CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context()).unwrap();
    assert!(toolset
        .items
        .iter()
        .any(|item| item.id.0 == "capability:alpha-v1"));
    assert!(!toolset
        .items
        .iter()
        .any(|item| item.id.0 == "capability:beta"));
    assert!(registry.activate(&context()).is_err());
    let managed_events = registry.take_events();
    assert!(managed_events.iter().any(|event| matches!(
        event,
        p::EventPayload::PluginContributionRegistered(payload)
            if payload.managed_snapshot.is_some()
    )));

    let mut bad_signature = bindings.clone();
    bad_signature.get_mut(&alpha.id()).unwrap().signature =
        p::PluginSignatureRef("signature:invalid".into());
    assert!(registry
        .activate_managed(
            &initial_policy,
            &bad_signature,
            &BoundSignatureVerifier,
            &context(),
        )
        .is_err());
    assert_eq!(
        registry.status(&alpha.id()).unwrap().generation,
        initial_generation
    );

    let replacement = plugin("plugin:alpha", "capability:alpha-v2", "hook:alpha");
    let bad_report = registry.reload_managed(
        replacement.clone(),
        &initial_policy,
        &bindings,
        &BoundSignatureVerifier,
        &context(),
    );
    assert_eq!(bad_report.outcome, PluginReloadOutcome::RetainedPrevious);
    assert_eq!(bad_report.generation, initial_generation);
    let still_old = CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context()).unwrap();
    assert!(still_old
        .items
        .iter()
        .any(|item| item.id.0 == "capability:alpha-v1"));
    assert!(!still_old
        .items
        .iter()
        .any(|item| item.id.0 == "capability:alpha-v2"));
    assert_eq!(
        registry.reload(replacement.clone(), &context()).outcome,
        PluginReloadOutcome::RetainedPrevious
    );

    bindings.insert(replacement.id(), binding(&replacement, "source:admin"));
    let good_report = registry.reload_managed(
        replacement,
        &initial_policy,
        &bindings,
        &BoundSignatureVerifier,
        &context(),
    );
    assert_eq!(good_report.outcome, PluginReloadOutcome::Activated);
    assert!(good_report.generation > initial_generation);
    let switched = CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context()).unwrap();
    assert!(switched
        .items
        .iter()
        .any(|item| item.id.0 == "capability:alpha-v2"));
    assert!(!switched
        .items
        .iter()
        .any(|item| item.id.0 == "capability:alpha-v1"));

    let revoke_policy = policy(2, vec![alpha.id(), beta.id()]);
    registry
        .activate_managed(
            &revoke_policy,
            &bindings,
            &BoundSignatureVerifier,
            &context(),
        )
        .unwrap();
    assert!(
        CapabilityRegistry::resolve_toolset(capabilities.as_ref(), &context())
            .unwrap()
            .items
            .is_empty()
    );
    assert!(registry
        .apply_hook(p::HookRef("hook:alpha".into()), shell_intent())
        .is_err());
    assert!(registry.take_events().iter().any(|event| matches!(
        event,
        p::EventPayload::PluginToggled(payload)
            if !payload.enabled
                && payload.managed_policy == Some(revoke_policy.reference.clone())
                && payload.managed_snapshot.is_some()
    )));
    assert!(registry
        .activate_managed(
            &initial_policy,
            &bindings,
            &BoundSignatureVerifier,
            &context(),
        )
        .is_err());
}

fn shell_intent() -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("intent:managed-hook".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("inspect".into()),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("capability:shell".into()),
        action_type: p::ActionType::Analyze,
        scope: p::Scope("workspace:alpha".into()),
        risk_hint: p::Risk::Low,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: "git".into(),
            args: vec!["status".into()],
            cwd: Some(".".into()),
            network: false,
        },
        requested_permissions: vec![p::PermissionRef("permission:read".into())],
        requested_at: 1,
        estimated_output_bytes: 128,
        estimated_duration: p::DurationMs(100),
    }
}
