//! Standardized communication ingress and bounded external-session membrane (prd/14).
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_protocol as p;
use url::Url;

pub type SessionId = p::CommunicationSessionId;
pub type DisclosureOutcome = p::DisclosureOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carrier {
    Software,
    Hardware,
}

impl Carrier {
    pub fn as_ref(self) -> p::CarrierRef {
        p::CarrierRef(
            match self {
                Self::Software => "software",
                Self::Hardware => "hardware",
            }
            .into(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParticipantProfile {
    Owner,
    External(p::ParticipantId),
    AgentRepresentingOwner,
    ExternalAgent(p::ParticipantId),
}

impl ParticipantProfile {
    pub fn participant_id(&self) -> p::ParticipantId {
        match self {
            Self::Owner => p::ParticipantId("owner".into()),
            Self::External(id) | Self::ExternalAgent(id) => id.clone(),
            Self::AgentRepresentingOwner => p::ParticipantId("forme-representative".into()),
        }
    }

    pub fn is_external(&self) -> bool {
        matches!(
            Self::clone(self),
            Self::External(_) | Self::ExternalAgent(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInput {
    pub schema_version: p::SchemaVersion,
    pub message_id: String,
    pub modality: p::Modality,
    pub content: String,
    pub session_ref: Option<SessionId>,
    pub received_at: p::Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunicationEvent {
    pub schema_version: p::SchemaVersion,
    pub message_id: String,
    pub modality: p::Modality,
    pub carrier: Carrier,
    pub channel_adapter: p::ProviderId,
    pub participant: ParticipantProfile,
    pub scope: p::Scope,
    pub session_ref: Option<SessionId>,
    pub content: String,
    pub received_at: p::Timestamp,
}

impl CommunicationEvent {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.message_id.trim().is_empty()
            || self.channel_adapter.0.trim().is_empty()
            || self.scope.0.trim().is_empty()
            || self.content.trim().is_empty()
        {
            return Err(p::Error("communication event is incomplete".into()));
        }
        Ok(())
    }

    pub fn event_payload(&self) -> p::EventPayload {
        p::EventPayload::CommunicationEventReceived(p::CommunicationEventReceivedPayload {
            modality: self.modality,
            carrier: self.carrier.as_ref(),
            channel_adapter: p::ChannelAdapterRef(self.channel_adapter.0.clone()),
            participant: self.participant.participant_id(),
            scope: self.scope.clone(),
            session_ref: self.session_ref.clone(),
            content_ref: Some(p::ContentRef(format!("message:{}", self.message_id))),
        })
    }
}

pub trait ChannelAdapter: p::ChannelAdapter + Send + Sync {
    fn normalize(&self, raw: RawInput) -> p::Result<CommunicationEvent>;
}

#[derive(Debug, Clone)]
pub struct TextChannelAdapter {
    pub schema_version: p::SchemaVersion,
    pub id: p::ProviderId,
    pub participant: ParticipantProfile,
    pub scope: p::Scope,
    pub enabled: bool,
}

impl p::ExternalProvider for TextChannelAdapter {
    fn kind(&self) -> p::ProviderKind {
        p::ProviderKind::Channel
    }

    fn id(&self) -> p::ProviderId {
        self.id.clone()
    }

    fn declared_capabilities(&self) -> p::CapabilitySet {
        p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef("channel:text".into())],
            permissions: Vec::new(),
        }
    }

    fn trust_default(&self) -> p::TrustTier {
        p::TrustTier::Untrusted
    }
}

impl p::ChannelAdapter for TextChannelAdapter {}

impl ChannelAdapter for TextChannelAdapter {
    fn normalize(&self, raw: RawInput) -> p::Result<CommunicationEvent> {
        if !self.enabled
            || self.schema_version.0 == 0
            || raw.schema_version.0 == 0
            || raw.modality != p::Modality::Text
            || raw.message_id.trim().is_empty()
            || raw.content.trim().is_empty()
        {
            return Err(p::Error(
                "text channel input is disabled, malformed, or not text".into(),
            ));
        }
        Ok(CommunicationEvent {
            schema_version: p::SchemaVersion(1),
            message_id: raw.message_id,
            modality: raw.modality,
            carrier: Carrier::Software,
            channel_adapter: self.id.clone(),
            participant: self.participant.clone(),
            scope: self.scope.clone(),
            session_ref: raw.session_ref,
            content: raw.content,
            received_at: raw.received_at,
        })
    }
}

#[derive(Debug, Clone)]
pub struct LoopbackChannelAdapter {
    pub schema_version: p::SchemaVersion,
    pub id: p::ProviderId,
    pub participant: ParticipantProfile,
    pub scope: p::Scope,
    pub connector: p::ProviderId,
    pub endpoint: String,
    pub schema_digest: p::SchemaDigest,
    pub credential_ref: Option<p::SecretRef>,
    pub capability_ref: p::CapabilityRef,
    pub permission: p::PermissionRef,
    pub enabled: bool,
}

impl LoopbackChannelAdapter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: p::ProviderId,
        participant: ParticipantProfile,
        scope: p::Scope,
        connector: p::ProviderId,
        endpoint: String,
        schema_digest: p::SchemaDigest,
        credential_ref: Option<p::SecretRef>,
        capability_ref: p::CapabilityRef,
        permission: p::PermissionRef,
    ) -> p::Result<Self> {
        let adapter = Self {
            schema_version: p::SchemaVersion(1),
            id,
            participant,
            scope,
            connector,
            endpoint,
            schema_digest,
            credential_ref,
            capability_ref,
            permission,
            enabled: true,
        };
        adapter.validate()?;
        Ok(adapter)
    }

    pub fn prepare_delivery(
        &self,
        fabric: &InMemoryCommunicationFabric,
        message: OutboundMessage,
    ) -> p::Result<PreparedDelivery> {
        self.validate()?;
        let authorization = fabric.authorize_delivery(&self.participant, &message)?;
        if !matches!(
            authorization.outcome,
            DisclosureOutcome::Answer | DisclosureOutcome::Approve
        ) {
            return Err(p::Error(
                "disclosure policy did not authorize an exact outbound body".into(),
            ));
        }
        if !authorization
            .envelope
            .capability
            .capabilities
            .contains(&self.capability_ref)
            || !authorization
                .envelope
                .capability
                .permissions
                .contains(&self.permission)
            || !authorization
                .envelope
                .action_type
                .contains(&p::ActionType::Deliver)
            || !scope_within(&self.scope, &authorization.envelope.scope)
        {
            return Err(p::Error(
                "communication session envelope does not permit connector delivery".into(),
            ));
        }
        let risk = if message.high_impact
            || message.sensitive
            || !message.confirmed
            || authorization.outcome == DisclosureOutcome::Approve
        {
            p::Risk::High
        } else {
            p::Risk::Medium
        };
        let intent = p::ActionIntent {
            schema_version: p::SchemaVersion(1),
            intent_id: p::ActionId(format!("communication-delivery:{}", message.message_id)),
            source: p::Source::Communication,
            goal: p::GoalRef(authorization.purpose.0.clone()),
            backend_hint: p::BackendKind::AppApi,
            capability_ref: self.capability_ref.clone(),
            action_type: p::ActionType::Deliver,
            scope: self.scope.clone(),
            risk_hint: risk,
            expected_effect: p::ExpectedEffect::Outward,
            rollback_expectation: p::RollbackBoundary("external-message-not-retractable".into()),
            parameters: p::ActionParameters::AppApi(p::AppApiActionSpec {
                schema_version: p::SchemaVersion(1),
                connector: self.connector.clone(),
                endpoint: self.endpoint.clone(),
                schema_digest: self.schema_digest.clone(),
                credential: self.credential_ref.clone(),
                operation: p::AppApiOperation::Mutation {
                    method: p::AppApiMutationMethod::Post,
                    body: Some(p::ExternalInput::Content(message.content_ref.clone())),
                    idempotency_key: p::IdempotencyKey(format!(
                        "communication:{}",
                        message.message_id
                    )),
                },
                timeout: message.timeout,
                participant: Some(self.participant.participant_id()),
                representation: Some(authorization.representation),
                disclosure_request: Some(message.request_ref.clone()),
            }),
            requested_permissions: vec![self.permission.clone()],
            requested_at: message.requested_at,
            estimated_output_bytes: message.estimated_output_bytes,
            estimated_duration: message.timeout,
        };
        Ok(PreparedDelivery {
            schema_version: p::SchemaVersion(1),
            session: message.session,
            message_id: message.message_id,
            intent,
            envelope: authorization.envelope,
        })
    }

    fn validate(&self) -> p::Result<()> {
        let endpoint = Url::parse(&self.endpoint)
            .map_err(|_| p::Error("loopback channel endpoint cannot be parsed".into()))?;
        let is_loopback = endpoint.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        });
        if !self.enabled
            || self.schema_version.0 == 0
            || self.id.0.trim().is_empty()
            || !self.participant.is_external()
            || self.scope.0.trim().is_empty()
            || self.connector.0.trim().is_empty()
            || self.schema_digest.0.trim().is_empty()
            || self.capability_ref.0.trim().is_empty()
            || self.permission.0.trim().is_empty()
            || !safe_header_component(&self.participant.participant_id().0)
            || self
                .credential_ref
                .as_ref()
                .is_some_and(|reference| reference.0.trim().is_empty())
            || endpoint.scheme() != "http"
            || !is_loopback
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(p::Error(
                "project-owned loopback channel configuration is incomplete".into(),
            ));
        }
        Ok(())
    }
}

impl p::ExternalProvider for LoopbackChannelAdapter {
    fn kind(&self) -> p::ProviderKind {
        p::ProviderKind::Channel
    }

