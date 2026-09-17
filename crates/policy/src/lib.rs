//! Runtime policy, approval routing, and delegated-envelope enforcement (prd/04).
#![forbid(unsafe_code)]

use forme_protocol as p;
use url::Url;

pub type PolicyDecision = p::PolicyDecision;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyLayerSource {
    Managed,
    User,
    Workspace,
    Agent,
    Surface,
    Session,
    SkillToolPlugin,
    Hook,
    Final,
}

impl PolicyLayerSource {
    const fn rank(self) -> u8 {
        match self {
            Self::Managed => 0,
            Self::User => 1,
            Self::Workspace => 2,
            Self::Agent => 3,
            Self::Surface => 4,
            Self::Session => 5,
            Self::SkillToolPlugin => 6,
            Self::Hook => 7,
            Self::Final => 8,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::User => "user",
            Self::Workspace => "workspace",
            Self::Agent => "agent",
            Self::Surface => "surface",
            Self::Session => "session",
            Self::SkillToolPlugin => "skill-tool-plugin",
            Self::Hook => "hook",
            Self::Final => "final",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyLayer {
    pub schema_version: p::SchemaVersion,
    pub source: PolicyLayerSource,
    pub rules: Vec<PolicyRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    pub schema_version: p::SchemaVersion,
    pub matcher: ActionMatcher,
    pub effect: p::PolicyDecision,
    pub scope: p::Scope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMatcher {
    pub backend: Option<p::BackendKind>,
    pub capability: Option<p::CapabilityRef>,
    pub action_type: Option<p::ActionType>,
    pub parameters: ArgMatcher,
}

impl Default for ActionMatcher {
    fn default() -> Self {
        Self {
            backend: None,
            capability: None,
            action_type: None,
            parameters: ArgMatcher::Any,
        }
    }
}

impl ActionMatcher {
    fn matches(&self, intent: &p::ActionIntent) -> bool {
        self.backend
            .is_none_or(|value| value == intent.backend_hint)
            && self
                .capability
                .as_ref()
                .is_none_or(|value| value == &intent.capability_ref)
            && self
                .action_type
                .is_none_or(|value| value == intent.action_type)
            && self.parameters.matches(intent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgMatcher {
    Any,
    ShellProgram(String),
    ShellArgsPrefix(Vec<String>),
    FilePathPrefix(String),
    McpTarget {
        server: p::McpServerRef,
        tool: p::ToolRef,
    },
}

impl ArgMatcher {
    pub fn matches(&self, intent: &p::ActionIntent) -> bool {
        match (self, &intent.parameters) {
            (Self::Any, _) => true,
            (Self::ShellProgram(expected), p::ActionParameters::Shell { program, .. }) => {
                expected == program
            }
            (Self::ShellArgsPrefix(expected), p::ActionParameters::Shell { args, .. }) => {
                args.starts_with(expected)
            }
            (Self::FilePathPrefix(root), p::ActionParameters::File { path, .. }) => {
                path_within(root, path)
            }
            (
                Self::McpTarget {
                    server: expected_server,
                    tool: expected_tool,
                },
                p::ActionParameters::Mcp { server, tool, .. },
            ) => expected_server == server && expected_tool == tool,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePolicy {
    pub schema_version: p::SchemaVersion,
    layers: Vec<PolicyLayer>,
}

impl EffectivePolicy {
    pub fn layers(&self) -> &[PolicyLayer] {
        &self.layers
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyContext {
    pub schema_version: p::SchemaVersion,
    pub session: p::SessionId,
    pub toolset: p::ToolsetRef,
    pub policy: EffectivePolicy,
    pub visible_capabilities: Vec<p::CapabilityRef>,
    pub granted_permissions: Vec<p::PermissionRef>,
    pub allowed_scopes: Vec<p::Scope>,
    pub shell_allowlist: Vec<String>,
    pub file_roots: Vec<String>,
    pub mcp_allowlist: Vec<String>,
    pub external: ExternalPolicyLimits,
    pub network_allowed: bool,
    pub sandbox_available: bool,
    pub delegation: Option<DelegationGrant>,
    pub envelope: Option<p::AutonomyEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalPolicyLimits {
    pub schema_version: p::SchemaVersion,
    pub browser_origins: Vec<String>,
    pub computer_surfaces: Vec<ComputerSurfaceLimit>,
    pub pty_programs: Vec<String>,
    pub pty_roots: Vec<String>,
    pub app_api_connectors: Vec<AppApiConnectorLimit>,
}

impl Default for ExternalPolicyLimits {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            browser_origins: Vec::new(),
            computer_surfaces: Vec::new(),
            pty_programs: Vec::new(),
            pty_roots: Vec::new(),
            app_api_connectors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppApiConnectorLimit {
    pub schema_version: p::SchemaVersion,
    pub connector: p::ProviderId,
    pub base_url: String,
    pub schema_digest: p::SchemaDigest,
    pub credential_ref: Option<p::SecretRef>,
    pub allowed_mutations: Vec<p::AppApiMutationMethod>,
    pub max_timeout: p::DurationMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputerSurfaceLimit {
    pub schema_version: p::SchemaVersion,
    pub surface: p::SurfaceRef,
    pub bounds: p::CoordinateBounds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelegationSubject {
    Owner,
    Agent,
    Subagent(p::RunId),
    External(p::ParticipantId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationGrant {
    pub schema_version: p::SchemaVersion,
    pub subject: DelegationSubject,
    pub envelope: p::AutonomyEnvelope,
    pub granted_by: p::Actor,
    pub audit_ref: p::EventId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeDecision {
    Within,
    OutOfScope,
    NeedsApproval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyEvaluation {
    pub schema_version: p::SchemaVersion,
    pub decision: p::PolicyDecision,
    pub rule_source: p::RuleSourceRef,
    pub reason: p::ReasonRef,
}

impl PolicyEvaluation {
    pub fn event_payload(&self) -> p::EventPayload {
        p::EventPayload::ToolPolicyEvaluated(p::ToolPolicyEvaluatedPayload {
            decision: self.decision,
            rule_source: self.rule_source.clone(),
            reason: self.reason.clone(),
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPolicyEngine;

pub trait PolicyEngine {
    fn merge(&self, layers: &[PolicyLayer]) -> EffectivePolicy;
    fn evaluate(&self, ctx: &PolicyContext, intent: &p::ActionIntent) -> PolicyDecision;
    fn enforce_envelope(
        &self,
        grant: &DelegationGrant,
        env: &p::AutonomyEnvelope,
        intent: &p::ActionIntent,
    ) -> EnvelopeDecision;
}

impl DefaultPolicyEngine {
    pub fn evaluate_detailed(
        &self,
        context: &PolicyContext,
        intent: &p::ActionIntent,
    ) -> PolicyEvaluation {
        if let Some(reason) = execution_recheck(context, intent) {
            return evaluation(p::PolicyDecision::Deny, "execution-recheck", reason);
        }

        if let (Some(grant), Some(envelope)) = (&context.delegation, &context.envelope) {
            match self.enforce_envelope(grant, envelope, intent) {
                EnvelopeDecision::OutOfScope => {
                    return evaluation(
                        p::PolicyDecision::Deny,
                        "delegation-envelope",
                        "action is outside the active delegated envelope",
                    );
                }
                EnvelopeDecision::NeedsApproval => {
                    return evaluation(
                        p::PolicyDecision::Ask,
                        "delegation-envelope",
                        "active delegated envelope requires approval",
                    );
                }
                EnvelopeDecision::Within => {}
            }
        } else if context.delegation.is_some() || context.envelope.is_some() {
            return evaluation(
                p::PolicyDecision::Deny,
                "delegation-envelope",
                "delegation and runtime envelope must be supplied together",
            );
        }

        let mut selected: Option<(p::PolicyDecision, PolicyLayerSource)> = None;
        for layer in context.policy.layers() {
            for rule in &layer.rules {
                if scope_contains(&rule.scope, &intent.scope)
                    && rule.matcher.matches(intent)
                    && selected.as_ref().is_none_or(|(decision, _)| {
                        decision_rank(rule.effect) > decision_rank(*decision)
                    })
                {
                    selected = Some((rule.effect, layer.source));
                }
            }
        }

        match selected {
            Some((decision, source)) => {
                evaluation(decision, source.as_str(), "matched the effective policy")
            }
            None => evaluation(
                p::PolicyDecision::Deny,
                "final",
                "no effective policy rule matched the action",
            ),
        }
    }
}

impl PolicyEngine for DefaultPolicyEngine {
    fn merge(&self, layers: &[PolicyLayer]) -> EffectivePolicy {
        let mut layers = layers.to_vec();
        layers.sort_by_key(|layer| layer.source.rank());
        EffectivePolicy {
            schema_version: p::SchemaVersion(1),
            layers,
        }
    }

    fn evaluate(&self, context: &PolicyContext, intent: &p::ActionIntent) -> PolicyDecision {
        self.evaluate_detailed(context, intent).decision
    }

    fn enforce_envelope(
        &self,
        grant: &DelegationGrant,
        runtime: &p::AutonomyEnvelope,
        intent: &p::ActionIntent,
    ) -> EnvelopeDecision {
        let granted = &grant.envelope;

        // Permission is deliberately checked before capability or approval state.
        if !permissions_contain(
            &granted.capability.permissions,
            &intent.requested_permissions,
        ) || !permissions_contain(
            &runtime.capability.permissions,
            &intent.requested_permissions,
        ) {
            return EnvelopeDecision::OutOfScope;
        }

        if !granted
            .capability
            .capabilities
            .contains(&intent.capability_ref)
            || !runtime
                .capability
                .capabilities
                .contains(&intent.capability_ref)
            || !scope_contains(&granted.scope, &intent.scope)
            || !scope_contains(&runtime.scope, &intent.scope)
            || !granted.action_type.contains(&intent.action_type)
            || !runtime.action_type.contains(&intent.action_type)
            || intent.risk_hint > granted.risk_limit
            || intent.risk_hint > runtime.risk_limit
            || !timebox_contains(&granted.timebox, intent.requested_at)
            || !timebox_contains(&runtime.timebox, intent.requested_at)
            || granted.budget.0.is_empty()
            || runtime.budget.0.is_empty()
            || !rollback_satisfies(&granted.rollback, intent)
            || !rollback_satisfies(&runtime.rollback, intent)
        {
            return EnvelopeDecision::OutOfScope;
        }

        if granted.approval_rule == p::ApprovalRule::Deny
            || runtime.approval_rule == p::ApprovalRule::Deny
        {
            EnvelopeDecision::OutOfScope
        } else if granted.approval_rule == p::ApprovalRule::Ask
            || runtime.approval_rule == p::ApprovalRule::Ask
        {
            EnvelopeDecision::NeedsApproval
        } else {
            EnvelopeDecision::Within
        }
    }
}

fn evaluation(decision: p::PolicyDecision, source: &str, reason: &str) -> PolicyEvaluation {
    PolicyEvaluation {
        schema_version: p::SchemaVersion(1),
        decision,
        rule_source: p::RuleSourceRef(source.to_owned()),
        reason: p::ReasonRef(reason.to_owned()),
    }
}

fn execution_recheck(context: &PolicyContext, intent: &p::ActionIntent) -> Option<&'static str> {
    if !context
        .visible_capabilities
        .contains(&intent.capability_ref)
    {
        return Some("capability is not present in the resolved toolset");
    }
    if !permissions_contain(&context.granted_permissions, &intent.requested_permissions) {
        return Some("requested permission is not currently granted");
    }
    if !context
        .allowed_scopes
        .iter()
        .any(|scope| scope_contains(scope, &intent.scope))
    {
        return Some("action scope is not currently permitted");
    }

    match (&intent.backend_hint, &intent.parameters) {
        (
            p::BackendKind::Shell,
            p::ActionParameters::Shell {
                program,
                cwd,
                network,
                ..
            },
        ) => {
            if !context.sandbox_available {
                return Some("shell sandbox is unavailable");
            }
            if !context
                .shell_allowlist
                .iter()
                .any(|allowed| allowed == program)
            {
                return Some("shell program is not allowlisted");
            }
            if *network && !context.network_allowed {
                return Some("network access is not permitted");
            }
            if cwd.as_ref().is_some_and(|path| {
                !context
                    .file_roots
                    .iter()
                    .any(|root| path_within(root, path))
            }) {
                return Some("shell working directory is outside permitted roots");
            }
        }
        (p::BackendKind::File, p::ActionParameters::File { path, .. }) => {
            if !context.sandbox_available {
                return Some("file sandbox is unavailable");
            }
            if !context
                .file_roots
                .iter()
                .any(|root| path_within(root, path))
            {
                return Some("file path is outside permitted roots");
            }
        }
        (p::BackendKind::Mcp, p::ActionParameters::Mcp { server, tool, .. }) => {
            let server_tool = format!("{}/{}", server.0, tool.0);
            if !context
                .mcp_allowlist
                .iter()
                .any(|allowed| allowed == &server.0 || allowed == &server_tool)
            {
                return Some("MCP target is not allowlisted");
            }
            if !context.sandbox_available {
                return Some("MCP sandbox is unavailable");
            }
        }
        (
            p::BackendKind::Notification,
            p::ActionParameters::Notification {
                surface,
                target,
                title,
                body_ref,
            },
        ) => {
            if intent.action_type != p::ActionType::Deliver
                || intent.expected_effect != p::ExpectedEffect::Outward
                || !surface.0.starts_with("surface:local")
                || target.0.trim().is_empty()
                || title.trim().is_empty()
                || body_ref.0.trim().is_empty()
            {
                return Some("local notification target or delivery parameters are invalid");
            }
        }
        (p::BackendKind::Browser, p::ActionParameters::Browser(spec)) => {
            if validate_browser(context, intent, spec).is_err() {
                return Some("browser origin or normalized action parameters are invalid");
            }
        }
        (p::BackendKind::Computer, p::ActionParameters::Computer(spec)) => {
            if validate_computer(context, intent, spec).is_err() {
                return Some(
                    "computer surface, bounds, or normalized action parameters are invalid",
                );
            }
        }
        (p::BackendKind::Pty, p::ActionParameters::Pty(spec)) => {
            if validate_pty(context, spec).is_err() {
                return Some(
                    "PTY program, root, environment, or normalized parameters are invalid",
                );
            }
        }
        (p::BackendKind::AppApi, p::ActionParameters::AppApi(spec)) => {
            if validate_app_api(context, intent, spec).is_err() {
                return Some(
                    "App API connector, schema, endpoint, credential, or operation is invalid",
                );
            }
        }
        (p::BackendKind::Remote, p::ActionParameters::Remote(spec)) => {
            let operation = &spec.placement.operation;
            if !context.network_allowed
                || spec.validate().is_err()
                || operation.capability != intent.capability_ref
                || operation.scope != intent.scope
                || operation.action_type != intent.action_type
                || operation.expected_effect != intent.expected_effect
                || operation.rollback_boundary != intent.rollback_expectation
            {
                return Some("remote plan, authority intent, or network permission is invalid");
            }
        }
        _ => return Some("backend and normalized action parameters disagree"),
    }

    None
}

fn validate_app_api(
    context: &PolicyContext,
    intent: &p::ActionIntent,
    spec: &p::AppApiActionSpec,
) -> p::Result<()> {
    if context.external.schema_version.0 == 0
        || !context.network_allowed
        || spec.schema_version.0 == 0
        || spec.connector.0.trim().is_empty()
        || spec.schema_digest.0.trim().is_empty()
        || spec.timeout.0 == 0
        || intent.expected_effect != p::ExpectedEffect::Outward
    {
        return Err(p::Error("App API action boundary is incomplete".into()));
    }
    let limit = context
        .external
        .app_api_connectors
        .iter()
        .find(|limit| limit.connector == spec.connector)
        .ok_or_else(|| p::Error("App API connector is not allowlisted".into()))?;
    if limit.schema_version.0 == 0
        || limit.schema_digest != spec.schema_digest
        || limit.credential_ref != spec.credential
        || limit.max_timeout.0 == 0
        || spec.timeout.0 > limit.max_timeout.0
    {
        return Err(p::Error(
            "App API connector identity, schema, credential, or timeout changed".into(),
        ));
    }
    ensure_app_api_endpoint(&limit.base_url, &spec.endpoint)?;
    if spec.participant.is_some() != spec.representation.is_some()
        || spec
            .participant
            .as_ref()
            .is_some_and(|participant| !safe_header_component(&participant.0))
        || spec
            .disclosure_request
            .as_ref()
            .is_some_and(|request| request.0.trim().is_empty())
        || (intent.action_type == p::ActionType::Deliver
            && (spec.participant.is_none()
                || spec.representation.is_none()
                || spec.disclosure_request.is_none()))
    {
        return Err(p::Error(
            "App API external participant or representation is incomplete".into(),
        ));
    }
    match &spec.operation {
        p::AppApiOperation::Read => {
            if !matches!(
                intent.action_type,
                p::ActionType::Observe | p::ActionType::Analyze
            ) {
                return Err(p::Error("App API read is not an observation".into()));
            }
        }
        p::AppApiOperation::Mutation {
            method,
            body,
            idempotency_key,
        } => {
            if matches!(
                intent.action_type,
                p::ActionType::Observe | p::ActionType::Analyze | p::ActionType::Prepare
            ) || !limit.allowed_mutations.contains(method)
                || !safe_header_component(&idempotency_key.0)
                || body.as_ref().is_some_and(invalid_external_input)
            {
                return Err(p::Error(
                    "App API mutation method, body, or idempotency boundary is invalid".into(),
                ));
            }
        }
    }
    Ok(())
}

fn ensure_app_api_endpoint(base: &str, endpoint: &str) -> p::Result<()> {
    let base =
        Url::parse(base).map_err(|_| p::Error("App API base URL cannot be parsed".into()))?;
    let endpoint =
        Url::parse(endpoint).map_err(|_| p::Error("App API endpoint cannot be parsed".into()))?;
    if !safe_app_api_url(&base)
        || !safe_app_api_url(&endpoint)
        || !base.path().ends_with('/')
        || endpoint.origin() != base.origin()
        || !endpoint.path().starts_with(base.path())
        || base.query().is_some()
        || base.fragment().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(p::Error(
            "App API endpoint is outside the approved connector base".into(),
        ));
    }
    Ok(())
}

fn safe_app_api_url(url: &Url) -> bool {
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        && url.host_str().is_some()
        && url.port_or_known_default().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

fn invalid_external_input(input: &p::ExternalInput) -> bool {
    match input {
        p::ExternalInput::Literal(value) => value.len() > 64 * 1024,
        p::ExternalInput::Content(reference) => reference.0.trim().is_empty(),
        p::ExternalInput::Secret(reference) => reference.0.trim().is_empty(),
    }
}

fn safe_header_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
}

fn validate_browser(
    context: &PolicyContext,
    intent: &p::ActionIntent,
    spec: &p::BrowserActionSpec,
) -> p::Result<()> {
    if context.external.schema_version.0 == 0
        || spec.schema_version.0 == 0
        || spec.driver.0.trim().is_empty()
        || spec.allowed_origins.is_empty()
        || spec.artifact_scope.0.trim().is_empty()
        || !scope_contains(&intent.scope, &spec.artifact_scope)
    {
        return Err(p::Error("browser action boundary is incomplete".into()));
    }
    let target = Url::parse(&spec.target_url)
        .map_err(|_| p::Error("browser target URL cannot be parsed".into()))?;
    if !matches!(target.scheme(), "http" | "https")
        || target.host_str().is_none()
        || !target.username().is_empty()
        || target.password().is_some()
    {
        return Err(p::Error(
            "browser target URL scheme or authority is unsafe".into(),
        ));
    }
    let target_origin = target.origin().ascii_serialization();
    let planned_origins = normalized_origins(&spec.allowed_origins)?;
    let runtime_origins = normalized_origins(&context.external.browser_origins)?;
    if !planned_origins.contains(&target_origin) || !runtime_origins.contains(&target_origin) {
        return Err(p::Error("browser target origin is not allowlisted".into()));
    }
    validate_browser_operation(&spec.operation)
}

fn normalized_origins(values: &[String]) -> p::Result<Vec<String>> {
    values
        .iter()
        .map(|value| {
            let parsed = Url::parse(value)
                .map_err(|_| p::Error("browser allowlist origin cannot be parsed".into()))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.path() != "/"
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(p::Error("browser allowlist entry must be an origin".into()));
            }
            Ok(parsed.origin().ascii_serialization())
        })
        .collect()
}

fn validate_browser_operation(operation: &p::BrowserOperation) -> p::Result<()> {
    match operation {
        p::BrowserOperation::Navigate | p::BrowserOperation::Screenshot { .. } => Ok(()),
        p::BrowserOperation::ReadText { selector: None } => Ok(()),
        p::BrowserOperation::ReadText {
            selector: Some(selector),
        }
        | p::BrowserOperation::Click { selector } => validate_selector(selector),
        p::BrowserOperation::Type { selector, input } => {
            validate_selector(selector)?;
            validate_external_input(input)
        }
    }
}

fn validate_selector(selector: &str) -> p::Result<()> {
    if selector.trim().is_empty()
        || selector.len() > 512
        || selector.chars().any(|character| character.is_control())
    {
        return Err(p::Error(
            "browser selector is outside the bounded grammar".into(),
        ));
    }
    Ok(())
}

fn validate_computer(
    context: &PolicyContext,
    intent: &p::ActionIntent,
    spec: &p::ComputerActionSpec,
) -> p::Result<()> {
    spec.bounds.validate()?;
    if context.external.schema_version.0 == 0
        || spec.schema_version.0 == 0
        || spec.driver.0.trim().is_empty()
        || spec.surface.0.trim().is_empty()
        || spec.artifact_scope.0.trim().is_empty()
        || !scope_contains(&intent.scope, &spec.artifact_scope)
    {
        return Err(p::Error("computer action boundary is incomplete".into()));
    }
    let allowed = context
        .external
        .computer_surfaces
        .iter()
        .find(|limit| limit.surface == spec.surface)
        .ok_or_else(|| p::Error("computer surface is not allowlisted".into()))?;
    allowed.bounds.validate()?;
    if allowed.schema_version.0 == 0 || !bounds_contain(allowed.bounds, spec.bounds) {
        return Err(p::Error("computer bounds exceed the surface grant".into()));
    }
    match &spec.operation {
        p::ComputerOperation::Move { x, y } | p::ComputerOperation::Click { x, y, .. } => {
            if !spec.bounds.contains(*x, *y) {
                return Err(p::Error(
                    "computer coordinates are outside plan bounds".into(),
                ));
            }
        }
        p::ComputerOperation::Type { input } => validate_external_input(input)?,
        p::ComputerOperation::Scroll { dx, dy } => {
            if dx.unsigned_abs() > 10_000 || dy.unsigned_abs() > 10_000 {
                return Err(p::Error("computer scroll exceeds the bounded delta".into()));
            }
        }
        p::ComputerOperation::Key { .. } | p::ComputerOperation::Screenshot => {}
    }
    Ok(())
}

fn bounds_contain(outer: p::CoordinateBounds, inner: p::CoordinateBounds) -> bool {
    inner.min_x >= outer.min_x
        && inner.min_y >= outer.min_y
        && inner.max_x_exclusive <= outer.max_x_exclusive
        && inner.max_y_exclusive <= outer.max_y_exclusive
}

fn validate_pty(context: &PolicyContext, spec: &p::PtyActionSpec) -> p::Result<()> {
    if context.external.schema_version.0 == 0
        || spec.schema_version.0 == 0
        || spec.program.trim().is_empty()
        || spec.cwd.trim().is_empty()
        || !(1..=500).contains(&spec.cols)
        || !(1..=200).contains(&spec.rows)
        || !context
            .external
            .pty_programs
            .iter()
            .any(|program| program == &spec.program)
        || !context
            .external
            .pty_roots
            .iter()
            .any(|root| path_within(root, &spec.cwd))
        || spec
            .args
            .iter()
            .any(|arg| arg.len() > 8_192 || arg.chars().any(|character| character == '\0'))
    {
        return Err(p::Error("PTY action boundary is incomplete".into()));
    }
    if let Some(input) = &spec.input {
        validate_external_input(input)?;
    }
    let mut names = std::collections::BTreeSet::new();
    for binding in &spec.environment {
        if binding.schema_version.0 == 0
            || binding.value.0.trim().is_empty()
            || !valid_environment_name(&binding.name)
            || !names.insert(binding.name.as_str())
        {
            return Err(p::Error("PTY secret binding is invalid".into()));
        }
    }
    Ok(())
}

fn validate_external_input(input: &p::ExternalInput) -> p::Result<()> {
    let valid = match input {
        p::ExternalInput::Literal(value) => {
            !value.is_empty() && value.len() <= 16_384 && !value.contains('\0')
        }
        p::ExternalInput::Content(reference) => !reference.0.trim().is_empty(),
        p::ExternalInput::Secret(reference) => !reference.0.trim().is_empty(),
    };
    if !valid {
        return Err(p::Error(
            "external input is empty or exceeds its boundary".into(),
        ));
    }
    Ok(())
}

fn valid_environment_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
        && name.len() <= 128
}

fn decision_rank(decision: p::PolicyDecision) -> u8 {
    match decision {
        p::PolicyDecision::Allow => 0,
        p::PolicyDecision::Ask => 1,
        p::PolicyDecision::Deny => 2,
    }
}

fn permissions_contain(granted: &[p::PermissionRef], requested: &[p::PermissionRef]) -> bool {
    requested
        .iter()
        .all(|permission| granted.contains(permission))
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

fn timebox_contains(timebox: &p::Timebox, at: p::Timestamp) -> bool {
    timebox.max_turns > 0 && at >= timebox.starts_at && at <= timebox.expires_at
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

fn path_within(root: &str, path: &str) -> bool {
    let Some(root) = normalized_path(root) else {
        return false;
    };
    let Some(path) = normalized_path(path) else {
        return false;
    };
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn normalized_path(path: &str) -> Option<String> {
    let path = path.replace('\\', "/").to_ascii_lowercase();
    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" if components.is_empty() => components.push(String::new()),
            "" | "." => {}
            ".." => {
                let last = components.pop()?;
                if last.is_empty() || last.ends_with(':') {
                    return None;
                }
            }
            value => components.push(value.to_owned()),
        }
    }
    if components.is_empty() {
        None
    } else {
        Some(components.join("/"))
    }
}
