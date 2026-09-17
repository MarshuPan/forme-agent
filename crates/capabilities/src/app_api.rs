use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use forme_policy::PolicyContext;
use forme_protocol as p;
use url::Url;

use crate::{
    Capability, CapabilityDescriptor, CapabilityRegistry, CapabilitySource,
    ExecutionCapabilityRechecker, InMemoryCapabilityRegistry,
};

#[derive(Debug, Clone)]
pub struct ProjectAppApiConnector {
    pub schema_version: p::SchemaVersion,
    pub provider_id: p::ProviderId,
    pub base_url: String,
    pub schema_digest: p::SchemaDigest,
    pub credential_ref: Option<p::SecretRef>,
    pub scope: p::Scope,
    pub capability_ref: p::CapabilityRef,
    pub permission: p::PermissionRef,
    pub allowed_mutations: Vec<p::AppApiMutationMethod>,
    pub requests_per_minute: u32,
    pub timeout: p::DurationMs,
    revoked: Arc<AtomicBool>,
}

impl ProjectAppApiConnector {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: p::ProviderId,
        base_url: String,
        schema_digest: p::SchemaDigest,
        credential_ref: Option<p::SecretRef>,
        scope: p::Scope,
        capability_ref: p::CapabilityRef,
        permission: p::PermissionRef,
        allowed_mutations: Vec<p::AppApiMutationMethod>,
        requests_per_minute: u32,
        timeout: p::DurationMs,
    ) -> p::Result<Self> {
        let connector = Self {
            schema_version: p::SchemaVersion(1),
            provider_id,
            base_url,
            schema_digest,
            credential_ref,
            scope,
            capability_ref,
            permission,
            allowed_mutations,
            requests_per_minute,
            timeout,
            revoked: Arc::new(AtomicBool::new(false)),
        };
        connector.validate()?;
        Ok(connector)
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::SeqCst)
    }

    pub fn revocation_flag(&self) -> Arc<AtomicBool> {
        self.revoked.clone()
    }

    fn revoke(&self) {
        self.revoked.store(true, Ordering::SeqCst);
    }

    fn source_ref(&self) -> p::CapabilitySourceRef {
        p::CapabilitySourceRef(format!("app-api:{}", self.provider_id.0))
    }

    fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.provider_id.0.trim().is_empty()
            || self.schema_digest.0.trim().is_empty()
            || self.scope.0.trim().is_empty()
            || self.capability_ref.0.trim().is_empty()
            || self.permission.0.trim().is_empty()
            || self.requests_per_minute == 0
            || self.timeout.0 == 0
        {
            return Err(p::Error(
                "App API connector configuration is incomplete".into(),
            ));
        }
        validate_base_url(&self.base_url)?;
        if self
            .credential_ref
            .as_ref()
            .is_some_and(|reference| reference.0.trim().is_empty())
        {
            return Err(p::Error("App API credential reference is empty".into()));
        }
        Ok(())
    }
}

impl p::ExternalProvider for ProjectAppApiConnector {
    fn kind(&self) -> p::ProviderKind {
        p::ProviderKind::AppApi
    }

    fn id(&self) -> p::ProviderId {
        self.provider_id.clone()
    }

    fn declared_capabilities(&self) -> p::CapabilitySet {
        p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![self.capability_ref.clone()],
            permissions: vec![self.permission.clone()],
        }
    }

    fn trust_default(&self) -> p::TrustTier {
        p::TrustTier::Untrusted
    }
}

impl p::AppApiConnector for ProjectAppApiConnector {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppApiRequest {
    pub schema_version: p::SchemaVersion,
    pub resource: String,
    pub operation: p::AppApiOperation,
    pub source: p::Source,
    pub goal: p::GoalRef,
    pub action_type: p::ActionType,
    pub risk: p::Risk,
    pub participant: Option<p::ParticipantId>,
    pub representation: Option<p::Representation>,
    pub disclosure_request: Option<p::DisclosureRequestRef>,
    pub requested_at: p::Timestamp,
    pub estimated_output_bytes: u64,
}

pub struct AppApiConnectorRegistry {
    providers: p::InMemoryProviderRegistry<ProjectAppApiConnector>,
    sequence: AtomicU64,
    events: Mutex<Vec<p::EventPayload>>,
}

impl AppApiConnectorRegistry {
    pub fn with_connectors(connectors: Vec<ProjectAppApiConnector>) -> p::Result<Self> {
        Ok(Self {
            providers: p::InMemoryProviderRegistry::with_discovered(connectors)?,
            sequence: AtomicU64::new(1),
            events: Mutex::new(Vec::new()),
        })
    }

