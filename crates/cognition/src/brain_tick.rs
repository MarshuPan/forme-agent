//! The owner-brain cognitive tick (prd/23 B1, architecture/09 §5): one ordered
//! tick of three phases over a fixed event range. The engine that performs the
//! phases lives with the runtime that governs impulses; this module owns the
//! typed contract and the deterministic rules the phases apply, so a retried
//! window derives exactly the reflections its first attempt derived.

use std::collections::{BTreeMap, BTreeSet};

use forme_protocol as p;
use forme_store::{CognitiveTickClaim, TickEventCursor, TickEventRange};

use crate::{
    CognitionSnapshot, CognitiveMapUpdateProposal, CommunicationPurpose, DeliveryMode,
    ImpulseSource, InterventionLevel, MapScope, MapUpdateKind, Observation, ProposalIntent,
};

pub const COGNITIVE_TICK_REPORT_SCHEMA_VERSION: p::SchemaVersion = p::SchemaVersion(1);
/// Failure events one window observes, most pressing first. The rest still
/// reach reflection through their groups.
pub const MAX_TICK_FAILURES: usize = 16;
/// Evidence references one failure group carries into its reflection.
pub const MAX_TICK_GROUP_EVIDENCE: usize = 32;
/// A tick reflection starts as a low-confidence candidate; only governance
/// with further evidence can raise it.
const TICK_REFLECTION_CONFIDENCE: f32 = 0.3;
const MAX_STATEMENT_SCOPE_CHARS: usize = 96;
const OWNER_SCOPE: &str = "owner";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CognitiveTickPhase {
    ObserveAndPropose,
    ReflectAndCandidate,
    MaintainMemory,
}

impl CognitiveTickPhase {
    /// The only order a tick runs its phases in.
    pub const ORDER: [Self; 3] = [
        Self::ObserveAndPropose,
        Self::ReflectAndCandidate,
        Self::MaintainMemory,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ObserveAndPropose => "observe-and-propose",
            Self::ReflectAndCandidate => "reflect-and-candidate",
            Self::MaintainMemory => "maintain-memory",
        }
    }

    /// Heavy phases yield to a foreground session. Observation of due
    /// commitments is deterministic and keeps running.
    pub const fn is_heavy(self) -> bool {
        !matches!(self, Self::ObserveAndPropose)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CognitiveTickPhaseStatus {
    /// The phase produced or confirmed outputs.
    Ran,
    /// The phase had nothing to act on and appended nothing.
    NoChange,
    /// The phase yielded to the foreground; its inputs wait for a later window.
    Deferred,
    /// An earlier phase failed, so this phase did not run.
    Skipped,
    Failed,
}

impl CognitiveTickPhaseStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ran => "ran",
            Self::NoChange => "no-change",
            Self::Deferred => "deferred",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveTickPhaseReport {
    pub phase: CognitiveTickPhase,
    pub status: CognitiveTickPhaseStatus,
    /// Events this attempt appended for the phase, in append order.
    pub event_refs: Vec<p::EventId>,
    pub model_calls: u32,
    pub deferred_reason: Option<p::ReasonRef>,
    /// Why a phase failed or was skipped; never set for a phase that ran.
    pub failure: Option<p::ReasonRef>,
}

impl CognitiveTickPhaseReport {
    pub fn completed(phase: CognitiveTickPhase, ran: bool, event_refs: Vec<p::EventId>) -> Self {
        Self {
            phase,
            status: if ran {
                CognitiveTickPhaseStatus::Ran
            } else {
                CognitiveTickPhaseStatus::NoChange
            },
            event_refs,
            model_calls: 0,
            deferred_reason: None,
            failure: None,
        }
    }

    pub fn deferred(phase: CognitiveTickPhase, reason: impl Into<String>) -> Self {
        Self {
            phase,
            status: CognitiveTickPhaseStatus::Deferred,
            event_refs: Vec::new(),
            model_calls: 0,
            deferred_reason: Some(p::ReasonRef(reason.into())),
            failure: None,
        }
    }

    pub fn skipped(phase: CognitiveTickPhase, reason: impl Into<String>) -> Self {
        Self {
            phase,
            status: CognitiveTickPhaseStatus::Skipped,
            event_refs: Vec::new(),
            model_calls: 0,
            deferred_reason: None,
            failure: Some(p::ReasonRef(reason.into())),
        }
    }

    /// A failed phase keeps the events it appended before failing, so the
    /// report never hides a partial write.
    pub fn failed(
        phase: CognitiveTickPhase,
        event_refs: Vec<p::EventId>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            phase,
            status: CognitiveTickPhaseStatus::Failed,
            event_refs,
            model_calls: 0,
            deferred_reason: None,
            failure: Some(p::ReasonRef(reason.into())),
        }
    }
}