    fn id(&self) -> p::ProviderId {
        self.id.clone()
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

impl p::ChannelAdapter for LoopbackChannelAdapter {}

impl ChannelAdapter for LoopbackChannelAdapter {
    fn normalize(&self, raw: RawInput) -> p::Result<CommunicationEvent> {
        self.validate()?;
        if raw.schema_version.0 == 0
            || raw.modality != p::Modality::Text
            || raw.message_id.trim().is_empty()
            || raw.content.trim().is_empty()
        {
            return Err(p::Error("loopback channel input is malformed".into()));
        }
        Ok(CommunicationEvent {
            schema_version: p::SchemaVersion(1),
            message_id: raw.message_id,
            modality: p::Modality::Text,
            carrier: Carrier::Software,
            channel_adapter: self.id.clone(),
            participant: self.participant.clone(),
            scope: self.scope.clone(),
            session_ref: raw.session_ref,
            content: raw.content,
            received_at: raw.received_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessage {
    pub schema_version: p::SchemaVersion,
    pub message_id: String,
    pub session: SessionId,
    pub request_ref: p::DisclosureRequestRef,
    pub category: String,
    pub content_ref: p::ContentRef,
    pub sensitive: bool,
    pub confirmed: bool,
    pub high_impact: bool,
    pub tokens: u64,
    pub purpose_aligned: bool,
    pub risk_escalated: bool,
    pub requested_at: p::Timestamp,
    pub timeout: p::DurationMs,
    pub estimated_output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDelivery {
    pub schema_version: p::SchemaVersion,
    pub session: SessionId,
    pub message_id: String,
    pub intent: p::ActionIntent,
    pub envelope: p::AutonomyEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliveryAuthorization {
    outcome: DisclosureOutcome,
    representation: p::Representation,
    purpose: p::PurposeRef,
    envelope: p::AutonomyEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthPrincipal {
    Owner(p::VerifiedPrincipal),
    External(p::ParticipantId),
    AgentRepresentingOwner(p::VerifiedPrincipal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    LocalSession,
    Token,
    MutualChannel,
    FederatedPeer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    pub schema_version: p::SchemaVersion,
    pub principal: AuthPrincipal,
    pub method: AuthMethod,
    pub expires_at: p::Timestamp,
}

impl AuthContext {
    pub fn local_owner(owner: p::VerifiedPrincipal, expires_at: p::Timestamp) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            principal: AuthPrincipal::Owner(owner),
            method: AuthMethod::LocalSession,
            expires_at,
        }
    }

    pub fn validate(&self, now: p::Timestamp) -> p::Result<()> {
        if self.schema_version.0 == 0 || self.expires_at <= now {
            return Err(p::Error(
                "authentication context is invalid or expired".into(),
            ));
        }
        match &self.principal {
            AuthPrincipal::Owner(principal) | AuthPrincipal::AgentRepresentingOwner(principal)
                if principal.0.trim().is_empty() =>
            {
                Err(p::Error("verified principal is empty".into()))
            }
            AuthPrincipal::External(participant) if participant.0.trim().is_empty() => {
                Err(p::Error("external principal is empty".into()))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunicationPurpose {
    pub schema_version: p::SchemaVersion,
    pub id: p::PurposeRef,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisclosureRule {
    pub schema_version: p::SchemaVersion,
    pub category: String,
    pub outcome: DisclosureOutcome,
    pub high_impact_requires_approval: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepresentationRule {
    AgentOnly,
    RepresentOwnerWithDisclosure,
    ConfirmUncertainRepresentation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisclosurePolicy {
    pub schema_version: p::SchemaVersion,
    pub id: p::DisclosurePolicyRef,
    pub allow: Vec<DisclosureRule>,
    pub representation: RepresentationRule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminationPolicy {
    pub schema_version: p::SchemaVersion,
    pub max_rounds: u32,
    pub terminate_on_purpose_drift: bool,
    pub terminate_on_risk_escalation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunicationBudget {
    pub schema_version: p::SchemaVersion,
    pub max_messages: u32,
    pub max_tokens: u64,
}

impl CommunicationBudget {
    fn as_protocol(&self) -> p::Budget {
        p::Budget(format!(
            "messages={};tokens={}",
            self.max_messages, self.max_tokens
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptPolicy {
    pub schema_version: p::SchemaVersion,
    pub id: p::TranscriptPolicyRef,
    pub retain: bool,
    pub candidate_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalCommunicationGrant {
    pub schema_version: p::SchemaVersion,
    pub grant_ref: p::GrantRef,
    pub participant: ParticipantProfile,
    pub purpose: CommunicationPurpose,
    pub modality_set: Vec<p::Modality>,
    pub disclosure: DisclosurePolicy,
    pub termination: TerminationPolicy,
    pub autonomy_envelope: p::AutonomyEnvelope,
    pub ttl: p::DurationMs,
    pub budget: CommunicationBudget,
    pub transcript_policy: TranscriptPolicy,
    pub issued_at: p::Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Open,
    Terminated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunicationSession {
    pub schema_version: p::SchemaVersion,
    pub session_id: SessionId,
    pub participant: ParticipantProfile,
    pub purpose: CommunicationPurpose,
    pub modality_set: Vec<p::Modality>,
    pub disclosure_policy: DisclosurePolicy,
    pub termination_policy: TerminationPolicy,
    pub autonomy_envelope: p::AutonomyEnvelope,
    pub ttl: p::DurationMs,
    pub budget: CommunicationBudget,
    pub transcript_policy: TranscriptPolicy,
    pub opened_at: p::Timestamp,
    pub expires_at: p::Timestamp,
    pub rounds: u32,
    pub messages: u32,
    pub tokens: u64,
    pub status: SessionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisclosureRequest {
    pub schema_version: p::SchemaVersion,
    pub session: SessionId,
    pub request_ref: p::DisclosureRequestRef,
    pub category: String,
    pub sensitive: bool,
    pub confirmed: bool,
    pub high_impact: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeObservation {
    pub schema_version: p::SchemaVersion,
    pub session: SessionId,
    pub tokens: u64,
    pub purpose_aligned: bool,
    pub risk_escalated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeOutcome {
    pub schema_version: p::SchemaVersion,
    pub terminated: bool,
    pub reason: Option<p::TerminationReason>,
}

pub trait CommunicationFabric {
    fn open_session(&self, grant: ExternalCommunicationGrant) -> p::Result<SessionId>;
    fn apply_disclosure(&self, request: DisclosureRequest) -> DisclosureOutcome;
    fn terminate(&self, session: SessionId, reason: p::TerminationReason) -> p::Result<()>;
}

type Clock = Arc<dyn Fn() -> p::Timestamp + Send + Sync>;

pub struct InMemoryCommunicationFabric {
    clock: Clock,
    sequence: AtomicU64,
    state: Mutex<FabricState>,
}

#[derive(Debug, Default)]
struct FabricState {
    sessions: BTreeMap<SessionId, CommunicationSession>,
    used_disclosure_requests: BTreeSet<(SessionId, p::DisclosureRequestRef)>,
    events: Vec<p::EventPayload>,
}

impl Default for InMemoryCommunicationFabric {
    fn default() -> Self {
        Self::with_clock(system_timestamp)
    }
}

impl InMemoryCommunicationFabric {
    pub fn with_clock<F>(clock: F) -> Self
    where
        F: Fn() -> p::Timestamp + Send + Sync + 'static,
    {
        Self {
            clock: Arc::new(clock),
            sequence: AtomicU64::new(1),
            state: Mutex::new(FabricState::default()),
        }
    }

    pub fn session(&self, id: &SessionId) -> Option<CommunicationSession> {
        self.lock_state()
            .ok()
            .and_then(|state| state.sessions.get(id).cloned())
    }

    pub fn take_events(&self) -> Vec<p::EventPayload> {
        self.lock_state()
            .map(|mut state| std::mem::take(&mut state.events))
            .unwrap_or_default()
    }

    pub fn record_exchange(&self, observation: ExchangeObservation) -> p::Result<ExchangeOutcome> {
        let now = (self.clock)();
        let mut state = self.lock_state()?;
        let session = state
            .sessions
            .get_mut(&observation.session)
            .ok_or_else(|| p::Error("communication session is not registered".into()))?;
        if session.status != SessionStatus::Open {
            return Err(p::Error("communication session is terminated".into()));
        }
        session.rounds = session.rounds.saturating_add(1);
        session.messages = session.messages.saturating_add(2);
        session.tokens = session.tokens.saturating_add(observation.tokens);
        let reason = if now >= session.expires_at {
            Some(p::TerminationReason("ttl_expired".into()))
        } else if session.rounds > session.termination_policy.max_rounds {
            Some(p::TerminationReason("round_limit".into()))
        } else if session.messages > session.budget.max_messages
            || session.tokens > session.budget.max_tokens
        {
            Some(p::TerminationReason("budget_exhausted".into()))
        } else if !observation.purpose_aligned
            && session.termination_policy.terminate_on_purpose_drift
        {
            Some(p::TerminationReason("purpose_drift".into()))
        } else if observation.risk_escalated
            && session.termination_policy.terminate_on_risk_escalation
        {
            Some(p::TerminationReason("risk_escalation".into()))
        } else {
            None
        };
        if let Some(reason) = &reason {
            session.status = SessionStatus::Terminated;
            state
                .events
                .push(p::EventPayload::CommunicationSessionTerminated(
                    p::CommunicationSessionTerminatedPayload {
                        session_id: observation.session,
                        termination_reason: reason.clone(),
                    },
                ));
        }
        Ok(ExchangeOutcome {
            schema_version: p::SchemaVersion(1),
            terminated: reason.is_some(),
            reason,
        })
    }

    fn authorize_delivery(
        &self,
        participant: &ParticipantProfile,
        message: &OutboundMessage,
    ) -> p::Result<DeliveryAuthorization> {
        validate_outbound_message(message)?;
        let now = (self.clock)();
        let mut state = self.lock_state()?;
        let expired = {
            let session = state
                .sessions
                .get_mut(&message.session)
                .ok_or_else(|| p::Error("communication session is not registered".into()))?;
            if session.status != SessionStatus::Open || &session.participant != participant {
                return Err(p::Error(
                    "communication delivery participant or session is inactive".into(),
                ));
            }
            let expired = now >= session.expires_at;
            if expired {
                session.status = SessionStatus::Terminated;
            }
            expired
        };
        if expired {
            state
                .events
                .push(p::EventPayload::CommunicationSessionTerminated(
                    p::CommunicationSessionTerminatedPayload {
                        session_id: message.session.clone(),
                        termination_reason: p::TerminationReason("ttl_expired".into()),
                    },
                ));
            return Err(p::Error("communication session TTL expired".into()));
        }
        if !state
            .used_disclosure_requests
            .insert((message.session.clone(), message.request_ref.clone()))
        {
            return Err(p::Error(
                "communication disclosure request was already consumed".into(),
            ));
        }
        let request = DisclosureRequest {
            schema_version: p::SchemaVersion(1),
            session: message.session.clone(),
            request_ref: message.request_ref.clone(),
            category: message.category.clone(),
            sensitive: message.sensitive,
            confirmed: message.confirmed,
            high_impact: message.high_impact,
        };
        let (outcome, representation, purpose, envelope, termination) = {
            let session = state
                .sessions
                .get_mut(&message.session)
                .ok_or_else(|| p::Error("communication session disappeared".into()))?;
            let outcome = disclosure_decision(session, &request);
            let representation = disclosure_representation(session);
            let purpose = session.purpose.id.clone();
            let envelope = session.autonomy_envelope.clone();
            let termination = if matches!(
                outcome,
                DisclosureOutcome::Answer | DisclosureOutcome::Approve
            ) {
                let next_rounds = session.rounds.saturating_add(1);
                let next_messages = session.messages.saturating_add(1);
                let next_tokens = session.tokens.saturating_add(message.tokens);
                let termination = if next_rounds > session.termination_policy.max_rounds {
                    Some(p::TerminationReason("round_limit".into()))
                } else if next_messages > session.budget.max_messages
                    || next_tokens > session.budget.max_tokens
                {
                    Some(p::TerminationReason("budget_exhausted".into()))
                } else if !message.purpose_aligned
                    && session.termination_policy.terminate_on_purpose_drift
                {
                    Some(p::TerminationReason("purpose_drift".into()))
                } else if message.risk_escalated
                    && session.termination_policy.terminate_on_risk_escalation
                {
                    Some(p::TerminationReason("risk_escalation".into()))
                } else {
                    None
                };
                if termination.is_some() {
                    session.status = SessionStatus::Terminated;
                } else {
                    session.rounds = next_rounds;
                    session.messages = next_messages;
                    session.tokens = next_tokens;
                }
                termination
            } else {
                None
            };
            (outcome, representation, purpose, envelope, termination)
        };
        state.events.push(p::EventPayload::DisclosurePolicyApplied(
            p::DisclosurePolicyAppliedPayload {
                request: message.request_ref.clone(),
                outcome,
                representation,
                binding: Some(p::DisclosureBinding {
                    schema_version: p::SchemaVersion(1),
                    session: message.session.clone(),
                    participant: participant.participant_id(),
                    purpose: purpose.clone(),
                    content_ref: message.content_ref.clone(),
                    category: message.category.clone(),
                    sensitive: message.sensitive,
                    confirmed: message.confirmed,
                    high_impact: message.high_impact,
                }),
            },
        ));
        if !matches!(
            outcome,
            DisclosureOutcome::Answer | DisclosureOutcome::Approve
        ) {
            return Ok(DeliveryAuthorization {
                outcome,
                representation,
                purpose,
                envelope,
            });
        }
        if let Some(reason) = termination {
            state
                .events
                .push(p::EventPayload::CommunicationSessionTerminated(
                    p::CommunicationSessionTerminatedPayload {
                        session_id: message.session.clone(),
                        termination_reason: reason,
                    },
                ));
            return Err(p::Error(
                "communication delivery exceeded a termination boundary".into(),
            ));
        }
        Ok(DeliveryAuthorization {
            outcome,
            representation,
            purpose,
            envelope,
        })
    }

    fn lock_state(&self) -> p::Result<MutexGuard<'_, FabricState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("communication fabric state is unavailable".into()))
    }
}

impl CommunicationFabric for InMemoryCommunicationFabric {
    fn open_session(&self, grant: ExternalCommunicationGrant) -> p::Result<SessionId> {
        validate_grant(&grant)?;
        let now = (self.clock)().max(grant.issued_at);
        let ttl = i64::try_from(grant.ttl.0)
            .map_err(|_| p::Error("communication TTL exceeds timestamp range".into()))?;
        let expires_at = now.saturating_add(ttl);
        if expires_at > grant.autonomy_envelope.timebox.expires_at {
            return Err(p::Error(
                "communication grant exceeds its autonomy envelope".into(),
            ));
        }
        let id = p::CommunicationSessionId(format!(
            "communication-session:{}",
            self.sequence.fetch_add(1, Ordering::SeqCst)
        ));
        let session = CommunicationSession {
            schema_version: p::SchemaVersion(1),
            session_id: id.clone(),
            participant: grant.participant.clone(),
            purpose: grant.purpose.clone(),
            modality_set: grant.modality_set.clone(),
            disclosure_policy: grant.disclosure.clone(),
            termination_policy: grant.termination.clone(),
            autonomy_envelope: grant.autonomy_envelope,
            ttl: grant.ttl,
            budget: grant.budget.clone(),
            transcript_policy: grant.transcript_policy.clone(),
            opened_at: now,
            expires_at,
            rounds: 0,
            messages: 0,
            tokens: 0,
            status: SessionStatus::Open,
        };
        let mut state = self.lock_state()?;
        state
            .events
            .push(p::EventPayload::ExternalCommunicationGranted(
                p::ExternalCommunicationGrantedPayload {
                    grant_ref: grant.grant_ref,
                    purpose: grant.purpose.id.clone(),
                    disclosure: grant.disclosure.id,
                    ttl: grant.ttl,
                    budget: grant.budget.as_protocol(),
                    transcript_policy: grant.transcript_policy.id,
                },
            ));
        state
            .events
            .push(p::EventPayload::CommunicationSessionOpened(
                p::CommunicationSessionOpenedPayload {
                    session_id: id.clone(),
                    participant: grant.participant.participant_id(),
                    purpose: grant.purpose.id,
                    ttl: grant.ttl,
                    budget: grant.budget.as_protocol(),
                },
            ));
        state.sessions.insert(id.clone(), session);
        Ok(id)
    }

    fn apply_disclosure(&self, request: DisclosureRequest) -> DisclosureOutcome {
        let mut state = match self.lock_state() {
            Ok(state) => state,
            Err(_) => return DisclosureOutcome::Refuse,
        };
        let active = state.sessions.get(&request.session).is_some_and(|session| {
            session.status == SessionStatus::Open && (self.clock)() < session.expires_at
        });
        if !active {
            return DisclosureOutcome::Refuse;
        }
        if !state
            .used_disclosure_requests
            .insert((request.session.clone(), request.request_ref.clone()))
        {
            return DisclosureOutcome::Refuse;
        }
        let Some(session) = state.sessions.get(&request.session) else {
            return DisclosureOutcome::Refuse;
        };
        let outcome = disclosure_decision(session, &request);
        let representation = disclosure_representation(session);
        state.events.push(p::EventPayload::DisclosurePolicyApplied(
            p::DisclosurePolicyAppliedPayload {
                request: request.request_ref,
                outcome,
                representation,
                binding: None,
            },
        ));
        outcome
    }

    fn terminate(&self, session: SessionId, reason: p::TerminationReason) -> p::Result<()> {
        let mut state = self.lock_state()?;
        let record = state
            .sessions
            .get_mut(&session)
            .ok_or_else(|| p::Error("communication session is not registered".into()))?;
        if record.status == SessionStatus::Terminated {
            return Ok(());
        }
        record.status = SessionStatus::Terminated;
        state
            .events
            .push(p::EventPayload::CommunicationSessionTerminated(
                p::CommunicationSessionTerminatedPayload {
                    session_id: session,
                    termination_reason: reason,
                },
            ));
        Ok(())
    }
}

fn validate_grant(grant: &ExternalCommunicationGrant) -> p::Result<()> {
    if grant.schema_version.0 == 0
        || grant.grant_ref.0.trim().is_empty()
        || grant.purpose.id.0.trim().is_empty()
        || grant.purpose.description.trim().is_empty()
        || grant.modality_set.is_empty()
        || grant.ttl.0 == 0
        || grant.budget.max_messages == 0
        || grant.budget.max_tokens == 0
        || grant.termination.max_rounds == 0
        || !grant.transcript_policy.candidate_only
        || !grant.participant.is_external()
        || !safe_header_component(&grant.participant.participant_id().0)
    {
        return Err(p::Error(
            "external communication grant must be bounded and candidate-only".into(),
        ));
    }
    Ok(())
}

fn disclosure_decision(
    session: &CommunicationSession,
    request: &DisclosureRequest,
) -> DisclosureOutcome {
    let rule = session
        .disclosure_policy
        .allow
        .iter()
        .find(|rule| rule.category == request.category || rule.category == "*");
    match rule {
        None => DisclosureOutcome::Refuse,
        Some(rule)
            if request.high_impact
                && (rule.high_impact_requires_approval
                    || !request.confirmed
                    || session.disclosure_policy.representation
                        == RepresentationRule::ConfirmUncertainRepresentation) =>
        {
            DisclosureOutcome::Approve
        }
        Some(rule) if request.sensitive && rule.outcome == DisclosureOutcome::Answer => {
            DisclosureOutcome::Blur
        }
        Some(rule) => rule.outcome,
    }
}

fn disclosure_representation(session: &CommunicationSession) -> p::Representation {
    match session.disclosure_policy.representation {
        RepresentationRule::AgentOnly => p::Representation::Agent,
        RepresentationRule::RepresentOwnerWithDisclosure
        | RepresentationRule::ConfirmUncertainRepresentation => {
            p::Representation::AgentRepresentingOwner
        }
    }
}

fn validate_outbound_message(message: &OutboundMessage) -> p::Result<()> {
    if message.schema_version.0 == 0
        || !safe_header_component(&message.message_id)
        || message.session.0.trim().is_empty()
        || message.request_ref.0.trim().is_empty()
        || message.category.trim().is_empty()
        || message.content_ref.0.trim().is_empty()
        || message.tokens == 0
        || message.timeout.0 == 0
        || message.estimated_output_bytes == 0
    {
        return Err(p::Error(
            "outbound communication message is incomplete".into(),
        ));
    }
    Ok(())
}

fn safe_header_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
}

fn scope_within(child: &p::Scope, parent: &p::Scope) -> bool {
    child == parent
        || child
            .0
            .strip_prefix(&parent.0)
            .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('/'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceGrant {
    pub schema_version: p::SchemaVersion,
    pub grant_ref: p::GrantRef,
    pub device_id: String,
    pub modalities: Vec<p::Modality>,
    pub purpose: p::PurposeRef,
    pub scope: p::Scope,
    pub expires_at: p::Timestamp,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationScope {
    pub schema_version: p::SchemaVersion,
    pub scope: p::Scope,
    pub foreground_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub schema_version: p::SchemaVersion,
    pub retain_raw: bool,
    pub ttl: p::DurationMs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceObservation {
    pub schema_version: p::SchemaVersion,
    pub device_id: String,
    pub foreground: bool,
    pub raw: RawInput,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizedDeviceObservation {
    schema_version: p::SchemaVersion,
    event: CommunicationEvent,
    observation: p::EventPayload,
    grant_ref: p::GrantRef,
    raw_retained: bool,
    retention_expires_at: p::Timestamp,
}

impl AuthorizedDeviceObservation {
    pub fn raw_retained(&self) -> bool {
        self.raw_retained
    }

    pub fn retention_expires_at(&self) -> p::Timestamp {
        self.retention_expires_at
    }

    pub fn into_gateway_parts(
        self,
        now: p::Timestamp,
    ) -> p::Result<(CommunicationEvent, p::EventPayload)> {
        if self.schema_version.0 == 0
            || self.grant_ref.0.trim().is_empty()
            || self.raw_retained
            || self.retention_expires_at <= self.event.received_at
            || self.retention_expires_at <= now
        {
            return Err(p::Error(
                "device observation retention boundary is invalid or expired".into(),
            ));
        }
        Ok((self.event, self.observation))
    }
}

#[derive(Debug, Clone)]
pub struct LocalDeviceChannelAdapter {
    pub schema_version: p::SchemaVersion,
    pub id: p::ProviderId,
    grant: Arc<Mutex<Option<DeviceGrant>>>,
    pub observation_scope: ObservationScope,
    pub retention: RetentionPolicy,
}

impl LocalDeviceChannelAdapter {
    pub fn new(
        id: p::ProviderId,
        grant: Option<DeviceGrant>,
        observation_scope: ObservationScope,
        retention: RetentionPolicy,
    ) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            id,
            grant: Arc::new(Mutex::new(grant)),
            observation_scope,
            retention,
        }
    }

    pub fn revoke(&self) -> p::Result<()> {
        let mut grant = self
            .grant
            .lock()
            .map_err(|_| p::Error("device grant state is unavailable".into()))?;
        let grant = grant
            .as_mut()
            .ok_or_else(|| p::Error("device has no grant to revoke".into()))?;
        grant.revoked = true;
        Ok(())
    }

    pub fn revoke_with_event(&self, lineage: p::LineageRef) -> p::Result<p::EventPayload> {
        let target = {
            let grant = self
                .grant
                .lock()
                .map_err(|_| p::Error("device grant state is unavailable".into()))?;
            let grant = grant
                .as_ref()
                .ok_or_else(|| p::Error("device has no grant to revoke".into()))?;
            p::ObjectRef(format!("device-grant:{}", grant.grant_ref.0))
        };
        self.revoke()?;
        Ok(p::EventPayload::RevocationEvent(
            p::RevocationEventPayload {
                target_object: target,
                evidence_lineage: lineage,
            },
        ))
    }

    pub fn normalize_observation(
        &self,
        observation: DeviceObservation,
    ) -> p::Result<AuthorizedDeviceObservation> {
        let grant = self
            .grant
            .lock()
            .map_err(|_| p::Error("device grant state is unavailable".into()))?
            .clone()
            .ok_or_else(|| p::Error("device input has no explicit grant".into()))?;
        if self.schema_version.0 == 0
            || observation.schema_version.0 == 0
            || self.observation_scope.schema_version.0 == 0
            || self.retention.schema_version.0 == 0
            || self.retention.ttl.0 == 0
            || self.retention.retain_raw
            || observation.device_id != grant.device_id
            || (self.observation_scope.foreground_only && !observation.foreground)
            || grant.revoked
            || observation.raw.received_at >= grant.expires_at
            || !grant.modalities.contains(&observation.raw.modality)
            || grant.scope != self.observation_scope.scope
        {
            return Err(p::Error(
                "device observation is outside its active grant or retention boundary".into(),
            ));
        }
        let ttl = i64::try_from(self.retention.ttl.0)
            .map_err(|_| p::Error("device retention TTL exceeds timestamp range".into()))?;
        let normalized_content = format!(
            "authorized {:?} device observation for scope {}",
            observation.raw.modality, grant.scope.0
        );
        let event = CommunicationEvent {
            schema_version: p::SchemaVersion(1),
            message_id: observation.raw.message_id,
            modality: observation.raw.modality,
            carrier: Carrier::Hardware,
            channel_adapter: self.id.clone(),
            participant: ParticipantProfile::Owner,
            scope: grant.scope.clone(),
            session_ref: observation.raw.session_ref,
            content: normalized_content,
            received_at: observation.raw.received_at,
        };
        event.validate()?;
        Ok(AuthorizedDeviceObservation {
            schema_version: p::SchemaVersion(1),
            event,
            observation: p::EventPayload::ObservationRecorded(p::ObservationRecordedPayload {
                source: p::Source::Communication,
                scope: grant.scope,
                grant_ref: Some(grant.grant_ref.clone()),
            }),
            grant_ref: grant.grant_ref,
            raw_retained: false,
            retention_expires_at: observation.raw.received_at.saturating_add(ttl),
        })
    }
}

impl p::ExternalProvider for LocalDeviceChannelAdapter {
    fn kind(&self) -> p::ProviderKind {
        p::ProviderKind::Channel
    }

    fn id(&self) -> p::ProviderId {
        self.id.clone()
    }

    fn declared_capabilities(&self) -> p::CapabilitySet {
        p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef("channel:local-device".into())],
            permissions: vec![p::PermissionRef("device:observe".into())],
        }
    }

    fn trust_default(&self) -> p::TrustTier {
        p::TrustTier::Untrusted
    }
}

impl p::ChannelAdapter for LocalDeviceChannelAdapter {}

impl ChannelAdapter for LocalDeviceChannelAdapter {
    fn normalize(&self, raw: RawInput) -> p::Result<CommunicationEvent> {
        let grant = self
            .grant
            .lock()
            .map_err(|_| p::Error("device grant state is unavailable".into()))?
            .clone()
            .ok_or_else(|| p::Error("device input has no explicit grant".into()))?;
        if grant.revoked
            || raw.received_at >= grant.expires_at
            || !grant.modalities.contains(&raw.modality)
            || grant.scope != self.observation_scope.scope
        {
            return Err(p::Error("device input is outside its active grant".into()));
        }
        Ok(CommunicationEvent {
            schema_version: p::SchemaVersion(1),
            message_id: raw.message_id,
            modality: raw.modality,
            carrier: Carrier::Hardware,
            channel_adapter: self.id.clone(),
            participant: ParticipantProfile::Owner,
            scope: grant.scope,
            session_ref: raw.session_ref,
            content: raw.content,
            received_at: raw.received_at,
        })
    }
}

fn system_timestamp() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload_kinds(events: &[p::EventPayload]) -> Vec<p::EventKind> {
        events.iter().map(p::EventPayload::kind).collect()
    }

    fn envelope(now: p::Timestamp) -> p::AutonomyEnvelope {
        p::AutonomyEnvelope {
            schema_version: p::SchemaVersion(1),
            scope: p::Scope("communication:test".into()),
            capability: p::CapabilitySet {
                schema_version: p::SchemaVersion(1),
                capabilities: vec![p::CapabilityRef("channel:text".into())],
                permissions: vec![p::PermissionRef("communicate".into())],
            },
            action_type: vec![p::ActionType::Deliver],
            risk_limit: p::Risk::Medium,
            approval_rule: p::ApprovalRule::Ask,
            budget: p::Budget("bounded".into()),
            timebox: p::Timebox {
                schema_version: p::SchemaVersion(1),
                starts_at: now,
                expires_at: now + 10_000,
                max_turns: 4,
            },
            rollback: p::RollbackReq {
                schema_version: p::SchemaVersion(1),
                required: false,
                boundary: None,
            },
        }
    }

    fn grant(participant: ParticipantProfile) -> ExternalCommunicationGrant {
        ExternalCommunicationGrant {
            schema_version: p::SchemaVersion(1),
            grant_ref: p::GrantRef("external-grant".into()),
            participant,
            purpose: CommunicationPurpose {
                schema_version: p::SchemaVersion(1),
                id: p::PurposeRef("bounded-support".into()),
                description: "Answer one scoped support question".into(),
            },
            modality_set: vec![p::Modality::Text],
            disclosure: DisclosurePolicy {
                schema_version: p::SchemaVersion(1),
                id: p::DisclosurePolicyRef("disclosure:test".into()),
                allow: vec![DisclosureRule {
                    schema_version: p::SchemaVersion(1),
                    category: "public".into(),
                    outcome: DisclosureOutcome::Answer,
                    high_impact_requires_approval: true,
                }],
                representation: RepresentationRule::AgentOnly,
            },
            termination: TerminationPolicy {
                schema_version: p::SchemaVersion(1),
                max_rounds: 1,
                terminate_on_purpose_drift: true,
                terminate_on_risk_escalation: true,
            },
            autonomy_envelope: envelope(100),
            ttl: p::DurationMs(1_000),
            budget: CommunicationBudget {
                schema_version: p::SchemaVersion(1),
                max_messages: 4,
                max_tokens: 100,
            },
            transcript_policy: TranscriptPolicy {
                schema_version: p::SchemaVersion(1),
                id: p::TranscriptPolicyRef("candidate-only".into()),
                retain: true,
                candidate_only: true,
            },
            issued_at: 100,
        }
    }

    #[test]
    fn s12_text_adapter_only_normalizes_and_defaults_untrusted() {
        let adapter = TextChannelAdapter {
            schema_version: p::SchemaVersion(1),
            id: p::ProviderId("channel:test".into()),
            participant: ParticipantProfile::External(p::ParticipantId("person-a".into())),
            scope: p::Scope("communication:test".into()),
            enabled: true,
        };
        let event = adapter
            .normalize(RawInput {
                schema_version: p::SchemaVersion(1),
                message_id: "message-1".into(),
                modality: p::Modality::Text,
                content: "hello".into(),
                session_ref: None,
                received_at: 100,
            })
            .unwrap();
        assert_eq!(event.carrier, Carrier::Software);
        assert_eq!(
            payload_kinds(&[event.event_payload()]),
            vec![p::EventKind::CommunicationEventReceived]
        );
        assert_eq!(
            p::ExternalProvider::trust_default(&adapter),
            p::TrustTier::Untrusted
        );
    }

    #[test]
    fn s13_external_session_is_bounded_and_terminates_on_round_limit() {
        let fabric = InMemoryCommunicationFabric::with_clock(|| 100);
        let session = fabric
            .open_session(grant(ParticipantProfile::External(p::ParticipantId(
                "person-a".into(),
            ))))
            .unwrap();
        assert!(
            !fabric
                .record_exchange(ExchangeObservation {
                    schema_version: p::SchemaVersion(1),
                    session: session.clone(),
                    tokens: 10,
                    purpose_aligned: true,
                    risk_escalated: false,
                })
                .unwrap()
                .terminated
        );
        assert!(
            fabric
                .record_exchange(ExchangeObservation {
                    schema_version: p::SchemaVersion(1),
                    session: session.clone(),
                    tokens: 10,
                    purpose_aligned: true,
                    risk_escalated: false,
                })
                .unwrap()
                .terminated
        );
        assert_eq!(
            fabric.session(&session).unwrap().status,
            SessionStatus::Terminated
        );
        assert_eq!(
            payload_kinds(&fabric.take_events()),
            vec![
                p::EventKind::ExternalCommunicationGranted,
                p::EventKind::CommunicationSessionOpened,
                p::EventKind::CommunicationSessionTerminated,
            ]
        );
    }

    #[test]
    fn s14_sensitive_disclosure_is_refused_or_blurred_and_audited() {
        let fabric = InMemoryCommunicationFabric::with_clock(|| 100);
        let session = fabric
            .open_session(grant(ParticipantProfile::External(p::ParticipantId(
                "person-a".into(),
            ))))
            .unwrap();
        assert_eq!(
            fabric.apply_disclosure(DisclosureRequest {
                schema_version: p::SchemaVersion(1),
                session: session.clone(),
                request_ref: p::DisclosureRequestRef("private-request".into()),
                category: "private".into(),
                sensitive: true,
                confirmed: false,
                high_impact: true,
            }),
            DisclosureOutcome::Refuse
        );
        assert_eq!(
            fabric.apply_disclosure(DisclosureRequest {
                schema_version: p::SchemaVersion(1),
                session,
                request_ref: p::DisclosureRequestRef("public-sensitive".into()),
                category: "public".into(),
                sensitive: true,
                confirmed: true,
                high_impact: false,
            }),
            DisclosureOutcome::Blur
        );
        assert_eq!(
            payload_kinds(&fabric.take_events()),
            vec![
                p::EventKind::ExternalCommunicationGranted,
                p::EventKind::CommunicationSessionOpened,
                p::EventKind::DisclosurePolicyApplied,
                p::EventKind::DisclosurePolicyApplied,
            ]
        );
    }

    #[test]
    fn s15_representation_never_claims_owner_identity_and_uncertainty_needs_approval() {
        let fabric = InMemoryCommunicationFabric::with_clock(|| 100);
        let mut bounded = grant(ParticipantProfile::External(p::ParticipantId(
            "person-a".into(),
        )));
        bounded.disclosure.representation = RepresentationRule::ConfirmUncertainRepresentation;
        let session = fabric.open_session(bounded).unwrap();
        assert_eq!(
            fabric.apply_disclosure(DisclosureRequest {
                schema_version: p::SchemaVersion(1),
                session,
                request_ref: p::DisclosureRequestRef("represented-view".into()),
                category: "public".into(),
                sensitive: false,
                confirmed: false,
                high_impact: true,
            }),
            DisclosureOutcome::Approve
        );
        let events = fabric.take_events();
        assert_eq!(
            payload_kinds(&events),
            vec![
                p::EventKind::ExternalCommunicationGranted,
                p::EventKind::CommunicationSessionOpened,
                p::EventKind::DisclosurePolicyApplied,
            ]
        );
        let event = events
            .into_iter()
            .find_map(|event| match event {
                p::EventPayload::DisclosurePolicyApplied(payload) => Some(payload),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            event.representation,
            p::Representation::AgentRepresentingOwner
        );
    }

    #[test]
    fn s16_device_requires_active_grant_and_revocation_blocks_new_events() {
        let scope = p::Scope("device:desk".into());
        let raw = RawInput {
            schema_version: p::SchemaVersion(1),
            message_id: "device-message".into(),
            modality: p::Modality::Voice,
            content: "spoken note".into(),
            session_ref: None,
            received_at: 100,
        };
        let adapter = LocalDeviceChannelAdapter::new(
            p::ProviderId("device:test".into()),
            None,
            ObservationScope {
                schema_version: p::SchemaVersion(1),
                scope: scope.clone(),
                foreground_only: true,
            },
            RetentionPolicy {
                schema_version: p::SchemaVersion(1),
                retain_raw: false,
                ttl: p::DurationMs(1_000),
            },
        );
        assert!(adapter.normalize(raw.clone()).is_err());
        let granted = LocalDeviceChannelAdapter::new(
            p::ProviderId("device:test".into()),
            Some(DeviceGrant {
                schema_version: p::SchemaVersion(1),
                grant_ref: p::GrantRef("device-grant".into()),
                device_id: "microphone:test".into(),
                modalities: vec![p::Modality::Voice],
                purpose: p::PurposeRef("note".into()),
                scope,
                expires_at: 1_000,
                revoked: false,
            }),
            adapter.observation_scope,
            adapter.retention,
        );
        let authorized = granted.normalize(raw.clone()).unwrap();
        assert_eq!(authorized.carrier, Carrier::Hardware);
        assert_eq!(
            payload_kinds(&[authorized.event_payload()]),
            vec![p::EventKind::CommunicationEventReceived]
        );
        granted.revoke().unwrap();
        assert!(granted.normalize(raw).is_err());
    }

    #[test]
    fn s17_agent_session_has_hard_purpose_budget_and_termination() {
        let fabric = InMemoryCommunicationFabric::with_clock(|| 100);
        let session = fabric
            .open_session(grant(ParticipantProfile::ExternalAgent(p::ParticipantId(
                "agent-a".into(),
            ))))
            .unwrap();
        let outcome = fabric
            .record_exchange(ExchangeObservation {
                schema_version: p::SchemaVersion(1),
                session,
                tokens: 1,
                purpose_aligned: false,
                risk_escalated: false,
            })
            .unwrap();
        assert!(outcome.terminated);
        assert_eq!(outcome.reason.unwrap().0, "purpose_drift");
        assert_eq!(
            payload_kinds(&fabric.take_events()),
            vec![
                p::EventKind::ExternalCommunicationGranted,
                p::EventKind::CommunicationSessionOpened,
                p::EventKind::CommunicationSessionTerminated,
            ]
        );
    }

    fn loopback_adapter(participant: ParticipantProfile) -> LoopbackChannelAdapter {
        LoopbackChannelAdapter::new(
            p::ProviderId("channel:project-loopback".into()),
            participant,
            p::Scope("communication:test".into()),
            p::ProviderId("connector:project-loopback".into()),
            "http://127.0.0.1:38193/api/messages".into(),
            p::SchemaDigest("sha256:project-loopback-v1".into()),
            None,
            p::CapabilityRef("channel:text".into()),
            p::PermissionRef("communicate".into()),
        )
        .unwrap()
    }

    fn outbound(session: SessionId, id: &str) -> OutboundMessage {
        OutboundMessage {
            schema_version: p::SchemaVersion(1),
            message_id: id.into(),
            session,
            request_ref: p::DisclosureRequestRef(format!("disclosure:{id}")),
            category: "public".into(),
            content_ref: p::ContentRef(format!("content:{id}")),
            sensitive: false,
            confirmed: true,
            high_impact: false,
            tokens: 10,
            purpose_aligned: true,
            risk_escalated: false,
            requested_at: 100,
            timeout: p::DurationMs(2_000),
            estimated_output_bytes: 4_096,
        }
    }

    #[test]
    fn s44_real_delivery_reserves_bounded_session_budget_before_transport() {
        let participant = ParticipantProfile::External(p::ParticipantId("person-a".into()));
        let fabric = InMemoryCommunicationFabric::with_clock(|| 100);
        let session = fabric.open_session(grant(participant.clone())).unwrap();
        fabric.take_events();
        let adapter = loopback_adapter(participant);
        let prepared = adapter
            .prepare_delivery(&fabric, outbound(session.clone(), "message-1"))
            .unwrap();
        assert_eq!(prepared.envelope.approval_rule, p::ApprovalRule::Ask);
        assert_eq!(
            payload_kinds(&fabric.take_events()),
            vec![p::EventKind::DisclosurePolicyApplied]
        );
        let current = fabric.session(&session).unwrap();
        assert_eq!(current.messages, 1);
        assert_eq!(current.rounds, 1);
        assert!(current.transcript_policy.candidate_only);

        assert!(adapter
            .prepare_delivery(&fabric, outbound(session.clone(), "message-2"))
            .is_err());
        assert_eq!(
            payload_kinds(&fabric.take_events()),
            vec![
                p::EventKind::DisclosurePolicyApplied,
                p::EventKind::CommunicationSessionTerminated,
            ]
        );
        assert_eq!(
            fabric.session(&session).unwrap().status,
            SessionStatus::Terminated
        );
    }

    #[test]
    fn s45_disclosure_and_representation_are_bound_before_delivery_intent() {
        let participant = ParticipantProfile::External(p::ParticipantId("person-a".into()));
        let fabric = InMemoryCommunicationFabric::with_clock(|| 100);
        let mut policy_grant = grant(participant.clone());
        policy_grant.disclosure.representation = RepresentationRule::ConfirmUncertainRepresentation;
        let session = fabric.open_session(policy_grant).unwrap();
        fabric.take_events();
        let adapter = loopback_adapter(participant);

        let mut refused = outbound(session.clone(), "private-message");
        refused.category = "private".into();
        refused.sensitive = true;
        assert!(adapter.prepare_delivery(&fabric, refused).is_err());
        assert_eq!(
            payload_kinds(&fabric.take_events()),
            vec![p::EventKind::DisclosurePolicyApplied]
        );

        let mut represented = outbound(session, "represented-message");
        represented.high_impact = true;
        represented.confirmed = false;
        let prepared = adapter.prepare_delivery(&fabric, represented).unwrap();
        assert_eq!(prepared.intent.risk_hint, p::Risk::High);
        let p::ActionParameters::AppApi(spec) = prepared.intent.parameters else {
            panic!("communication delivery did not use AppApi parameters");
        };
        assert_eq!(
            spec.representation,
            Some(p::Representation::AgentRepresentingOwner)
        );
        assert_ne!(spec.participant.unwrap().0, "owner");
    }

    #[test]
    fn s45_disclosure_request_ref_is_one_shot_within_a_session() {
        let participant = ParticipantProfile::External(p::ParticipantId("person-a".into()));
        let fabric = InMemoryCommunicationFabric::with_clock(|| 100);
        let session = fabric.open_session(grant(participant.clone())).unwrap();
        fabric.take_events();
        let adapter = loopback_adapter(participant);
        let first = outbound(session.clone(), "one-shot");
        adapter.prepare_delivery(&fabric, first.clone()).unwrap();
        assert_eq!(
            payload_kinds(&fabric.take_events()),
            vec![p::EventKind::DisclosurePolicyApplied]
        );

        let mut replay = first;
        replay.message_id = "changed-message".into();
        replay.content_ref = p::ContentRef("content:changed-after-disclosure".into());
        assert!(adapter.prepare_delivery(&fabric, replay).is_err());
        assert!(fabric.take_events().is_empty());
        assert_eq!(fabric.session(&session).unwrap().messages, 1);
    }

    #[test]
    fn s46_device_observation_enforces_identity_foreground_retention_and_revocation() {
        let adapter = LocalDeviceChannelAdapter::new(
            p::ProviderId("device:project-loopback".into()),
            Some(DeviceGrant {
                schema_version: p::SchemaVersion(1),
                grant_ref: p::GrantRef("device-grant:project".into()),
                device_id: "sensor:project".into(),
                modalities: vec![p::Modality::Text],
                purpose: p::PurposeRef("bounded-observation".into()),
                scope: p::Scope("device:project".into()),
                expires_at: 1_000,
                revoked: false,
            }),
            ObservationScope {
                schema_version: p::SchemaVersion(1),
                scope: p::Scope("device:project".into()),
                foreground_only: true,
            },
            RetentionPolicy {
                schema_version: p::SchemaVersion(1),
                retain_raw: false,
                ttl: p::DurationMs(500),
            },
        );
        let raw = RawInput {
            schema_version: p::SchemaVersion(1),
            message_id: "observation-1".into(),
            modality: p::Modality::Text,
            content: "untrusted observed text".into(),
            session_ref: None,
            received_at: 100,
        };
        let authorized = adapter
            .normalize_observation(DeviceObservation {
                schema_version: p::SchemaVersion(1),
                device_id: "sensor:project".into(),
                foreground: true,
                raw: raw.clone(),
            })
            .unwrap();
        assert!(!authorized.raw_retained());
        assert_eq!(authorized.retention_expires_at(), 600);
        assert!(adapter
            .normalize_observation(DeviceObservation {
                schema_version: p::SchemaVersion(1),
                device_id: "sensor:other".into(),
                foreground: true,
                raw: raw.clone(),
            })
            .is_err());
        let revocation = adapter
            .revoke_with_event(p::LineageRef("owner:device-revoke".into()))
            .unwrap();
        assert_eq!(revocation.kind(), p::EventKind::RevocationEvent);
        assert!(adapter
            .normalize_observation(DeviceObservation {
                schema_version: p::SchemaVersion(1),
                device_id: "sensor:project".into(),
                foreground: true,
                raw,
            })
            .is_err());
    }

    #[test]
    fn s47_external_agent_session_uses_the_external_membrane_and_hard_termination() {
        let participant =
            ParticipantProfile::ExternalAgent(p::ParticipantId("peer:project-agent".into()));
        let fabric = InMemoryCommunicationFabric::with_clock(|| 100);
        let session = fabric.open_session(grant(participant.clone())).unwrap();
        fabric.take_events();
        let adapter = loopback_adapter(participant);
        let mut drifted = outbound(session.clone(), "a2a-drift");
        drifted.purpose_aligned = false;
        assert!(adapter.prepare_delivery(&fabric, drifted).is_err());
        assert_eq!(
            payload_kinds(&fabric.take_events()),
            vec![
                p::EventKind::DisclosurePolicyApplied,
                p::EventKind::CommunicationSessionTerminated,
            ]
        );
        assert_eq!(
            fabric.session(&session).unwrap().status,
            SessionStatus::Terminated
        );
    }
}