    pub fn configure(&self, id: p::ProviderId) -> p::Result<()> {
        let connector = self.connector(&id)?;
        p::ProviderRegistry::register(
            &self.providers,
            connector.clone(),
            p::ProviderConfig {
                schema_version: p::SchemaVersion(1),
                credential_ref: connector.credential_ref.clone(),
                endpoint: Some(connector.base_url.clone()),
                manifest: Some(serde_json::json!({
                    "schema_digest": connector.schema_digest,
                    "requests_per_minute": connector.requests_per_minute,
                    "timeout_ms": connector.timeout.0,
                })),
            },
        )?;
        Ok(())
    }

    pub fn enable(&self, id: p::ProviderId) -> p::Result<()> {
        p::ProviderRegistry::set_enabled(&self.providers, id, true)
    }

    pub fn bind_trust(&self, id: p::ProviderId, tier: p::TrustTier, by: p::Actor) -> p::Result<()> {
        p::ProviderRegistry::bind_trust(&self.providers, id, tier, by)
    }

    pub fn grant(&self, id: p::ProviderId, envelope: p::AutonomyEnvelope) -> p::Result<()> {
        p::ProviderRegistry::grant(&self.providers, id, envelope)
    }

    pub fn revoke(&self, id: p::ProviderId, lineage: p::LineageRef) -> p::Result<()> {
        let connector = self.connector(&id)?;
        connector.revoke();
        let record = self.providers.record(&id)?;
        if matches!(
            record.state,
            p::ProviderLifecycle::Configured
                | p::ProviderLifecycle::Enabled
                | p::ProviderLifecycle::Trusted(_)
        ) {
            p::ProviderRegistry::set_enabled(&self.providers, id.clone(), false)?;
        }
        self.lock_events()?.push(p::EventPayload::RevocationEvent(
            p::RevocationEventPayload {
                target_object: p::ObjectRef(format!("app-api-connector:{}", id.0)),
                evidence_lineage: lineage,
            },
        ));
        Ok(())
    }

    pub fn prepare(&self, id: p::ProviderId, request: AppApiRequest) -> p::Result<p::ActionIntent> {
        validate_request(&request)?;
        let record = self.providers.record(&id)?;
        let p::ProviderLifecycle::Trusted(tier) = record.state else {
            return Err(p::Error(
                "App API connector is disabled or untrusted".into(),
            ));
        };
        if tier == p::TrustTier::Untrusted || record.provider.is_revoked() {
            return Err(p::Error("App API connector is disabled or revoked".into()));
        }
        let grant = record
            .grant
            .ok_or_else(|| p::Error("App API connector has no autonomy grant".into()))?;
        let endpoint = resolve_resource(&record.provider.base_url, &request.resource)?;
        validate_operation(&record.provider, &request.operation)?;
        if !grant
            .capability
            .capabilities
            .contains(&record.provider.capability_ref)
            || !grant
                .capability
                .permissions
                .contains(&record.provider.permission)
            || !grant.action_type.contains(&request.action_type)
            || request.risk > grant.risk_limit
            || request.requested_at < grant.timebox.starts_at
            || request.requested_at > grant.timebox.expires_at
        {
            return Err(p::Error(
                "App API request is outside the connector grant".into(),
            ));
        }
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        let rollback = match &request.operation {
            p::AppApiOperation::Read => "read-only-no-rollback-needed",
            p::AppApiOperation::Mutation { .. } => "api-mutation-not-retractable",
        };
        Ok(p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId(format!("app-api:{}:{sequence}", id.0)),
            source: request.source,
            goal: request.goal,
            backend_hint: p::BackendKind::AppApi,
            capability_ref: record.provider.capability_ref,
            action_type: request.action_type,
            scope: grant.scope,
            risk_hint: request.risk,
            expected_effect: p::ExpectedEffect::Outward,
            rollback_expectation: p::RollbackBoundary(rollback.into()),
            parameters: p::ActionParameters::AppApi(p::AppApiActionSpec {
                schema_version: p::SchemaVersion(1),
                connector: id,
                endpoint,
                schema_digest: record.provider.schema_digest,
                credential: record.provider.credential_ref,
                operation: request.operation,
                timeout: record.provider.timeout,
                participant: request.participant,
                representation: request.representation,
                disclosure_request: request.disclosure_request,
            }),
            requested_permissions: vec![record.provider.permission],
            requested_at: request.requested_at,
            estimated_output_bytes: request.estimated_output_bytes,
            estimated_duration: record.provider.timeout,
        })
    }

