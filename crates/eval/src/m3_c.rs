use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

use crate::m3_a::{
    portable_payload_digest, scan_portable_value, validate_portable_archive, PortableReplayBundle,
    ReplayEventRecord,
};

const ARTIFACT_TYPE: &str = "m3-c-governed-evolution-golden";
const DIGEST_DOMAIN: &[u8] = b"forme-m3-c-governed-evolution-golden-v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M3CControlEventTrace {
    pub schema_version: p::SchemaVersion,
    pub event_id: p::EventId,
    pub payload: p::EventPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M3CGovernedActionTrace {
    pub schema_version: p::SchemaVersion,
    pub approval_requested_event: p::EventId,
    pub approval_requested: p::ApprovalRequestedPayload,
    pub approval_resolved_event: p::EventId,
    pub approval_resolved: p::ApprovalResolvedPayload,
    pub action_planned_event: p::EventId,
    pub action_planned: p::ActionPlannedPayload,
    pub action_completed_event: p::EventId,
    pub action_completed: p::ActionCompletedPayload,
    pub verification_event: p::EventId,
    pub verification: p::VerificationFinishedPayload,
    pub requested_plan_digest: p::PlanDigest,
    pub granted_plan_digest: p::PlanDigest,
    pub approver: p::VerifiedPrincipal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M3CGovernedRunTrace {
    pub schema_version: p::SchemaVersion,
    pub run: p::RunId,
    pub session_bound_event: p::EventId,
    pub session_bound: p::SessionBoundPayload,
    pub expected_strategy: Option<p::StrategyVersionRef>,
    pub action: M3CGovernedActionTrace,
    pub event_refs: Vec<p::EventId>,
    pub event_kinds: Vec<p::EventKind>,
    pub mutation_ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M3CGoldenArtifactBody {
    pub schema_version: p::SchemaVersion,
    pub case_ref: p::EvaluationCaseRef,
    pub scope: p::Scope,
    pub baseline_replay: PortableReplayBundle,
    pub baseline_live: M3CGovernedRunTrace,
    pub candidate: p::StrategyCandidate,
    pub evaluation: p::EvolutionEvaluation,
    pub regression_evaluation: p::EvolutionEvaluation,
    pub control_events: Vec<M3CControlEventTrace>,
    pub activated_snapshot: p::EvolutionSnapshot,
    pub live_v2: M3CGovernedRunTrace,
    pub rolled_back_snapshot: p::EvolutionSnapshot,
    pub restored_v1: M3CGovernedRunTrace,
    pub final_active: p::ActiveStrategyRef,
    pub lineage: PortableReplayBundle,
    pub event_order: Vec<p::EventId>,
    pub event_kinds: Vec<p::EventKind>,
    pub external_mutation_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableM3CGoldenArtifact {
    pub schema_version: p::SchemaVersion,
    pub artifact_type: String,
    pub digest: p::SchemaDigest,
    pub body: M3CGoldenArtifactBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3CArtifactReceipt {
    pub schema_version: p::SchemaVersion,
    pub path: PathBuf,
    pub content_ref: p::ContentRef,
    pub digest: p::SchemaDigest,
}

pub struct M3CArtifactStore {
    root: PathBuf,
}

impl M3CArtifactStore {
    pub fn new(root: impl AsRef<Path>) -> p::Result<Self> {
        fs::create_dir_all(root.as_ref())
            .map_err(|error| p::Error(format!("failed to create M3-C artifact root: {error}")))?;
        let root = fs::canonicalize(root.as_ref())
            .map_err(|error| p::Error(format!("failed to resolve M3-C artifact root: {error}")))?;
        if !root.is_dir() {
            return Err(p::Error("M3-C artifact root is not a directory".into()));
        }
        Ok(Self { root })
    }

    pub fn write(&self, body: &M3CGoldenArtifactBody) -> p::Result<M3CArtifactReceipt> {
        validate_body(body)?;
        let value = serde_json::to_value(body)
            .map_err(|error| p::Error(format!("failed to inspect M3-C artifact: {error}")))?;
        scan_portable_value(&value)?;
        let digest = digest_body(body)?;
        let artifact = PortableM3CGoldenArtifact {
            schema_version: p::SchemaVersion(1),
            artifact_type: ARTIFACT_TYPE.into(),
            digest: p::SchemaDigest(format!("fnv64:{digest}")),
            body: body.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&artifact)
            .map_err(|error| p::Error(format!("failed to encode M3-C artifact: {error}")))?;
        let path = self.root.join(format!("governed-golden-{digest}.json"));
        if path.parent() != Some(self.root.as_path()) {
            return Err(p::Error("M3-C artifact path escapes its root".into()));
        }
        if path.exists() {
            let existing = fs::read(&path).map_err(|error| {
                p::Error(format!("failed to read existing M3-C artifact: {error}"))
            })?;
            if existing != bytes {
                return Err(p::Error(
                    "M3-C artifact digest path is bound to different content".into(),
                ));
            }
        } else {
            fs::write(&path, bytes)
                .map_err(|error| p::Error(format!("failed to write M3-C artifact: {error}")))?;
        }
        self.verify(path)
    }

    pub fn verify(&self, path: impl AsRef<Path>) -> p::Result<M3CArtifactReceipt> {
        let path = fs::canonicalize(path.as_ref())
            .map_err(|error| p::Error(format!("failed to resolve M3-C artifact: {error}")))?;
        if path.parent() != Some(self.root.as_path()) {
            return Err(p::Error(
                "M3-C artifact path escapes its configured root".into(),
            ));
        }
        let bytes = fs::read(&path)
            .map_err(|error| p::Error(format!("failed to read M3-C artifact: {error}")))?;
        let artifact: PortableM3CGoldenArtifact = serde_json::from_slice(&bytes)
            .map_err(|error| p::Error(format!("failed to parse M3-C artifact: {error}")))?;
        if artifact.schema_version != p::SchemaVersion(1) || artifact.artifact_type != ARTIFACT_TYPE
        {
            return Err(p::Error(
                "M3-C artifact type or schema is unsupported".into(),
            ));
        }
        validate_body(&artifact.body)?;
        let value = serde_json::to_value(&artifact)
            .map_err(|error| p::Error(format!("failed to inspect M3-C artifact: {error}")))?;
        scan_portable_value(&value)?;
        let digest = digest_body(&artifact.body)?;
        let expected_digest = p::SchemaDigest(format!("fnv64:{digest}"));
        if artifact.digest != expected_digest
            || path.file_name().and_then(|name| name.to_str())
                != Some(format!("governed-golden-{digest}.json").as_str())
        {
            return Err(p::Error(
                "M3-C artifact content digest or filename does not match".into(),
            ));
        }
        Ok(M3CArtifactReceipt {
            schema_version: p::SchemaVersion(1),
            path,
            content_ref: p::ContentRef(format!("artifact:fnv64:{digest}")),
            digest: expected_digest,
        })
    }

    pub fn verify_complete_set(&self) -> p::Result<M3CArtifactReceipt> {
        let entries = fs::read_dir(&self.root)
            .map_err(|error| p::Error(format!("failed to list M3-C artifact root: {error}")))?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| p::Error(format!("failed to inspect M3-C artifact root: {error}")))?;
        if entries.len() != 1
            || !entries[0]
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        {
            return Err(p::Error(
                "M3-C artifact root must contain exactly one regular artifact".into(),
            ));
        }
        self.verify(entries[0].path())
    }
}

fn validate_body(body: &M3CGoldenArtifactBody) -> p::Result<()> {
    if body.schema_version != p::SchemaVersion(1)
        || body.case_ref.0.trim().is_empty()
        || body.scope.0.trim().is_empty()
        || body.external_mutation_count != 3
        || body.event_order.is_empty()
        || body.event_order.len() != body.event_kinds.len()
        || body.event_order.iter().collect::<BTreeSet<_>>().len() != body.event_order.len()
    {
        return Err(p::Error(
            "M3-C golden artifact boundary is incomplete".into(),
        ));
    }
    body.candidate.validate()?;
    body.evaluation.validate()?;
    body.regression_evaluation.validate()?;
    body.activated_snapshot.validate()?;
    body.rolled_back_snapshot.validate()?;
    body.final_active.validate()?;
    validate_portable_archive(&body.baseline_replay)?;
    validate_portable_archive(&body.lineage)?;
    if body.evaluation.verdict != p::EvaluationVerdict::Pass
        || body.evaluation.candidate != body.candidate.proposed_version
        || body.evaluation.baseline != body.candidate.baseline
        || body.evaluation.bundle != body.baseline_replay.manifest.bundle
        || body.regression_evaluation.verdict != p::EvaluationVerdict::Fail
        || body.regression_evaluation.candidate != body.candidate.proposed_version
        || body.regression_evaluation.baseline != body.candidate.baseline
        || body.regression_evaluation.bundle != body.baseline_replay.manifest.bundle
    {
        return Err(p::Error(
            "M3-C baseline, candidate, holdout, or regression facts do not align".into(),
        ));
    }

    let lineage = body
        .lineage
        .events
        .iter()
        .map(|record| (record.envelope.event_id.clone(), record))
        .collect::<BTreeMap<_, _>>();
    if lineage.len() != body.lineage.events.len()
        || body.event_order.len() != lineage.len()
        || body
            .event_order
            .iter()
            .zip(&body.event_kinds)
            .any(|(event, kind)| {
                lineage
                    .get(event)
                    .is_none_or(|record| record.envelope.kind != *kind)
            })
    {
        return Err(p::Error(
            "M3-C event order does not match its portable lineage".into(),
        ));
    }
    let ordered = body.event_order.iter().collect::<BTreeSet<_>>();
    if ordered != lineage.keys().collect::<BTreeSet<_>>() {
        return Err(p::Error(
            "M3-C event order omits or invents lineage events".into(),
        ));
    }

    validate_run(&body.baseline_live, &lineage, None, None, 1)?;
    let baseline_refs = body
        .baseline_replay
        .events
        .iter()
        .map(|record| &record.envelope.event_id)
        .collect::<BTreeSet<_>>();
    if baseline_refs
        != body
            .baseline_live
            .event_refs
            .iter()
            .collect::<BTreeSet<_>>()
    {
        return Err(p::Error(
            "M3-C baseline run and replay bundle use different evidence".into(),
        ));
    }

    let control = validate_control_events(body, &lineage)?;
    validate_snapshot(
        &body.activated_snapshot,
        &body.scope,
        &body.candidate.proposed_version,
    )?;
    validate_snapshot(
        &body.rolled_back_snapshot,
        &body.scope,
        &body.candidate.baseline,
    )?;
    if control.activation.to != body.candidate.proposed_version
        || control.activation.from.as_ref() != Some(&body.candidate.baseline)
        || control.activation.evaluation != body.evaluation.evaluation
        || control.activation.promotion != control.promotion_event
        || control.activation.owner_confirmation.is_none()
        || control.activation.impact == p::EvolutionImpact::Constitutional
        || control.activation.scope != body.scope
        || control.activated_snapshot != &body.activated_snapshot.snapshot
    {
        return Err(p::Error(
            "M3-C activation did not follow evaluated owner-controlled promotion".into(),
        ));
    }
    validate_run(
        &body.live_v2,
        &lineage,
        Some(&body.activated_snapshot.snapshot),
        Some(&body.candidate.proposed_version),
        2,
    )?;
    if control.rollback.failed != body.candidate.proposed_version
        || control.rollback.restored != body.candidate.baseline
        || control.rollback.scope != body.scope
        || control.rollback.external_effects_reverted != p::HistoricalFalse
        || control.rolled_back_snapshot != &body.rolled_back_snapshot.snapshot
        || !body
            .regression_evaluation
            .ground_truth
            .contains(&p::EvidenceRef(control.failure.failure_ref.0.clone()))
    {
        return Err(p::Error(
            "M3-C regression and rollback lineage is incomplete or rewrites history".into(),
        ));
    }
    validate_run(
        &body.restored_v1,
        &lineage,
        Some(&body.rolled_back_snapshot.snapshot),
        Some(&body.candidate.baseline),
        3,
    )?;
    if body.final_active.domain != body.candidate.domain
        || body.final_active.scope != body.scope
        || body.final_active.version != body.candidate.baseline
        || body.final_active.aggregate != control.rollback.aggregate
        || body.final_active.activation_event != control.rollback_event
    {
        return Err(p::Error(
            "M3-C final active strategy is not the restored known-good version".into(),
        ));
    }
    if !is_subsequence(
        &body.event_kinds,
        &[
            p::EventKind::CandidateCreated,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::EvolutionEvaluationRecorded,
            p::EventKind::CandidatePromoted,
            p::EventKind::StrategyActivated,
            p::EventKind::SessionBound,
            p::EventKind::ApprovalRequested,
            p::EventKind::ApprovalResolved,
            p::EventKind::CompetenceGateEvaluated,
            p::EventKind::ActionPlanned,
            p::EventKind::ActionStarted,
            p::EventKind::ActionCompleted,
            p::EventKind::VerificationFinished,
            p::EventKind::FailureEvidenceRecorded,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::EvolutionEvaluationRecorded,
            p::EventKind::StrategyRolledBack,
            p::EventKind::SessionBound,
            p::EventKind::ApprovalRequested,
            p::EventKind::ApprovalResolved,
            p::EventKind::CompetenceGateEvaluated,
            p::EventKind::ActionPlanned,
            p::EventKind::ActionStarted,
            p::EventKind::ActionCompleted,
            p::EventKind::VerificationFinished,
        ],
    ) {
        return Err(p::Error(
            "M3-C golden event sequence skips a governed evolution phase".into(),
        ));
    }
    Ok(())
}

struct ControlFacts<'a> {
    promotion_event: p::EventId,
    activation: &'a p::StrategyActivation,
    activated_snapshot: &'a p::EvolutionSnapshotRef,
    failure: &'a p::FailureEvidenceRecordedPayload,
    rollback_event: p::EventId,
    rollback: &'a p::StrategyRollback,
    rolled_back_snapshot: &'a p::EvolutionSnapshotRef,
}

fn validate_control_events<'a>(
    body: &'a M3CGoldenArtifactBody,
    lineage: &BTreeMap<p::EventId, &ReplayEventRecord>,
) -> p::Result<ControlFacts<'a>> {
    if body.control_events.len() != 7 {
        return Err(p::Error(
            "M3-C golden requires seven typed control facts".into(),
        ));
    }
    for control in &body.control_events {
        if control.schema_version != p::SchemaVersion(1) {
            return Err(p::Error("M3-C control trace is unversioned".into()));
        }
        validate_payload_binding(&control.event_id, &control.payload, lineage)?;
    }
    let p::EventPayload::CandidateCreated(created) = &body.control_events[0].payload else {
        return Err(p::Error(
            "M3-C first control fact is not candidate creation".into(),
        ));
    };
    if created.strategy_candidate.as_ref() != Some(&body.candidate)
        || created.candidate_id != body.candidate.candidate
    {
        return Err(p::Error(
            "M3-C candidate event does not match typed candidate".into(),
        ));
    }
    let p::EventPayload::EvolutionEvaluationRecorded(evaluated) = &body.control_events[1].payload
    else {
        return Err(p::Error(
            "M3-C second control fact is not evaluation".into(),
        ));
    };
    validate_evaluation_event(evaluated, &body.evaluation)?;
    let p::EventPayload::CandidatePromoted(promoted) = &body.control_events[2].payload else {
        return Err(p::Error("M3-C third control fact is not promotion".into()));
    };
    if promoted.candidate_id != body.candidate.candidate {
        return Err(p::Error("M3-C promotion targets another candidate".into()));
    }
    let p::EventPayload::StrategyActivated(activated) = &body.control_events[3].payload else {
        return Err(p::Error(
            "M3-C fourth control fact is not activation".into(),
        ));
    };
    activated.activation.validate()?;
    let p::EventPayload::FailureEvidenceRecorded(failure) = &body.control_events[4].payload else {
        return Err(p::Error(
            "M3-C fifth control fact is not failure evidence".into(),
        ));
    };
    let p::EventPayload::EvolutionEvaluationRecorded(regression) = &body.control_events[5].payload
    else {
        return Err(p::Error(
            "M3-C sixth control fact is not regression evaluation".into(),
        ));
    };
    validate_evaluation_event(regression, &body.regression_evaluation)?;
    let p::EventPayload::StrategyRolledBack(rolled_back) = &body.control_events[6].payload else {
        return Err(p::Error("M3-C seventh control fact is not rollback".into()));
    };
    rolled_back.rollback.validate()?;
    Ok(ControlFacts {
        promotion_event: body.control_events[2].event_id.clone(),
        activation: &activated.activation,
        activated_snapshot: &activated.active_snapshot,
        failure,
        rollback_event: body.control_events[6].event_id.clone(),
        rollback: &rolled_back.rollback,
        rolled_back_snapshot: &rolled_back.active_snapshot,
    })
}