/// One failure event inside a window's range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickFailure {
    /// The event that recorded the failure.
    pub source: p::EventId,
    pub scope: p::Scope,
    pub class: String,
    pub impact: p::Impact,
    /// An action whose side effect is unknown awaits an owner ruling; it is
    /// surfaced first and never retried by the tick.
    pub outcome_unknown: bool,
    pub evidence: p::EvidenceRef,
}

impl TickFailure {
    pub fn seed(&self) -> p::NodeId {
        p::NodeId(format!("failure-event:{}", self.source.0))
    }

    /// The Tension observation phase 1 raises for this failure. Background
    /// findings ride along with the next exchange; they never interrupt the
    /// owner.
    pub fn observation(&self) -> Observation {
        let (estimated_value, urgency) = if self.outcome_unknown {
            (90, 90)
        } else {
            let value = match self.impact {
                p::Impact::High => 80,
                p::Impact::Medium => 60,
                p::Impact::Low => 30,
            };
            (value, 50)
        };
        Observation {
            schema_version: p::SchemaVersion(1),
            source: p::Source::Internal,
            scope: self.scope.clone(),
            grant_ref: None,
            authorized: true,
            seed: vec![self.seed()],
            signal: ImpulseSource::Tension,
            estimated_value,
            urgency,
            requested_level: InterventionLevel::L1Suggest,
            delivery: DeliveryMode::Hitchhike,
            proposal_intent: ProposalIntent::Communication(CommunicationPurpose::Insight),
        }
    }

    fn pressing_first(&self, other: &Self) -> std::cmp::Ordering {
        other
            .outcome_unknown
            .cmp(&self.outcome_unknown)
            .then_with(|| impact_rank(other.impact).cmp(&impact_rank(self.impact)))
            .then_with(|| self.source.0.cmp(&other.source.0))
    }
}

/// Failures of one class in one scope, observed inside a single window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickFailureGroup {
    pub scope: p::Scope,
    pub class: String,
    pub impact: p::Impact,
    pub outcome_unknown: bool,
    /// Evidence in range order, bounded by `MAX_TICK_GROUP_EVIDENCE`.
    pub evidence: Vec<p::EvidenceRef>,
    /// Every failure the group saw, including evidence beyond the bound.
    pub count: usize,
}

impl TickFailureGroup {
    fn key(&self) -> String {
        format!("{}|{}", escape(&self.scope.0), self.class)
    }

    /// Identity of the group's inputs: the same failures always derive the
    /// same candidate.
    pub fn digest(&self) -> String {
        let mut material = self.key();
        for evidence in &self.evidence {
            material.push('|');
            material.push_str(&escape(&evidence.0));
        }
        short_digest(material.as_bytes())
    }

    pub fn candidate_id(&self) -> p::CandidateId {
        p::CandidateId(format!("candidate:tick-reflection:{}", self.digest()))
    }

    /// The blind-spot reflection phase 2 proposes. It is only a candidate:
    /// promotion stays with governance and never happens inside a tick.
    pub fn reflection(&self) -> CognitiveMapUpdateProposal {
        let scope_label = self
            .scope
            .0
            .chars()
            .take(MAX_STATEMENT_SCOPE_CHARS)
            .collect::<String>();
        CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: Some(self.candidate_id()),
            scope: MapScope {
                schema_version: p::SchemaVersion(1),
                scope: self.scope.clone(),
            },
            kind: MapUpdateKind::BlindSpot,
            confidence: p::Confidence(TICK_REFLECTION_CONFIDENCE),
            evidence: self.evidence.clone(),
            frame: None,
            quality: None,
            blindspot: Some(p::BlindSpotModelRef(format!(
                "blindspot:{}:{}",
                self.scope.0, self.class
            ))),
            resource: None,
            reflection_inputs: self.evidence.clone(),
            statement: format!(
                "{} failures in {scope_label} reveal a condition the plan did not anticipate",
                self.class
            ),
            provenance: crate::system_provenance(),
        }
    }
}