    pub fn index_active(
        &self,
        capabilities: &InMemoryCapabilityRegistry,
        context: &p::ResolveContext,
    ) -> p::Result<()> {
        for connector in p::ProviderRegistry::discover(&self.providers) {
            capabilities.remove_source(&connector.source_ref())?;
        }
        for active in p::ProviderRegistry::resolve(&self.providers, context).providers {
            if active.provider.is_revoked() {
                continue;
            }
            capabilities.index(CapabilitySource {
                schema_version: p::SchemaVersion(1),
                source_ref: active.provider.source_ref(),
                trust: active.trust,
                entries: vec![CapabilityDescriptor {
                    schema_version: p::SchemaVersion(1),
                    id: active.provider.capability_ref.clone(),
                    capability: Capability::AppApi(active.provider.provider_id.clone()),
                    scope: active.grant.scope,
                    permissions: vec![active.provider.permission],
                    risk: Some(active.grant.risk_limit),
                    enabled: true,
                }],
            })?;
        }
        Ok(())
    }

    pub fn connector(&self, id: &p::ProviderId) -> p::Result<ProjectAppApiConnector> {
        self.providers.record(id).map(|record| record.provider)
    }

    pub fn take_provider_transitions(&self) -> Vec<p::ProviderTransition> {
        self.providers.take_transitions()
    }

    pub fn take_events(&self) -> Vec<p::EventPayload> {
        self.lock_events()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default()
    }

    fn lock_events(&self) -> p::Result<MutexGuard<'_, Vec<p::EventPayload>>> {
        self.events
            .lock()
            .map_err(|_| p::Error("App API connector event state is unavailable".into()))
    }
}

impl ExecutionCapabilityRechecker for AppApiConnectorRegistry {
    fn recheck(&self, intent: &p::ActionIntent, context: &PolicyContext) -> p::Result<()> {
        let p::ActionParameters::AppApi(spec) = &intent.parameters else {
            return if intent.backend_hint == p::BackendKind::AppApi {
                Err(p::Error(
                    "App API execution requires App API parameters".into(),
                ))
            } else {
                Ok(())
            };
        };
        let record = self.providers.record(&spec.connector)?;
        if !matches!(
            record.state,
            p::ProviderLifecycle::Trusted(tier) if tier != p::TrustTier::Untrusted
        ) || record.provider.is_revoked()
        {
            return Err(p::Error(
                "App API connector was disabled or revoked before execution".into(),
            ));
        }
        let grant = record
            .grant
            .as_ref()
            .ok_or_else(|| p::Error("App API connector grant was revoked".into()))?;
        validate_bound_spec(&record.provider, spec)?;
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
            || intent.requested_at < grant.timebox.starts_at
            || intent.requested_at > grant.timebox.expires_at
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
                "App API execution no longer fits its connector grant or policy".into(),
            ));
        }
        Ok(())
    }
}

