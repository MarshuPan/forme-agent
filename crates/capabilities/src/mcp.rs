use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_policy::PolicyContext;
use forme_protocol as p;
use sha2::{Digest, Sha256};

use crate::{
    Capability, CapabilityDescriptor, CapabilityRegistry, CapabilitySource,
    InMemoryCapabilityRegistry,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpAllowlist {
    pub schema_version: p::SchemaVersion,
    pub tools: Vec<String>,
    pub resources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdioMcpServer {
    pub schema_version: p::SchemaVersion,
    pub provider_id: p::ProviderId,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub allowlist: McpAllowlist,
    pub timeout: p::DurationMs,
    pub declared_capabilities: p::CapabilitySet,
}

impl StdioMcpServer {
    pub fn id(&self) -> p::ProviderId {
        self.provider_id.clone()
    }

    fn source_ref(&self) -> p::CapabilitySourceRef {
        p::CapabilitySourceRef(format!("mcp:{}", self.provider_id.0))
    }
}

impl p::ExternalProvider for StdioMcpServer {
    fn kind(&self) -> p::ProviderKind {
        p::ProviderKind::Mcp
    }

    fn id(&self) -> p::ProviderId {
        self.provider_id.clone()
    }

    fn declared_capabilities(&self) -> p::CapabilitySet {
        self.declared_capabilities.clone()
    }

    fn trust_default(&self) -> p::TrustTier {
        p::TrustTier::Untrusted
    }
}

impl p::McpServer for StdioMcpServer {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpTool {
    pub schema_version: p::SchemaVersion,
    pub tool_ref: p::McpToolRef,
    pub capability_ref: p::CapabilityRef,
    pub server: p::ProviderId,
    pub name: String,
    pub summary: String,
    pub input_schema: Option<serde_json::Value>,
    pub schema_digest: Option<p::SchemaDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpResource {
    pub schema_version: p::SchemaVersion,
    pub server: p::ProviderId,
    pub uri: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpDiscovery {
    pub schema_version: p::SchemaVersion,
    pub server: p::ProviderId,
    pub tools: Vec<McpTool>,
    pub resources: Vec<McpResource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSearchQuery {
    pub schema_version: p::SchemaVersion,
    pub text: String,
    pub limit: usize,
    pub context: p::ResolveContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSearchHit {
    pub schema_version: p::SchemaVersion,
    pub tool: McpTool,
    pub score: u32,
    pub matched_terms: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpErrorClass {
    Timeout,
    SchemaMismatch,
    ServerError,
}

impl McpErrorClass {
    pub fn as_protocol_ref(self) -> p::McpErrorClass {
        let value = match self {
            Self::Timeout => "timeout",
            Self::SchemaMismatch => "schema_mismatch",
            Self::ServerError => "server_error",
        };
        p::McpErrorClass(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpFailure {
    pub schema_version: p::SchemaVersion,
    pub server: p::ProviderId,
    pub class: McpErrorClass,
    pub detail: String,
}

pub trait McpRegistry {
    fn discover(&self, server: p::ProviderId) -> p::Result<Vec<McpTool>>;
    fn prepare_call(&self, tool: p::McpToolRef, arguments: serde_json::Value) -> p::ActionIntent;
}

pub trait ExecutionCapabilityRechecker {
    fn recheck(&self, intent: &p::ActionIntent, context: &PolicyContext) -> p::Result<()>;
}

pub struct StdioMcpRegistry {
    providers: p::InMemoryProviderRegistry<StdioMcpServer>,
    state: Mutex<McpState>,
    search_limit: usize,
}

#[derive(Default)]
struct McpState {
    discoveries: BTreeMap<p::ProviderId, McpDiscovery>,
    tools: BTreeMap<p::McpToolRef, McpTool>,
    deferred_schemas: BTreeMap<p::McpToolRef, serde_json::Value>,
    failures: Vec<McpFailure>,
    events: Vec<p::EventPayload>,
    next_call_sequence: u64,
}

impl StdioMcpRegistry {
    pub fn with_servers(servers: Vec<StdioMcpServer>) -> p::Result<Self> {
        Self::with_search_limit(servers, 8)
    }

    pub fn with_search_limit(servers: Vec<StdioMcpServer>, search_limit: usize) -> p::Result<Self> {
        if search_limit == 0 {
            return Err(p::Error("MCP tool search limit must be non-zero".into()));
        }
        Ok(Self {
            providers: p::InMemoryProviderRegistry::with_discovered(servers)?,
            state: Mutex::new(McpState::default()),
            search_limit,
        })
    }

    pub fn configure(&self, id: p::ProviderId) -> p::Result<()> {
        let server = self
            .servers()
            .into_iter()
            .find(|server| server.provider_id == id)
            .ok_or_else(|| p::Error("MCP server is not discovered".into()))?;
        p::ProviderRegistry::register(
            &self.providers,
            server.clone(),
            p::ProviderConfig {
                schema_version: p::SchemaVersion(1),
                credential_ref: None,
                endpoint: None,
                manifest: Some(serde_json::json!({
                    "transport": "stdio",
                    "command": server.command,
                })),
            },
        )?;
        Ok(())
    }

    pub fn enable(&self, id: p::ProviderId) -> p::Result<()> {
        p::ProviderRegistry::set_enabled(&self.providers, id, true)
    }

    pub fn disable(&self, id: p::ProviderId) -> p::Result<()> {
        p::ProviderRegistry::set_enabled(&self.providers, id.clone(), false)?;
        let mut state = self.lock_state()?;
        state.discoveries.remove(&id);
        state.tools.retain(|_, tool| tool.server != id);
        let retained = state.tools.keys().cloned().collect::<BTreeSet<_>>();
        state
            .deferred_schemas
            .retain(|tool, _| retained.contains(tool));
        Ok(())
    }

    pub fn bind_trust(&self, id: p::ProviderId, tier: p::TrustTier, by: p::Actor) -> p::Result<()> {
        p::ProviderRegistry::bind_trust(&self.providers, id, tier, by)
    }

    pub fn grant(&self, id: p::ProviderId, envelope: p::AutonomyEnvelope) -> p::Result<()> {
        p::ProviderRegistry::grant(&self.providers, id, envelope)
    }

    pub fn discovery(&self, id: &p::ProviderId) -> Option<McpDiscovery> {
        self.lock_state()
            .ok()
            .and_then(|state| state.discoveries.get(id).cloned())
    }

    pub fn discover_detailed(&self, id: p::ProviderId) -> Result<McpDiscovery, McpFailure> {
        let record = self
            .providers
            .record(&id)
            .map_err(|error| failure(id.clone(), McpErrorClass::ServerError, error.to_string()))?;
        if !matches!(
            record.state,
            p::ProviderLifecycle::Trusted(tier) if tier != p::TrustTier::Untrusted
        ) || record.grant.is_none()
        {
            return Err(failure(
                id,
                McpErrorClass::ServerError,
                "MCP server is not active and granted",
            ));
        }

        match discover_over_stdio(&record.provider) {
            Ok(catalog) => {
                let discovery = catalog.discovery;
                let mut state = self.lock_state().map_err(|error| {
                    failure(
                        record.provider.provider_id.clone(),
                        McpErrorClass::ServerError,
                        error.to_string(),
                    )
                })?;
                state
                    .tools
                    .retain(|_, tool| tool.server != record.provider.provider_id);
                let retained = state.tools.keys().cloned().collect::<BTreeSet<_>>();
                state
                    .deferred_schemas
                    .retain(|tool, _| retained.contains(tool));
                for tool in &discovery.tools {
                    state.tools.insert(tool.tool_ref.clone(), tool.clone());
                }
                state.deferred_schemas.extend(catalog.schemas);
                state
                    .events
                    .push(p::EventPayload::McpDiscovered(p::McpDiscoveredPayload {
                        server: p::McpServerRef(discovery.server.0.clone()),
                        tools: discovery
                            .tools
                            .iter()
                            .map(|tool| p::ToolRef(tool.name.clone()))
                            .collect(),
                        resources: discovery
                            .resources
                            .iter()
                            .map(|resource| p::ResourceRef(resource.uri.clone()))
                            .collect(),
                    }));
                state
                    .discoveries
                    .insert(discovery.server.clone(), discovery.clone());
                Ok(discovery)
            }
            Err(failure) => {
                if let Ok(mut state) = self.lock_state() {
                    state.discoveries.remove(&failure.server);
                    state.tools.retain(|_, tool| tool.server != failure.server);
                    let retained = state.tools.keys().cloned().collect::<BTreeSet<_>>();
                    state
                        .deferred_schemas
                        .retain(|tool, _| retained.contains(tool));
                    state.failures.push(failure.clone());
                }
                Err(failure)
            }
        }
    }

    pub fn refresh(&self, id: p::ProviderId) -> Result<McpDiscovery, McpFailure> {
        self.discover_detailed(id)
    }

    pub fn search(&self, query: McpSearchQuery) -> p::Result<Vec<McpSearchHit>> {
        if query.schema_version.0 == 0
            || query.text.trim().is_empty()
            || query.limit == 0
            || query.context.schema_version.0 == 0
        {
            return Err(p::Error("MCP tool search query is incomplete".into()));
        }
        let terms = search_terms(&query.text);
        if terms.is_empty() {
            return Err(p::Error(
                "MCP tool search query has no searchable terms".into(),
            ));
        }
        let active = p::ProviderRegistry::resolve(&self.providers, &query.context)
            .providers
            .into_iter()
            .map(|provider| provider.provider.provider_id)
            .collect::<BTreeSet<_>>();
        let query_text = query.text.to_lowercase();
        let state = self.lock_state()?;
        let mut hits = state
            .tools
            .values()
            .filter(|tool| active.contains(&tool.server))
            .filter_map(|tool| rank_tool(tool, &query_text, &terms))
            .collect::<Vec<_>>();
        hits.sort_by_key(|hit| (Reverse(hit.score), hit.tool.tool_ref.clone()));
        hits.truncate(query.limit.min(self.search_limit));
        Ok(hits)
    }

    pub fn resolve_schema_detailed(&self, tool_ref: p::McpToolRef) -> Result<McpTool, McpFailure> {
        let (mut tool, raw_schema) = {
            let state = self.lock_state().map_err(|error| {
                failure(
                    p::ProviderId("unknown".into()),
                    McpErrorClass::ServerError,
                    error.to_string(),
                )
            })?;
            let tool = state.tools.get(&tool_ref).cloned().ok_or_else(|| {
                failure(
                    p::ProviderId("unknown".into()),
                    McpErrorClass::ServerError,
                    "MCP tool is not discovered or visible",
                )
            })?;
            let raw = state
                .deferred_schemas
                .get(&tool_ref)
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            (tool, raw)
        };
        let record = self.providers.record(&tool.server).map_err(|error| {
            failure(
                tool.server.clone(),
                McpErrorClass::ServerError,
                error.to_string(),
            )
        })?;
        if !matches!(
            record.state,
            p::ProviderLifecycle::Trusted(tier) if tier != p::TrustTier::Untrusted
        ) || record.grant.is_none()
            || !record.provider.allowlist.tools.contains(&tool.name)
        {
            return Err(failure(
                tool.server,
                McpErrorClass::ServerError,
                "MCP server or tool is no longer enabled, trusted, granted, and allowlisted",
            ));
        }
        if !raw_schema.is_object() {
            return Err(failure(
                tool.server,
                McpErrorClass::SchemaMismatch,
                "selected MCP tool has no object input schema",
            ));
        }
        let digest = schema_digest(&raw_schema).map_err(|error| {
            failure(
                tool.server.clone(),
                McpErrorClass::SchemaMismatch,
                error.to_string(),
            )
        })?;
        tool.input_schema = Some(raw_schema);
        tool.schema_digest = Some(digest);
        let mut state = self.lock_state().map_err(|error| {
            failure(
                tool.server.clone(),
                McpErrorClass::ServerError,
                error.to_string(),
            )
        })?;
        state.tools.insert(tool_ref, tool.clone());
        if let Some(discovery) = state.discoveries.get_mut(&tool.server) {
            if let Some(entry) = discovery
                .tools
                .iter_mut()
                .find(|entry| entry.tool_ref == tool.tool_ref)
            {
                *entry = tool.clone();
            }
        }
        Ok(tool)
    }

    pub fn resolve_schema(&self, tool_ref: p::McpToolRef) -> p::Result<McpTool> {
        self.resolve_schema_detailed(tool_ref)
            .map_err(|failure| p::Error(failure.detail))
    }

    pub fn try_prepare_call(
        &self,
        tool_ref: p::McpToolRef,
        arguments: serde_json::Value,
    ) -> p::Result<p::ActionIntent> {
        let (tool, sequence) = {
            let mut state = self.lock_state()?;
            let tool = state
                .tools
                .get(&tool_ref)
                .cloned()
                .ok_or_else(|| p::Error("MCP tool is not discovered or allowlisted".into()))?;
            state.next_call_sequence = state.next_call_sequence.saturating_add(1);
            (tool, state.next_call_sequence)
        };
        let record = self.providers.record(&tool.server)?;
        let p::ProviderLifecycle::Trusted(trust) = record.state else {
            return Err(p::Error("MCP server is disabled".into()));
        };
        if trust == p::TrustTier::Untrusted {
            return Err(p::Error("MCP server is untrusted".into()));
        }
        let grant = record
            .grant
            .ok_or_else(|| p::Error("MCP server has no autonomy grant".into()))?;
        if !record.provider.allowlist.tools.contains(&tool.name) {
            return Err(p::Error("MCP tool is not allowlisted".into()));
        }
        let schema = tool
            .input_schema
            .as_ref()
            .ok_or_else(|| p::Error("MCP tool schema has not been selected and resolved".into()))?;
        let digest = tool
            .schema_digest
            .clone()
            .ok_or_else(|| p::Error("MCP tool schema digest is unavailable".into()))?;
        validate_arguments(schema, &arguments)?;

        Ok(p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId(format!("mcp:{}:{}:{sequence}", tool.server.0, tool.name)),
            source: p::Source::UserTurn,
            goal: p::GoalRef("invoke an allowlisted MCP tool".into()),
            backend_hint: p::BackendKind::Mcp,
            capability_ref: tool.capability_ref,
            action_type: p::ActionType::Execute,
            scope: grant.scope,
            risk_hint: p::Risk::Low,
            expected_effect: p::ExpectedEffect::Internal,
            rollback_expectation: p::RollbackBoundary("none".into()),
            parameters: p::ActionParameters::Mcp {
                server: p::McpServerRef(tool.server.0),
                tool: p::ToolRef(tool.name),
                arguments,
                schema_digest: Some(digest),
                transport: p::McpTransport::Stdio,
                stdio: p::McpStdioSpec {
                    schema_version: p::SchemaVersion(1),
                    command: record.provider.command,
                    args: record.provider.args,
                    env: record.provider.env,
                },
                timeout: record.provider.timeout,
            },
            requested_permissions: grant.capability.permissions,
            requested_at: system_timestamp(),
            estimated_output_bytes: 64 * 1024,
            estimated_duration: record.provider.timeout,
        })
    }

    pub fn index_discovered(
        &self,
        capabilities: &InMemoryCapabilityRegistry,
        context: &p::ResolveContext,
    ) -> p::Result<()> {
        for server in self.servers() {
            capabilities.remove_source(&server.source_ref())?;
        }
        let active = p::ProviderRegistry::resolve(&self.providers, context);
        let state = self.lock_state()?;
        for provider in active.providers {
            let Some(discovery) = state.discoveries.get(&provider.provider.provider_id) else {
                continue;
            };
            capabilities.index(CapabilitySource {
                schema_version: p::SchemaVersion(1),
                source_ref: provider.provider.source_ref(),
                trust: provider.trust,
                entries: discovery
                    .tools
                    .iter()
                    .map(|tool| CapabilityDescriptor {
                        schema_version: p::SchemaVersion(1),
                        id: tool.capability_ref.clone(),
                        capability: Capability::McpTool(tool.tool_ref.clone()),
                        scope: provider.grant.scope.clone(),
                        permissions: provider.grant.capability.permissions.clone(),
                        risk: Some(p::Risk::Low),
                        enabled: true,
                    })
                    .collect(),
            })?;
        }
        Ok(())
    }

    pub fn take_events(&self) -> Vec<p::EventPayload> {
        self.lock_state()
            .map(|mut state| std::mem::take(&mut state.events))
            .unwrap_or_default()
    }

    pub fn take_failures(&self) -> Vec<McpFailure> {
        self.lock_state()
            .map(|mut state| std::mem::take(&mut state.failures))
            .unwrap_or_default()
    }

    pub fn take_provider_transitions(&self) -> Vec<p::ProviderTransition> {
        self.providers.take_transitions()
    }

    fn servers(&self) -> Vec<StdioMcpServer> {
        p::ProviderRegistry::discover(&self.providers)
    }

    fn lock_state(&self) -> p::Result<MutexGuard<'_, McpState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("MCP registry state is unavailable".into()))
    }
}

impl McpRegistry for StdioMcpRegistry {
    fn discover(&self, server: p::ProviderId) -> p::Result<Vec<McpTool>> {
        self.discover_detailed(server)
            .map(|discovery| discovery.tools)
            .map_err(|failure| p::Error(failure.detail))
    }

    fn prepare_call(&self, tool: p::McpToolRef, arguments: serde_json::Value) -> p::ActionIntent {
        self.try_prepare_call(tool.clone(), arguments.clone())
            .unwrap_or_else(|_| unresolved_intent(tool, arguments))
    }
}

impl ExecutionCapabilityRechecker for StdioMcpRegistry {
    fn recheck(&self, intent: &p::ActionIntent, context: &PolicyContext) -> p::Result<()> {
        let p::ActionParameters::Mcp {
            server,
            tool,
            arguments,
            schema_digest: Some(bound_digest),
            ..
        } = &intent.parameters
        else {
            return if intent.backend_hint == p::BackendKind::Mcp {
                Err(p::Error(
                    "MCP execution requires a selected schema digest".into(),
                ))
            } else {
                Ok(())
            };
        };
        let tool_ref = p::McpToolRef(format!("{}/{}", server.0, tool.0));
        let (indexed, current_schema) = {
            let state = self.lock_state()?;
            let indexed = state
                .tools
                .get(&tool_ref)
                .cloned()
                .ok_or_else(|| p::Error("MCP tool disappeared before execution".into()))?;
            let schema = state
                .deferred_schemas
                .get(&tool_ref)
                .cloned()
                .ok_or_else(|| p::Error("MCP schema disappeared before execution".into()))?;
            (indexed, schema)
        };
        let provider_id = p::ProviderId(server.0.clone());
        let record = self.providers.record(&provider_id)?;
        if !matches!(
            record.state,
            p::ProviderLifecycle::Trusted(tier) if tier != p::TrustTier::Untrusted
        ) {
            return Err(p::Error(
                "MCP server is disabled or no longer granted".into(),
            ));
        }
        let grant = record
            .grant
            .as_ref()
            .ok_or_else(|| p::Error("MCP server is disabled or no longer granted".into()))?;
        let now = system_timestamp();
        if !scope_contains(&grant.scope, &intent.scope)
            || !grant
                .capability
                .capabilities
                .contains(&intent.capability_ref)
            || !intent
                .requested_permissions
                .iter()
                .all(|permission| grant.capability.permissions.contains(permission))
            || !grant.action_type.contains(&intent.action_type)
            || intent.risk_hint > grant.risk_limit
            || grant.approval_rule == p::ApprovalRule::Deny
            || now < grant.timebox.starts_at
            || now > grant.timebox.expires_at
            || grant.timebox.max_turns == 0
            || !rollback_satisfies(&grant.rollback, intent)
        {
            return Err(p::Error(
                "MCP execution no longer fits its provider grant".into(),
            ));
        }
        if indexed.name != tool.0 || !record.provider.allowlist.tools.contains(&tool.0) {
            return Err(p::Error("MCP target is no longer allowlisted".into()));
        }
        let target = format!("{}/{}", server.0, tool.0);
        if !context
            .mcp_allowlist
            .iter()
            .any(|allowed| allowed == &server.0 || allowed == &target)
            || !context
                .visible_capabilities
                .contains(&intent.capability_ref)
            || !intent
                .requested_permissions
                .iter()
                .all(|permission| context.granted_permissions.contains(permission))
            || !context
                .allowed_scopes
                .iter()
                .any(|scope| scope_contains(scope, &intent.scope))
        {
            return Err(p::Error(
                "MCP execution is outside the current policy visibility or scope".into(),
            ));
        }
        if let Some(envelope) = &context.envelope {
            if !scope_contains(&envelope.scope, &intent.scope)
                || !envelope
                    .capability
                    .capabilities
                    .contains(&intent.capability_ref)
                || !intent
                    .requested_permissions
                    .iter()
                    .all(|permission| envelope.capability.permissions.contains(permission))
                || !envelope.action_type.contains(&intent.action_type)
                || intent.risk_hint > envelope.risk_limit
            {
                return Err(p::Error(
                    "MCP execution no longer fits the autonomy envelope".into(),
                ));
            }
        }
        let current_digest = schema_digest(&current_schema)?;
        if &current_digest != bound_digest || indexed.schema_digest.as_ref() != Some(bound_digest) {
            return Err(p::Error(
                "MCP schema changed after the execution plan was prepared".into(),
            ));
        }
        validate_arguments(&current_schema, arguments)
    }
}

fn search_terms(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn rank_tool(tool: &McpTool, query: &str, terms: &[String]) -> Option<McpSearchHit> {
    let name = tool.name.to_lowercase();
    let summary = tool.summary.to_lowercase();
    let mut score = u32::from(name.contains(query)) * 20;
    let mut matched_terms = Vec::new();
    for term in terms {
        let name_match = name.contains(term);
        let summary_match = summary.contains(term);
        if name_match || summary_match {
            score = score
                .saturating_add(u32::from(name_match) * 10)
                .saturating_add(u32::from(summary_match) * 4);
            matched_terms.push(term.clone());
        }
    }
    (score > 0).then(|| McpSearchHit {
        schema_version: p::SchemaVersion(1),
        tool: tool.clone(),
        score,
        explanation: format!(
            "matched {} query terms within tool name or summary",
            matched_terms.len()
        ),
        matched_terms,
    })
}

fn schema_digest(schema: &serde_json::Value) -> p::Result<p::SchemaDigest> {
    let bytes = serde_json::to_vec(schema)
        .map_err(|error| p::Error(format!("failed to canonicalize MCP schema: {error}")))?;
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        use core::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}")
            .map_err(|_| p::Error("failed to encode MCP schema digest".into()))?;
    }
    Ok(p::SchemaDigest(encoded))
}

fn validate_arguments(schema: &serde_json::Value, arguments: &serde_json::Value) -> p::Result<()> {
    if !schema.is_object() || !arguments.is_object() {
        return Err(p::Error(
            "MCP selected schema and arguments must both be objects".into(),
        ));
    }
    if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
        for field in required.iter().filter_map(serde_json::Value::as_str) {
            if arguments.get(field).is_none() {
                return Err(p::Error(format!(
                    "MCP arguments omit required field {field}"
                )));
            }
        }
    }
    Ok(())
}

fn scope_contains(granted: &p::Scope, requested: &p::Scope) -> bool {
    if granted.0 == "*" || granted == requested {
        return true;
    }
    requested
        .0
        .strip_prefix(&granted.0)
        .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('/'))
}

fn rollback_satisfies(requirement: &p::RollbackReq, intent: &p::ActionIntent) -> bool {
    if !requirement.required {
        return true;
    }
    if intent.rollback_expectation.0.is_empty() || intent.rollback_expectation.0 == "none" {
        return false;
    }
    requirement
        .boundary
        .as_ref()
        .is_none_or(|boundary| boundary == &intent.rollback_expectation)
}

struct McpCatalog {
    discovery: McpDiscovery,
    schemas: BTreeMap<p::McpToolRef, serde_json::Value>,
}

fn discover_over_stdio(server: &StdioMcpServer) -> Result<McpCatalog, McpFailure> {
    if server.command.trim().is_empty() || server.timeout.0 == 0 {
        return Err(failure(
            server.provider_id.clone(),
            McpErrorClass::SchemaMismatch,
            "MCP stdio configuration is incomplete",
        ));
    }
    let mut command = Command::new(&server.command);
    command
        .args(&server.args)
        .env_clear()
        .envs(server.env.iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = command.spawn().map_err(|error| {
        failure(
            server.provider_id.clone(),
            McpErrorClass::ServerError,
            format!("failed to start MCP stdio process: {error}"),
        )
    })?;
    let mut rpc = RpcSession::new(child, server)?;
    let initialize = rpc.request(
        "initialize",
        serde_json::json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "forme", "version": "0.0.1" }
        }),
    )?;
    if !initialize.is_object() {
        return Err(failure(
            server.provider_id.clone(),
            McpErrorClass::SchemaMismatch,
            "MCP initialize result must be an object",
        ));
    }
    rpc.notify("notifications/initialized", serde_json::json!({}))?;
    let tools_result = rpc.request("tools/list", serde_json::json!({}))?;
    let (tools, schemas) = parse_tools(server, tools_result)?;
    let resources_result = rpc.request("resources/list", serde_json::json!({}))?;
    let resources = parse_resources(server, resources_result)?;
    Ok(McpCatalog {
        discovery: McpDiscovery {
            schema_version: p::SchemaVersion(1),
            server: server.provider_id.clone(),
            tools,
            resources,
        },
        schemas,
    })
}

fn parse_tools(
    server: &StdioMcpServer,
    result: serde_json::Value,
) -> Result<(Vec<McpTool>, BTreeMap<p::McpToolRef, serde_json::Value>), McpFailure> {
    let tools = result
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            failure(
                server.provider_id.clone(),
                McpErrorClass::SchemaMismatch,
                "MCP tools/list result has no tools array",
            )
        })?;
    let mut parsed = Vec::new();
    let mut schemas = BTreeMap::new();
    for tool in tools {
        let name = tool
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                failure(
                    server.provider_id.clone(),
                    McpErrorClass::SchemaMismatch,
                    "MCP tool has no valid name",
                )
            })?;
        if server.allowlist.tools.iter().any(|allowed| allowed == name) {
            let tool_ref = p::McpToolRef(format!("{}/{}", server.provider_id.0, name));
            schemas.insert(
                tool_ref.clone(),
                tool.get("inputSchema")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            );
            parsed.push(McpTool {
                schema_version: p::SchemaVersion(1),
                tool_ref,
                capability_ref: p::CapabilityRef(format!("mcp:{}:{name}", server.provider_id.0)),
                server: server.provider_id.clone(),
                name: name.to_owned(),
                summary: tool
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(name)
                    .to_owned(),
                input_schema: None,
                schema_digest: None,
            });
        }
    }
    Ok((parsed, schemas))
}