/// What one window's event range says, reduced to the signals the phases act
/// on. Collection is deterministic for a given range.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickSignals {
    /// Events in the range.
    pub events: usize,
    /// Terminal run outcomes in the range.
    pub outcomes: usize,
    /// Failure events, most pressing first, bounded by `MAX_TICK_FAILURES`.
    pub failures: Vec<TickFailure>,
    /// Failure events beyond the bound; they still reach reflection through
    /// their groups.
    pub unobserved_failures: usize,
    /// Every failure grouped by scope and class, most pressing first.
    pub groups: Vec<TickFailureGroup>,
}

impl TickSignals {
    /// Collects signals from range events. Callers pass runs in ascending id
    /// order and each run by `stream_seq`, the only ordering authority.
    pub fn collect<'a>(events: impl IntoIterator<Item = &'a p::Event>) -> Self {
        let mut collector = TickSignalCollector::default();
        for event in events {
            collector.observe(event);
        }
        collector.finish()
    }

    pub fn is_empty(&self) -> bool {
        self.events == 0
    }
}

/// Streams range events into `TickSignals` without holding the range.
#[derive(Debug, Default)]
pub struct TickSignalCollector {
    events: usize,
    outcomes: usize,
    failures: Vec<TickFailure>,
    /// Failure refs a recorded failure evidence already represents.
    recorded: BTreeSet<String>,
    /// Action failures, kept until the whole range is seen: one that also
    /// has a recorded failure evidence is represented once, by the richer
    /// recorded evidence.
    action_failures: Vec<(p::EventId, p::FailureEvidenceRef)>,
}

impl TickSignalCollector {
    pub fn observe(&mut self, event: &p::Event) {
        self.events += 1;
        let source = event.event_id.clone();
        let event_evidence = || p::EvidenceRef(format!("event:{}", event.event_id.0));
        match &event.payload {
            p::EventPayload::RunComplete(_)
            | p::EventPayload::RunAborted(_)
            | p::EventPayload::RunLimited(_) => self.outcomes += 1,
            p::EventPayload::RunFailed(_) => {
                self.outcomes += 1;
                self.add(
                    source,
                    owner_scope(),
                    "run-failed",
                    p::Impact::Medium,
                    false,
                    event_evidence(),
                );
            }
            p::EventPayload::ActionFailed(payload) => {
                self.action_failures
                    .push((source, payload.failure_ref.clone()));
            }
            p::EventPayload::ActionOutcomeUnknown(_) => self.add(
                source,
                owner_scope(),
                "outcome-unknown",
                p::Impact::High,
                true,
                event_evidence(),
            ),
            // Only a failed verdict is a failure; an unverifiable outcome
            // means no done contract applied, not that the work failed.
            p::EventPayload::VerificationFinished(payload)
                if payload.outcome == p::VerificationOutcome::Fail =>
            {
                self.add(
                    source,
                    owner_scope(),
                    "verification-failed",
                    p::Impact::Medium,
                    false,
                    event_evidence(),
                );
            }
            p::EventPayload::FailureEvidenceRecorded(payload) => {
                self.recorded.insert(payload.failure_ref.0.clone());
                self.add(
                    source,
                    payload.scope.clone(),
                    &format!("failure:{}", failure_class_label(payload.class)),
                    payload.impact,
                    false,
                    p::EvidenceRef(payload.failure_ref.0.clone()),
                );
            }
            _ => {}
        }
    }

