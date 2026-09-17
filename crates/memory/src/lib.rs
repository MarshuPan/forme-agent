//! Event-sourced memory substrate, projections, candidates, and intentions (prd/06).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_protocol as p;
use forme_store::EventStore;

mod m2_c;
mod m3_c;

pub use m2_c::*;
pub use m3_c::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimeSlice {
    Session,
    Daily,
    LongTerm,
}

impl TimeSlice {
    fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Daily => "daily",
            Self::LongTerm => "long-term",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "session" => Some(Self::Session),
            "daily" => Some(Self::Daily),
            "long-term" => Some(Self::LongTerm),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryScope {
    pub schema_version: p::SchemaVersion,
    pub slice: TimeSlice,
    pub workspace: Option<p::WorkspaceRef>,
    pub scope: p::Scope,
}

impl MemoryScope {
    fn event_scope(&self) -> p::Scope {
        p::Scope(format!(
            "memory|{}|{}|{}",
            self.slice.as_str(),
            escape_component(
                self.workspace
                    .as_ref()
                    .map(|workspace| workspace.0.as_str())
                    .unwrap_or("")
            ),
            escape_component(&self.scope.0)
        ))
    }

    fn from_event_scope(scope: &p::Scope) -> Self {
        let mut fields = scope.0.splitn(4, '|');
        if fields.next() == Some("memory") {
            if let (Some(slice), Some(workspace), Some(label)) =
                (fields.next(), fields.next(), fields.next())
            {
                if let Some(slice) = TimeSlice::parse(slice) {
                    let workspace = unescape_component(workspace);
                    return Self {
                        schema_version: p::SchemaVersion(1),
                        slice,
                        workspace: (!workspace.is_empty()).then_some(p::WorkspaceRef(workspace)),
                        scope: p::Scope(unescape_component(label)),
                    };
                }
            }
        }
        Self {
            schema_version: p::SchemaVersion(1),
            slice: TimeSlice::Daily,
            workspace: None,
            scope: scope.clone(),
        }
    }
}

fn escape_component(value: &str) -> String {
    value.replace('%', "%25").replace('|', "%7C")
}

fn unescape_component(value: &str) -> String {
    value.replace("%7C", "|").replace("%25", "%")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeKind {
    Event,
    Episodic,
    Reflection,
    Fact,
    Attribute,
    Goal,
    CognitiveObjectRef,
}

impl NodeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Episodic => "episodic",
            Self::Reflection => "reflection",
            Self::Fact => "fact",
            Self::Attribute => "attribute",
            Self::Goal => "goal",
            Self::CognitiveObjectRef => "cognitive-object-ref",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "event" => Self::Event,
            "episodic" => Self::Episodic,
            "reflection" => Self::Reflection,
            "attribute" => Self::Attribute,
            "goal" => Self::Goal,
            "cognitive-object-ref" => Self::CognitiveObjectRef,
            _ => Self::Fact,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryNode {
    pub schema_version: p::SchemaVersion,
    pub id: p::NodeId,
    pub kind: NodeKind,
    pub content_ref: p::ContentRef,
    pub tier: p::StabilityTier,
    pub confidence: p::Confidence,
    pub scope: MemoryScope,
    pub resting_activation: p::RestingActivation,
    pub recency: p::Timestamp,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    Temporal,
    Causal,
    Association,
    Contradicts,
    PartOf,
    Refines,
}

impl EdgeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Temporal => "temporal",
            Self::Causal => "causal",
            Self::Association => "association",
            Self::Contradicts => "contradicts",
            Self::PartOf => "part-of",
            Self::Refines => "refines",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "temporal" => Self::Temporal,
            "causal" => Self::Causal,
            "contradicts" => Self::Contradicts,
            "part-of" => Self::PartOf,
            "refines" => Self::Refines,
            _ => Self::Association,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryEdge {
    pub schema_version: p::SchemaVersion,
    pub id: p::EdgeId,
    pub from: p::NodeId,
    pub to: p::NodeId,
    pub kind: EdgeKind,
    pub weight: p::Weight,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQuery {
    pub schema_version: p::SchemaVersion,
    pub scope: Option<MemoryScope>,
    pub kind: Option<NodeKind>,
    pub content_contains: Option<String>,
}

impl Default for GraphQuery {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            scope: None,
            kind: None,
            content_contains: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationBudget {
    pub schema_version: p::SchemaVersion,
    pub max_hops: u8,
    pub top_k_frontier: u16,
    pub per_tick: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Activated {
    pub schema_version: p::SchemaVersion,
    pub node: p::NodeId,
    pub level: p::Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySummary {
    pub schema_version: p::SchemaVersion,
    pub scope: p::Scope,
    pub text: String,
    pub source_refs: Vec<p::EventId>,
    pub raw_event_count: u64,
    pub is_raw_dump: bool,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallCue {
    pub schema_version: p::SchemaVersion,
    pub scope: MemoryScope,
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedMemoryQuery {
    pub schema_version: p::SchemaVersion,
    pub requester: MemoryScope,
    pub target: MemoryScope,
    pub query: String,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopedMemoryHit {
    pub schema_version: p::SchemaVersion,
    pub reference: MemoryRef,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopicMemoryCandidate {
    pub schema_version: p::SchemaVersion,
    pub id: p::CandidateId,
    pub topic: String,
    pub scope: MemoryScope,
    pub summary: p::SummaryRef,
    pub evidence: Vec<p::EvidenceRef>,
    pub confidence: p::Confidence,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeWindow {
    pub schema_version: p::SchemaVersion,
    pub starts_at: p::Timestamp,
    pub ends_at: p::Timestamp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    pub schema_version: p::SchemaVersion,
    pub node: MemoryNode,
    pub occurred_at: p::Timestamp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryRef {
    pub schema_version: p::SchemaVersion,
    pub node: p::NodeId,
    pub content_ref: p::ContentRef,
    pub tier: p::StabilityTier,
    pub scope: MemoryScope,
    pub recency: p::Timestamp,
}

/// Five emergent shapes. Commitment is deterministic and bypasses diffusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationShape {
    Association,
    Change,
    Pressure,
    Gap,
    Tension,
}

pub trait MemoryGraph {
    fn add_node(&self, node: MemoryNode) -> p::Result<p::NodeId>;
    fn add_edge(&self, edge: MemoryEdge) -> p::Result<p::EdgeId>;
    fn seed(&self, seeds: &[p::NodeId], shape: ActivationShape);
    fn spread(&self, budget: ActivationBudget) -> Vec<Activated>;
    fn set_edge_weight(&self, edge: p::EdgeId, w: f32) -> p::Result<()>;
    fn query(&self, q: GraphQuery) -> Vec<MemoryNode>;
}

pub trait MemoryProjection {
    fn timeline(&self, scope: MemoryScope, window: TimeWindow) -> Vec<Episode>;
    fn recall(&self, cue: RecallCue, k: usize) -> Vec<MemoryRef>;
    fn summary(&self, scope: MemoryScope) -> MemorySummary;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateState {
    Candidate,
    Promoted,
    Rejected,
    Downgraded,
    Decayed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateFilter {
    pub schema_version: p::SchemaVersion,
    pub state: Option<CandidateState>,
    pub target_prefix: Option<String>,
}

impl Default for CandidateFilter {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            state: None,
            target_prefix: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateSpec {
    pub schema_version: p::SchemaVersion,
    pub id: p::CandidateId,
    pub target: p::CandidateTargetRef,
    pub evidence_refs: Vec<p::EvidenceRef>,
    pub confidence: p::Confidence,
    pub provenance: p::Provenance,
    pub target_tier: p::StabilityTier,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateRecord {
    pub update: p::CandidateUpdate,
    pub spec: CandidateSpec,
    pub state: CandidateState,
    pub decided_by: Option<p::Actor>,
}

pub trait CandidateStore {
    fn create(&self, candidate: p::CandidateUpdate) -> p::Result<p::CandidateId>;
    fn list(&self, filter: CandidateFilter) -> Vec<p::CandidateUpdate>;
    /// Contract duty: every accepted transition emits the matching H-group event.
    fn transition(&self, id: p::CandidateId, to: CandidateState, by: p::Actor) -> p::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentionTrigger {
    At(p::Timestamp),
    OnEvent(p::EventKind),
    OnCondition(p::IntentionTriggerRef),
}

impl IntentionTrigger {
    fn as_ref(&self) -> p::IntentionTriggerRef {
        match self {
            Self::At(at) => p::IntentionTriggerRef(format!("at:{at}")),
            Self::OnEvent(kind) => p::IntentionTriggerRef(format!("event:{}", kind.as_str())),
            Self::OnCondition(condition) => {
                p::IntentionTriggerRef(format!("condition:{}", condition.0))
            }
        }
    }

    fn from_ref(value: &p::IntentionTriggerRef) -> Self {
        if let Some(at) = value.0.strip_prefix("at:").and_then(|at| at.parse().ok()) {
            return Self::At(at);
        }
        if let Some(kind) = value.0.strip_prefix("event:") {
            if let Ok(kind) = p::EventKind::from_str(kind) {
                return Self::OnEvent(kind);
            }
        }
        Self::OnCondition(p::IntentionTriggerRef(
            value
                .0
                .strip_prefix("condition:")
                .unwrap_or(&value.0)
                .to_owned(),
        ))
    }
}

fn intention_storage_ref(intention: &ProspectiveIntention) -> p::IntentionTriggerRef {
    p::IntentionTriggerRef(format!(
        "{}|seed={}|expires={}",
        intention.trigger.as_ref().0,
        escape_component(&intention.seed.0),
        intention
            .expires_at
            .map(|expires_at| expires_at.to_string())
            .unwrap_or_default()
    ))
}

fn intention_trigger_from_storage(value: &p::IntentionTriggerRef) -> IntentionTrigger {
    IntentionTrigger::from_ref(&p::IntentionTriggerRef(
        value.0.split('|').next().unwrap_or(&value.0).to_owned(),
    ))
}

fn intention_seed_from_storage(value: &p::IntentionTriggerRef) -> p::SeedRef {
    p::SeedRef(
        value
            .0
            .split('|')
            .find_map(|field| field.strip_prefix("seed="))
            .map(unescape_component)
            .unwrap_or_else(|| "recovered-from-legacy-event".into()),
    )
}

fn intention_expiry_from_storage(value: &p::IntentionTriggerRef) -> Option<p::Timestamp> {
    value
        .0
        .split('|')
        .find_map(|field| field.strip_prefix("expires="))
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
}

fn intention_from_protocol(intention: &p::ProspectiveIntention) -> ProspectiveIntention {
    ProspectiveIntention {
        schema_version: intention.schema_version,
        id: intention.id.clone(),
        source: intention.source,
        trigger: match &intention.trigger {
            p::IntentionTrigger::At(at) => IntentionTrigger::At(*at),
            p::IntentionTrigger::OnEvent(kind) => IntentionTrigger::OnEvent(*kind),
            p::IntentionTrigger::OnCondition(reference) => {
                IntentionTrigger::OnCondition(reference.clone())
            }
        },
        state: match intention.state {
            p::IntentionState::Pending => IntentionState::Pending,
            p::IntentionState::Fired => IntentionState::Fired,
            p::IntentionState::Done => IntentionState::Done,
            p::IntentionState::Expired => IntentionState::Expired,
            p::IntentionState::Cancelled => IntentionState::Cancelled,
        },
        seed: intention.seed.clone(),
        provenance: intention.provenance.clone(),
        expires_at: intention.expires_at,
    }
}

fn protocol_intention(intention: &ProspectiveIntention) -> p::ProspectiveIntention {
    p::ProspectiveIntention {
        schema_version: intention.schema_version,
        id: intention.id.clone(),
        source: intention.source,
        trigger: match &intention.trigger {
            IntentionTrigger::At(at) => p::IntentionTrigger::At(*at),
            IntentionTrigger::OnEvent(kind) => p::IntentionTrigger::OnEvent(*kind),
            IntentionTrigger::OnCondition(reference) => {
                p::IntentionTrigger::OnCondition(reference.clone())
            }
        },
        state: match intention.state {
            IntentionState::Pending => p::IntentionState::Pending,
            IntentionState::Fired => p::IntentionState::Fired,
            IntentionState::Done => p::IntentionState::Done,
            IntentionState::Expired => p::IntentionState::Expired,
            IntentionState::Cancelled => p::IntentionState::Cancelled,
        },
        seed: intention.seed.clone(),
        provenance: intention.provenance.clone(),
        expires_at: intention.expires_at,
    }
}

fn schedule_command(
    intention: &ProspectiveIntention,
    binding: &p::ScheduleBinding,
) -> p::ScheduleCommand {
    p::ScheduleCommand {
        schema_version: binding.schema_version,
        intention: protocol_intention(intention),
        session: binding.session.clone(),
        envelope: binding.envelope.clone(),
        budget: binding.budget.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentionState {
    Pending,
    Fired,
    Done,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveIntention {
    pub schema_version: p::SchemaVersion,
    pub id: p::IntentionId,
    pub source: p::IntentionSource,
    pub trigger: IntentionTrigger,
    pub state: IntentionState,
    pub seed: p::SeedRef,
    pub provenance: p::Provenance,
    pub expires_at: Option<p::Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedIntention {
    pub schema_version: p::SchemaVersion,
    pub intention: ProspectiveIntention,
    pub schedule: Option<p::ScheduleBinding>,
    pub lease_until: p::Timestamp,
    pub claim_event: p::EventId,
}

impl ClaimedIntention {
    pub fn scheduled_claim(&self) -> Option<p::ScheduleClaim> {
        let binding = self.schedule.as_ref()?;
        Some(p::ScheduleClaim {
            schema_version: self.schema_version,
            command: schedule_command(&self.intention, binding),
            lease_until: self.lease_until,
            claim_event: self.claim_event.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledIntentionRecord {
    pub schema_version: p::SchemaVersion,
    pub command: p::ScheduleCommand,
    pub lease_until: Option<p::Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentionOutcome {
    Fired,
    Done,
    Expired,
    Cancelled,
}

pub trait IntentionStore {
    fn create(&self, intention: ProspectiveIntention) -> p::Result<p::IntentionId>;
    fn create_schedule(&self, command: p::ScheduleCommand) -> p::Result<p::IntentionId>;
    fn claim_due(&self, now: p::Timestamp, lease_ms: i64) -> Vec<ClaimedIntention>;
    fn claim_due_unscheduled(&self, now: p::Timestamp, lease_ms: i64) -> Vec<ClaimedIntention>;
    fn claim_due_scheduled(
        &self,
        now: p::Timestamp,
        lease_ms: i64,
        max_claims: usize,
    ) -> Vec<ClaimedIntention>;
    fn resolve(&self, id: p::IntentionId, outcome: IntentionOutcome) -> p::Result<()>;
    fn list_scheduled(&self) -> Vec<ScheduledIntentionRecord>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidencePriority {
    Process,
    Imported,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserAttributeCandidate {
    pub schema_version: p::SchemaVersion,
    pub candidate_id: p::CandidateId,
    pub attribute: p::UserAttributeRef,
    pub value: p::UserAttributeValueRef,
    pub evidence: Vec<p::EvidenceRef>,
    pub confidence: p::Confidence,
    pub first_observed_at: p::Timestamp,
    pub last_updated_at: p::Timestamp,
    pub stability: p::StabilityTier,
    pub scope: p::Scope,
    pub conflicts: Vec<p::CandidateId>,
    pub feedback: Vec<p::FeedbackRef>,
    pub provenance: p::Provenance,
    pub evidence_priority: EvidencePriority,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserModelAttribute {
    pub schema_version: p::SchemaVersion,
    pub attribute: p::UserAttributeRef,
    pub value: p::UserAttributeValueRef,
    pub evidence: Vec<p::EvidenceRef>,
    pub confidence: p::Confidence,
    pub stability: p::StabilityTier,
    pub scope: p::Scope,
    pub first_observed_at: p::Timestamp,
    pub last_updated_at: p::Timestamp,
    pub conflicts: Vec<p::CandidateId>,
    pub feedback: Vec<p::FeedbackRef>,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedHistoricalEvidence {
    pub schema_version: p::SchemaVersion,
    pub source: p::HistoricalSourceRef,
    pub low_weight: bool,
    pub bootstrap_only: bool,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeScale {
    Session,
    Recent,
    Repeated,
    LongTerm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeScaleTierMap;

impl TimeScaleTierMap {
    pub fn tier(self, scale: TimeScale) -> p::StabilityTier {
        match scale {
            TimeScale::Session => p::StabilityTier::Session,
            TimeScale::Recent => p::StabilityTier::Working,
            TimeScale::Repeated | TimeScale::LongTerm => p::StabilityTier::Stable,
        }
    }
}

#[derive(Debug, Clone)]
struct IntentionRecord {
    intention: ProspectiveIntention,
    schedule: Option<p::ScheduleBinding>,
    lease_until: Option<p::Timestamp>,
    claim_generation: p::EventId,
}

#[derive(Debug, Default)]
struct MemoryState {
    nodes: BTreeMap<p::NodeId, MemoryNode>,
    node_events: BTreeMap<p::NodeId, p::EventId>,
    edges: BTreeMap<p::EdgeId, MemoryEdge>,
    candidates: BTreeMap<p::CandidateId, CandidateRecord>,
    intentions: BTreeMap<p::IntentionId, IntentionRecord>,
    user_candidates: BTreeMap<p::CandidateId, UserAttributeCandidate>,
    stable_user_attributes: BTreeMap<(p::UserAttributeRef, p::Scope), UserModelAttribute>,
    imported_history: Vec<ImportedHistoricalEvidence>,
    capability_updates: BTreeMap<p::CandidateId, p::CapabilityUpdateProposal>,
    dormant_seed: Option<(Vec<p::NodeId>, ActivationShape)>,
}

type Clock = Arc<dyn Fn() -> p::Timestamp + Send + Sync>;
static NEXT_MEMORY_INSTANCE: AtomicU64 = AtomicU64::new(0);

pub struct EventSourcedMemory<S: EventStore> {
    store: Arc<S>,
    aggregate_run: p::RunId,
    state: Mutex<MemoryState>,
    event_sequence: AtomicU64,
    candidate_sequence: AtomicU64,
    instance_id: String,
    clock: Clock,
}

impl<S> EventSourcedMemory<S>
where
    S: EventStore,
{
    pub fn open(store: Arc<S>, aggregate_run: p::RunId) -> p::Result<Self> {
        Self::with_clock(store, aggregate_run, system_timestamp)
    }

    pub fn with_clock<F>(store: Arc<S>, aggregate_run: p::RunId, clock: F) -> p::Result<Self>
    where
        F: Fn() -> p::Timestamp + Send + Sync + 'static,
    {
        if aggregate_run.0.trim().is_empty() {
            return Err(p::Error("memory aggregate run is empty".into()));
        }
        let events = store
            .read_run(aggregate_run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let mut state = MemoryState::default();
        let mut max_stream_seq = 0;
        for event in &events {
            max_stream_seq = max_stream_seq.max(event.stream_seq);
            fold_event(&mut state, event);
        }
        let candidate_next = next_candidate_sequence(&aggregate_run, &state);
        Ok(Self {
            store,
            aggregate_run,
            state: Mutex::new(state),
            event_sequence: AtomicU64::new(max_stream_seq.saturating_add(1)),
            candidate_sequence: AtomicU64::new(candidate_next),
            instance_id: memory_instance_id(),
            clock: Arc::new(clock),
        })
    }

    pub fn aggregate_run(&self) -> &p::RunId {
        &self.aggregate_run
    }

    pub fn reserve_candidate_id(&self) -> p::CandidateId {
        p::CandidateId(format!(
            "candidate:{}:{}",
            self.aggregate_run.0,
            self.candidate_sequence.fetch_add(1, Ordering::SeqCst)
        ))
    }

    pub fn create_candidate_spec(&self, spec: CandidateSpec) -> p::Result<p::CandidateId> {
        validate_candidate_spec(&spec)?;
        let mut state = self.lock_state()?;
        self.create_candidate_spec_locked(&mut state, spec)
    }

    pub fn candidate_record(&self, id: &p::CandidateId) -> Option<CandidateRecord> {
        self.lock_state()
            .ok()
            .and_then(|state| state.candidates.get(id).cloned())
    }

    pub fn create_user_attribute_candidate(
        &self,
        candidate: UserAttributeCandidate,
    ) -> p::Result<p::CandidateId> {
        validate_user_candidate(&candidate)?;
        let mut state = self.lock_state()?;
        if let Some(existing) = state.user_candidates.get(&candidate.candidate_id) {
            return if existing == &candidate {
                Ok(candidate.candidate_id)
            } else {
                Err(p::Error("user candidate id collision".into()))
            };
        }
        self.append_payload(
            candidate.provenance.clone(),
            p::EventPayload::UserAttributeCandidateCreated(
                p::UserAttributeCandidateCreatedPayload {
                    candidate_id: candidate.candidate_id.clone(),
                    attribute: candidate.attribute.clone(),
                    value: candidate.value.clone(),
                    evidence: candidate.evidence.clone(),
                    confidence: candidate.confidence,
                    first_at: candidate.first_observed_at,
                    last_at: candidate.last_updated_at,
                    stability: candidate.stability,
                    scope: candidate.scope.clone(),
                    conflicts: candidate.conflicts.clone(),
                    feedback: candidate.feedback.clone(),
                },
            ),
        )?;
        state
            .user_candidates
            .insert(candidate.candidate_id.clone(), candidate.clone());
        self.create_candidate_spec_locked(
            &mut state,
            CandidateSpec {
                schema_version: candidate.schema_version,
                id: candidate.candidate_id.clone(),
                target: p::CandidateTargetRef(format!(
                    "user-attribute:{}:{}",
                    candidate.attribute.0, candidate.scope.0
                )),
                evidence_refs: candidate.evidence,
                confidence: candidate.confidence,
                provenance: candidate.provenance,
                target_tier: candidate.stability,
            },
        )
    }

    pub fn import_historical(&self, evidence: ImportedHistoricalEvidence) -> p::Result<()> {
        if evidence.schema_version.0 == 0
            || evidence.source.0.trim().is_empty()
            || !evidence.low_weight
            || !evidence.bootstrap_only
        {
            return Err(p::Error(
                "historical evidence must be versioned, low-weight, and bootstrap-only".into(),
            ));
        }
        let mut state = self.lock_state()?;
        if state
            .imported_history
            .iter()
            .any(|existing| existing.source == evidence.source)
        {
            return Ok(());
        }
        self.append_payload(
            evidence.provenance.clone(),
            p::EventPayload::ImportedHistoricalEvidenceRecorded(
                p::ImportedHistoricalEvidenceRecordedPayload {
                    source: evidence.source.clone(),
                    low_weight: p::RequiredTrue,
                    bootstrap_only: p::RequiredTrue,
                },
            ),
        )?;
        state.imported_history.push(evidence);
        Ok(())
    }

    pub fn imported_history(&self) -> Vec<ImportedHistoricalEvidence> {
        self.lock_state()
            .map(|state| state.imported_history.clone())
            .unwrap_or_default()
    }

    pub fn user_candidate(&self, id: &p::CandidateId) -> Option<UserAttributeCandidate> {
        self.lock_state()
            .ok()
            .and_then(|state| state.user_candidates.get(id).cloned())
    }

    pub fn user_attribute(
        &self,
        attribute: &p::UserAttributeRef,
        scope: &p::Scope,
    ) -> Option<UserModelAttribute> {
        self.lock_state().ok().and_then(|state| {
            state
                .stable_user_attributes
                .get(&(attribute.clone(), scope.clone()))
                .cloned()
        })
    }

    pub fn candidate_records(&self) -> Vec<CandidateRecord> {
        self.lock_state()
            .map(|state| state.candidates.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn search_scoped(&self, query: ScopedMemoryQuery) -> p::Result<Vec<ScopedMemoryHit>> {
        if query.schema_version.0 == 0
            || query.requester.schema_version.0 == 0
            || query.target.schema_version.0 == 0
            || query.limit == 0
            || query.requester.scope.0.trim().is_empty()
            || query.target.scope.0.trim().is_empty()
        {
            return Err(p::Error("scoped memory query is incomplete".into()));
        }
        if !memory_scope_contains(&query.requester, &query.target) {
            return Err(p::Error(
                "scoped memory query cannot cross its requester boundary".into(),
            ));
        }
        let mut hits = self
            .lock_state()?
            .nodes
            .values()
            .filter(|node| {
                node.scope == query.target
                    && (query.query.is_empty() || node.content_ref.0.contains(&query.query))
            })
            .map(|node| ScopedMemoryHit {
                schema_version: p::SchemaVersion(1),
                reference: MemoryRef {
                    schema_version: p::SchemaVersion(1),
                    node: node.id.clone(),
                    content_ref: node.content_ref.clone(),
                    tier: node.tier,
                    scope: node.scope.clone(),
                    recency: node.recency,
                },
                provenance: node.provenance.clone(),
            })
            .collect::<Vec<_>>();
        hits.sort_by_key(|hit| std::cmp::Reverse(hit.reference.recency));
        hits.truncate(query.limit.min(32));
        Ok(hits)
    }

    pub fn create_topic_candidate(
        &self,
        candidate: TopicMemoryCandidate,
    ) -> p::Result<p::CandidateId> {
        if candidate.schema_version.0 == 0
            || candidate.id.0.trim().is_empty()
            || candidate.topic.trim().is_empty()
            || candidate.scope.schema_version.0 == 0
            || candidate.scope.scope.0.trim().is_empty()
            || candidate.summary.0.trim().is_empty()
            || candidate.evidence.is_empty()
            || !candidate.confidence.0.is_finite()
        {
            return Err(p::Error("topic memory candidate is incomplete".into()));
        }
        self.create_candidate_spec(CandidateSpec {
            schema_version: candidate.schema_version,
            id: candidate.id,
            target: p::CandidateTargetRef(format!(
                "topic-summary:{}:{}",
                candidate.scope.scope.0, candidate.topic
            )),
            evidence_refs: candidate.evidence,
            confidence: candidate.confidence,
            provenance: candidate.provenance,
            target_tier: p::StabilityTier::Working,
        })
    }

    pub fn user_candidates_in_scope(&self, scope: &p::Scope) -> Vec<UserAttributeCandidate> {
        self.lock_state()
            .map(|state| {
                state
                    .user_candidates
                    .values()
                    .filter(|candidate| &candidate.scope == scope)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn create_candidate_spec_locked(
        &self,
        state: &mut MemoryState,
        spec: CandidateSpec,
    ) -> p::Result<p::CandidateId> {
        if let Some(existing) = state.candidates.get(&spec.id) {
            return if existing.spec == spec {
                Ok(spec.id)
            } else {
                Err(p::Error("candidate id collision".into()))
            };
        }
        self.append_payload(
            spec.provenance.clone(),
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: spec.id.clone(),
                target: spec.target.clone(),
                evidence_refs: spec.evidence_refs.clone(),
                confidence: spec.confidence,
                provenance: spec.provenance.clone(),
                target_tier: spec.target_tier,
                capability_update: None,
                strategy_candidate: None,
            }),
        )?;
        let id = spec.id.clone();
        state.candidates.insert(
            id.clone(),
            CandidateRecord {
                update: p::CandidateUpdate {
                    schema_version: spec.schema_version,
                },
                spec,
                state: CandidateState::Candidate,
                decided_by: None,
            },
        );
        Ok(id)
    }

    fn claim_due_matching(
        &self,
        now: p::Timestamp,
        lease_ms: i64,
        schedule_filter: Option<bool>,
        max_claims: usize,
    ) -> Vec<ClaimedIntention> {
        if lease_ms <= 0 || max_claims == 0 {
            return Vec::new();
        }
        let Ok(mut state) = self.lock_state() else {
            return Vec::new();
        };
        let ids = state.intentions.keys().cloned().collect::<Vec<_>>();
        let mut claimed = Vec::new();
        for id in ids {
            if claimed.len() >= max_claims {
                break;
            }
            let Some(snapshot) = state.intentions.get(&id).cloned() else {
                continue;
            };
            if schedule_filter.is_some_and(|scheduled| scheduled != snapshot.schedule.is_some()) {
                continue;
            }
            if matches!(
                snapshot.intention.state,
                IntentionState::Done | IntentionState::Expired | IntentionState::Cancelled
            ) {
                continue;
            }
            if snapshot
                .intention
                .expires_at
                .is_some_and(|expires_at| expires_at <= now)
            {
                if let Ok(resolved_event) = self.append_payload(
                    system_provenance(),
                    intention_resolved_payload(&id, IntentionOutcome::Expired),
                ) {
                    if let Some(record) = state.intentions.get_mut(&id) {
                        record.intention.state = IntentionState::Expired;
                        record.lease_until = None;
                        record.claim_generation = resolved_event;
                    }
                }
                continue;
            }
            let due = match snapshot.intention.state {
                IntentionState::Pending => {
                    matches!(snapshot.intention.trigger, IntentionTrigger::At(at) if at <= now)
                        && snapshot
                            .lease_until
                            .is_none_or(|lease_until| lease_until <= now)
                }
                IntentionState::Fired => snapshot
                    .lease_until
                    .is_none_or(|lease_until| lease_until <= now),
                IntentionState::Done | IntentionState::Expired | IntentionState::Cancelled => false,
            };
            if !due {
                continue;
            }
            let lease_until = now.saturating_add(lease_ms);
            let lease_payload =
                p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
                    deltas_ref: p::MemoryDeltasRef(format!(
                        "intention-lease|{}|{lease_until}|{}",
                        escape_component(&snapshot.claim_generation.0),
                        escape_component(&id.0),
                    )),
                });
            let Ok((lease_event, lease_claimed)) =
                self.append_payload_once(system_provenance(), lease_payload)
            else {
                continue;
            };
            if !lease_claimed {
                continue;
            }
            if let Some(record) = state.intentions.get_mut(&id) {
                record.lease_until = Some(lease_until);
                record.claim_generation = lease_event;
            }
            let Ok(claim_event) = self.append_payload(
                system_provenance(),
                intention_resolved_payload(&id, IntentionOutcome::Fired),
            ) else {
                continue;
            };
            if let Some(record) = state.intentions.get_mut(&id) {
                record.intention.state = IntentionState::Fired;
                record.claim_generation = claim_event.clone();
                claimed.push(ClaimedIntention {
                    schema_version: p::SchemaVersion(1),
                    intention: record.intention.clone(),
                    schedule: record.schedule.clone(),
                    lease_until,
                    claim_event,
                });
            }
        }
        claimed
    }

    fn append_payload(
        &self,
        provenance: p::Provenance,
        payload: p::EventPayload,
    ) -> p::Result<p::EventId> {
        self.append_payload_once(provenance, payload)
            .map(|(event, _)| event)
    }

    fn append_payload_once(
        &self,
        provenance: p::Provenance,
        payload: p::EventPayload,
    ) -> p::Result<(p::EventId, bool)> {
        let sequence = self.event_sequence.fetch_add(1, Ordering::SeqCst);
        let requested = p::EventId(format!(
            "memory-event:{}:{}:{sequence}",
            self.aggregate_run.0, self.instance_id
        ));
        let event = p::Event::new(
            requested.clone(),
            self.aggregate_run.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            (self.clock)(),
            provenance,
        );
        let persisted = self.store.append(event)?;
        let claimed = persisted == requested;
        Ok((persisted, claimed))
    }

    fn lock_state(&self) -> p::Result<MutexGuard<'_, MemoryState>> {
        self.state
            .lock()
            .map_err(|_| p::Error("memory projection state is unavailable".into()))
    }
}

fn memory_scope_contains(requester: &MemoryScope, target: &MemoryScope) -> bool {
    let requester_rank = match requester.slice {
        TimeSlice::Session => 1,
        TimeSlice::Daily => 2,
        TimeSlice::LongTerm => 3,
    };
    let target_rank = match target.slice {
        TimeSlice::Session => 1,
        TimeSlice::Daily => 2,
        TimeSlice::LongTerm => 3,
    };
    requester.workspace == target.workspace
        && requester_rank >= target_rank
        && (requester.scope == target.scope
            || target
                .scope
                .0
                .strip_prefix(&requester.scope.0)
                .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('/')))
}

impl<S> MemoryGraph for EventSourcedMemory<S>
where
    S: EventStore,
{
    fn add_node(&self, node: MemoryNode) -> p::Result<p::NodeId> {
        validate_node(&node)?;
        let mut state = self.lock_state()?;
        if let Some(existing) = state.nodes.get(&node.id) {
            return if existing == &node {
                Ok(node.id)
            } else {
                Err(p::Error("memory node id collision".into()))
            };
        }
        let event_id = self.append_payload(
            node.provenance.clone(),
            p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                node_id: node.id.clone(),
                kind: p::MemoryNodeType(node.kind.as_str().into()),
                content_ref: node.content_ref.clone(),
                tier: node.tier,
                confidence: node.confidence,
                scope: node.scope.event_scope(),
                resting_activation: node.resting_activation,
                recency: p::Recency(node.recency),
            }),
        )?;
        state.node_events.insert(node.id.clone(), event_id);
        state.nodes.insert(node.id.clone(), node.clone());
        Ok(node.id)
    }

    fn add_edge(&self, edge: MemoryEdge) -> p::Result<p::EdgeId> {
        validate_edge(&edge)?;
        let mut state = self.lock_state()?;
        if !state.nodes.contains_key(&edge.from) || !state.nodes.contains_key(&edge.to) {
            return Err(p::Error("memory edge references an unknown node".into()));
        }
        if let Some(existing) = state.edges.get(&edge.id) {
            return if existing == &edge {
                Ok(edge.id)
            } else {
                Err(p::Error("memory edge id collision".into()))
            };
        }
        self.append_payload(
            edge.provenance.clone(),
            p::EventPayload::MemoryEdgeAppended(p::MemoryEdgeAppendedPayload {
                edge_id: edge.id.clone(),
                from: edge.from.clone(),
                to: edge.to.clone(),
                kind: p::MemoryEdgeType(edge.kind.as_str().into()),
                weight: edge.weight,
            }),
        )?;
        state.edges.insert(edge.id.clone(), edge.clone());
        Ok(edge.id)
    }

    fn seed(&self, seeds: &[p::NodeId], shape: ActivationShape) {
        if let Ok(mut state) = self.lock_state() {
            let valid = seeds
                .iter()
                .filter(|seed| state.nodes.contains_key(*seed))
                .cloned()
                .collect::<Vec<_>>();
            state.dormant_seed = (!valid.is_empty()).then_some((valid, shape));
        }
    }

    fn spread(&self, _budget: ActivationBudget) -> Vec<Activated> {
        // M0 intentionally keeps the schema and trait while diffusion remains dormant until M2.
        Vec::new()
    }

    fn set_edge_weight(&self, _edge: p::EdgeId, _weight: f32) -> p::Result<()> {
        Err(p::Error(
            "activation learning is dormant in M0; edge weights are immutable".into(),
        ))
    }

    fn query(&self, query: GraphQuery) -> Vec<MemoryNode> {
        if query.schema_version.0 == 0 {
            return Vec::new();
        }
        self.lock_state()
            .map(|state| {
                state
                    .nodes
                    .values()
                    .filter(|node| {
                        query
                            .scope
                            .as_ref()
                            .map(|scope| &node.scope == scope)
                            .unwrap_or(true)
                            && query.kind.map(|kind| node.kind == kind).unwrap_or(true)
                            && query
                                .content_contains
                                .as_ref()
                                .map(|needle| node.content_ref.0.contains(needle))
                                .unwrap_or(true)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl<S> MemoryProjection for EventSourcedMemory<S>
where
    S: EventStore,
{
    fn timeline(&self, scope: MemoryScope, window: TimeWindow) -> Vec<Episode> {
        if window.schema_version.0 == 0 || window.starts_at > window.ends_at {
            return Vec::new();
        }
        self.lock_state()
            .map(|state| {
                state
                    .nodes
                    .values()
                    .filter(|node| {
                        node.scope == scope
                            && node.recency >= window.starts_at
                            && node.recency <= window.ends_at
                    })
                    .cloned()
                    .map(|node| Episode {
                        schema_version: p::SchemaVersion(1),
                        occurred_at: node.recency,
                        node,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn recall(&self, cue: RecallCue, k: usize) -> Vec<MemoryRef> {
        if cue.schema_version.0 == 0 || k == 0 {
            return Vec::new();
        }
        let mut found = self
            .lock_state()
            .map(|state| {
                state
                    .nodes
                    .values()
                    .filter(|node| {
                        node.scope == cue.scope
                            && (cue.query.is_empty() || node.content_ref.0.contains(&cue.query))
                    })
                    .map(|node| MemoryRef {
                        schema_version: p::SchemaVersion(1),
                        node: node.id.clone(),
                        content_ref: node.content_ref.clone(),
                        tier: node.tier,
                        scope: node.scope.clone(),
                        recency: node.recency,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        found.sort_by_key(|reference| std::cmp::Reverse(reference.recency));
        found.truncate(k);
        found
    }

    fn summary(&self, scope: MemoryScope) -> MemorySummary {
        let (nodes, refs) = self
            .lock_state()
            .map(|state| {
                let nodes = state
                    .nodes
                    .values()
                    .filter(|node| node.scope == scope)
                    .collect::<Vec<_>>();
                let refs = nodes
                    .iter()
                    .filter_map(|node| state.node_events.get(&node.id).cloned())
                    .collect::<Vec<_>>();
                (nodes.len(), refs)
            })
            .unwrap_or_default();
        MemorySummary {
            schema_version: p::SchemaVersion(1),
            scope: scope.scope,
            text: format!("{nodes} bounded memory records are available in this scope"),
            source_refs: refs,
            raw_event_count: nodes as u64,
            is_raw_dump: false,
            provenance: system_provenance(),
        }
    }
}

impl<S> CandidateStore for EventSourcedMemory<S>
where
    S: EventStore,
{
    fn create(&self, candidate: p::CandidateUpdate) -> p::Result<p::CandidateId> {
        if candidate.schema_version.0 == 0 {
            return Err(p::Error("candidate update is not versioned".into()));
        }
        let id = self.reserve_candidate_id();
        self.create_candidate_spec(CandidateSpec {
            schema_version: candidate.schema_version,
            id,
            target: p::CandidateTargetRef("candidate:unspecified".into()),
            evidence_refs: Vec::new(),
            confidence: p::Confidence(0.0),
            provenance: system_provenance(),
            target_tier: p::StabilityTier::Ephemeral,
        })
    }

    fn list(&self, filter: CandidateFilter) -> Vec<p::CandidateUpdate> {
        if filter.schema_version.0 == 0 {
            return Vec::new();
        }
        self.lock_state()
            .map(|state| {
                state
                    .candidates
                    .values()
                    .filter(|candidate| {
                        filter
                            .state
                            .map(|expected| candidate.state == expected)
                            .unwrap_or(true)
                            && filter
                                .target_prefix
                                .as_ref()
                                .map(|prefix| candidate.spec.target.0.starts_with(prefix))
                                .unwrap_or(true)
                    })
                    .map(|candidate| candidate.update.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn transition(&self, id: p::CandidateId, to: CandidateState, by: p::Actor) -> p::Result<()> {
        let mut state = self.lock_state()?;
        let current = state
            .candidates
            .get(&id)
            .ok_or_else(|| p::Error("candidate is not registered".into()))?
            .state;
        if current == to {
            return Ok(());
        }
        if !valid_candidate_transition(current, to) {
            return Err(p::Error("candidate transition is not allowed".into()));
        }
        if to == CandidateState::Promoted && by != p::Actor::Owner {
            return Err(p::Error(
                "M0 stable promotion requires an authenticated owner decision".into(),
            ));
        }
        let decision_actor = if by == p::Actor::Owner {
            p::DecisionActor::User
        } else {
            p::DecisionActor::Auto
        };
        let reason = p::ReasonRef(format!("candidate transition {current:?} -> {to:?}"));
        let payload = match to {
            CandidateState::Candidate => {
                return Err(p::Error(
                    "candidate cannot transition back to candidate".into(),
                ))
            }
            CandidateState::Promoted => {
                p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                    candidate_id: id.clone(),
                    by: decision_actor,
                    reason,
                })
            }
            CandidateState::Rejected => {
                p::EventPayload::CandidateRejected(p::CandidateRejectedPayload {
                    candidate_id: id.clone(),
                    by: decision_actor,
                    reason,
                })
            }
            CandidateState::Downgraded => {
                p::EventPayload::CandidateDowngraded(p::CandidateDowngradedPayload {
                    candidate_id: id.clone(),
                    by: decision_actor,
                    reason,
                })
            }
            CandidateState::Decayed => {
                p::EventPayload::CandidateDecayed(p::CandidateDecayedPayload {
                    candidate_id: id.clone(),
                    by: decision_actor,
                    reason,
                })
            }
        };
        self.append_payload(system_provenance(), payload)?;
        let record = state
            .candidates
            .get_mut(&id)
            .ok_or_else(|| p::Error("candidate disappeared during transition".into()))?;
        record.state = to;
        record.decided_by = Some(by);
        if to == CandidateState::Promoted {
            promote_user_candidate(&mut state, &id);
        } else if matches!(to, CandidateState::Downgraded | CandidateState::Decayed) {
            demote_user_candidate(&mut state, &id);
        }
        Ok(())
    }
}

impl<S> IntentionStore for EventSourcedMemory<S>
where
    S: EventStore,
{
    fn create(&self, intention: ProspectiveIntention) -> p::Result<p::IntentionId> {
        validate_intention(&intention)?;
        let mut state = self.lock_state()?;
        if let Some(existing) = state.intentions.get(&intention.id) {
            return if existing.intention == intention {
                Ok(intention.id)
            } else {
                Err(p::Error("intention id collision".into()))
            };
        }
        let created_event = self.append_payload(
            intention.provenance.clone(),
            p::EventPayload::ProspectiveIntentionCreated(p::ProspectiveIntentionCreatedPayload {
                intention_id: intention.id.clone(),
                source: intention.source,
                trigger: intention_storage_ref(&intention),
                schedule: None,
                goal_frame: None,
            }),
        )?;
        state.intentions.insert(
            intention.id.clone(),
            IntentionRecord {
                intention: intention.clone(),
                schedule: None,
                lease_until: None,
                claim_generation: created_event,
            },
        );
        Ok(intention.id)
    }

    fn create_schedule(&self, command: p::ScheduleCommand) -> p::Result<p::IntentionId> {
        command.validate()?;
        let intention = intention_from_protocol(&command.intention);
        validate_intention(&intention)?;
        let binding = command.binding();
        let mut state = self.lock_state()?;
        if let Some(existing) = state.intentions.get(&intention.id) {
            return if same_intention_definition(&existing.intention, &intention)
                && existing.schedule.as_ref() == Some(&binding)
            {
                Ok(intention.id)
            } else {
                Err(p::Error("intention id collision".into()))
            };
        }
        let created_event = self.append_payload(
            intention.provenance.clone(),
            p::EventPayload::ProspectiveIntentionCreated(p::ProspectiveIntentionCreatedPayload {
                intention_id: intention.id.clone(),
                source: intention.source,
                trigger: intention_storage_ref(&intention),
                schedule: Some(binding.clone()),
                goal_frame: None,
            }),
        )?;
        state.intentions.insert(
            intention.id.clone(),
            IntentionRecord {
                intention: intention.clone(),
                schedule: Some(binding),
                lease_until: None,
                claim_generation: created_event,
            },
        );
        Ok(intention.id)
    }

    fn claim_due(&self, now: p::Timestamp, lease_ms: i64) -> Vec<ClaimedIntention> {
        self.claim_due_matching(now, lease_ms, None, usize::MAX)
    }

    fn claim_due_unscheduled(&self, now: p::Timestamp, lease_ms: i64) -> Vec<ClaimedIntention> {
        self.claim_due_matching(now, lease_ms, Some(false), usize::MAX)
    }

    fn claim_due_scheduled(
        &self,
        now: p::Timestamp,
        lease_ms: i64,
        max_claims: usize,
    ) -> Vec<ClaimedIntention> {
        self.claim_due_matching(now, lease_ms, Some(true), max_claims)
    }

    fn resolve(&self, id: p::IntentionId, outcome: IntentionOutcome) -> p::Result<()> {
        let mut state = self.lock_state()?;
        let current = state
            .intentions
            .get(&id)
            .ok_or_else(|| p::Error("intention is not registered".into()))?
            .intention
            .state;
        let target = intention_state(outcome);
        if current == target {
            return Ok(());
        }
        if !valid_intention_transition(current, target) {
            return Err(p::Error("intention transition is not allowed".into()));
        }
        let resolved_event = self.append_payload(
            system_provenance(),
            intention_resolved_payload(&id, outcome),
        )?;
        if let Some(record) = state.intentions.get_mut(&id) {
            record.intention.state = target;
            if target != IntentionState::Fired {
                record.lease_until = None;
            }
            record.claim_generation = resolved_event;
        }
        Ok(())
    }

    fn list_scheduled(&self) -> Vec<ScheduledIntentionRecord> {
        self.lock_state()
            .map(|state| {
                state
                    .intentions
                    .values()
                    .filter_map(|record| {
                        let binding = record.schedule.clone()?;
                        Some(ScheduledIntentionRecord {
                            schema_version: p::SchemaVersion(1),
                            command: schedule_command(&record.intention, &binding),
                            lease_until: record.lease_until,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn validate_node(node: &MemoryNode) -> p::Result<()> {
    if node.schema_version.0 == 0
        || node.scope.schema_version.0 == 0
        || node.id.0.trim().is_empty()
        || node.content_ref.0.trim().is_empty()
        || node.scope.scope.0.trim().is_empty()
        || !node.confidence.0.is_finite()
        || !node.resting_activation.0.is_finite()
    {
        return Err(p::Error("memory node is incomplete or non-finite".into()));
    }
    if node.provenance.trust_tier == p::TrustTier::Untrusted
        && matches!(
            node.tier,
            p::StabilityTier::Fixed | p::StabilityTier::Constitutional | p::StabilityTier::Stable
        )
    {
        return Err(p::Error(
            "untrusted content cannot enter a stable memory tier".into(),
        ));
    }
    Ok(())
}

fn validate_edge(edge: &MemoryEdge) -> p::Result<()> {
    if edge.schema_version.0 == 0
        || edge.id.0.trim().is_empty()
        || edge.from.0.trim().is_empty()
        || edge.to.0.trim().is_empty()
        || edge.from == edge.to
        || !edge.weight.0.is_finite()
    {
        return Err(p::Error("memory edge is incomplete or non-finite".into()));
    }
    if edge.provenance.trust_tier == p::TrustTier::Untrusted {
        return Err(p::Error(
            "untrusted content cannot modify memory graph structure".into(),
        ));
    }
    Ok(())
}

fn validate_candidate_spec(spec: &CandidateSpec) -> p::Result<()> {
    if spec.schema_version.0 == 0
        || spec.id.0.trim().is_empty()
        || spec.target.0.trim().is_empty()
        || !spec.confidence.0.is_finite()
    {
        return Err(p::Error("candidate is incomplete or non-finite".into()));
    }
    if spec.provenance.trust_tier == p::TrustTier::Untrusted
        && matches!(
            spec.target_tier,
            p::StabilityTier::Fixed | p::StabilityTier::Constitutional | p::StabilityTier::Stable
        )
    {
        return Err(p::Error(
            "untrusted candidate cannot target a stable tier".into(),
        ));
    }
    Ok(())
}

fn validate_user_candidate(candidate: &UserAttributeCandidate) -> p::Result<()> {
    if candidate.schema_version.0 == 0
        || candidate.candidate_id.0.trim().is_empty()
        || candidate.attribute.0.trim().is_empty()
        || candidate.value.0.trim().is_empty()
        || candidate.scope.0.trim().is_empty()
        || candidate.evidence.is_empty()
        || candidate.first_observed_at > candidate.last_updated_at
        || !candidate.confidence.0.is_finite()
    {
        return Err(p::Error("user attribute candidate is incomplete".into()));
    }
    if candidate.evidence_priority == EvidencePriority::Imported
        && matches!(
            candidate.stability,
            p::StabilityTier::Fixed | p::StabilityTier::Constitutional | p::StabilityTier::Stable
        )
    {
        return Err(p::Error(
            "imported evidence cannot target a stable user attribute".into(),
        ));
    }
    if candidate.provenance.trust_tier == p::TrustTier::Untrusted
        && matches!(
            candidate.stability,
            p::StabilityTier::Fixed | p::StabilityTier::Constitutional | p::StabilityTier::Stable
        )
    {
        return Err(p::Error(
            "untrusted evidence cannot target a stable user attribute".into(),
        ));
    }
    Ok(())
}

fn validate_intention(intention: &ProspectiveIntention) -> p::Result<()> {
    if intention.schema_version.0 == 0
        || intention.id.0.trim().is_empty()
        || intention.seed.0.trim().is_empty()
        || intention.state != IntentionState::Pending
    {
        return Err(p::Error("prospective intention is incomplete".into()));
    }
    if let (IntentionTrigger::At(at), Some(expires_at)) = (&intention.trigger, intention.expires_at)
    {
        if expires_at <= *at {
            return Err(p::Error("intention expires before it is due".into()));
        }
    }
    Ok(())
}

fn same_intention_definition(left: &ProspectiveIntention, right: &ProspectiveIntention) -> bool {
    left.schema_version == right.schema_version
        && left.id == right.id
        && left.source == right.source
        && left.trigger == right.trigger
        && left.seed == right.seed
        && left.provenance == right.provenance
        && left.expires_at == right.expires_at
}

fn valid_candidate_transition(from: CandidateState, to: CandidateState) -> bool {
    matches!(
        (from, to),
        (
            CandidateState::Candidate,
            CandidateState::Promoted
                | CandidateState::Rejected
                | CandidateState::Downgraded
                | CandidateState::Decayed
        ) | (CandidateState::Promoted, CandidateState::Downgraded)
            | (
                CandidateState::Downgraded,
                CandidateState::Promoted | CandidateState::Decayed
            )
    )
}

fn promote_user_candidate(state: &mut MemoryState, id: &p::CandidateId) {
    let Some(candidate) = state.user_candidates.get(id).cloned() else {
        return;
    };
    if candidate.evidence_priority != EvidencePriority::Process {
        return;
    }
    state.stable_user_attributes.insert(
        (candidate.attribute.clone(), candidate.scope.clone()),
        UserModelAttribute {
            schema_version: candidate.schema_version,
            attribute: candidate.attribute,
            value: candidate.value,
            evidence: candidate.evidence,
            confidence: candidate.confidence,
            stability: candidate.stability,
            scope: candidate.scope,
            first_observed_at: candidate.first_observed_at,
            last_updated_at: candidate.last_updated_at,
            conflicts: candidate.conflicts,
            feedback: candidate.feedback,
            provenance: candidate.provenance,
        },
    );
}

fn demote_user_candidate(state: &mut MemoryState, id: &p::CandidateId) {
    let Some(candidate) = state.user_candidates.get(id) else {
        return;
    };
    state
        .stable_user_attributes
        .remove(&(candidate.attribute.clone(), candidate.scope.clone()));
}

fn next_candidate_sequence(run: &p::RunId, state: &MemoryState) -> u64 {
    let prefix = format!("candidate:{}:", run.0);
    state
        .candidates
        .keys()
        .filter_map(|id| id.0.strip_prefix(&prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or(state.candidates.len() as u64)
        .saturating_add(1)
}

fn intention_state(outcome: IntentionOutcome) -> IntentionState {
    match outcome {
        IntentionOutcome::Fired => IntentionState::Fired,
        IntentionOutcome::Done => IntentionState::Done,
        IntentionOutcome::Expired => IntentionState::Expired,
        IntentionOutcome::Cancelled => IntentionState::Cancelled,
    }
}

fn valid_intention_transition(from: IntentionState, to: IntentionState) -> bool {
    matches!(
        (from, to),
        (IntentionState::Pending, IntentionState::Fired)
            | (
                IntentionState::Pending,
                IntentionState::Expired | IntentionState::Cancelled
            )
            | (
                IntentionState::Fired,
                IntentionState::Fired
                    | IntentionState::Done
                    | IntentionState::Expired
                    | IntentionState::Cancelled
            )
    )
}

fn intention_resolved_payload(id: &p::IntentionId, outcome: IntentionOutcome) -> p::EventPayload {
    p::EventPayload::ProspectiveIntentionResolved(p::ProspectiveIntentionResolvedPayload {
        intention_id: id.clone(),
        outcome: match outcome {
            IntentionOutcome::Fired => p::IntentionOutcome::Fired,
            IntentionOutcome::Done => p::IntentionOutcome::Done,
            IntentionOutcome::Expired => p::IntentionOutcome::Expired,
            IntentionOutcome::Cancelled => p::IntentionOutcome::Cancelled,
        },
    })
}

fn fold_event(state: &mut MemoryState, event: &p::Event) {
    match &event.payload {
        p::EventPayload::MemoryNodeAppended(payload) => {
            let scope = MemoryScope::from_event_scope(&payload.scope);
            state
                .node_events
                .insert(payload.node_id.clone(), event.event_id.clone());
            state.nodes.insert(
                payload.node_id.clone(),
                MemoryNode {
                    schema_version: event.schema_version,
                    id: payload.node_id.clone(),
                    kind: NodeKind::parse(&payload.kind.0),
                    content_ref: payload.content_ref.clone(),
                    tier: payload.tier,
                    confidence: payload.confidence,
                    scope,
                    resting_activation: payload.resting_activation,
                    recency: payload.recency.0,
                    provenance: event.provenance.clone(),
                },
            );
        }
        p::EventPayload::MemoryEdgeAppended(payload) => {
            state.edges.insert(
                payload.edge_id.clone(),
                MemoryEdge {
                    schema_version: event.schema_version,
                    id: payload.edge_id.clone(),
                    from: payload.from.clone(),
                    to: payload.to.clone(),
                    kind: EdgeKind::parse(&payload.kind.0),
                    weight: payload.weight,
                    provenance: event.provenance.clone(),
                },
            );
        }
        p::EventPayload::CandidateCreated(payload) => {
            if let Some(proposal) = &payload.capability_update {
                state
                    .capability_updates
                    .insert(payload.candidate_id.clone(), proposal.clone());
            }
            state.candidates.insert(
                payload.candidate_id.clone(),
                CandidateRecord {
                    update: p::CandidateUpdate {
                        schema_version: event.schema_version,
                    },
                    spec: CandidateSpec {
                        schema_version: event.schema_version,
                        id: payload.candidate_id.clone(),
                        target: payload.target.clone(),
                        evidence_refs: payload.evidence_refs.clone(),
                        confidence: payload.confidence,
                        provenance: payload.provenance.clone(),
                        target_tier: payload.target_tier,
                    },
                    state: CandidateState::Candidate,
                    decided_by: None,
                },
            );
        }
        p::EventPayload::CandidatePromoted(payload) => {
            if let Some(record) = state.candidates.get_mut(&payload.candidate_id) {
                record.state = CandidateState::Promoted;
            }
            promote_user_candidate(state, &payload.candidate_id);
        }
        p::EventPayload::CandidateRejected(payload) => {
            if let Some(record) = state.candidates.get_mut(&payload.candidate_id) {
                record.state = CandidateState::Rejected;
            }
        }
        p::EventPayload::CandidateDowngraded(payload) => {
            if let Some(record) = state.candidates.get_mut(&payload.candidate_id) {
                record.state = CandidateState::Downgraded;
            }
            demote_user_candidate(state, &payload.candidate_id);
        }
        p::EventPayload::CandidateDecayed(payload) => {
            if let Some(record) = state.candidates.get_mut(&payload.candidate_id) {
                record.state = CandidateState::Decayed;
            }
            demote_user_candidate(state, &payload.candidate_id);
        }
        p::EventPayload::UserAttributeCandidateCreated(payload) => {
            state.user_candidates.insert(
                payload.candidate_id.clone(),
                UserAttributeCandidate {
                    schema_version: event.schema_version,
                    candidate_id: payload.candidate_id.clone(),
                    attribute: payload.attribute.clone(),
                    value: payload.value.clone(),
                    evidence: payload.evidence.clone(),
                    confidence: payload.confidence,
                    first_observed_at: payload.first_at,
                    last_updated_at: payload.last_at,
                    stability: payload.stability,
                    scope: payload.scope.clone(),
                    conflicts: payload.conflicts.clone(),
                    feedback: payload.feedback.clone(),
                    provenance: event.provenance.clone(),
                    evidence_priority: EvidencePriority::Process,
                },
            );
        }
        p::EventPayload::ImportedHistoricalEvidenceRecorded(payload) => {
            state.imported_history.push(ImportedHistoricalEvidence {
                schema_version: event.schema_version,
                source: payload.source.clone(),
                low_weight: true,
                bootstrap_only: true,
                provenance: event.provenance.clone(),
            });
        }
        p::EventPayload::ProspectiveIntentionCreated(payload) => {
            state.intentions.insert(
                payload.intention_id.clone(),
                IntentionRecord {
                    intention: ProspectiveIntention {
                        schema_version: event.schema_version,
                        id: payload.intention_id.clone(),
                        source: payload.source,
                        trigger: intention_trigger_from_storage(&payload.trigger),
                        state: IntentionState::Pending,
                        seed: intention_seed_from_storage(&payload.trigger),
                        provenance: event.provenance.clone(),
                        expires_at: intention_expiry_from_storage(&payload.trigger),
                    },
                    schedule: payload.schedule.clone(),
                    lease_until: None,
                    claim_generation: event.event_id.clone(),
                },
            );
        }
        p::EventPayload::MemoryMaintenanceApplied(payload) => {
            if let Some((id, lease_until)) = parse_intention_lease(&payload.deltas_ref) {
                if let Some(record) = state.intentions.get_mut(&id) {
                    record.lease_until = Some(lease_until);
                    record.claim_generation = event.event_id.clone();
                }
            }
        }
        p::EventPayload::ProspectiveIntentionResolved(payload) => {
            if let Some(record) = state.intentions.get_mut(&payload.intention_id) {
                record.intention.state = match payload.outcome {
                    p::IntentionOutcome::Fired => IntentionState::Fired,
                    p::IntentionOutcome::Done => IntentionState::Done,
                    p::IntentionOutcome::Expired => IntentionState::Expired,
                    p::IntentionOutcome::Cancelled => IntentionState::Cancelled,
                };
                if record.intention.state != IntentionState::Fired {
                    record.lease_until = None;
                }
                record.claim_generation = event.event_id.clone();
            }
        }
        _ => {}
    }
}

fn parse_intention_lease(reference: &p::MemoryDeltasRef) -> Option<(p::IntentionId, i64)> {
    if let Some(value) = reference.0.strip_prefix("intention-lease|") {
        let mut fields = value.split('|');
        let _generation = fields.next()?;
        let lease_until = fields.next()?.parse::<i64>().ok()?;
        let id = fields.next().map(unescape_component)?;
        if fields.next().is_some() {
            return None;
        }
        return Some((p::IntentionId(id), lease_until));
    }
    let value = reference.0.strip_prefix("intention-lease:")?;
    let (lease_until, id) = value.split_once(':')?;
    Some((
        p::IntentionId(id.to_owned()),
        lease_until.parse::<i64>().ok()?,
    ))
}

fn memory_instance_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!(
        "{}-{nanos}-{}",
        std::process::id(),
        NEXT_MEMORY_INSTANCE.fetch_add(1, Ordering::SeqCst)
    )
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicI64;

    use forme_store::{EventStore, SqliteEventStore, StoreOptions};

    use super::*;

    fn provenance(tier: p::TrustTier) -> p::Provenance {
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: tier,
            caused_by: None,
        }
    }

    fn scope(label: &str, slice: TimeSlice) -> MemoryScope {
        MemoryScope {
            schema_version: p::SchemaVersion(1),
            slice,
            workspace: Some(p::WorkspaceRef("workspace:test".into())),
            scope: p::Scope(label.into()),
        }
    }

    fn node(id: &str, scope: MemoryScope, recency: i64) -> MemoryNode {
        MemoryNode {
            schema_version: p::SchemaVersion(1),
            id: p::NodeId(id.into()),
            kind: NodeKind::Episodic,
            content_ref: p::ContentRef(format!("content:{id}")),
            tier: p::StabilityTier::Working,
            confidence: p::Confidence(0.7),
            scope,
            resting_activation: p::RestingActivation(0.0),
            recency,
            provenance: provenance(p::TrustTier::VerifiedProcess),
        }
    }

    #[test]
    fn graph_projection_rebuilds_from_events_while_activation_stays_dormant() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let run = p::RunId("memory:graph-test".into());
        let memory = EventSourcedMemory::with_clock(store.clone(), run.clone(), || 100).unwrap();
        let project = scope("project-a", TimeSlice::Daily);
        let other = scope("project-b", TimeSlice::Daily);
        memory
            .add_node(node("node-a", project.clone(), 10))
            .unwrap();
        memory
            .add_node(node("node-b", project.clone(), 20))
            .unwrap();
        memory.add_node(node("node-c", other, 30)).unwrap();
        memory
            .add_edge(MemoryEdge {
                schema_version: p::SchemaVersion(1),
                id: p::EdgeId("edge-a-b".into()),
                from: p::NodeId("node-a".into()),
                to: p::NodeId("node-b".into()),
                kind: EdgeKind::Temporal,
                weight: p::Weight(0.5),
                provenance: provenance(p::TrustTier::VerifiedProcess),
            })
            .unwrap();

        assert_eq!(
            memory
                .timeline(
                    project.clone(),
                    TimeWindow {
                        schema_version: p::SchemaVersion(1),
                        starts_at: 0,
                        ends_at: 25,
                    },
                )
                .len(),
            2
        );
        assert_eq!(
            memory.recall(
                RecallCue {
                    schema_version: p::SchemaVersion(1),
                    scope: project.clone(),
                    query: "node".into(),
                },
                1,
            )[0]
            .node,
            p::NodeId("node-b".into())
        );
        memory.seed(&[p::NodeId("node-a".into())], ActivationShape::Association);
        assert!(memory
            .spread(ActivationBudget {
                schema_version: p::SchemaVersion(1),
                max_hops: 2,
                top_k_frontier: 4,
                per_tick: 8,
            })
            .is_empty());
        assert!(memory
            .set_edge_weight(p::EdgeId("edge-a-b".into()), 0.8)
            .is_err());

        let rebuilt = EventSourcedMemory::with_clock(store.clone(), run.clone(), || 200).unwrap();
        assert_eq!(
            rebuilt.query(GraphQuery {
                scope: Some(project),
                ..GraphQuery::default()
            }),
            memory.query(GraphQuery {
                scope: Some(scope("project-a", TimeSlice::Daily)),
                ..GraphQuery::default()
            })
        );
        let sequences = store
            .read_run(run)
            .collect::<p::Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|event| event.stream_seq)
            .collect::<Vec<_>>();
        assert_eq!(sequences, vec![1, 2, 3, 4]);
    }

    #[test]
    fn candidate_store_emits_every_required_lifecycle_event() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let run = p::RunId("memory:candidate-test".into());
        let memory = EventSourcedMemory::with_clock(store.clone(), run.clone(), || 100).unwrap();
        let id = CandidateStore::create(
            &memory,
            p::CandidateUpdate {
                schema_version: p::SchemaVersion(1),
            },
        )
        .unwrap();
        assert!(memory
            .transition(id.clone(), CandidateState::Promoted, p::Actor::System)
            .is_err());
        memory
            .transition(id, CandidateState::Promoted, p::Actor::Owner)
            .unwrap();
        let kinds = store
            .read_run(run)
            .collect::<p::Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                p::EventKind::CandidateCreated,
                p::EventKind::CandidatePromoted
            ]
        );
    }

    #[test]
    fn intention_claim_lease_survives_rebuild_and_resolution_is_idempotent() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let run = p::RunId("memory:intention-test".into());
        let clock = Arc::new(AtomicI64::new(100));
        let first_clock = clock.clone();
        let memory = EventSourcedMemory::with_clock(store.clone(), run.clone(), move || {
            first_clock.load(Ordering::SeqCst)
        })
        .unwrap();
        let id = p::IntentionId("intention:review".into());
        IntentionStore::create(
            &memory,
            ProspectiveIntention {
                schema_version: p::SchemaVersion(1),
                id: id.clone(),
                source: p::IntentionSource::Commitment,
                trigger: IntentionTrigger::At(100),
                state: IntentionState::Pending,
                seed: p::SeedRef("review".into()),
                provenance: provenance(p::TrustTier::OwnerInput),
                expires_at: None,
            },
        )
        .unwrap();
        assert_eq!(memory.claim_due(100, 100).len(), 1);
        assert!(memory.claim_due(150, 100).is_empty());

        clock.store(150, Ordering::SeqCst);
        let rebuilt_clock = clock.clone();
        let rebuilt = EventSourcedMemory::with_clock(store, run, move || {
            rebuilt_clock.load(Ordering::SeqCst)
        })
        .unwrap();
        assert!(rebuilt.claim_due(199, 100).is_empty());
        let reclaimed = rebuilt.claim_due(200, 100);
        assert_eq!(reclaimed.len(), 1);
        assert_eq!(reclaimed[0].intention.seed, p::SeedRef("review".into()));
        rebuilt.resolve(id.clone(), IntentionOutcome::Done).unwrap();
        rebuilt.resolve(id, IntentionOutcome::Done).unwrap();
        assert!(rebuilt.claim_due(500, 100).is_empty());
    }

    #[test]
    fn scheduled_intention_binding_and_lease_survive_rebuild() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let run = p::RunId("memory:scheduled-intention-test".into());
        let clock = Arc::new(AtomicI64::new(100));
        let first_clock = clock.clone();
        let memory = EventSourcedMemory::with_clock(store.clone(), run.clone(), move || {
            first_clock.load(Ordering::SeqCst)
        })
        .unwrap();
        let command = p::ScheduleCommand {
            schema_version: p::SchemaVersion(1),
            intention: p::ProspectiveIntention {
                schema_version: p::SchemaVersion(1),
                id: p::IntentionId("intention:scheduled-rebuild".into()),
                source: p::IntentionSource::Commitment,
                trigger: p::IntentionTrigger::At(100),
                state: p::IntentionState::Pending,
                seed: p::SeedRef("review scheduled evidence | exactly".into()),
                provenance: provenance(p::TrustTier::OwnerInput),
                expires_at: Some(300),
            },
            session: p::SessionId("session:scheduled-rebuild".into()),
            envelope: p::AutonomyEnvelope {
                schema_version: p::SchemaVersion(1),
                scope: p::Scope("workspace:scheduled-rebuild".into()),
                capability: p::CapabilitySet {
                    schema_version: p::SchemaVersion(1),
                    capabilities: vec![p::CapabilityRef("capability:local-notification".into())],
                    permissions: vec![p::PermissionRef("permission:local-notification".into())],
                },
                action_type: vec![p::ActionType::Deliver],
                risk_limit: p::Risk::Low,
                approval_rule: p::ApprovalRule::Allow,
                budget: p::Budget("units:2".into()),
                timebox: p::Timebox {
                    schema_version: p::SchemaVersion(1),
                    starts_at: 90,
                    expires_at: 400,
                    max_turns: 2,
                },
                rollback: p::RollbackReq {
                    schema_version: p::SchemaVersion(1),
                    required: false,
                    boundary: None,
                },
            },
            budget: p::Budget("units:1".into()),
        };
        command.validate().unwrap();
        assert_eq!(
            memory.create_schedule(command.clone()).unwrap(),
            command.intention.id
        );
        assert_eq!(memory.list_scheduled()[0].command, command);
        let contender = EventSourcedMemory::with_clock(store.clone(), run.clone(), || 100).unwrap();
        let (first_claim, competing_claim) = std::thread::scope(|scope| {
            let first = scope.spawn(|| memory.claim_due_scheduled(100, 50, 1));
            let second = scope.spawn(|| contender.claim_due_scheduled(100, 50, 1));
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(first_claim.len() + competing_claim.len(), 1);
        let claimed = first_claim
            .first()
            .or_else(|| competing_claim.first())
            .unwrap();
        assert_eq!(claimed.lease_until, 150);

        clock.store(125, Ordering::SeqCst);
        let rebuilt_clock = clock.clone();
        let rebuilt = EventSourcedMemory::with_clock(store, run, move || {
            rebuilt_clock.load(Ordering::SeqCst)
        })
        .unwrap();
        let rebuilt_record = &rebuilt.list_scheduled()[0];
        assert_eq!(rebuilt_record.command.binding(), command.binding());
        assert_eq!(
            rebuilt_record.command.intention.seed,
            command.intention.seed
        );
        assert_eq!(
            rebuilt_record.command.intention.expires_at,
            command.intention.expires_at
        );
        assert_eq!(
            rebuilt_record.command.intention.state,
            p::IntentionState::Fired
        );
        assert_eq!(rebuilt_record.lease_until, Some(150));
        assert_eq!(
            rebuilt.create_schedule(command.clone()).unwrap(),
            command.intention.id
        );
        assert!(rebuilt.claim_due_scheduled(149, 50, 1).is_empty());
        let reclaimed = rebuilt.claim_due_scheduled(150, 50, 1);
        assert_eq!(reclaimed.len(), 1);
        assert_eq!(
            reclaimed[0].scheduled_claim().unwrap().command.binding(),
            command.binding()
        );
    }

    #[test]
    fn s8_user_candidate_is_complete_and_imported_history_never_targets_stable() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let run = p::RunId("memory:user-model-test".into());
        let memory = EventSourcedMemory::with_clock(store.clone(), run.clone(), || 100).unwrap();
        let id = memory.reserve_candidate_id();
        memory
            .create_user_attribute_candidate(UserAttributeCandidate {
                schema_version: p::SchemaVersion(1),
                candidate_id: id.clone(),
                attribute: p::UserAttributeRef("review-style".into()),
                value: p::UserAttributeValueRef("rigorous".into()),
                evidence: vec![p::EvidenceRef("event:1".into())],
                confidence: p::Confidence(0.9),
                first_observed_at: 10,
                last_updated_at: 20,
                stability: p::StabilityTier::Stable,
                scope: p::Scope("workspace:test".into()),
                conflicts: Vec::new(),
                feedback: vec![p::FeedbackRef("owner-confirmed".into())],
                provenance: provenance(p::TrustTier::OwnerInput),
                evidence_priority: EvidencePriority::Process,
            })
            .unwrap();
        memory
            .import_historical(ImportedHistoricalEvidence {
                schema_version: p::SchemaVersion(1),
                source: p::HistoricalSourceRef("archive:notes".into()),
                low_weight: true,
                bootstrap_only: true,
                provenance: provenance(p::TrustTier::ApprovedSource),
            })
            .unwrap();
        memory
            .transition(id, CandidateState::Promoted, p::Actor::Owner)
            .unwrap();
        assert_eq!(
            memory
                .user_attribute(
                    &p::UserAttributeRef("review-style".into()),
                    &p::Scope("workspace:test".into()),
                )
                .unwrap()
                .value,
            p::UserAttributeValueRef("rigorous".into())
        );
        assert_eq!(memory.imported_history().len(), 1);
        assert_eq!(
            TimeScaleTierMap.tier(TimeScale::Session),
            p::StabilityTier::Session
        );
        assert_eq!(
            TimeScaleTierMap.tier(TimeScale::Repeated),
            p::StabilityTier::Stable
        );
        let kinds = store
            .read_run(run)
            .collect::<p::Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                p::EventKind::UserAttributeCandidateCreated,
                p::EventKind::CandidateCreated,
                p::EventKind::ImportedHistoricalEvidenceRecorded,
                p::EventKind::CandidatePromoted,
            ]
        );
    }

    #[test]
    fn s37_scoped_memory_and_candidate_review_never_pollute_broader_stable_state() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let run = p::RunId("memory:m1c-scoped-review".into());
        let memory = EventSourcedMemory::with_clock(store.clone(), run.clone(), || 100).unwrap();
        let project = scope("project-a", TimeSlice::Daily);
        let session = scope("project-a/session-1", TimeSlice::Session);
        let other = scope("project-b/session-1", TimeSlice::Session);
        memory
            .add_node(node("session-note", session.clone(), 30))
            .unwrap();
        memory
            .add_node(node("project-note", project.clone(), 20))
            .unwrap();
        memory.add_node(node("other-note", other, 40)).unwrap();

        let hits = memory
            .search_scoped(ScopedMemoryQuery {
                schema_version: p::SchemaVersion(1),
                requester: project.clone(),
                target: session.clone(),
                query: "session-note".into(),
                limit: 100,
            })
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reference.node, p::NodeId("session-note".into()));
        assert_eq!(hits[0].reference.scope, session);
        assert!(memory
            .search_scoped(ScopedMemoryQuery {
                schema_version: p::SchemaVersion(1),
                requester: scope("project-a/session-1", TimeSlice::Session),
                target: project.clone(),
                query: String::new(),
                limit: 1,
            })
            .is_err());

        let topic_id = memory.reserve_candidate_id();
        memory
            .create_topic_candidate(TopicMemoryCandidate {
                schema_version: p::SchemaVersion(1),
                id: topic_id.clone(),
                topic: "release-review".into(),
                scope: project.clone(),
                summary: p::SummaryRef("summary:release-review".into()),
                evidence: vec![p::EvidenceRef("event:release-review".into())],
                confidence: p::Confidence(0.7),
                provenance: provenance(p::TrustTier::VerifiedProcess),
            })
            .unwrap();
        assert_eq!(
            memory.candidate_record(&topic_id).unwrap().state,
            CandidateState::Candidate
        );

        let user_id = memory.reserve_candidate_id();
        let session_scope = p::Scope("project-a/session-1".into());
        memory
            .create_user_attribute_candidate(UserAttributeCandidate {
                schema_version: p::SchemaVersion(1),
                candidate_id: user_id.clone(),
                attribute: p::UserAttributeRef("review-depth".into()),
                value: p::UserAttributeValueRef("detailed".into()),
                evidence: vec![p::EvidenceRef("event:session-review".into())],
                confidence: p::Confidence(0.8),
                first_observed_at: 10,
                last_updated_at: 20,
                stability: p::StabilityTier::Stable,
                scope: session_scope.clone(),
                conflicts: vec![p::CandidateId("candidate:conflicting-review".into())],
                feedback: Vec::new(),
                provenance: provenance(p::TrustTier::OwnerInput),
                evidence_priority: EvidencePriority::Process,
            })
            .unwrap();
        assert_eq!(memory.user_candidates_in_scope(&session_scope).len(), 1);
        assert!(memory
            .transition(user_id.clone(), CandidateState::Promoted, p::Actor::System)
            .is_err());
        let project_scope = p::Scope("project-a".into());
        let global_scope = p::Scope("*".into());
        for broader in [&project_scope, &global_scope] {
            assert!(memory
                .user_attribute(&p::UserAttributeRef("review-depth".into()), broader)
                .is_none());
        }
        memory
            .transition(user_id, CandidateState::Promoted, p::Actor::Owner)
            .unwrap();
        assert!(memory
            .user_attribute(&p::UserAttributeRef("review-depth".into()), &session_scope,)
            .is_some());
        for broader in [&project_scope, &global_scope] {
            assert!(memory
                .user_attribute(&p::UserAttributeRef("review-depth".into()), broader)
                .is_none());
        }

        let kinds = store
            .read_run(run)
            .collect::<p::Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| matches!(
                    kind,
                    p::EventKind::CandidateCreated
                        | p::EventKind::UserAttributeCandidateCreated
                        | p::EventKind::CandidatePromoted
                ))
                .copied()
                .collect::<Vec<_>>(),
            vec![
                p::EventKind::CandidateCreated,
                p::EventKind::UserAttributeCandidateCreated,
                p::EventKind::CandidateCreated,
                p::EventKind::CandidatePromoted,
            ]
        );
    }
}