fn validate_evaluation_event(
    payload: &p::EvolutionEvaluationRecordedPayload,
    evaluation: &p::EvolutionEvaluation,
) -> p::Result<()> {
    if payload.evaluation != evaluation.evaluation
        || payload.baseline != evaluation.baseline
        || payload.candidate != evaluation.candidate
        || payload.verdict != evaluation.verdict
        || payload.hard_invariants
            != evaluation
                .hard_invariants
                .iter()
                .map(|invariant| invariant.reference.clone())
                .collect::<Vec<_>>()
        || payload.ground_truth != evaluation.ground_truth
    {
        return Err(p::Error(
            "M3-C evaluation event does not match its typed report".into(),
        ));
    }
    Ok(())
}

fn validate_run(
    trace: &M3CGovernedRunTrace,
    lineage: &BTreeMap<p::EventId, &ReplayEventRecord>,
    expected_snapshot: Option<&p::EvolutionSnapshotRef>,
    expected_strategy: Option<&p::StrategyVersionRef>,
    mutation_ordinal: u32,
) -> p::Result<()> {
    if trace.schema_version != p::SchemaVersion(1)
        || trace.run.0.trim().is_empty()
        || trace.event_refs.is_empty()
        || trace.event_refs.len() != trace.event_kinds.len()
        || trace.event_refs.iter().collect::<BTreeSet<_>>().len() != trace.event_refs.len()
        || trace.mutation_ordinal != mutation_ordinal
        || trace.expected_strategy.as_ref() != expected_strategy
        || trace.session_bound.evolution_snapshot.as_ref() != expected_snapshot
        || trace.session_bound.effect_mode != expected_snapshot.map(|_| p::EffectMode::LiveGoverned)
    {
        return Err(p::Error("M3-C governed run binding is incomplete".into()));
    }
    for (event, kind) in trace.event_refs.iter().zip(&trace.event_kinds) {
        let record = lineage
            .get(event)
            .ok_or_else(|| p::Error("M3-C governed run event is absent from lineage".into()))?;
        if record.envelope.run_id != trace.run || record.envelope.kind != *kind {
            return Err(p::Error(
                "M3-C governed run event identity or kind does not match lineage".into(),
            ));
        }
    }
    validate_payload_binding(
        &trace.session_bound_event,
        &p::EventPayload::SessionBound(trace.session_bound.clone()),
        lineage,
    )?;
    validate_action(&trace.action, lineage)?;
    if !is_subsequence(
        &trace.event_kinds,
        &[
            p::EventKind::SessionBound,
            p::EventKind::ToolCallProposed,
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::ApprovalRequested,
            p::EventKind::RunWaiting,
            p::EventKind::ApprovalResolved,
            p::EventKind::RunResumed,
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::CompetenceGateEvaluated,
            p::EventKind::ActionPlanned,
            p::EventKind::ActionStarted,
            p::EventKind::ActionCompleted,
            p::EventKind::CapabilityEvidenceRecorded,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ],
    ) || trace.event_kinds.iter().any(|kind| {
        matches!(
            kind,
            p::EventKind::ActionDenied
                | p::EventKind::ActionFailed
                | p::EventKind::ActionOutcomeUnknown
        )
    }) {
        return Err(p::Error(
            "M3-C live run did not preserve the M2 governed action sequence".into(),
        ));
    }
    Ok(())
}