fn parse_resources(
    server: &StdioMcpServer,
    result: serde_json::Value,
) -> Result<Vec<McpResource>, McpFailure> {
    let resources = result
        .get("resources")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            failure(
                server.provider_id.clone(),
                McpErrorClass::SchemaMismatch,
                "MCP resources/list result has no resources array",
            )
        })?;
    let mut parsed = Vec::new();
    for resource in resources {
        let uri = resource
            .get("uri")
            .and_then(serde_json::Value::as_str)
            .filter(|uri| !uri.is_empty())
            .ok_or_else(|| {
                failure(
                    server.provider_id.clone(),
                    McpErrorClass::SchemaMismatch,
                    "MCP resource has no valid URI",
                )
            })?;
        let name = resource
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if server
            .allowlist
            .resources
            .iter()
            .any(|allowed| allowed == uri || name.as_ref().is_some_and(|name| allowed == name))
        {
            parsed.push(McpResource {
                schema_version: p::SchemaVersion(1),
                server: server.provider_id.clone(),
                uri: uri.to_owned(),
                name,
            });
        }
    }
    Ok(parsed)
}

struct RpcSession {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    timeout: Duration,
    server: p::ProviderId,
    next_id: u64,
}

impl RpcSession {
    fn new(mut child: Child, server: &StdioMcpServer) -> Result<Self, McpFailure> {
        let stdin = child.stdin.take().ok_or_else(|| {
            failure(
                server.provider_id.clone(),
                McpErrorClass::ServerError,
                "MCP process did not expose stdin",
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            failure(
                server.provider_id.clone(),
                McpErrorClass::ServerError,
                "MCP process did not expose stdout",
            )
        })?;
        let (sender, lines) = mpsc::sync_channel(64);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
            timeout: Duration::from_millis(server.timeout.0),
            server: server.provider_id.clone(),
            next_id: 0,
        })
    }

    fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpFailure> {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        self.receive(id)
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) -> Result<(), McpFailure> {
        self.send(serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send(&mut self, value: serde_json::Value) -> Result<(), McpFailure> {
        serde_json::to_writer(&mut self.stdin, &value).map_err(|error| {
            failure(
                self.server.clone(),
                McpErrorClass::ServerError,
                format!("failed to encode MCP request: {error}"),
            )
        })?;
        self.stdin.write_all(b"\n").map_err(|error| {
            failure(
                self.server.clone(),
                McpErrorClass::ServerError,
                format!("failed to write MCP request: {error}"),
            )
        })?;
        self.stdin.flush().map_err(|error| {
            failure(
                self.server.clone(),
                McpErrorClass::ServerError,
                format!("failed to flush MCP request: {error}"),
            )
        })
    }

    fn receive(&self, id: u64) -> Result<serde_json::Value, McpFailure> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(failure(
                    self.server.clone(),
                    McpErrorClass::Timeout,
                    "MCP response deadline elapsed",
                ));
            }
            let line = match self.lines.recv_timeout(remaining) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(failure(
                        self.server.clone(),
                        McpErrorClass::Timeout,
                        "MCP response deadline elapsed",
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(failure(
                        self.server.clone(),
                        McpErrorClass::ServerError,
                        "MCP process ended before responding",
                    ));
                }
            };
            let Ok(response) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if response.get("id").and_then(serde_json::Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(failure(
                    self.server.clone(),
                    McpErrorClass::ServerError,
                    format!("MCP server returned an error: {error}"),
                ));
            }
            return response.get("result").cloned().ok_or_else(|| {
                failure(
                    self.server.clone(),
                    McpErrorClass::SchemaMismatch,
                    "MCP response has no result",
                )
            });
        }
    }
}