    pub fn finish(mut self) -> TickSignals {
        for (source, failure) in std::mem::take(&mut self.action_failures) {
            if !self.recorded.contains(&failure.0) {
                self.add(
                    source,
                    owner_scope(),
                    "action-failed",
                    p::Impact::Medium,
                    false,
                    p::EvidenceRef(failure.0),
                );
            }
        }
        let mut groups = BTreeMap::<(String, String), TickFailureGroup>::new();
        for failure in &self.failures {
            let group = groups
                .entry((failure.scope.0.clone(), failure.class.clone()))
                .or_insert_with(|| TickFailureGroup {
                    scope: failure.scope.clone(),
                    class: failure.class.clone(),
                    impact: failure.impact,
                    outcome_unknown: failure.outcome_unknown,
                    evidence: Vec::new(),
                    count: 0,
                });
            group.count += 1;
            if impact_rank(failure.impact) > impact_rank(group.impact) {
                group.impact = failure.impact;
            }
            group.outcome_unknown |= failure.outcome_unknown;
            if group.evidence.len() < MAX_TICK_GROUP_EVIDENCE
                && !group.evidence.contains(&failure.evidence)
            {
                group.evidence.push(failure.evidence.clone());
            }
        }
        let mut groups = groups.into_values().collect::<Vec<_>>();
        groups.sort_by(|left, right| {
            right
                .outcome_unknown
                .cmp(&left.outcome_unknown)
                .then_with(|| impact_rank(right.impact).cmp(&impact_rank(left.impact)))
                .then_with(|| left.key().cmp(&right.key()))
        });
        let mut failures = self.failures;
        failures.sort_by(TickFailure::pressing_first);
        let unobserved_failures = failures.len().saturating_sub(MAX_TICK_FAILURES);
        failures.truncate(MAX_TICK_FAILURES);
        TickSignals {
            events: self.events,
            outcomes: self.outcomes,
            failures,
            unobserved_failures,
            groups,
        }
    }

    fn add(
        &mut self,
        source: p::EventId,
        scope: p::Scope,
        class: &str,
        impact: p::Impact,
        outcome_unknown: bool,
        evidence: p::EvidenceRef,
    ) {
        self.failures.push(TickFailure {
            source,
            scope,
            class: class.to_owned(),
            impact,
            outcome_unknown,
            evidence,
        });
    }
}

/// Everything one claimed window runs on. The range is fixed by the claim, so
/// every attempt of the window reads the same inputs.
#[derive(Debug, Clone)]
pub struct CognitiveTickInput {
    pub schema_version: p::SchemaVersion,
    pub claim: CognitiveTickClaim,
    /// The stream the tick appends its own events to.
    pub run: p::RunId,
    pub signals: TickSignals,
    /// Stable cognition reconstructed at the aggregate head in the claim.
    pub stable_objects: Vec<crate::StableCognition>,
    /// Competence and time the phases read; observations are derived here.
    pub snapshot: CognitionSnapshot,
    pub foreground_active: bool,
    pub now: p::Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CognitiveTickReport {
    pub schema_version: p::SchemaVersion,
    pub claim: CognitiveTickClaim,
    pub run: p::RunId,
    /// One report per phase, in `CognitiveTickPhase::ORDER`.
    pub phases: Vec<CognitiveTickPhaseReport>,
    pub event_range: TickEventRange,
    /// What finishing the window consumes: the whole range, or only its start
    /// when heavy phases deferred, so their inputs wait for a later window.
    pub consumed: TickEventCursor,
    pub digest: p::SchemaDigest,
}

impl CognitiveTickReport {
    /// Seals a report. Phases must be exactly the three phases in order.
    pub fn seal(
        claim: CognitiveTickClaim,
        run: p::RunId,
        phases: Vec<CognitiveTickPhaseReport>,
    ) -> p::Result<Self> {
        if phases.iter().map(|report| report.phase).collect::<Vec<_>>() != CognitiveTickPhase::ORDER
        {
            return Err(p::Error(
                "a cognitive tick report must list its three phases in order".into(),
            ));
        }
        let deferred = phases
            .iter()
            .any(|report| report.status == CognitiveTickPhaseStatus::Deferred);
        let consumed = if deferred {
            claim.range.from.clone()
        } else {
            claim.range.to.clone()
        };
        let event_range = claim.range.clone();
        let digest = report_digest(&claim, &run, &phases, &consumed);
        Ok(Self {
            schema_version: COGNITIVE_TICK_REPORT_SCHEMA_VERSION,
            claim,
            run,
            phases,
            event_range,
            consumed,
            digest,
        })
    }

    pub fn phase(&self, phase: CognitiveTickPhase) -> Option<&CognitiveTickPhaseReport> {
        self.phases.iter().find(|report| report.phase == phase)
    }

    pub fn deferred(&self) -> bool {
        self.phases
            .iter()
            .any(|report| report.status == CognitiveTickPhaseStatus::Deferred)
    }

