//! Deterministic verification, failure evidence, digests, and trace export (prd/15).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use forme_protocol as p;
use forme_store::EventStore;

mod m3_a;
mod m3_b;
mod m3_c;
mod m4;
mod m5;

pub use m3_a::{
    portable_replay_from_events, ArtifactReceipt, DeterministicEvolutionEvaluator,
    DeterministicReplayEngine, EvolutionEvaluator, M3ArtifactKind, M3ArtifactStore,
    PortableActorClass, PortableEventEnvelope, PortableReplayBundle, PortableSessionDelta,
    PortableTranscriptDelta, ReplayEngine, ReplayEventRecord, TraceManifest,
};
pub use m3_b::{
    LongHorizonArtifactBody, LongHorizonArtifactCheckpoint, LongHorizonArtifactDisposition,
    M3BArtifactReceipt, M3BArtifactStore, PortableLongHorizonArtifact,
};
pub use m3_c::{
    M3CArtifactReceipt, M3CArtifactStore, M3CControlEventTrace, M3CGoldenArtifactBody,
    M3CGovernedActionTrace, M3CGovernedRunTrace, PortableM3CGoldenArtifact,
};
pub use m4::{
    FederationArtifactBundle, FederationArtifactReceipt, FederationArtifactStore,
    PortableFederationArtifact, RemoteAuthorityVerifier, RemoteGroundTruth,
};
pub use m5::{
    M5AdmissionArtifact, M5AdmissionEvidence, M5ApprovalEvidence, M5ArtifactBundle,
    M5ArtifactReceipt, M5ArtifactStore, M5DistributionArtifact, M5DistributionReceiptEvidence,
    M5ExecutorRecordEvidence, M5InstallArtifact, M5PublisherArtifact, M5PublisherGrantEvidence,
    M5TraceArtifact, PortableM5Artifact,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionEvidence {
    pub schema_version: p::SchemaVersion,
    pub result_ref: p::ActionResultRef,
    pub succeeded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEvidence {
    pub schema_version: p::SchemaVersion,
    pub event_id: p::EventId,
    pub kind: p::EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvidence {
    pub schema_version: p::SchemaVersion,
    pub path: String,
    pub digest: Option<String>,
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceBundle {
    pub schema_version: p::SchemaVersion,
    pub action_results: Vec<ActionEvidence>,
    pub events: Vec<EventEvidence>,
    pub file_states: Vec<FileEvidence>,
    pub final_output: Option<String>,
    pub failures: Vec<FailureEvidence>,
    pub independent_verification: bool,
}

impl Default for EvidenceBundle {
    fn default() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            action_results: Vec::new(),
            events: Vec::new(),
            file_states: Vec::new(),
            final_output: None,
            failures: Vec::new(),
            independent_verification: false,
        }
    }
}

impl EvidenceBundle {
    pub fn from_events(events: &[p::Event]) -> Self {
        let mut bundle = Self {
            events: events
                .iter()
                .map(|event| EventEvidence {
                    schema_version: event.schema_version,
                    event_id: event.event_id.clone(),
                    kind: event.kind,
                })
                .collect(),
            independent_verification: events.iter().any(|event| {
                matches!(
                    event.kind,
                    p::EventKind::ActionCompleted | p::EventKind::VerificationFinished
                )
            }),
            ..Self::default()
        };
        for event in events {
            match &event.payload {
                p::EventPayload::ActionCompleted(payload) => {
                    bundle.action_results.push(ActionEvidence {
                        schema_version: event.schema_version,
                        result_ref: payload.result_ref.clone(),
                        succeeded: true,
                    });
                }
                p::EventPayload::ActionFailed(payload) => {
                    bundle.action_results.push(ActionEvidence {
                        schema_version: event.schema_version,
                        result_ref: p::ActionResultRef(payload.failure_ref.0.clone()),
                        succeeded: false,
                    });
                }
                p::EventPayload::FailureEvidenceRecorded(payload) => {
                    bundle
                        .failures
                        .push(FailureEvidence::from_event(event, payload));
                }
                _ => {}
            }
        }
        bundle
    }

    pub fn with_final_output(mut self, output: impl Into<String>) -> Self {
        self.final_output = Some(output.into());
        self
    }

    pub fn latest_successful_action(&self) -> Option<p::ActionResultRef> {
        self.action_results
            .iter()
            .rev()
            .find(|evidence| evidence.succeeded)
            .map(|evidence| evidence.result_ref.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceRequirement {
    ActionSucceeded(p::ActionResultRef),
    EventObserved(p::EventKind),
    FileMatches { path: String, digest: String },
    FinalOutputNonEmpty,
    IndependentVerification,
    NoBlockingFailures,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneCriterion {
    pub schema_version: p::SchemaVersion,
    pub id: String,
    pub evidence: EvidenceRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneContractRef {
    pub schema_version: p::SchemaVersion,
    pub reference: p::DoneContractRef,
    pub criteria: Vec<DoneCriterion>,
}

impl DoneContractRef {
    pub fn final_output(reference: p::DoneContractRef) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            reference,
            criteria: vec![DoneCriterion {
                schema_version: p::SchemaVersion(1),
                id: "final-output".into(),
                evidence: EvidenceRequirement::FinalOutputNonEmpty,
            }],
        }
    }

    pub fn action(reference: p::DoneContractRef, result: p::ActionResultRef) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            reference,
            criteria: vec![DoneCriterion {
                schema_version: p::SchemaVersion(1),
                id: "action-result".into(),
                evidence: EvidenceRequirement::ActionSucceeded(result),
            }],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureRef {
    pub schema_version: p::SchemaVersion,
    pub reference: p::FailureEvidenceRef,
}

/// Unverifiable is not Pass. It is persisted in trace and lowers commitment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationOutcome {
    Pass,
    Fail(FailureRef),
    Unverifiable(String),
}

pub trait Verifier {
    fn verify(&self, evidence: &EvidenceBundle, done: &DoneContractRef) -> VerificationOutcome;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeterministicVerifier;

impl Verifier for DeterministicVerifier {
    fn verify(&self, bundle: &EvidenceBundle, done: &DoneContractRef) -> VerificationOutcome {
        if bundle.schema_version.0 == 0
            || done.schema_version.0 == 0
            || done.reference.0.trim().is_empty()
            || done.criteria.is_empty()
        {
            return VerificationOutcome::Unverifiable(
                "verification contract or evidence bundle is incomplete".into(),
            );
        }
        let mut unverifiable = None;
        for criterion in &done.criteria {
            if criterion.schema_version.0 == 0 || criterion.id.trim().is_empty() {
                return VerificationOutcome::Unverifiable(
                    "done criterion is not versioned or identified".into(),
                );
            }
            match verify_criterion(bundle, criterion) {
                CriterionOutcome::Pass => {}
                CriterionOutcome::Fail(reference) => {
                    return VerificationOutcome::Fail(FailureRef {
                        schema_version: p::SchemaVersion(1),
                        reference,
                    });
                }
                CriterionOutcome::Unverifiable(reason) => {
                    unverifiable.get_or_insert(reason);
                }
            }
        }
        unverifiable
            .map(VerificationOutcome::Unverifiable)
            .unwrap_or(VerificationOutcome::Pass)
    }
}

enum CriterionOutcome {
    Pass,
    Fail(p::FailureEvidenceRef),
    Unverifiable(String),
}

fn verify_criterion(bundle: &EvidenceBundle, criterion: &DoneCriterion) -> CriterionOutcome {
    match &criterion.evidence {
        EvidenceRequirement::ActionSucceeded(expected) => match bundle
            .action_results
            .iter()
            .find(|evidence| &evidence.result_ref == expected)
        {
            Some(evidence) if evidence.succeeded => CriterionOutcome::Pass,
            Some(_) => CriterionOutcome::Fail(failure_ref(&criterion.id)),
            None => CriterionOutcome::Unverifiable(format!(
                "criterion {} has no matching action evidence",
                criterion.id
            )),
        },
        EvidenceRequirement::EventObserved(expected) => {
            if bundle.events.iter().any(|event| event.kind == *expected) {
                CriterionOutcome::Pass
            } else {
                CriterionOutcome::Unverifiable(format!(
                    "criterion {} has no matching event evidence",
                    criterion.id
                ))
            }
        }
        EvidenceRequirement::FileMatches { path, digest } => match bundle
            .file_states
            .iter()
            .find(|evidence| &evidence.path == path)
        {
            Some(evidence) if evidence.exists && evidence.digest.as_ref() == Some(digest) => {
                CriterionOutcome::Pass
            }
            Some(_) => CriterionOutcome::Fail(failure_ref(&criterion.id)),
            None => CriterionOutcome::Unverifiable(format!(
                "criterion {} has no matching file evidence",
                criterion.id
            )),
        },
        EvidenceRequirement::FinalOutputNonEmpty => match &bundle.final_output {
            Some(output) if !output.trim().is_empty() => CriterionOutcome::Pass,
            Some(_) => CriterionOutcome::Fail(failure_ref(&criterion.id)),
            None => CriterionOutcome::Unverifiable(format!(
                "criterion {} has no final-output evidence",
                criterion.id
            )),
        },
        EvidenceRequirement::IndependentVerification => {
            if bundle.independent_verification {
                CriterionOutcome::Pass
            } else {
                CriterionOutcome::Fail(p::FailureEvidenceRef("failure:self-eval-trap".into()))
            }
        }
        EvidenceRequirement::NoBlockingFailures => bundle
            .failures
            .iter()
            .find(|failure| failure.blocking)
            .map(|failure| CriterionOutcome::Fail(failure.failure_ref.clone()))
            .unwrap_or(CriterionOutcome::Pass),
    }
}

fn failure_ref(criterion: &str) -> p::FailureEvidenceRef {
    p::FailureEvidenceRef(format!("verification-failure:{criterion}"))
}

/// Canonical section 11: the single thirteen-class failure taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FailureClass {
    GoalFraming,
    Context,
    CognitiveMap,
    ResourceSelection,
    Execution,
    Verification,
    Trust,
    Proactivity,
    Learning,
    Handoff,
    SelfEvalTrap,
    SafetyPolicy,
    MemoryMisevolution,
}

impl FailureClass {
    pub const ALL: [Self; 13] = [
        Self::GoalFraming,
        Self::Context,
        Self::CognitiveMap,
        Self::ResourceSelection,
        Self::Execution,
        Self::Verification,
        Self::Trust,
        Self::Proactivity,
        Self::Learning,
        Self::Handoff,
        Self::SelfEvalTrap,
        Self::SafetyPolicy,
        Self::MemoryMisevolution,
    ];

    pub fn as_protocol(self) -> p::FailureClass {
        match self {
            Self::GoalFraming => p::FailureClass::GoalFramingFailure,
            Self::Context => p::FailureClass::ContextFailure,
            Self::CognitiveMap => p::FailureClass::CognitiveMapFailure,
            Self::ResourceSelection => p::FailureClass::ResourceSelectionFailure,
            Self::Execution => p::FailureClass::ExecutionFailure,
            Self::Verification => p::FailureClass::VerificationFailure,
            Self::Trust => p::FailureClass::TrustFailure,
            Self::Proactivity => p::FailureClass::ProactivityFailure,
            Self::Learning => p::FailureClass::LearningFailure,
            Self::Handoff => p::FailureClass::HandoffFailure,
            Self::SelfEvalTrap => p::FailureClass::SelfEvalTrap,
            Self::SafetyPolicy => p::FailureClass::SafetyPolicyFailure,
            Self::MemoryMisevolution => p::FailureClass::MemoryMisevolution,
        }
    }

    pub fn from_protocol(class: p::FailureClass) -> Self {
        match class {
            p::FailureClass::GoalFramingFailure => Self::GoalFraming,
            p::FailureClass::ContextFailure => Self::Context,
            p::FailureClass::CognitiveMapFailure => Self::CognitiveMap,
            p::FailureClass::ResourceSelectionFailure => Self::ResourceSelection,
            p::FailureClass::ExecutionFailure => Self::Execution,
            p::FailureClass::VerificationFailure => Self::Verification,
            p::FailureClass::TrustFailure => Self::Trust,
            p::FailureClass::ProactivityFailure => Self::Proactivity,
            p::FailureClass::LearningFailure => Self::Learning,
            p::FailureClass::HandoffFailure => Self::Handoff,
            p::FailureClass::SelfEvalTrap => Self::SelfEvalTrap,
            p::FailureClass::SafetyPolicyFailure => Self::SafetyPolicy,
            p::FailureClass::MemoryMisevolution => Self::MemoryMisevolution,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawFailureKind {
    GoalFraming,
    Context,
    CognitiveMap,
    ResourceSelection,
    Execution,
    Verification,
    Trust,
    Proactivity,
    Learning,
    Handoff,
    SelfEvalTrap,
    SafetyPolicy,
    MemoryMisevolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFailure {
    pub schema_version: p::SchemaVersion,
    pub kind: RawFailureKind,
    pub detail: String,
}

pub trait FailureClassifier {
    fn classify(&self, failure: RawFailure) -> FailureClass;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TaxonomyClassifier;

impl FailureClassifier for TaxonomyClassifier {
    fn classify(&self, failure: RawFailure) -> FailureClass {
        match failure.kind {
            RawFailureKind::GoalFraming => FailureClass::GoalFraming,
            RawFailureKind::Context => FailureClass::Context,
            RawFailureKind::CognitiveMap => FailureClass::CognitiveMap,
            RawFailureKind::ResourceSelection => FailureClass::ResourceSelection,
            RawFailureKind::Execution => FailureClass::Execution,
            RawFailureKind::Verification => FailureClass::Verification,
            RawFailureKind::Trust => FailureClass::Trust,
            RawFailureKind::Proactivity => FailureClass::Proactivity,
            RawFailureKind::Learning => FailureClass::Learning,
            RawFailureKind::Handoff => FailureClass::Handoff,
            RawFailureKind::SelfEvalTrap => FailureClass::SelfEvalTrap,
            RawFailureKind::SafetyPolicy => FailureClass::SafetyPolicy,
            RawFailureKind::MemoryMisevolution => FailureClass::MemoryMisevolution,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureEvidence {
    pub schema_version: p::SchemaVersion,
    pub failure_ref: p::FailureEvidenceRef,
    pub class: FailureClass,
    pub impact: p::Impact,
    pub scope: p::Scope,
    pub related_refs: Vec<p::EvidenceRef>,
    pub suggested_fix: Option<p::SuggestedFixRef>,
    pub detail: String,
    pub blocking: bool,
    pub observed_at: p::Timestamp,
    pub provenance: p::Provenance,
}

impl FailureEvidence {
    pub fn event_payload(&self) -> p::EventPayload {
        p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
            failure_ref: self.failure_ref.clone(),
            class: self.class.as_protocol(),
            impact: self.impact,
            scope: self.scope.clone(),
            related_refs: self.related_refs.clone(),
            suggested_fix: self.suggested_fix.clone(),
        })
    }

    fn from_event(event: &p::Event, payload: &p::FailureEvidenceRecordedPayload) -> Self {
        Self {
            schema_version: event.schema_version,
            failure_ref: payload.failure_ref.clone(),
            class: FailureClass::from_protocol(payload.class),
            impact: payload.impact,
            scope: payload.scope.clone(),
            related_refs: payload.related_refs.clone(),
            suggested_fix: payload.suggested_fix.clone(),
            detail: payload
                .suggested_fix
                .as_ref()
                .map(|fix| fix.0.clone())
                .unwrap_or_default(),
            blocking: payload.impact == p::Impact::High,
            observed_at: event.ts_unix_ms,
            provenance: event.provenance.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureDigest {
    pub schema_version: p::SchemaVersion,
    pub digest_ref: p::FailureDigestRef,
    pub members: Vec<p::FailureEvidenceRef>,
    pub summary: p::DigestSummaryRef,
    pub classes: BTreeMap<FailureClass, usize>,
    pub scopes: BTreeMap<p::Scope, usize>,
}

impl FailureDigest {
    pub fn from_evidence(digest_ref: p::FailureDigestRef, evidence: &[FailureEvidence]) -> Self {
        let mut classes = BTreeMap::new();
        let mut scopes = BTreeMap::new();
        for item in evidence {
            *classes.entry(item.class).or_insert(0) += 1;
            *scopes.entry(item.scope.clone()).or_insert(0) += 1;
        }
        Self {
            schema_version: p::SchemaVersion(1),
            digest_ref,
            members: evidence
                .iter()
                .map(|item| item.failure_ref.clone())
                .collect(),
            summary: p::DigestSummaryRef(format!(
                "{} failure records across {} classes and {} scopes",
                evidence.len(),
                classes.len(),
                scopes.len()
            )),
            classes,
            scopes,
        }
    }

    pub fn from_events(digest_ref: p::FailureDigestRef, events: &[p::Event]) -> Self {
        let evidence = events
            .iter()
            .filter_map(|event| match &event.payload {
                p::EventPayload::FailureEvidenceRecorded(payload) => {
                    Some(FailureEvidence::from_event(event, payload))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        Self::from_evidence(digest_ref, &evidence)
    }

    pub fn event_payload(&self) -> p::EventPayload {
        p::EventPayload::FailureDigestUpdated(p::FailureDigestUpdatedPayload {
            digest_ref: self.digest_ref.clone(),
            members: self.members.clone(),
            summary: self.summary.clone(),
        })
    }
}

type Clock = Arc<dyn Fn() -> p::Timestamp + Send + Sync>;

pub struct FailureLedger<S: EventStore> {
    store: Arc<S>,
    run: p::RunId,
    failures: Mutex<Vec<FailureEvidence>>,
    event_sequence: AtomicU64,
    clock: Clock,
}

impl<S> FailureLedger<S>
where
    S: EventStore,
{
    pub fn open(store: Arc<S>, run: p::RunId) -> p::Result<Self> {
        Self::with_clock(store, run, system_timestamp)
    }

    pub fn with_clock<F>(store: Arc<S>, run: p::RunId, clock: F) -> p::Result<Self>
    where
        F: Fn() -> p::Timestamp + Send + Sync + 'static,
    {
        let events = store.read_run(run.clone()).collect::<p::Result<Vec<_>>>()?;
        let failures = events
            .iter()
            .filter_map(|event| match &event.payload {
                p::EventPayload::FailureEvidenceRecorded(payload) => {
                    Some(FailureEvidence::from_event(event, payload))
                }
                _ => None,
            })
            .collect();
        let next = events
            .iter()
            .map(|event| event.stream_seq)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Ok(Self {
            store,
            run,
            failures: Mutex::new(failures),
            event_sequence: AtomicU64::new(next),
            clock: Arc::new(clock),
        })
    }

    pub fn record(&self, mut evidence: FailureEvidence) -> p::Result<p::FailureEvidenceRef> {
        validate_failure(&evidence)?;
        let mut failures = self
            .failures
            .lock()
            .map_err(|_| p::Error("failure ledger state is unavailable".into()))?;
        if let Some(existing) = failures
            .iter()
            .find(|existing| existing.failure_ref == evidence.failure_ref)
        {
            return if existing == &evidence {
                Ok(evidence.failure_ref)
            } else {
                Err(p::Error("failure evidence id collision".into()))
            };
        }
        if evidence.observed_at == 0 {
            evidence.observed_at = (self.clock)();
        }
        self.append(evidence.provenance.clone(), evidence.event_payload())?;
        failures.push(evidence.clone());
        let digest = FailureDigest::from_evidence(
            p::FailureDigestRef(format!("failure-digest:{}", self.run.0)),
            &failures,
        );
        self.append(system_provenance(), digest.event_payload())?;
        Ok(evidence.failure_ref)
    }

    pub fn digest(&self) -> FailureDigest {
        let failures = self
            .failures
            .lock()
            .map(|failures| failures.clone())
            .unwrap_or_default();
        FailureDigest::from_evidence(
            p::FailureDigestRef(format!("failure-digest:{}", self.run.0)),
            &failures,
        )
    }

    fn append(&self, provenance: p::Provenance, payload: p::EventPayload) -> p::Result<p::EventId> {
        let sequence = self.event_sequence.fetch_add(1, Ordering::SeqCst);
        self.store.append(p::Event::new(
            p::EventId(format!("eval-event:{}:{sequence}", self.run.0)),
            self.run.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            (self.clock)(),
            provenance,
        ))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraceExport {
    pub schema_version: p::SchemaVersion,
    pub run: p::RunId,
    pub events: Vec<p::Event>,
    pub failures: Vec<FailureEvidence>,
    pub verification_outcomes: Vec<p::VerificationOutcome>,
    pub has_unverifiable: bool,
}

pub struct TraceExporter<S: EventStore> {
    store: Arc<S>,
}

impl<S> TraceExporter<S>
where
    S: EventStore,
{
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    pub fn export(&self, run: p::RunId) -> p::Result<TraceExport> {
        let events = self
            .store
            .read_run(run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let failures = events
            .iter()
            .filter_map(|event| match &event.payload {
                p::EventPayload::FailureEvidenceRecorded(payload) => {
                    Some(FailureEvidence::from_event(event, payload))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let verification_outcomes = events
            .iter()
            .filter_map(|event| match &event.payload {
                p::EventPayload::VerificationFinished(payload) => Some(payload.outcome.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let has_unverifiable = verification_outcomes
            .iter()
            .any(|outcome| matches!(outcome, p::VerificationOutcome::Unverifiable(_)));
        Ok(TraceExport {
            schema_version: p::SchemaVersion(1),
            run,
            events,
            failures,
            verification_outcomes,
            has_unverifiable,
        })
    }
}

fn validate_failure(evidence: &FailureEvidence) -> p::Result<()> {
    if evidence.schema_version.0 == 0
        || evidence.failure_ref.0.trim().is_empty()
        || evidence.scope.0.trim().is_empty()
        || evidence.detail.trim().is_empty()
    {
        return Err(p::Error("failure evidence is incomplete".into()));
    }
    Ok(())
}

#[derive(Default)]
pub struct ManualEvalArchive {
    reports: Mutex<BTreeMap<p::EvalRef, p::ManualEvalReport>>,
}

impl ManualEvalArchive {
    pub fn save(&self, report: p::ManualEvalReport) -> p::Result<()> {
        report.validate()?;
        let mut reports = self
            .reports
            .lock()
            .map_err(|_| p::Error("manual eval archive is unavailable".into()))?;
        if let Some(existing) = reports.get(&report.eval_ref) {
            if existing == &report {
                return Ok(());
            }
            return Err(p::Error(
                "manual eval ref is already bound to another report".into(),
            ));
        }
        reports.insert(report.eval_ref.clone(), report);
        Ok(())
    }

    pub fn export(&self, eval_ref: &p::EvalRef) -> p::Result<p::ManualEvalReport> {
        self.reports
            .lock()
            .map_err(|_| p::Error("manual eval archive is unavailable".into()))?
            .get(eval_ref)
            .cloned()
            .ok_or_else(|| p::Error("manual eval report was not found".into()))
    }
}

pub fn evaluate_manual_trace(
    case: &p::ManualEvalCase,
    profile: &p::EvalProfile,
    trace: &p::TraceView,
) -> p::Result<p::ManualEvalReport> {
    case.validate()?;
    profile.validate()?;
    if case.policy != profile.policy || case.workspace != profile.workspace {
        return Err(p::Error(
            "manual eval case does not match its frozen profile".into(),
        ));
    }
    let last_stream_seq = trace
        .events
        .last()
        .map(|event| event.stream_seq)
        .ok_or_else(|| p::Error("manual eval trace is empty".into()))?;
    if trace.schema_version.0 == 0
        || trace.run.0.trim().is_empty()
        || trace.snapshot_upper_bound != last_stream_seq
        || trace.events.iter().any(|event| event.run_id != trace.run)
    {
        return Err(p::Error("manual eval trace boundary is invalid".into()));
    }
    let kinds = trace
        .events
        .iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();
    let rubric_violation = case
        .required_events
        .iter()
        .any(|required| !kinds.contains(required))
        || case
            .forbidden_events
            .iter()
            .any(|forbidden| kinds.contains(forbidden));
    let completed = kinds.contains(&p::EventKind::RunComplete);
    let verification = if trace
        .verification_outcomes
        .iter()
        .any(|outcome| matches!(outcome, p::VerificationOutcome::Fail))
    {
        p::VerificationOutcome::Fail
    } else if let Some(reason) =
        trace
            .verification_outcomes
            .iter()
            .find_map(|outcome| match outcome {
                p::VerificationOutcome::Unverifiable(reason) => Some(reason.clone()),
                _ => None,
            })
    {
        p::VerificationOutcome::Unverifiable(reason)
    } else if trace
        .verification_outcomes
        .iter()
        .any(|outcome| matches!(outcome, p::VerificationOutcome::Pass))
    {
        p::VerificationOutcome::Pass
    } else {
        p::VerificationOutcome::Unverifiable(p::ReasonRef(
            "trace has no verification outcome".into(),
        ))
    };
    let outcome = if rubric_violation {
        p::VerificationOutcome::Fail
    } else if !completed {
        p::VerificationOutcome::Unverifiable(p::ReasonRef(
            "run did not reach verified completion".into(),
        ))
    } else {
        verification
    };
    let report = p::ManualEvalReport {
        schema_version: p::SchemaVersion(1),
        eval_ref: profile.eval_ref.clone(),
        case_ref: case.case_ref.clone(),
        run: trace.run.clone(),
        trace_refs: trace
            .events
            .iter()
            .map(|event| event.event_id.clone())
            .collect(),
        outcome,
        rubric: case.rubric.clone(),
        snapshot: profile.replay_snapshot.clone(),
        profile: profile.clone(),
    };
    report.validate()?;
    Ok(report)
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
    use forme_store::{EventStore, SqliteEventStore, StoreOptions};

    use super::*;

    fn provenance() -> p::Provenance {
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        }
    }

    fn trace_event(run: &p::RunId, sequence: u64, payload: p::EventPayload) -> p::Event {
        let mut event = p::Event::new(
            p::EventId(format!("manual-eval:{sequence}")),
            run.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            sequence as i64,
            provenance(),
        );
        event.stream_seq = sequence;
        event
    }

    fn criterion(requirement: EvidenceRequirement) -> DoneContractRef {
        DoneContractRef {
            schema_version: p::SchemaVersion(1),
            reference: p::DoneContractRef("done:test".into()),
            criteria: vec![DoneCriterion {
                schema_version: p::SchemaVersion(1),
                id: "criterion:test".into(),
                evidence: requirement,
            }],
        }
    }

    #[test]
    fn verifier_distinguishes_pass_fail_unverifiable_and_self_eval_trap() {
        let verifier = DeterministicVerifier;
        let pass = EvidenceBundle::default().with_final_output("verified answer");
        assert_eq!(
            verifier.verify(&pass, &criterion(EvidenceRequirement::FinalOutputNonEmpty)),
            VerificationOutcome::Pass
        );
        assert!(matches!(
            verifier.verify(
                &EvidenceBundle::default(),
                &criterion(EvidenceRequirement::FinalOutputNonEmpty)
            ),
            VerificationOutcome::Unverifiable(_)
        ));
        assert!(matches!(
            verifier.verify(
                &EvidenceBundle::default().with_final_output("  "),
                &criterion(EvidenceRequirement::FinalOutputNonEmpty)
            ),
            VerificationOutcome::Fail(_)
        ));
        assert!(matches!(
            verifier.verify(
                &EvidenceBundle::default(),
                &criterion(EvidenceRequirement::IndependentVerification)
            ),
            VerificationOutcome::Fail(FailureRef { reference, .. })
                if reference.0 == "failure:self-eval-trap"
        ));
    }

    #[test]
    fn taxonomy_maps_all_thirteen_classes_without_fallback() {
        let classifier = TaxonomyClassifier;
        let raw = [
            RawFailureKind::GoalFraming,
            RawFailureKind::Context,
            RawFailureKind::CognitiveMap,
            RawFailureKind::ResourceSelection,
            RawFailureKind::Execution,
            RawFailureKind::Verification,
            RawFailureKind::Trust,
            RawFailureKind::Proactivity,
            RawFailureKind::Learning,
            RawFailureKind::Handoff,
            RawFailureKind::SelfEvalTrap,
            RawFailureKind::SafetyPolicy,
            RawFailureKind::MemoryMisevolution,
        ];
        let mapped = raw
            .into_iter()
            .map(|kind| {
                classifier.classify(RawFailure {
                    schema_version: p::SchemaVersion(1),
                    kind,
                    detail: "fixture".into(),
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(mapped, FailureClass::ALL);
        assert_eq!(
            mapped
                .into_iter()
                .map(FailureClass::as_protocol)
                .collect::<Vec<_>>()
                .len(),
            13
        );
    }

    #[test]
    fn manual_eval_never_turns_verification_failure_into_success() {
        let run = p::RunId("run:manual-eval-fail".into());
        let case = p::ManualEvalCase {
            schema_version: p::SchemaVersion(1),
            case_ref: p::EvalCaseRef("case:manual-eval-fail".into()),
            kind: p::GoldenTaskKind::FinalOnly,
            request: p::RunRequest {
                schema_version: p::SchemaVersion(1),
                source: p::Source::UserTurn,
                session: p::SessionRef("session:manual-eval-fail".into()),
                agent_profile: p::AgentProfileRef("agent:eval".into()),
                input: p::RunInput("fixture".into()),
                budget: None,
                idempotency_key: Some(p::IdempotencyKey("manual-eval-fail".into())),
            },
            workspace: p::WorkspaceRef("workspace:test".into()),
            done_contract: p::DoneContractRef("done:manual-eval-fail".into()),
            allowed_capabilities: Vec::new(),
            policy: p::PolicyProfileRef("policy:test".into()),
            rubric: p::RubricRef("rubric:no-false-success".into()),
            required_events: vec![
                p::EventKind::VerificationFinished,
                p::EventKind::RunComplete,
            ],
            forbidden_events: vec![p::EventKind::CandidatePromoted],
        };
        let profile = p::EvalProfile {
            schema_version: p::SchemaVersion(1),
            eval_ref: p::EvalRef("eval:manual-eval-fail".into()),
            model: p::ModelProfileRef("model:test".into()),
            policy: case.policy.clone(),
            toolset: p::ToolsetRef("toolset:test".into()),
            workspace: case.workspace.clone(),
            event_schema: p::SchemaVersion(1),
            replay_snapshot: p::ReplaySnapshotRef("snapshot:manual-eval-fail".into()),
        };
        let trace = p::TraceView {
            schema_version: p::SchemaVersion(1),
            run: run.clone(),
            snapshot_upper_bound: 2,
            events: vec![
                trace_event(
                    &run,
                    1,
                    p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                        verifier_kind: p::VerifierKind("deterministic".into()),
                        outcome: p::VerificationOutcome::Fail,
                        against: case.done_contract.clone(),
                    }),
                ),
                trace_event(
                    &run,
                    2,
                    p::EventPayload::RunComplete(p::RunCompletePayload {
                        stop_reason: p::StopReason("final_output".into()),
                        result_ref: None,
                    }),
                ),
            ],
            failure_refs: Vec::new(),
            verification_outcomes: vec![p::VerificationOutcome::Fail],
        };
        let before = trace.clone();
        let report = evaluate_manual_trace(&case, &profile, &trace).unwrap();
        assert_eq!(report.outcome, p::VerificationOutcome::Fail);
        assert_eq!(trace, before);
        let archive = ManualEvalArchive::default();
        archive.save(report.clone()).unwrap();
        archive.save(report.clone()).unwrap();
        assert_eq!(archive.export(&profile.eval_ref).unwrap(), report);
    }

    #[test]
    fn s10_failure_ledger_updates_digest_and_final_pass_does_not_hide_history() {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let run = p::RunId("eval:s10".into());
        let ledger = FailureLedger::with_clock(store.clone(), run.clone(), || 100).unwrap();
        for (id, class) in [
            ("tool", FailureClass::Execution),
            ("approval", FailureClass::Trust),
            ("resource", FailureClass::ResourceSelection),
        ] {
            ledger
                .record(FailureEvidence {
                    schema_version: p::SchemaVersion(1),
                    failure_ref: p::FailureEvidenceRef(format!("failure:{id}")),
                    class,
                    impact: p::Impact::Medium,
                    scope: p::Scope("workspace:test".into()),
                    related_refs: vec![p::EvidenceRef(format!("trace:{id}"))],
                    suggested_fix: Some(p::SuggestedFixRef("review evidence".into())),
                    detail: format!("{id} failed"),
                    blocking: false,
                    observed_at: 100,
                    provenance: provenance(),
                })
                .unwrap();
        }
        store
            .append(p::Event::new(
                p::EventId("verification:pass".into()),
                run.clone(),
                None,
                p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                    verifier_kind: p::VerifierKind("deterministic".into()),
                    outcome: p::VerificationOutcome::Pass,
                    against: p::DoneContractRef("done:test".into()),
                }),
                p::SchemaVersion(1),
                101,
                provenance(),
            ))
            .unwrap();

        let digest = ledger.digest();
        assert_eq!(digest.members.len(), 3);
        assert_eq!(digest.classes.len(), 3);
        let trace = TraceExporter::new(store.clone())
            .export(run.clone())
            .unwrap();
        assert_eq!(trace.failures.len(), 3);
        assert_eq!(
            trace.verification_outcomes,
            vec![p::VerificationOutcome::Pass]
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
                p::EventKind::FailureEvidenceRecorded,
                p::EventKind::FailureDigestUpdated,
                p::EventKind::FailureEvidenceRecorded,
                p::EventKind::FailureDigestUpdated,
                p::EventKind::FailureEvidenceRecorded,
                p::EventKind::FailureDigestUpdated,
                p::EventKind::VerificationFinished,
            ]
        );
    }
}