impl Drop for RpcSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn failure(server: p::ProviderId, class: McpErrorClass, detail: impl Into<String>) -> McpFailure {
    McpFailure {
        schema_version: p::SchemaVersion(1),
        server,
        class,
        detail: detail.into(),
    }
}

fn unresolved_intent(tool: p::McpToolRef, arguments: serde_json::Value) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId(format!("unresolved-mcp:{}", tool.0)),
        source: p::Source::Internal,
        goal: p::GoalRef("reject an unresolved MCP proposal".into()),
        backend_hint: p::BackendKind::Mcp,
        capability_ref: p::CapabilityRef("mcp:unresolved".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope(String::new()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Internal,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Mcp {
            server: p::McpServerRef(String::new()),
            tool: p::ToolRef(tool.0),
            arguments,
            schema_digest: None,
            transport: p::McpTransport::Stdio,
            stdio: p::McpStdioSpec {
                schema_version: p::SchemaVersion(1),
                command: String::new(),
                args: Vec::new(),
                env: Vec::new(),
            },
            timeout: p::DurationMs(0),
        },
        requested_permissions: Vec::new(),
        requested_at: system_timestamp(),
        estimated_output_bytes: 0,
        estimated_duration: p::DurationMs(0),
    }
}

fn system_timestamp() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