fn validate_action(
    action: &M3CGovernedActionTrace,
    lineage: &BTreeMap<p::EventId, &ReplayEventRecord>,
) -> p::Result<()> {
    if action.schema_version != p::SchemaVersion(1)
        || action.approver.0.trim().is_empty()
        || action.approval_requested.approval_id != action.approval_resolved.approval_id
        || action.approval_resolved.outcome != p::ApprovalOutcome::Granted
        || action.action_planned.approval_ref.as_ref()
            != Some(&action.approval_requested.approval_id)
        || action.action_planned.plan_digest != action.requested_plan_digest
        || action.action_planned.plan_digest != action.granted_plan_digest
        || action.action_planned.backend != p::BackendKind::Browser
        || action.action_planned.expected_effect != p::ExpectedEffect::Outward
        || action.action_planned.source != p::Source::UserTurn
        || action.action_completed.intent_id != action.action_planned.intent_id
        || action.verification.outcome != p::VerificationOutcome::Pass
    {
        return Err(p::Error(
            "M3-C action approval is not bound to the executed browser plan".into(),
        ));
    }
    let receipt = action
        .action_completed
        .receipt
        .as_ref()
        .ok_or_else(|| p::Error("M3-C browser action has no external receipt".into()))?;
    if receipt.effect != p::EffectStatus::Committed || receipt.trust != p::TrustTier::Untrusted {
        return Err(p::Error(
            "M3-C external ground truth or untrusted provenance is missing".into(),
        ));
    }
    for (event, payload) in [
        (
            &action.approval_requested_event,
            p::EventPayload::ApprovalRequested(action.approval_requested.clone()),
        ),
        (
            &action.approval_resolved_event,
            p::EventPayload::ApprovalResolved(action.approval_resolved.clone()),
        ),
        (
            &action.action_planned_event,
            p::EventPayload::ActionPlanned(action.action_planned.clone()),
        ),
        (
            &action.action_completed_event,
            p::EventPayload::ActionCompleted(action.action_completed.clone()),
        ),
        (
            &action.verification_event,
            p::EventPayload::VerificationFinished(action.verification.clone()),
        ),
    ] {
        validate_payload_binding(event, &payload, lineage)?;
    }
    Ok(())
}

