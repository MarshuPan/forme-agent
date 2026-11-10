//! Discovery requires a request-specific owner ingress rule, then exact approval.
use crate::{p, ArgMatcher, PolicyContext, PolicyLayerSource};

pub(crate) fn recheck(
    context: &PolicyContext,
    intent: &p::ActionIntent,
    spec: &p::ManagedMcpDiscoverySpec,
) -> Option<&'static str> {
    if spec.validate_intent(intent).is_err()
        || !context.sandbox_available
        || context
            .delegation
            .as_ref()
            .is_some_and(|grant| grant.subject != crate::DelegationSubject::Owner)
    {
        return Some("managed MCP discovery intent or sandbox is invalid");
    }
    // Envelope approval routing must not mask an effective deny.
    if context.policy.layers().iter().any(|layer| {
        layer.rules.iter().any(|rule| {
            rule.effect == p::PolicyDecision::Deny
                && crate::scope_contains(&rule.scope, &intent.scope)
                && rule.matcher.matches(intent)
        })
    }) {
        return Some("managed MCP discovery is denied by the effective policy");
    }
    // A generic MCP allowlist, broad Any rule, or tool grant is not a discovery
    // request. Only authenticated owner ingress may insert this exact Session
    // rule; model/formal-tool paths cannot construct discovery intents.
    let explicit = context.policy.layers().iter().any(|layer| {
        layer.schema_version == p::SchemaVersion(1)
            && layer.source == PolicyLayerSource::Session
            && layer.rules.iter().any(|rule| {
                rule.schema_version == p::SchemaVersion(1)
                    && rule.effect == p::PolicyDecision::Ask
                    && rule.scope == intent.scope
                    && rule.matcher.backend == Some(p::BackendKind::Mcp)
                    && rule.matcher.capability.as_ref() == Some(&intent.capability_ref)
                    && rule.matcher.action_type == Some(p::ActionType::Execute)
                    && matches!(
                        rule.matcher.parameters,
                        ArgMatcher::McpDiscoveryTarget { .. }
                    )
                    && rule.matcher.matches(intent)
            })
    });
    if !explicit {
        return Some("managed MCP discovery has no exact owner request policy");
    }
    None
}
