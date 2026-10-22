//! Trusted local gateway: authentication, provenance stamping, and harness routing (prd/14).
#![forbid(unsafe_code)]

mod client;
mod controls;
mod ecosystem;
mod evaluation;
mod evolution;
mod federation;
mod ingress;
mod lifecycle;
pub mod offline_data;
mod runtime;
mod scheduled;
mod server;
pub mod web;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use forme_communication::{
    AuthContext, AuthMethod, AuthPrincipal, AuthorizedDeviceObservation, ChannelAdapter,
    CommunicationEvent, CommunicationFabric, DisclosureOutcome, DisclosureRequest,
    ExternalCommunicationGrant, InMemoryCommunicationFabric, LocalDeviceChannelAdapter,
    LoopbackChannelAdapter, OutboundMessage, ParticipantProfile, RawInput, SessionId,
};
use forme_harness::{
    ApprovalGrant, ApprovalTicket, EcosystemGatewayControl as HarnessEcosystemGatewayControl,
    EvolutionGatewayControl as HarnessEvolutionGatewayControl,
    FederationActionGateway as HarnessFederationActionGateway, FederationControlResult,
    FederationGatewayControl as HarnessFederationGatewayControl,
    GatewayControl as HarnessGatewayControl, HarnessActionIngress, HarnessIngress,
    IngressAuthority, ManualEvaluator as HarnessManualEvaluator, ProductionRuntime,
    ProductionRuntimeBuilder, ReactiveHarness,
    SchedulerGatewayControl as HarnessSchedulerGatewayControl,
    SchedulerService as HarnessSchedulerService,
};
pub use forme_harness::{EventStream, EvolutionActivationResult, EvolutionEvaluationResult};
use forme_protocol as p;

pub use client::HttpGatewayClient;
pub use lifecycle::GatewayLifecycleConfig;
pub use server::{
    serve_environment, serve_environment_until, serve_managed_until, CoreServiceConfig,
    GatewayServerConfig,
};

pub trait Gateway {
    fn submit(&self, event: CommunicationEvent) -> p::Result<p::RunId>;
    fn stream_events(&self, run: p::RunId) -> EventStream;
    fn approve(&self, ticket: ApprovalTicket, grant: ApprovalGrant) -> p::Result<()>;
}

pub trait RunGateway: Send + Sync {
    fn submit_run(&self, request: p::RunRequest) -> p::Result<p::RunId>;
    fn stream_run(&self, run: p::RunId) -> EventStream;
    fn wait_run(&self, run: p::RunId) -> p::Result<p::RunResult>;
    fn answer(&self, run: p::RunId) -> p::Result<Option<String>>;

    /// Returns the runtime-bound agent profile for clients that construct a
    /// `RunRequest` (notably the CLI). Legacy test gateways retain the stable
    /// default through this additive method.
    fn agent_profile(&self) -> p::AgentProfileRef {
        p::AgentProfileRef("agent:forme-local".into())
    }

    /// Digest of the production composition manifest serving this gateway.
    /// Additive: gateways without a production runtime report that fact.
    fn runtime_manifest_digest(&self) -> p::Result<p::SchemaDigest> {
        Err(p::Error(
            "this gateway is not backed by a production runtime".into(),
        ))
    }
}

pub fn environment_gateway() -> p::Result<Box<dyn RunGateway>> {
    let runtime = Arc::new(ProductionRuntimeBuilder::from_environment()?.build()?);
    let gateway = LocalGateway::from_production_runtime(runtime)?;
    Ok(Box::new(gateway))
}

pub struct LocalGateway<H: HarnessIngress + Send + Sync> {
    harness: Arc<H>,
    production_runtime: Option<Arc<ProductionRuntime>>,
    ingress_authority: IngressAuthority,
    owner: p::VerifiedPrincipal,
    agent_profile: p::AgentProfileRef,
    sequence: AtomicU64,
}

pub struct OwnerClientGateway<H: HarnessFederationGatewayControl + Send + Sync> {
    harness: Arc<H>,
    owner: p::VerifiedPrincipal,
    sequence: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FederatedChannelPurpose {
    OwnerControl,
    ReplicaAck,
    RetentionReceipt,
    DeviceSignal,
}

/// Opaque evidence that a transport adapter matched the presented peer
/// certificate to an owner-provisioned grant. Payloads cannot construct this
/// binding and owner authentication remains a separate proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedFederatedChannel {
    schema_version: p::SchemaVersion,
    peer: p::FederatedPeerRef,
    session: p::FederatedSessionRef,
    transport_identity: p::TransportIdentityDigest,
    grant: p::FederatedPeerGrantRef,
    purpose: FederatedChannelPurpose,
    expires_at: p::Timestamp,
}

pub trait EvolutionRunGateway: Send + Sync {
    fn evolution_snapshot(&self, scope: p::Scope) -> p::Result<p::EvolutionSnapshot>;
    fn record_strategy_candidate(
        &self,
        run: p::RunId,
        candidate: p::StrategyCandidate,
    ) -> p::Result<p::EventId>;
    fn evaluate_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        comparison: p::EvolutionComparison,
    ) -> p::Result<EvolutionEvaluationResult>;
    fn promote_strategy(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
    ) -> p::Result<p::EventId>;
    fn activate_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        promotion: p::EventId,
    ) -> p::Result<EvolutionActivationResult>;
    #[allow(clippy::too_many_arguments)]
    fn rollback_strategy(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        domain: p::StrategyDomain,
        scope: p::Scope,
        restored: p::StrategyVersionRef,
        triggers: Vec<p::EvidenceRef>,
        in_flight: p::InFlightDisposition,
    ) -> p::Result<EvolutionActivationResult>;
    fn set_auto_activation_paused(&self, paused: bool) -> p::Result<()>;
    fn auto_activation_paused(&self) -> p::Result<bool>;
}

fn validate_principal(
    participant: &ParticipantProfile,
    principal: &AuthPrincipal,
    owner: &p::VerifiedPrincipal,
) -> p::Result<()> {
    let valid = match (participant, principal) {
        (ParticipantProfile::Owner, AuthPrincipal::Owner(authenticated)) => authenticated == owner,
        (
            ParticipantProfile::AgentRepresentingOwner,
            AuthPrincipal::AgentRepresentingOwner(authenticated),
        ) => authenticated == owner,
        (ParticipantProfile::External(expected), AuthPrincipal::External(actual))
        | (ParticipantProfile::ExternalAgent(expected), AuthPrincipal::External(actual)) => {
            expected == actual
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(p::Error(
            "gateway authentication does not match the event participant".into(),
        ))
    }
}

fn gateway_provenance(
    participant: &ParticipantProfile,
    principal: &AuthPrincipal,
) -> p::Provenance {
    match (participant, principal) {
        (ParticipantProfile::Owner, AuthPrincipal::Owner(_)) => p::Provenance {
            source: p::Source::UserTurn,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
        (ParticipantProfile::AgentRepresentingOwner, AuthPrincipal::AgentRepresentingOwner(_)) => {
            p::Provenance {
                source: p::Source::Communication,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            }
        }
        (ParticipantProfile::External(participant), AuthPrincipal::External(_))
        | (ParticipantProfile::ExternalAgent(participant), AuthPrincipal::External(_)) => {
            p::Provenance {
                source: p::Source::Communication,
                actor: p::Actor::External(participant.clone()),
                trust_tier: p::TrustTier::Untrusted,
                caused_by: None,
            }
        }
        _ => p::Provenance {
            source: p::Source::Communication,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::Untrusted,
            caused_by: None,
        },
    }
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