fn validate_payload_binding(
    event: &p::EventId,
    payload: &p::EventPayload,
    lineage: &BTreeMap<p::EventId, &ReplayEventRecord>,
) -> p::Result<()> {
    let record = lineage
        .get(event)
        .ok_or_else(|| p::Error("M3-C typed event is absent from portable lineage".into()))?;
    if record.envelope.kind != payload.kind()
        || record.envelope.payload_digest != portable_payload_digest(payload)?
    {
        return Err(p::Error(
            "M3-C typed event payload does not match its authoritative digest".into(),
        ));
    }
    Ok(())
}

fn validate_snapshot(
    snapshot: &p::EvolutionSnapshot,
    scope: &p::Scope,
    version: &p::StrategyVersionRef,
) -> p::Result<()> {
    if snapshot.strategies.iter().any(|active| {
        active.domain == p::StrategyDomain::Loop
            && &active.scope == scope
            && &active.version == version
    }) {
        Ok(())
    } else {
        Err(p::Error(
            "M3-C evolution snapshot does not contain the expected active strategy".into(),
        ))
    }
}

fn is_subsequence(actual: &[p::EventKind], expected: &[p::EventKind]) -> bool {
    let mut cursor = 0;
    for expected_kind in expected {
        let Some(offset) = actual[cursor..]
            .iter()
            .position(|kind| kind == expected_kind)
        else {
            return false;
        };
        cursor += offset + 1;
    }
    true
}

fn digest_body(body: &M3CGoldenArtifactBody) -> p::Result<String> {
    let bytes = serde_json::to_vec(body)
        .map_err(|error| p::Error(format!("failed to canonicalize M3-C artifact: {error}")))?;
    Ok(format!("{:016x}", fnv64(DIGEST_DOMAIN, &bytes)))
}

fn fnv64(domain: &[u8], bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in domain.iter().chain(bytes) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