    pub fn failed(&self) -> bool {
        self.phases
            .iter()
            .any(|report| report.status == CognitiveTickPhaseStatus::Failed)
    }

    pub fn model_calls(&self) -> u32 {
        self.phases
            .iter()
            .map(|report| report.model_calls)
            .fold(0, u32::saturating_add)
    }

    pub fn event_refs(&self) -> impl Iterator<Item = &p::EventId> {
        self.phases
            .iter()
            .flat_map(|report| report.event_refs.iter())
    }
}

/// Runs one claimed window's three phases in order: observe and propose,
/// reflect into candidates, maintain memory. Implementations append only
/// through the store's single writer and never start an action, change
/// stable cognition directly, or take a foreground session lock.
pub trait CognitiveTickEngine {
    fn run(&self, input: CognitiveTickInput) -> p::Result<CognitiveTickReport>;
}

fn report_digest(
    claim: &CognitiveTickClaim,
    run: &p::RunId,
    phases: &[CognitiveTickPhaseReport],
    consumed: &TickEventCursor,
) -> p::SchemaDigest {
    let mut encoded = Vec::new();
    push_field(&mut encoded, b"forme-cognitive-tick-report-v1");
    push_field(&mut encoded, claim.owner_digest.0.as_bytes());
    push_field(&mut encoded, &claim.window.to_le_bytes());
    push_field(&mut encoded, &claim.attempt.to_le_bytes());
    push_field(&mut encoded, run.0.as_bytes());
    for cursor in [&claim.range.from, &claim.range.to, consumed] {
        push_field(&mut encoded, &(cursor.0.len() as u64).to_le_bytes());
        for (stream, position) in &cursor.0 {
            push_field(&mut encoded, stream.0.as_bytes());
            push_field(&mut encoded, &position.to_le_bytes());
        }
    }
    for report in phases {
        push_field(&mut encoded, report.phase.as_str().as_bytes());
        push_field(&mut encoded, report.status.as_str().as_bytes());
        push_field(&mut encoded, &report.model_calls.to_le_bytes());
        push_field(
            &mut encoded,
            &(report.event_refs.len() as u64).to_le_bytes(),
        );
        for event in &report.event_refs {
            push_field(&mut encoded, event.0.as_bytes());
        }
        for reason in [&report.deferred_reason, &report.failure] {
            match reason {
                Some(reason) => {
                    encoded.push(1);
                    push_field(&mut encoded, reason.0.as_bytes());
                }
                None => encoded.push(0),
            }
        }
    }
    p::sha256_content_digest(&encoded)
}

fn push_field(encoded: &mut Vec<u8>, value: &[u8]) {
    encoded.extend_from_slice(&(value.len() as u64).to_le_bytes());
    encoded.extend_from_slice(value);
}

fn short_digest(material: &[u8]) -> String {
    let digest = p::sha256_content_digest(material);
    let hex = digest.0.strip_prefix("sha256:").unwrap_or(&digest.0);
    hex.chars().take(32).collect()
}

fn escape(value: &str) -> String {
    value.replace('%', "%25").replace('|', "%7C")
}

fn owner_scope() -> p::Scope {
    p::Scope(OWNER_SCOPE.into())
}

fn impact_rank(impact: p::Impact) -> u8 {
    match impact {
        p::Impact::Low => 0,
        p::Impact::Medium => 1,
        p::Impact::High => 2,
    }
}

fn failure_class_label(class: p::FailureClass) -> &'static str {
    match class {
        p::FailureClass::GoalFramingFailure => "goal-framing",
        p::FailureClass::ContextFailure => "context",
        p::FailureClass::CognitiveMapFailure => "cognitive-map",
        p::FailureClass::ResourceSelectionFailure => "resource-selection",
        p::FailureClass::ExecutionFailure => "execution",
        p::FailureClass::VerificationFailure => "verification",
        p::FailureClass::TrustFailure => "trust",
        p::FailureClass::ProactivityFailure => "proactivity",
        p::FailureClass::LearningFailure => "learning",
        p::FailureClass::HandoffFailure => "handoff",
        p::FailureClass::SelfEvalTrap => "self-eval-trap",
        p::FailureClass::SafetyPolicyFailure => "safety-policy",
        p::FailureClass::MemoryMisevolution => "memory-misevolution",
    }
}