fn validate_request(request: &AppApiRequest) -> p::Result<()> {
    if request.schema_version.0 == 0
        || request.resource.trim().is_empty()
        || request.goal.0.trim().is_empty()
        || request.estimated_output_bytes == 0
    {
        return Err(p::Error("App API request is incomplete".into()));
    }
    if request.action_type == p::ActionType::Deliver
        && (request
            .participant
            .as_ref()
            .is_none_or(|participant| !safe_header_component(&participant.0))
            || request.representation.is_none()
            || request
                .disclosure_request
                .as_ref()
                .is_none_or(|reference| reference.0.trim().is_empty()))
    {
        return Err(p::Error(
            "App API delivery has no bound participant, representation, or disclosure decision"
                .into(),
        ));
    }
    match &request.operation {
        p::AppApiOperation::Read
            if !matches!(
                request.action_type,
                p::ActionType::Observe | p::ActionType::Analyze
            ) =>
        {
            Err(p::Error("App API read must remain an observation".into()))
        }
        p::AppApiOperation::Mutation { .. }
            if matches!(
                request.action_type,
                p::ActionType::Observe | p::ActionType::Analyze | p::ActionType::Prepare
            ) =>
        {
            Err(p::Error(
                "App API mutation requires an external action type".into(),
            ))
        }
        _ => Ok(()),
    }
}

fn validate_bound_spec(
    connector: &ProjectAppApiConnector,
    spec: &p::AppApiActionSpec,
) -> p::Result<()> {
    if spec.schema_version.0 == 0
        || spec.connector != connector.provider_id
        || spec.schema_digest != connector.schema_digest
        || spec.credential != connector.credential_ref
        || spec.timeout.0 == 0
        || spec.timeout.0 > connector.timeout.0
    {
        return Err(p::Error(
            "App API plan no longer matches connector identity or schema".into(),
        ));
    }
    ensure_endpoint_within(&connector.base_url, &spec.endpoint)?;
    validate_operation(connector, &spec.operation)
}

fn validate_operation(
    connector: &ProjectAppApiConnector,
    operation: &p::AppApiOperation,
) -> p::Result<()> {
    match operation {
        p::AppApiOperation::Read => Ok(()),
        p::AppApiOperation::Mutation {
            method,
            body,
            idempotency_key,
        } => {
            if !connector.allowed_mutations.contains(method)
                || !safe_header_component(&idempotency_key.0)
                || body.as_ref().is_some_and(|input| match input {
                    p::ExternalInput::Literal(value) => value.len() > 64 * 1024,
                    p::ExternalInput::Content(reference) => reference.0.trim().is_empty(),
                    p::ExternalInput::Secret(reference) => reference.0.trim().is_empty(),
                })
            {
                return Err(p::Error(
                    "App API mutation is not allowlisted or bounded".into(),
                ));
            }
            Ok(())
        }
    }
}

fn safe_header_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
}

fn validate_base_url(value: &str) -> p::Result<Url> {
    let url =
        Url::parse(value).map_err(|_| p::Error("App API base URL cannot be parsed".into()))?;
    if !safe_http_endpoint(&url)
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().ends_with('/')
    {
        return Err(p::Error(
            "App API base URL must be a credential-free HTTP origin and path prefix".into(),
        ));
    }
    Ok(url)
}

fn resolve_resource(base: &str, resource: &str) -> p::Result<String> {
    let base = validate_base_url(base)?;
    let endpoint = base
        .join(resource)
        .map_err(|_| p::Error("App API resource cannot be resolved".into()))?;
    ensure_parsed_endpoint_within(&base, &endpoint)?;
    Ok(endpoint.to_string())
}

fn ensure_endpoint_within(base: &str, endpoint: &str) -> p::Result<()> {
    let base = validate_base_url(base)?;
    let endpoint =
        Url::parse(endpoint).map_err(|_| p::Error("App API endpoint cannot be parsed".into()))?;
    ensure_parsed_endpoint_within(&base, &endpoint)
}

fn ensure_parsed_endpoint_within(base: &Url, endpoint: &Url) -> p::Result<()> {
    if !safe_http_endpoint(endpoint)
        || endpoint.origin() != base.origin()
        || !endpoint.path().starts_with(base.path())
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(p::Error(
            "App API endpoint is outside the connector boundary".into(),
        ));
    }
    Ok(())
}

fn safe_http_endpoint(url: &Url) -> bool {
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        && url.host_str().is_some()
        && url.port_or_known_default().is_some()
        && url.username().is_empty()
        && url.password().is_none()
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
