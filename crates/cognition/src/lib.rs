//! Semantic cognition owner: map, A3 governance, retraction, and validity audits.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_memory as memory;
use forme_memory::CandidateStore;
use forme_protocol as p;
use forme_store::EventStore;

mod temporal;
pub use temporal::*;

mod m3_a;
pub use m3_a::{
    ConservativeStrategyEvolutionGovernor, EvolutionDecision, StrategyEvolutionGovernor,
};

mod m3_c;
pub use m3_c::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MapScope {
    pub schema_version: p::SchemaVersion,
    pub scope: p::Scope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapNode {
    pub schema_version: p::SchemaVersion,
    pub object: p::ObjectRef,
    pub statement: String,
    pub tier: p::StabilityTier,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEdge {
    pub schema_version: p::SchemaVersion,
    pub from: p::ObjectRef,
    pub to: p::ObjectRef,
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgmentFrame {
    pub schema_version: p::SchemaVersion,
    pub reference: p::JudgmentFrameRef,
    pub statement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityModel {
    pub schema_version: p::SchemaVersion,
    pub reference: p::QualityModelRef,
    pub statement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlindSpotModel {
    pub schema_version: p::SchemaVersion,
    pub reference: p::BlindSpotModelRef,
    pub statement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveMapView {
    pub schema_version: p::SchemaVersion,
    pub scope: MapScope,
    pub nodes: Vec<MapNode>,
    pub edges: Vec<MapEdge>,
    pub frames: Vec<JudgmentFrame>,
    pub quality: Vec<QualityModel>,
    pub blindspots: Vec<BlindSpotModel>,
    pub retracted: Vec<p::ObjectRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapConfidence {
    pub schema_version: p::SchemaVersion,
    pub scope: MapScope,
    pub value: p::Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapUpdateKind {
    Frame,
    Quality,
    BlindSpot,
    ResourceRelation,
}

impl MapUpdateKind {
    fn as_ref(self) -> p::MapUpdateKind {
        p::MapUpdateKind(
            match self {
                Self::Frame => "frame",
                Self::Quality => "quality",
                Self::BlindSpot => "blindspot",
                Self::ResourceRelation => "resource-relation",
            }
            .into(),
        )
    }

    fn parse(value: &str) -> Self {
        match value {
            "quality" => Self::Quality,
            "blindspot" => Self::BlindSpot,
            "resource-relation" => Self::ResourceRelation,
            _ => Self::Frame,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CognitiveMapUpdateProposal {
    pub schema_version: p::SchemaVersion,
    pub candidate_id: Option<p::CandidateId>,
    pub scope: MapScope,
    pub kind: MapUpdateKind,
    pub confidence: p::Confidence,
    pub evidence: Vec<p::EvidenceRef>,
    pub frame: Option<p::JudgmentFrameRef>,
    pub quality: Option<p::QualityModelRef>,
    pub blindspot: Option<p::BlindSpotModelRef>,
    pub resource: Option<p::ResourceRef>,
    pub reflection_inputs: Vec<p::EvidenceRef>,
    pub statement: String,
    pub provenance: p::Provenance,
}

pub trait CognitiveMapStore {
    fn read(&self, scope: MapScope) -> CognitiveMapView;
    fn confidence(&self, scope: MapScope) -> MapConfidence;
    fn propose_update(&self, proposal: CognitiveMapUpdateProposal) -> p::Result<p::CandidateId>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GovernanceDecision {
    Promote,
    Confirm,
    Reject,
    Downgrade,
    Rollback,
    Decay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeDirection {
    CautionIncreasing,
    ConfidenceOrAutonomyIncreasing,
    CorrectionDowngrade,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernanceEvidence {
    pub schema_version: p::SchemaVersion,
    pub reference: p::EvidenceRef,
    pub observed_at: p::Timestamp,
    pub verified_process: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PromotionAsymmetry {
    pub caution_confidence: p::Confidence,
    pub caution_evidence: usize,
    pub confidence_autonomy_confidence: p::Confidence,
    pub confidence_autonomy_evidence: usize,
    pub confidence_autonomy_timepoints: usize,
}

impl Default for PromotionAsymmetry {
    fn default() -> Self {
        Self {
            caution_confidence: p::Confidence(0.4),
            caution_evidence: 1,
            confidence_autonomy_confidence: p::Confidence(0.8),
            confidence_autonomy_evidence: 2,
            confidence_autonomy_timepoints: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StableCognitionKind {
    Frame,
    Quality,
    BlindSpot,
    ResourceRelation,
    UserAttribute,
    AgentSelf,
    Trust,
    Partnership,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StableCognition {
    pub schema_version: p::SchemaVersion,
    pub object: p::ObjectRef,
    pub scope: MapScope,
    pub kind: StableCognitionKind,
    pub statement: String,
    pub tier: p::StabilityTier,
    pub confidence: p::Confidence,
    pub evidence: Vec<GovernanceEvidence>,
    pub last_reproduced_at: p::Timestamp,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GovernanceCandidate {
    pub schema_version: p::SchemaVersion,
    pub candidate_id: p::CandidateId,
    pub direction: ChangeDirection,
    pub confidence: p::Confidence,
    pub evidence: Vec<GovernanceEvidence>,
    pub impact: p::Impact,
    pub provenance: p::Provenance,
    pub conflicts: Vec<p::CandidateId>,
    pub owner_confirmed: bool,
    pub target: StableCognition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetractionEvent {
    pub schema_version: p::SchemaVersion,
    pub target_object: p::ObjectRef,
    pub evidence_lineage: p::LineageRef,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReevaluationTask {
    pub schema_version: p::SchemaVersion,
    pub derived: p::ObjectRef,
    pub trigger: p::ReevaluationTriggerRef,
    pub candidate_id: p::CandidateId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub schema_version: p::SchemaVersion,
    pub now: p::Timestamp,
    pub stale_after_ms: i64,
}

pub trait EvolutionGovernor {
    fn intake(&self, candidate: p::CandidateUpdate) -> GovernanceDecision;
    fn on_retraction(&self, event: RetractionEvent) -> Vec<ReevaluationTask>;
    fn decay(&self, tick: Tick) -> Vec<p::CandidateId>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickTrigger {
    PostTurn,
    Idle,
    Schedule,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpulseSource {
    Gap,
    Change,
    Tension,
    Association,
    Pressure,
    Commitment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InterventionLevel {
    L0Observe,
    L1Suggest,
    L2Prepare,
    L3ActWithApproval,
    L4Autonomous,
    L5HighImpact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Internalize,
    ActIndependently,
    Collaborate,
    ExternalProxy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Value(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueDecision {
    Worth(Value),
    NotWorth(p::ReasonRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    Hitchhike,
    Interrupt,
    Digest,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommunicationPurpose {
    AskToLearn,
    Reminder,
    Insight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalIntent {
    Action,
    Communication(CommunicationPurpose),
    Learning,
    Delegation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub schema_version: p::SchemaVersion,
    pub source: p::Source,
    pub scope: p::Scope,
    pub grant_ref: Option<p::GrantRef>,
    pub authorized: bool,
    pub seed: Vec<p::NodeId>,
    pub signal: ImpulseSource,
    pub estimated_value: u64,
    pub urgency: u64,
    pub requested_level: InterventionLevel,
    pub delivery: DeliveryMode,
    pub proposal_intent: ProposalIntent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapConfidenceInput {
    pub schema_version: p::SchemaVersion,
    pub reference: p::MapConfidenceRef,
    pub value: p::Confidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSelfInput {
    pub schema_version: p::SchemaVersion,
    pub reference: p::AgentSelfModelRef,
    pub confidence: p::Confidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityEvidenceInput {
    pub schema_version: p::SchemaVersion,
    pub reference: p::CapabilityEvidenceRef,
    pub verified_success: bool,
    pub reliability: p::Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustInput {
    pub schema_version: p::SchemaVersion,
    pub reference: p::TrustProfileRef,
    pub ceiling: InterventionLevel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureEvidenceInput {
    pub schema_version: p::SchemaVersion,
    pub reference: p::FailureEvidenceRef,
    pub impact: p::Impact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationEvidenceInput {
    pub schema_version: p::SchemaVersion,
    pub reference: p::EvidenceRef,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompetenceInputs {
    pub schema_version: p::SchemaVersion,
    pub map_confidence: Option<MapConfidenceInput>,
    pub self_model: Option<AgentSelfInput>,
    pub capability_evidence: Vec<CapabilityEvidenceInput>,
    pub trust: Option<TrustInput>,
    pub failure: Vec<FailureEvidenceInput>,
    pub verification: Vec<VerificationEvidenceInput>,
}

impl Default for CompetenceInputs {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            map_confidence: None,
            self_model: None,
            capability_evidence: Vec::new(),
            trust: None,
            failure: Vec::new(),
            verification: Vec::new(),
        }
    }
}

impl CompetenceInputs {
    pub fn protocol_reads(&self) -> p::CompetenceInputs {
        p::CompetenceInputs {
            map_confidence: self
                .map_confidence
                .as_ref()
                .map(|input| input.reference.clone()),
            agent_self_model: self
                .self_model
                .as_ref()
                .map(|input| input.reference.clone()),
            capability_evidence: self
                .capability_evidence
                .iter()
                .map(|input| input.reference.clone())
                .collect(),
            trust_profile: self.trust.as_ref().map(|input| input.reference.clone()),
            failure_evidence: self
                .failure
                .iter()
                .map(|input| input.reference.clone())
                .collect(),
            verification_evidence: self
                .verification
                .iter()
                .map(|input| input.reference.clone())
                .collect(),
        }
    }
}

pub trait CompetenceGate {
    fn ceiling(
        &self,
        scope: p::Scope,
        risk: p::Risk,
        context: &CompetenceInputs,
    ) -> InterventionLevel;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompetenceThresholds {
    pub verified: p::Confidence,
    pub strong: p::Confidence,
    pub map_floor: p::Confidence,
    pub self_floor: p::Confidence,
}

impl Default for CompetenceThresholds {
    fn default() -> Self {
        Self {
            verified: p::Confidence(0.7),
            strong: p::Confidence(0.9),
            map_floor: p::Confidence(0.6),
            self_floor: p::Confidence(0.6),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EvidenceCompetenceGate {
    thresholds: CompetenceThresholds,
}

impl EvidenceCompetenceGate {
    pub fn new(thresholds: CompetenceThresholds) -> Self {
        Self { thresholds }
    }
}

impl CompetenceGate for EvidenceCompetenceGate {
    fn ceiling(
        &self,
        _scope: p::Scope,
        risk: p::Risk,
        context: &CompetenceInputs,
    ) -> InterventionLevel {
        let verified = context
            .capability_evidence
            .iter()
            .filter(|item| {
                item.verified_success && item.reliability.0 >= self.thresholds.verified.0
            })
            .count()
            + context
                .verification
                .iter()
                .filter(|item| item.passed)
                .count();
        let strong = context
            .capability_evidence
            .iter()
            .any(|item| item.verified_success && item.reliability.0 >= self.thresholds.strong.0);
        let mut ceiling = match (verified, strong, risk) {
            (0, _, p::Risk::High) => InterventionLevel::L0Observe,
            (0, _, _) => InterventionLevel::L1Suggest,
            (1, _, p::Risk::High) => InterventionLevel::L2Prepare,
            (1, _, _) => InterventionLevel::L3ActWithApproval,
            (_, true, p::Risk::Low) => InterventionLevel::L4Autonomous,
            _ => InterventionLevel::L3ActWithApproval,
        };

        let map_cap = context
            .map_confidence
            .as_ref()
            .map(|input| {
                if input.value.0 >= self.thresholds.map_floor.0 {
                    InterventionLevel::L5HighImpact
                } else if input.value.0 >= 0.3 {
                    InterventionLevel::L1Suggest
                } else {
                    InterventionLevel::L0Observe
                }
            })
            .unwrap_or(InterventionLevel::L1Suggest);
        let self_cap = context
            .self_model
            .as_ref()
            .map(|input| {
                if input.confidence.0 >= self.thresholds.self_floor.0 {
                    InterventionLevel::L5HighImpact
                } else if input.confidence.0 >= 0.3 {
                    InterventionLevel::L1Suggest
                } else {
                    InterventionLevel::L0Observe
                }
            })
            .unwrap_or(InterventionLevel::L1Suggest);
        ceiling = ceiling.min(map_cap).min(self_cap);
        if let Some(trust) = &context.trust {
            ceiling = ceiling.min(trust.ceiling);
        } else {
            ceiling = ceiling.min(InterventionLevel::L1Suggest);
        }
        for failure in &context.failure {
            ceiling = lower_level(
                ceiling,
                if failure.impact == p::Impact::High {
                    2
                } else {
                    1
                },
            );
        }
        ceiling
    }
}

fn lower_level(level: InterventionLevel, steps: usize) -> InterventionLevel {
    let levels = [
        InterventionLevel::L0Observe,
        InterventionLevel::L1Suggest,
        InterventionLevel::L2Prepare,
        InterventionLevel::L3ActWithApproval,
        InterventionLevel::L4Autonomous,
        InterventionLevel::L5HighImpact,
    ];
    let index = levels.iter().position(|item| *item == level).unwrap_or(0);
    levels[index.saturating_sub(steps)]
}

#[derive(Debug, Clone, PartialEq)]
pub struct CognitionSnapshot {
    pub schema_version: p::SchemaVersion,
    pub now: p::Timestamp,
    pub observations: Vec<Observation>,
    pub competence: CompetenceInputs,
}

impl Default for CognitionSnapshot {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            now: current_time_ms(),
            observations: Vec::new(),
            competence: CompetenceInputs::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Impulse {
    pub schema_version: p::SchemaVersion,
    pub source: ImpulseSource,
    pub observation_source: p::Source,
    pub reach: Reach,
    pub seed: Vec<p::NodeId>,
    pub activation_shape: Option<p::ActivationShape>,
    pub scope: p::Scope,
    pub grant_ref: Option<p::GrantRef>,
    pub value: ValueDecision,
    pub urgency: u64,
    pub requested_level: InterventionLevel,
    pub delivery: DeliveryMode,
    pub proposal_intent: ProposalIntent,
    pub intention_id: Option<p::IntentionId>,
    pub capability_evidence: Vec<p::CapabilityEvidenceRef>,
}

impl Impulse {
    pub fn origin_key(&self) -> String {
        if let Some(intention) = &self.intention_id {
            return format!("intention:{}", intention.0);
        }
        format!(
            "{}:{}",
            self.scope.0,
            self.seed
                .iter()
                .map(|seed| seed.0.as_str())
                .collect::<Vec<_>>()
                .join("|")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmissionGuard {
    pub schema_version: p::SchemaVersion,
    pub value: ValueDecision,
    pub competence: InterventionLevel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalCore {
    pub schema_version: p::SchemaVersion,
    pub reference: p::ProposalRef,
    pub origin_key: String,
    pub scope: p::Scope,
    pub level: InterventionLevel,
    pub delivery: DeliveryMode,
    pub reason_summary: p::ReasonRef,
    pub confirmation_required: bool,
    pub intention_id: Option<p::IntentionId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionProposal {
    pub core: ProposalCore,
    pub augmentation_strategy: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunicationProposal {
    pub core: ProposalCore,
    pub purpose: CommunicationPurpose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearningTask {
    pub core: ProposalCore,
    pub target_gap: Option<p::NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationProposal {
    pub core: ProposalCore,
    pub capability_evidence: Vec<p::CapabilityEvidenceRef>,
    pub requested_envelope: p::AutonomyEnvelopeRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposal {
    Action(ActionProposal),
    Communication(CommunicationProposal),
    Learning(LearningTask),
    Delegation(DelegationProposal),
}

impl Proposal {
    pub fn core(&self) -> &ProposalCore {
        match self {
            Self::Action(proposal) => &proposal.core,
            Self::Communication(proposal) => &proposal.core,
            Self::Learning(proposal) => &proposal.core,
            Self::Delegation(proposal) => &proposal.core,
        }
    }

    pub fn reference(&self) -> p::ProposalRef {
        self.core().reference.clone()
    }

    pub fn level(&self) -> InterventionLevel {
        self.core().level
    }

    pub fn scope(&self) -> p::Scope {
        self.core().scope.clone()
    }

    pub fn kind_ref(&self) -> p::ProposalKind {
        p::ProposalKind(
            match self {
                Self::Action(_) => "action",
                Self::Communication(proposal)
                    if proposal.purpose == CommunicationPurpose::AskToLearn =>
                {
                    "ask-to-learn"
                }
                Self::Communication(_) => "communication",
                Self::Learning(_) => "learning",
                Self::Delegation(_) => "delegation",
            }
            .into(),
        )
    }

    pub fn downgraded(mut self, level: InterventionLevel) -> Self {
        let delegation = matches!(self, Self::Delegation(_));
        let core = match &mut self {
            Self::Action(proposal) => &mut proposal.core,
            Self::Communication(proposal) => &mut proposal.core,
            Self::Learning(proposal) => &mut proposal.core,
            Self::Delegation(proposal) => &mut proposal.core,
        };
        core.level = core.level.min(level);
        if core.level <= InterventionLevel::L2Prepare && !delegation {
            core.confirmation_required = false;
        }
        self
    }
}

pub trait ProactivityEngine {
    fn tick(&self, trigger: TickTrigger, snapshot: &CognitionSnapshot) -> Vec<Impulse>;
    fn emit(&self, impulse: Impulse, guard: EmissionGuard) -> Option<Proposal>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeWindow {
    pub schema_version: p::SchemaVersion,
    pub starts_minute_utc: u16,
    pub ends_minute_utc: u16,
}

impl TimeWindow {
    fn contains(&self, timestamp: p::Timestamp) -> bool {
        let minute = timestamp.div_euclid(60_000).rem_euclid(1_440) as u16;
        if self.starts_minute_utc <= self.ends_minute_utc {
            (self.starts_minute_utc..self.ends_minute_utc).contains(&minute)
        } else {
            minute >= self.starts_minute_utc || minute < self.ends_minute_utc
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatePolicy {
    pub schema_version: p::SchemaVersion,
    pub max_interrupts: usize,
    pub window_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionBudget {
    pub schema_version: p::SchemaVersion,
    pub quiet_hours: Vec<TimeWindow>,
    pub interrupt_rate: RatePolicy,
    pub urgent_interrupt_threshold: u64,
}

impl Default for AttentionBudget {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            quiet_hours: Vec::new(),
            interrupt_rate: RatePolicy {
                schema_version: p::SchemaVersion(1),
                max_interrupts: 2,
                window_ms: 3_600_000,
            },
            urgent_interrupt_threshold: 90,
        }
    }
}

#[derive(Debug, Default)]
struct AttentionLedger {
    interruptions: Mutex<VecDeque<p::Timestamp>>,
}

impl AttentionLedger {
    fn delivery(
        &self,
        requested: DeliveryMode,
        now: p::Timestamp,
        urgency: u64,
        budget: &AttentionBudget,
    ) -> DeliveryMode {
        if requested != DeliveryMode::Interrupt {
            return requested;
        }
        if budget.quiet_hours.iter().any(|window| window.contains(now))
            && urgency < budget.urgent_interrupt_threshold
        {
            return DeliveryMode::Hitchhike;
        }
        let Ok(mut interruptions) = self.interruptions.lock() else {
            return DeliveryMode::Hitchhike;
        };
        while interruptions
            .front()
            .is_some_and(|at| at.saturating_add(budget.interrupt_rate.window_ms) <= now)
        {
            interruptions.pop_front();
        }
        if interruptions.len() >= budget.interrupt_rate.max_interrupts {
            DeliveryMode::Digest
        } else {
            interruptions.push_back(now);
            DeliveryMode::Interrupt
        }
    }

    fn used(&self) -> usize {
        self.interruptions
            .lock()
            .map(|items| items.len())
            .unwrap_or(0)
    }
}

pub trait ProspectiveIntentions: Send + Sync {
    fn create(&self, intention: memory::ProspectiveIntention) -> p::Result<p::IntentionId>;
    fn claim_due(&self, now: p::Timestamp, lease_ms: i64) -> Vec<memory::ClaimedIntention>;
    fn resolve(&self, id: p::IntentionId, outcome: memory::IntentionOutcome) -> p::Result<()>;
}

impl<S> ProspectiveIntentions for memory::EventSourcedMemory<S>
where
    S: EventStore + Send + Sync + 'static,
{
    fn create(&self, intention: memory::ProspectiveIntention) -> p::Result<p::IntentionId> {
        memory::IntentionStore::create(self, intention)
    }

    fn claim_due(&self, now: p::Timestamp, lease_ms: i64) -> Vec<memory::ClaimedIntention> {
        memory::IntentionStore::claim_due_unscheduled(self, now, lease_ms)
    }

    fn resolve(&self, id: p::IntentionId, outcome: memory::IntentionOutcome) -> p::Result<()> {
        memory::IntentionStore::resolve(self, id, outcome)
    }
}

pub trait ScheduledIntentions: Send + Sync {
    fn schedule(&self, command: p::ScheduleCommand) -> p::Result<p::IntentionId>;
    fn claim_due(
        &self,
        now: p::Timestamp,
        lease_ms: i64,
        max_claims: usize,
    ) -> p::Result<Vec<p::ScheduleClaim>>;
    fn resolve(&self, id: p::IntentionId, outcome: p::IntentionOutcome) -> p::Result<()>;
    fn list(&self) -> p::Result<Vec<p::ScheduledJob>>;
}

impl<S> ScheduledIntentions for memory::EventSourcedMemory<S>
where
    S: EventStore + Send + Sync + 'static,
{
    fn schedule(&self, command: p::ScheduleCommand) -> p::Result<p::IntentionId> {
        memory::IntentionStore::create_schedule(self, command)
    }

    fn claim_due(
        &self,
        now: p::Timestamp,
        lease_ms: i64,
        max_claims: usize,
    ) -> p::Result<Vec<p::ScheduleClaim>> {
        Ok(
            memory::IntentionStore::claim_due_scheduled(self, now, lease_ms, max_claims)
                .into_iter()
                .filter_map(|claim| claim.scheduled_claim())
                .collect(),
        )
    }

    fn resolve(&self, id: p::IntentionId, outcome: p::IntentionOutcome) -> p::Result<()> {
        memory::IntentionStore::resolve(
            self,
            id,
            match outcome {
                p::IntentionOutcome::Fired => memory::IntentionOutcome::Fired,
                p::IntentionOutcome::Done => memory::IntentionOutcome::Done,
                p::IntentionOutcome::Expired => memory::IntentionOutcome::Expired,
                p::IntentionOutcome::Cancelled => memory::IntentionOutcome::Cancelled,
            },
        )
    }

    fn list(&self) -> p::Result<Vec<p::ScheduledJob>> {
        Ok(memory::IntentionStore::list_scheduled(self)
            .into_iter()
            .map(|record| {
                let binding = record.command.binding();
                p::ScheduledJob {
                    schema_version: record.schema_version,
                    intention: record.command.intention,
                    binding,
                    lease_until: record.lease_until,
                    run: None,
                    run_status: None,
                    manual_review: false,
                }
            })
            .collect())
    }
}

pub struct IntentionServices {
    pub proactive: Arc<dyn ProspectiveIntentions>,
    pub scheduled: Arc<dyn ScheduledIntentions>,
}

pub fn event_sourced_intention_services<S>(
    store: Arc<S>,
    aggregate_run: p::RunId,
) -> p::Result<IntentionServices>
where
    S: EventStore + Send + Sync + 'static,
{
    let intentions = Arc::new(memory::EventSourcedMemory::open(store, aggregate_run)?);
    let proactive: Arc<dyn ProspectiveIntentions> = intentions.clone();
    let scheduled: Arc<dyn ScheduledIntentions> = intentions;
    Ok(IntentionServices {
        proactive,
        scheduled,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProactivityConfig {
    pub schema_version: p::SchemaVersion,
    pub minimum_value: u64,
    pub workspace_capacity: usize,
    pub intention_lease_ms: i64,
    pub attention: AttentionBudget,
}

impl Default for ProactivityConfig {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            minimum_value: 40,
            workspace_capacity: 10,
            intention_lease_ms: 60_000,
            attention: AttentionBudget::default(),
        }
    }
}

pub struct M0ProactivityEngine {
    config: ProactivityConfig,
    intentions: Option<Arc<dyn ProspectiveIntentions>>,
    attention: AttentionLedger,
    proposal_sequence: AtomicU64,
    suppressed: Mutex<BTreeSet<String>>,
}

impl Default for M0ProactivityEngine {
    fn default() -> Self {
        Self::new(ProactivityConfig::default())
    }
}

impl M0ProactivityEngine {
    pub fn new(config: ProactivityConfig) -> Self {
        Self {
            config,
            intentions: None,
            attention: AttentionLedger::default(),
            proposal_sequence: AtomicU64::new(1),
            suppressed: Mutex::new(BTreeSet::new()),
        }
    }

    pub fn with_intention_store(mut self, store: Arc<dyn ProspectiveIntentions>) -> Self {
        self.intentions = Some(store);
        self
    }

    pub fn backed_by<S>(
        config: ProactivityConfig,
        store: Arc<S>,
        aggregate_run: p::RunId,
    ) -> p::Result<Self>
    where
        S: EventStore + Send + Sync + 'static,
    {
        let intentions = Arc::new(memory::EventSourcedMemory::open(store, aggregate_run)?);
        Ok(Self::new(config).with_intention_store(intentions))
    }

    pub fn resolve(
        &self,
        proposal: &Proposal,
        outcome: p::ProposalOutcome,
        defer_until: Option<p::Timestamp>,
    ) -> p::Result<Option<p::IntentionId>> {
        match outcome {
            p::ProposalOutcome::Reject => {
                self.suppressed
                    .lock()
                    .map_err(|_| p::Error("proactivity suppression state is unavailable".into()))?
                    .insert(proposal.core().origin_key.clone());
                Ok(None)
            }
            p::ProposalOutcome::Adopt => {
                if let (Some(store), Some(intention)) =
                    (&self.intentions, &proposal.core().intention_id)
                {
                    store.resolve(intention.clone(), memory::IntentionOutcome::Done)?;
                }
                Ok(None)
            }
            p::ProposalOutcome::Defer => {
                let store = self.intentions.as_ref().ok_or_else(|| {
                    p::Error("deferred proposal requires an IntentionStore".into())
                })?;
                let at = defer_until.ok_or_else(|| {
                    p::Error("deferred proposal requires a future trigger".into())
                })?;
                let id = p::IntentionId(format!("deferred:{}", proposal.reference().0));
                store.create(memory::ProspectiveIntention {
                    schema_version: p::SchemaVersion(1),
                    id: id.clone(),
                    source: p::IntentionSource::DeferredProposal,
                    trigger: memory::IntentionTrigger::At(at),
                    state: memory::IntentionState::Pending,
                    seed: p::SeedRef(proposal.core().origin_key.clone()),
                    provenance: internal_cognition_provenance(),
                    expires_at: None,
                })?;
                Ok(Some(id))
            }
        }
    }

    pub fn interruptions_used(&self) -> usize {
        self.attention.used()
    }
}

impl ProactivityEngine for M0ProactivityEngine {
    fn tick(&self, trigger: TickTrigger, snapshot: &CognitionSnapshot) -> Vec<Impulse> {
        if snapshot.schema_version.0 == 0 {
            return Vec::new();
        }
        let mut impulses = Vec::new();
        if matches!(trigger, TickTrigger::Schedule | TickTrigger::Diff) {
            if let Some(intentions) = &self.intentions {
                for claimed in intentions.claim_due(snapshot.now, self.config.intention_lease_ms) {
                    if intentions
                        .resolve(claimed.intention.id.clone(), memory::IntentionOutcome::Done)
                        .is_err()
                    {
                        continue;
                    }
                    impulses.push(Impulse {
                        schema_version: p::SchemaVersion(1),
                        source: ImpulseSource::Commitment,
                        observation_source: p::Source::Schedule,
                        reach: Reach::Collaborate,
                        seed: vec![p::NodeId(claimed.intention.seed.0.clone())],
                        activation_shape: None,
                        scope: p::Scope("owner".into()),
                        grant_ref: None,
                        value: ValueDecision::Worth(Value(u64::MAX)),
                        urgency: 100,
                        requested_level: InterventionLevel::L1Suggest,
                        delivery: DeliveryMode::Hitchhike,
                        proposal_intent: ProposalIntent::Communication(
                            CommunicationPurpose::Reminder,
                        ),
                        intention_id: Some(claimed.intention.id),
                        capability_evidence: Vec::new(),
                    });
                }
            }
        }
        for observation in snapshot
            .observations
            .iter()
            .filter(|observation| observation.authorized)
        {
            if observation.signal == ImpulseSource::Commitment {
                continue;
            }
            let shape = activation_shape(observation.signal);
            let value = if observation.estimated_value >= self.config.minimum_value {
                ValueDecision::Worth(Value(observation.estimated_value))
            } else {
                ValueDecision::NotWorth(p::ReasonRef(
                    "estimated value does not justify attention".into(),
                ))
            };
            let delivery = self.attention.delivery(
                observation.delivery,
                snapshot.now,
                observation.urgency,
                &self.config.attention,
            );
            let impulse = Impulse {
                schema_version: p::SchemaVersion(1),
                source: observation.signal,
                observation_source: observation.source,
                reach: if observation.signal == ImpulseSource::Gap {
                    Reach::Collaborate
                } else {
                    Reach::Internalize
                },
                seed: observation.seed.clone(),
                activation_shape: shape,
                scope: observation.scope.clone(),
                grant_ref: observation.grant_ref.clone(),
                value,
                urgency: observation.urgency,
                requested_level: observation.requested_level,
                delivery,
                proposal_intent: observation.proposal_intent,
                intention_id: None,
                capability_evidence: snapshot
                    .competence
                    .capability_evidence
                    .iter()
                    .map(|evidence| evidence.reference.clone())
                    .collect(),
            };
            let suppressed = self
                .suppressed
                .lock()
                .map(|items| items.contains(&impulse.origin_key()))
                .unwrap_or(true);
            if !suppressed {
                impulses.push(impulse);
            }
        }
        impulses.sort_by(|left, right| {
            impulse_score(right)
                .cmp(&impulse_score(left))
                .then_with(|| left.origin_key().cmp(&right.origin_key()))
        });
        impulses.truncate(self.config.workspace_capacity);
        impulses
    }

    fn emit(&self, impulse: Impulse, guard: EmissionGuard) -> Option<Proposal> {
        if !matches!(guard.value, ValueDecision::Worth(_)) {
            return None;
        }
        let level = impulse.requested_level.min(guard.competence);
        let sequence = self.proposal_sequence.fetch_add(1, Ordering::SeqCst);
        let core = ProposalCore {
            schema_version: p::SchemaVersion(1),
            reference: p::ProposalRef(format!("proposal:{sequence}")),
            origin_key: impulse.origin_key(),
            scope: impulse.scope.clone(),
            level,
            delivery: impulse.delivery,
            reason_summary: p::ReasonRef(format!(
                "{:?} opportunity passed the value gate",
                impulse.source
            )),
            confirmation_required: level >= InterventionLevel::L3ActWithApproval
                || impulse.proposal_intent == ProposalIntent::Delegation,
            intention_id: impulse.intention_id,
        };
        match impulse.proposal_intent {
            ProposalIntent::Action => Some(Proposal::Action(ActionProposal {
                core,
                augmentation_strategy: "prepare a governed action proposal".into(),
            })),
            ProposalIntent::Communication(purpose) => {
                Some(Proposal::Communication(CommunicationProposal {
                    core,
                    purpose,
                }))
            }
            ProposalIntent::Learning => Some(Proposal::Learning(LearningTask {
                target_gap: impulse.seed.first().cloned(),
                core,
            })),
            ProposalIntent::Delegation => Some(Proposal::Delegation(DelegationProposal {
                requested_envelope: p::AutonomyEnvelopeRef(format!(
                    "delegation-request:{}",
                    core.reference.0
                )),
                capability_evidence: impulse.capability_evidence,
                core,
            })),
        }
    }
}

fn activation_shape(source: ImpulseSource) -> Option<p::ActivationShape> {
    match source {
        ImpulseSource::Gap => Some(p::ActivationShape::Gap),
        ImpulseSource::Change => Some(p::ActivationShape::Change),
        ImpulseSource::Tension => Some(p::ActivationShape::Tension),
        ImpulseSource::Association => Some(p::ActivationShape::Association),
        ImpulseSource::Pressure => Some(p::ActivationShape::Pressure),
        ImpulseSource::Commitment => None,
    }
}

fn impulse_score(impulse: &Impulse) -> u128 {
    let value = match impulse.value {
        ValueDecision::Worth(Value(value)) => value,
        ValueDecision::NotWorth(_) => 0,
    };
    u128::from(value).saturating_mul(u128::from(impulse.urgency))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspaceItemKind {
    Goal,
    Run,
    Impulse,
    Intention,
    Candidate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceItem {
    pub schema_version: p::SchemaVersion,
    pub id: String,
    pub kind: AgentWorkspaceItemKind,
    pub value: u64,
    pub urgency: u64,
    pub event_ref: p::EventId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceProjection {
    pub schema_version: p::SchemaVersion,
    pub snapshot_ref: p::AgentWorkspaceSnapshotRef,
    pub capacity: usize,
    pub items: Vec<AgentWorkspaceItem>,
}

impl AgentWorkspaceProjection {
    pub fn rebuild(events: &[p::Event], capacity: usize) -> Self {
        let mut items = BTreeMap::<String, AgentWorkspaceItem>::new();
        for event in events {
            match &event.payload {
                p::EventPayload::RunAccepted(_) => {
                    items.insert(
                        format!("run:{}", event.run_id.0),
                        workspace_item(
                            event,
                            format!("run:{}", event.run_id.0),
                            AgentWorkspaceItemKind::Run,
                            100,
                            100,
                        ),
                    );
                }
                p::EventPayload::RunComplete(_)
                | p::EventPayload::RunAborted(_)
                | p::EventPayload::RunFailed(_)
                | p::EventPayload::RunLimited(_) => {
                    items.remove(&format!("run:{}", event.run_id.0));
                }
                p::EventPayload::GoalFramed(payload) => {
                    items.insert(
                        format!("goal:{}", payload.goal_frame.0),
                        workspace_item(
                            event,
                            format!("goal:{}", payload.goal_frame.0),
                            AgentWorkspaceItemKind::Goal,
                            95,
                            90,
                        ),
                    );
                }
                p::EventPayload::ImpulseRaised(_) => {
                    items.insert(
                        format!("impulse:{}", event.event_id.0),
                        workspace_item(
                            event,
                            format!("impulse:{}", event.event_id.0),
                            AgentWorkspaceItemKind::Impulse,
                            80,
                            75,
                        ),
                    );
                }
                p::EventPayload::ProspectiveIntentionCreated(payload) => {
                    items.insert(
                        format!("intention:{}", payload.intention_id.0),
                        workspace_item(
                            event,
                            format!("intention:{}", payload.intention_id.0),
                            AgentWorkspaceItemKind::Intention,
                            90,
                            85,
                        ),
                    );
                }
                p::EventPayload::ProspectiveIntentionResolved(payload) => {
                    if payload.outcome != p::IntentionOutcome::Fired {
                        items.remove(&format!("intention:{}", payload.intention_id.0));
                    }
                }
                p::EventPayload::CandidateCreated(payload) => {
                    items.insert(
                        format!("candidate:{}", payload.candidate_id.0),
                        workspace_item(
                            event,
                            format!("candidate:{}", payload.candidate_id.0),
                            AgentWorkspaceItemKind::Candidate,
                            60,
                            50,
                        ),
                    );
                }
                p::EventPayload::CandidatePromoted(payload) => {
                    items.remove(&format!("candidate:{}", payload.candidate_id.0));
                }
                p::EventPayload::CandidateRejected(payload) => {
                    items.remove(&format!("candidate:{}", payload.candidate_id.0));
                }
                p::EventPayload::CandidateDowngraded(payload) => {
                    items.remove(&format!("candidate:{}", payload.candidate_id.0));
                }
                _ => {}
            }
        }
        let mut items = items.into_values().collect::<Vec<_>>();
        items.sort_by(|left, right| {
            right
                .value
                .saturating_mul(right.urgency)
                .cmp(&left.value.saturating_mul(left.urgency))
                .then_with(|| left.id.cmp(&right.id))
        });
        items.truncate(capacity);
        let snapshot_ref = events
            .last()
            .map(|event| {
                p::AgentWorkspaceSnapshotRef(format!(
                    "agent-workspace:{}:{}",
                    event.run_id.0, event.stream_seq
                ))
            })
            .unwrap_or_else(|| p::AgentWorkspaceSnapshotRef("agent-workspace:empty".into()));
        Self {
            schema_version: p::SchemaVersion(1),
            snapshot_ref,
            capacity,
            items,
        }
    }
}

fn workspace_item(
    event: &p::Event,
    id: String,
    kind: AgentWorkspaceItemKind,
    value: u64,
    urgency: u64,
) -> AgentWorkspaceItem {
    AgentWorkspaceItem {
        schema_version: p::SchemaVersion(1),
        id,
        kind,
        value,
        urgency,
        event_ref: event.event_id.clone(),
    }
}

fn internal_cognition_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn current_time_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[derive(Debug, Default)]
struct CognitionState {
    proposals: BTreeMap<p::CandidateId, CognitiveMapUpdateProposal>,
    stable: BTreeMap<p::ObjectRef, StableCognition>,
    retracted: BTreeSet<p::ObjectRef>,
    invalidated: BTreeSet<p::ObjectRef>,
    lineage: BTreeMap<p::ObjectRef, BTreeSet<p::ObjectRef>>,
}

type Clock = Arc<dyn Fn() -> p::Timestamp + Send + Sync>;

pub struct CognitiveRuntime<S: EventStore> {
    store: Arc<S>,
    memory: Arc<memory::EventSourcedMemory<S>>,
    aggregate_run: p::RunId,
    state: Mutex<CognitionState>,
    event_sequence: AtomicU64,
    thresholds: PromotionAsymmetry,
    clock: Clock,
}

impl<S> CognitiveRuntime<S>
where
    S: EventStore,
{
    pub fn open(
        store: Arc<S>,
        memory: Arc<memory::EventSourcedMemory<S>>,
        aggregate_run: p::RunId,
    ) -> p::Result<Self> {
        Self::with_clock(
            store,
            memory,
            aggregate_run,
            PromotionAsymmetry::default(),
            system_timestamp,
        )
    }

    pub fn with_clock<F>(
        store: Arc<S>,
        memory: Arc<memory::EventSourcedMemory<S>>,
        aggregate_run: p::RunId,
        thresholds: PromotionAsymmetry,
        clock: F,
    ) -> p::Result<Self>
    where
        F: Fn() -> p::Timestamp + Send + Sync + 'static,
    {
        if memory.aggregate_run() != &aggregate_run {
            return Err(p::Error(
                "cognition and memory must share one aggregate event stream".into(),
            ));
        }
        validate_thresholds(thresholds)?;
        let events = store
            .read_run(aggregate_run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let mut state = CognitionState::default();
        for event in &events {
            fold_event(&mut state, event);
        }
        let next = events
            .iter()
            .map(|event| event.stream_seq)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Ok(Self {
            store,
            memory,
            aggregate_run,
            state: Mutex::new(state),
            event_sequence: AtomicU64::new(next),
            thresholds,
            clock: Arc::new(clock),
        })
    }

    pub fn govern(&self, candidate: &GovernanceCandidate) -> GovernanceDecision {
        if candidate.schema_version.0 == 0
            || candidate.target.schema_version.0 == 0
            || candidate.candidate_id.0.trim().is_empty()
            || candidate.target.object.0.trim().is_empty()
            || candidate.target.statement.trim().is_empty()
            || !candidate.confidence.0.is_finite()
        {
            return GovernanceDecision::Reject;
        }
        if candidate.direction == ChangeDirection::CorrectionDowngrade {
            return if candidate.provenance.actor == p::Actor::Owner {
                GovernanceDecision::Downgrade
            } else {
                GovernanceDecision::Confirm
            };
        }
        if candidate.provenance.trust_tier == p::TrustTier::Untrusted
            || !candidate.conflicts.is_empty()
            || matches!(
                candidate.target.tier,
                p::StabilityTier::Fixed | p::StabilityTier::Constitutional
            )
            || candidate.impact == p::Impact::High && !candidate.owner_confirmed
        {
            return GovernanceDecision::Confirm;
        }
        let verified = candidate
            .evidence
            .iter()
            .filter(|evidence| evidence.verified_process)
            .collect::<Vec<_>>();
        let timepoints = verified
            .iter()
            .map(|evidence| evidence.observed_at)
            .collect::<BTreeSet<_>>()
            .len();
        let meets_threshold = match candidate.direction {
            ChangeDirection::CautionIncreasing => {
                candidate.confidence.0 >= self.thresholds.caution_confidence.0
                    && verified.len() >= self.thresholds.caution_evidence
            }
            ChangeDirection::ConfidenceOrAutonomyIncreasing => {
                candidate.confidence.0 >= self.thresholds.confidence_autonomy_confidence.0
                    && verified.len() >= self.thresholds.confidence_autonomy_evidence
                    && timepoints >= self.thresholds.confidence_autonomy_timepoints
            }
            ChangeDirection::CorrectionDowngrade => false,
        };
        if meets_threshold && candidate.owner_confirmed {
            GovernanceDecision::Promote
        } else {
            GovernanceDecision::Confirm
        }
    }

    pub fn govern_and_apply(
        &self,
        candidate: GovernanceCandidate,
    ) -> p::Result<GovernanceDecision> {
        let decision = self.govern(&candidate);
        match decision {
            GovernanceDecision::Promote => {
                self.memory.transition(
                    candidate.candidate_id.clone(),
                    memory::CandidateState::Promoted,
                    p::Actor::Owner,
                )?;
                let mut target = candidate.target;
                target.active = true;
                self.lock_state()?
                    .stable
                    .insert(target.object.clone(), target);
            }
            GovernanceDecision::Reject => {
                self.memory.transition(
                    candidate.candidate_id,
                    memory::CandidateState::Rejected,
                    p::Actor::System,
                )?;
            }
            GovernanceDecision::Downgrade => {
                self.memory.transition(
                    candidate.candidate_id,
                    memory::CandidateState::Downgraded,
                    p::Actor::Owner,
                )?;
                if let Some(stable) = self.lock_state()?.stable.get_mut(&candidate.target.object) {
                    stable.active = false;
                }
            }
            GovernanceDecision::Confirm
            | GovernanceDecision::Rollback
            | GovernanceDecision::Decay => {}
        }
        Ok(decision)
    }

    pub fn record_lineage(&self, source: p::ObjectRef, derived: p::ObjectRef) -> p::Result<()> {
        if source.0.trim().is_empty() || derived.0.trim().is_empty() || source == derived {
            return Err(p::Error("cognition lineage edge is invalid".into()));
        }
        self.append_payload(
            system_provenance(),
            p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
                deltas_ref: p::MemoryDeltasRef(format!(
                    "cognition-lineage|{}|{}",
                    escape_component(&source.0),
                    escape_component(&derived.0)
                )),
            }),
        )?;
        self.lock_state()?
            .lineage
            .entry(source)
            .or_default()
            .insert(derived);
        Ok(())
    }

    pub fn is_object_active(&self, object: &p::ObjectRef) -> bool {
        self.lock_state()
            .map(|state| {
                !state.retracted.contains(object)
                    && state
                        .stable
                        .get(object)
                        .map(|stable| stable.active)
                        .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    pub fn is_object_usable(&self, object: &p::ObjectRef) -> bool {
        self.lock_state()
            .map(|state| {
                !state.retracted.contains(object)
                    && !state.invalidated.contains(object)
                    && state
                        .stable
                        .get(object)
                        .map(|stable| stable.active)
                        .unwrap_or(true)
            })
            .unwrap_or(false)
    }

    pub fn stable_objects(&self) -> Vec<StableCognition> {
        self.lock_state()
            .map(|state| state.stable.values().cloned().collect())
            .unwrap_or_default()
    }

    fn append_payload(
        &self,
        provenance: p::Provenance,
        payload: p::EventPayload,
    ) -> p::Result<p::EventId> {
        let sequence = self.event_sequence.fetch_add(1, Ordering::SeqCst);
        self.store.append(p::Event::new(
            p::EventId(format!(
                "cognition-event:{}:{sequence}",
                self.aggregate_run.0
            )),
            self.aggregate_run.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            (self.clock)(),
            provenance,
        ))
    }

    fn lock_state(&self) -> p::Result<MutexGuard<'_, CognitionState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("cognition state is unavailable".into()))
    }

    fn transitive_derived(state: &CognitionState, source: &p::ObjectRef) -> Vec<p::ObjectRef> {
        let mut queue = VecDeque::from([source.clone()]);
        let mut visited = BTreeSet::from([source.clone()]);
        let mut derived = Vec::new();
        while let Some(current) = queue.pop_front() {
            if let Some(children) = state.lineage.get(&current) {
                for child in children {
                    if visited.insert(child.clone()) {
                        derived.push(child.clone());
                        queue.push_back(child.clone());
                    }
                }
            }
        }
        derived
    }

    fn audit_stability(&self, tick: Tick) -> Vec<p::CandidateId> {
        if tick.schema_version.0 == 0 || tick.stale_after_ms <= 0 {
            return Vec::new();
        }
        let stale = match self.lock_state() {
            Ok(state) => state
                .stable
                .values()
                .filter(|stable| {
                    stable.active
                        && stable
                            .last_reproduced_at
                            .saturating_add(tick.stale_after_ms)
                            <= tick.now
                })
                .cloned()
                .collect::<Vec<_>>(),
            Err(_) => return Vec::new(),
        };
        let mut candidates = Vec::new();
        for stable in stale {
            let failure_ref = p::FailureEvidenceRef(format!(
                "memory-misevolution:{}:{}",
                stable.object.0, tick.now
            ));
            if self
                .append_payload(
                    system_provenance(),
                    p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
                        failure_ref: failure_ref.clone(),
                        class: p::FailureClass::MemoryMisevolution,
                        impact: p::Impact::Medium,
                        scope: stable.scope.scope.clone(),
                        related_refs: stable
                            .evidence
                            .iter()
                            .map(|evidence| evidence.reference.clone())
                            .collect(),
                        suggested_fix: Some(p::SuggestedFixRef(
                            "re-evaluate stale stable cognition".into(),
                        )),
                    }),
                )
                .is_err()
            {
                continue;
            }
            if self
                .append_payload(
                    system_provenance(),
                    p::EventPayload::ReevaluationTaskCreated(p::ReevaluationTaskCreatedPayload {
                        derived_refs: vec![stable.object.clone()],
                        trigger: p::ReevaluationTriggerRef(format!("stale:{}", stable.object.0)),
                    }),
                )
                .is_err()
            {
                continue;
            }
            let candidate_id = self.memory.reserve_candidate_id();
            if self
                .memory
                .create_candidate_spec(memory::CandidateSpec {
                    schema_version: p::SchemaVersion(1),
                    id: candidate_id.clone(),
                    target: p::CandidateTargetRef(format!("downgrade:{}", stable.object.0)),
                    evidence_refs: vec![p::EvidenceRef(failure_ref.0)],
                    confidence: p::Confidence(1.0),
                    provenance: system_provenance(),
                    target_tier: p::StabilityTier::Working,
                })
                .is_ok()
            {
                if let Ok(mut state) = self.lock_state() {
                    if let Some(record) = state.stable.get_mut(&stable.object) {
                        record.active = false;
                    }
                    state.invalidated.insert(stable.object.clone());
                }
                candidates.push(candidate_id);
            }
        }
        candidates
    }
}

impl<S> CognitiveMapStore for CognitiveRuntime<S>
where
    S: EventStore,
{
    fn read(&self, scope: MapScope) -> CognitiveMapView {
        let Ok(state) = self.lock_state() else {
            return empty_map(scope);
        };
        let stable = state
            .stable
            .values()
            .filter(|stable| stable.scope == scope && stable.active && is_map_kind(stable.kind))
            .cloned()
            .collect::<Vec<_>>();
        CognitiveMapView {
            schema_version: p::SchemaVersion(1),
            scope,
            nodes: stable
                .iter()
                .map(|stable| MapNode {
                    schema_version: stable.schema_version,
                    object: stable.object.clone(),
                    statement: stable.statement.clone(),
                    tier: stable.tier,
                    active: stable.active,
                })
                .collect(),
            edges: Vec::new(),
            frames: stable
                .iter()
                .filter(|stable| stable.kind == StableCognitionKind::Frame)
                .map(|stable| JudgmentFrame {
                    schema_version: stable.schema_version,
                    reference: p::JudgmentFrameRef(stable.object.0.clone()),
                    statement: stable.statement.clone(),
                })
                .collect(),
            quality: stable
                .iter()
                .filter(|stable| stable.kind == StableCognitionKind::Quality)
                .map(|stable| QualityModel {
                    schema_version: stable.schema_version,
                    reference: p::QualityModelRef(stable.object.0.clone()),
                    statement: stable.statement.clone(),
                })
                .collect(),
            blindspots: stable
                .iter()
                .filter(|stable| stable.kind == StableCognitionKind::BlindSpot)
                .map(|stable| BlindSpotModel {
                    schema_version: stable.schema_version,
                    reference: p::BlindSpotModelRef(stable.object.0.clone()),
                    statement: stable.statement.clone(),
                })
                .collect(),
            retracted: state.retracted.iter().cloned().collect(),
        }
    }

    fn confidence(&self, scope: MapScope) -> MapConfidence {
        let values = self
            .lock_state()
            .map(|state| {
                state
                    .stable
                    .values()
                    .filter(|stable| {
                        stable.scope == scope && stable.active && is_map_kind(stable.kind)
                    })
                    .map(|stable| stable.confidence.0)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let value = if values.is_empty() {
            0.0
        } else {
            values.iter().sum::<f32>() / values.len() as f32
        };
        MapConfidence {
            schema_version: p::SchemaVersion(1),
            scope,
            value: p::Confidence(value),
        }
    }

    fn propose_update(
        &self,
        mut proposal: CognitiveMapUpdateProposal,
    ) -> p::Result<p::CandidateId> {
        validate_proposal(&proposal)?;
        let candidate_id = proposal
            .candidate_id
            .clone()
            .unwrap_or_else(|| self.memory.reserve_candidate_id());
        proposal.candidate_id = Some(candidate_id.clone());
        if self.lock_state()?.proposals.contains_key(&candidate_id) {
            return Ok(candidate_id);
        }
        self.append_payload(
            proposal.provenance.clone(),
            p::EventPayload::ReflectionProduced(p::ReflectionProducedPayload {
                inputs: proposal.reflection_inputs.clone(),
                candidate_refs: vec![candidate_id.clone()],
            }),
        )?;
        self.append_payload(
            proposal.provenance.clone(),
            p::EventPayload::CognitiveMapUpdateProposed(p::CognitiveMapUpdateProposedPayload {
                candidate_id: candidate_id.clone(),
                kind: proposal.kind.as_ref(),
                evidence: proposal.evidence.clone(),
                frame: proposal.frame.clone(),
                quality: proposal.quality.clone(),
                blindspot: proposal.blindspot.clone(),
                resource: proposal.resource.clone(),
                confidence: proposal.confidence,
            }),
        )?;
        self.memory.create_candidate_spec(memory::CandidateSpec {
            schema_version: proposal.schema_version,
            id: candidate_id.clone(),
            target: p::CandidateTargetRef(format!(
                "cognitive-map|{}|{}|{}",
                escape_component(&proposal.scope.scope.0),
                proposal.kind.as_ref().0,
                escape_component(&proposal.statement)
            )),
            evidence_refs: proposal.evidence.clone(),
            confidence: proposal.confidence,
            provenance: proposal.provenance.clone(),
            target_tier: p::StabilityTier::Stable,
        })?;
        self.lock_state()?
            .proposals
            .insert(candidate_id.clone(), proposal);
        Ok(candidate_id)
    }
}

impl<S> EvolutionGovernor for CognitiveRuntime<S>
where
    S: EventStore,
{
    fn intake(&self, candidate: p::CandidateUpdate) -> GovernanceDecision {
        if candidate.schema_version.0 == 0 {
            GovernanceDecision::Reject
        } else {
            // The frozen envelope carries no evidence gates, so the safe result is confirmation.
            GovernanceDecision::Confirm
        }
    }

    fn on_retraction(&self, event: RetractionEvent) -> Vec<ReevaluationTask> {
        if event.schema_version.0 == 0 || event.target_object.0.trim().is_empty() {
            return Vec::new();
        }
        if self
            .append_payload(
                event.provenance.clone(),
                p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                    target_object: event.target_object.clone(),
                    evidence_lineage: event.evidence_lineage.clone(),
                }),
            )
            .is_err()
        {
            return Vec::new();
        }
        let derived = match self.lock_state() {
            Ok(mut state) => {
                state.retracted.insert(event.target_object.clone());
                if let Some(target) = state.stable.get_mut(&event.target_object) {
                    target.active = false;
                }
                let derived = Self::transitive_derived(&state, &event.target_object);
                for object in &derived {
                    state.invalidated.insert(object.clone());
                    if let Some(stable) = state.stable.get_mut(object) {
                        stable.active = false;
                    }
                }
                derived
            }
            Err(_) => return Vec::new(),
        };
        let trigger = p::ReevaluationTriggerRef(format!("retraction:{}", event.target_object.0));
        if self
            .append_payload(
                event.provenance.clone(),
                p::EventPayload::ReevaluationTaskCreated(p::ReevaluationTaskCreatedPayload {
                    derived_refs: derived.clone(),
                    trigger: trigger.clone(),
                }),
            )
            .is_err()
        {
            return Vec::new();
        }
        let mut tasks = Vec::new();
        for object in derived {
            let candidate_id = self.memory.reserve_candidate_id();
            if self
                .memory
                .create_candidate_spec(memory::CandidateSpec {
                    schema_version: p::SchemaVersion(1),
                    id: candidate_id.clone(),
                    target: p::CandidateTargetRef(format!("downgrade:{}", object.0)),
                    evidence_refs: vec![p::EvidenceRef(event.evidence_lineage.0.clone())],
                    confidence: p::Confidence(1.0),
                    provenance: event.provenance.clone(),
                    target_tier: p::StabilityTier::Working,
                })
                .is_ok()
            {
                tasks.push(ReevaluationTask {
                    schema_version: p::SchemaVersion(1),
                    derived: object,
                    trigger: trigger.clone(),
                    candidate_id,
                });
            }
        }
        tasks
    }

    fn decay(&self, tick: Tick) -> Vec<p::CandidateId> {
        self.audit_stability(tick)
    }
}

fn validate_thresholds(thresholds: PromotionAsymmetry) -> p::Result<()> {
    if !thresholds.caution_confidence.0.is_finite()
        || !thresholds.confidence_autonomy_confidence.0.is_finite()
        || thresholds.caution_confidence.0 < 0.0
        || thresholds.confidence_autonomy_confidence.0 > 1.0
        || thresholds.caution_confidence.0 >= thresholds.confidence_autonomy_confidence.0
        || thresholds.caution_evidence == 0
        || thresholds.confidence_autonomy_evidence <= thresholds.caution_evidence
        || thresholds.confidence_autonomy_timepoints == 0
    {
        return Err(p::Error(
            "promotion asymmetry thresholds are invalid".into(),
        ));
    }
    Ok(())
}

fn validate_proposal(proposal: &CognitiveMapUpdateProposal) -> p::Result<()> {
    if proposal.schema_version.0 == 0
        || proposal.scope.schema_version.0 == 0
        || proposal.scope.scope.0.trim().is_empty()
        || proposal.statement.trim().is_empty()
        || proposal.evidence.is_empty()
        || !proposal.confidence.0.is_finite()
        || proposal.confidence.0 < 0.0
        || proposal.confidence.0 > 0.5
    {
        return Err(p::Error(
            "cognitive-map proposal must be versioned, evidenced, and low-confidence".into(),
        ));
    }
    let has_payload = match proposal.kind {
        MapUpdateKind::Frame => proposal.frame.is_some(),
        MapUpdateKind::Quality => proposal.quality.is_some(),
        MapUpdateKind::BlindSpot => proposal.blindspot.is_some(),
        MapUpdateKind::ResourceRelation => proposal.resource.is_some(),
    };
    if !has_payload {
        return Err(p::Error(
            "cognitive-map proposal does not match its update kind".into(),
        ));
    }
    Ok(())
}

fn empty_map(scope: MapScope) -> CognitiveMapView {
    CognitiveMapView {
        schema_version: p::SchemaVersion(1),
        scope,
        nodes: Vec::new(),
        edges: Vec::new(),
        frames: Vec::new(),
        quality: Vec::new(),
        blindspots: Vec::new(),
        retracted: Vec::new(),
    }
}

fn is_map_kind(kind: StableCognitionKind) -> bool {
    matches!(
        kind,
        StableCognitionKind::Frame
            | StableCognitionKind::Quality
            | StableCognitionKind::BlindSpot
            | StableCognitionKind::ResourceRelation
    )
}

fn fold_event(state: &mut CognitionState, event: &p::Event) {
    match &event.payload {
        p::EventPayload::CognitiveMapUpdateProposed(payload) => {
            state.proposals.insert(
                payload.candidate_id.clone(),
                CognitiveMapUpdateProposal {
                    schema_version: event.schema_version,
                    candidate_id: Some(payload.candidate_id.clone()),
                    scope: MapScope {
                        schema_version: p::SchemaVersion(1),
                        scope: p::Scope("recovered".into()),
                    },
                    kind: MapUpdateKind::parse(&payload.kind.0),
                    confidence: payload.confidence,
                    evidence: payload.evidence.clone(),
                    frame: payload.frame.clone(),
                    quality: payload.quality.clone(),
                    blindspot: payload.blindspot.clone(),
                    resource: payload.resource.clone(),
                    reflection_inputs: Vec::new(),
                    statement: proposal_statement(payload),
                    provenance: event.provenance.clone(),
                },
            );
        }
        p::EventPayload::CandidateCreated(payload) => {
            if let Some((scope, _kind, statement)) = parse_cognitive_target(&payload.target) {
                if let Some(proposal) = state.proposals.get_mut(&payload.candidate_id) {
                    proposal.scope = MapScope {
                        schema_version: p::SchemaVersion(1),
                        scope,
                    };
                    proposal.statement = statement;
                }
            }
        }
        p::EventPayload::CandidatePromoted(payload) => {
            if let Some(proposal) = state.proposals.get(&payload.candidate_id).cloned() {
                let stable = stable_from_proposal(&proposal);
                state.stable.insert(stable.object.clone(), stable);
            }
        }
        p::EventPayload::RetractionEvent(payload) => {
            state.retracted.insert(payload.target_object.clone());
            if let Some(stable) = state.stable.get_mut(&payload.target_object) {
                stable.active = false;
            }
        }
        p::EventPayload::ReevaluationTaskCreated(payload) => {
            for object in &payload.derived_refs {
                state.invalidated.insert(object.clone());
                if let Some(stable) = state.stable.get_mut(object) {
                    stable.active = false;
                }
            }
        }
        p::EventPayload::MemoryMaintenanceApplied(payload) => {
            if let Some((source, derived)) = parse_lineage(&payload.deltas_ref) {
                state.lineage.entry(source).or_default().insert(derived);
            }
        }
        _ => {}
    }
}

fn stable_from_proposal(proposal: &CognitiveMapUpdateProposal) -> StableCognition {
    let (kind, object) = match proposal.kind {
        MapUpdateKind::Frame => (
            StableCognitionKind::Frame,
            p::ObjectRef(
                proposal
                    .frame
                    .as_ref()
                    .map(|reference| reference.0.clone())
                    .unwrap_or_else(|| "frame:unknown".into()),
            ),
        ),
        MapUpdateKind::Quality => (
            StableCognitionKind::Quality,
            p::ObjectRef(
                proposal
                    .quality
                    .as_ref()
                    .map(|reference| reference.0.clone())
                    .unwrap_or_else(|| "quality:unknown".into()),
            ),
        ),
        MapUpdateKind::BlindSpot => (
            StableCognitionKind::BlindSpot,
            p::ObjectRef(
                proposal
                    .blindspot
                    .as_ref()
                    .map(|reference| reference.0.clone())
                    .unwrap_or_else(|| "blindspot:unknown".into()),
            ),
        ),
        MapUpdateKind::ResourceRelation => (
            StableCognitionKind::ResourceRelation,
            p::ObjectRef(
                proposal
                    .resource
                    .as_ref()
                    .map(|reference| reference.0.clone())
                    .unwrap_or_else(|| "resource:unknown".into()),
            ),
        ),
    };
    StableCognition {
        schema_version: proposal.schema_version,
        object,
        scope: proposal.scope.clone(),
        kind,
        statement: proposal.statement.clone(),
        tier: p::StabilityTier::Stable,
        confidence: proposal.confidence,
        evidence: proposal
            .evidence
            .iter()
            .map(|reference| GovernanceEvidence {
                schema_version: p::SchemaVersion(1),
                reference: reference.clone(),
                observed_at: 0,
                verified_process: true,
            })
            .collect(),
        last_reproduced_at: 0,
        active: true,
    }
}

fn proposal_statement(payload: &p::CognitiveMapUpdateProposedPayload) -> String {
    payload
        .frame
        .as_ref()
        .map(|reference| reference.0.clone())
        .or_else(|| {
            payload
                .quality
                .as_ref()
                .map(|reference| reference.0.clone())
        })
        .or_else(|| {
            payload
                .blindspot
                .as_ref()
                .map(|reference| reference.0.clone())
        })
        .or_else(|| {
            payload
                .resource
                .as_ref()
                .map(|reference| reference.0.clone())
        })
        .unwrap_or_else(|| "recovered cognitive proposal".into())
}

fn parse_cognitive_target(
    target: &p::CandidateTargetRef,
) -> Option<(p::Scope, MapUpdateKind, String)> {
    let value = target.0.strip_prefix("cognitive-map|")?;
    let mut fields = value.splitn(3, '|');
    let scope = fields.next()?;
    let kind = fields.next()?;
    let statement = fields.next()?;
    Some((
        p::Scope(unescape_component(scope)),
        MapUpdateKind::parse(kind),
        unescape_component(statement),
    ))
}

fn parse_lineage(reference: &p::MemoryDeltasRef) -> Option<(p::ObjectRef, p::ObjectRef)> {
    let value = reference.0.strip_prefix("cognition-lineage|")?;
    let (source, derived) = value.split_once('|')?;
    Some((
        p::ObjectRef(unescape_component(source)),
        p::ObjectRef(unescape_component(derived)),
    ))
}

fn escape_component(value: &str) -> String {
    value.replace('%', "%25").replace('|', "%7C")
}

fn unescape_component(value: &str) -> String {
    value.replace("%7C", "|").replace("%25", "%")
}

fn system_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn system_timestamp() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
