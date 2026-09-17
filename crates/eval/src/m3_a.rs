use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use forme_protocol as p;
use forme_store::EventStore;
use serde::{Deserialize, Serialize};

pub trait ReplayEngine {
    fn build(&self, request: p::ReplayRequest) -> p::Result<p::ReplayBundle>;
    fn exact(&self, bundle: &p::ReplayBundle) -> p::Result<p::ExactReplayReport>;
}

pub trait EvolutionEvaluator {
    fn compare(&self, input: p::EvolutionComparison) -> p::Result<p::EvolutionEvaluation>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum M3ArtifactKind {
    ReplayBundle,
    EvolutionEvaluation,
    PromotionActivation,
    Rollback,
    TraceManifest,
}

impl M3ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReplayBundle => "replay-bundle",
            Self::EvolutionEvaluation => "evolution-evaluation",
            Self::PromotionActivation => "promotion-activation",
            Self::Rollback => "rollback",
            Self::TraceManifest => "trace-manifest",
        }
    }

    fn parse(value: &str) -> p::Result<Self> {
        match value {
            "replay-bundle" => Ok(Self::ReplayBundle),
            "evolution-evaluation" => Ok(Self::EvolutionEvaluation),
            "promotion-activation" => Ok(Self::PromotionActivation),
            "rollback" => Ok(Self::Rollback),
            "trace-manifest" => Ok(Self::TraceManifest),
            _ => Err(p::Error("unknown M3 artifact type".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactReceipt {
    pub schema_version: p::SchemaVersion,
    pub kind: M3ArtifactKind,
    pub content_ref: p::ContentRef,
    pub digest: p::SchemaDigest,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceManifest {
    pub schema_version: p::SchemaVersion,
    pub events: Vec<p::EventId>,
    pub checksums: Vec<p::SchemaDigest>,
    pub evaluation: p::EvolutionEvaluationRef,
    pub active_snapshot: p::EvolutionSnapshotRef,
}

impl TraceManifest {
    pub fn validate(&self) -> p::Result<()> {
        let event_count = self.events.iter().collect::<BTreeSet<_>>().len();
        if self.schema_version.0 == 0
            || self.events.is_empty()
            || self.events.len() != self.checksums.len()
            || event_count != self.events.len()
            || self.events.iter().any(|item| item.0.trim().is_empty())
            || self.checksums.iter().any(|item| !is_fnv64_digest(item))
            || self.evaluation.0.trim().is_empty()
            || self.active_snapshot.0.trim().is_empty()
        {
            return Err(p::Error("M3 trace manifest is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct M3ArtifactStore {
    root: PathBuf,
}

impl M3ArtifactStore {
    pub fn open(root: impl AsRef<Path>) -> p::Result<Self> {
        std::fs::create_dir_all(root.as_ref())
            .map_err(|error| p::Error(format!("failed to create M3 artifact root: {error}")))?;
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|error| p::Error(format!("failed to resolve M3 artifact root: {error}")))?;
        if !root.is_dir() {
            return Err(p::Error("M3 artifact root is not a directory".into()));
        }
        Ok(Self { root })
    }

    pub fn write_replay(&self, archive: &PortableReplayBundle) -> p::Result<ArtifactReceipt> {
        validate_portable_archive(archive)?;
        self.write_value(
            M3ArtifactKind::ReplayBundle,
            serde_json::json!({
                "artifact_type": M3ArtifactKind::ReplayBundle.as_str(),
                "schema_version": 1,
                "manifest": archive.manifest,
                "events": archive.events.iter().map(|record| serde_json::json!({
                    "schema_version": record.schema_version,
                    "envelope": record.envelope,
                    "digest": record.digest,
                })).collect::<Vec<_>>(),
                "digest": archive.digest,
            }),
        )
    }

    pub fn write_evaluation(
        &self,
        evaluation: &p::EvolutionEvaluation,
    ) -> p::Result<ArtifactReceipt> {
        evaluation.validate()?;
        self.write_value(
            M3ArtifactKind::EvolutionEvaluation,
            serde_json::json!({
                "artifact_type": M3ArtifactKind::EvolutionEvaluation.as_str(),
                "schema_version": 1,
                "evaluation": evaluation,
            }),
        )
    }

    pub fn write_activation(
        &self,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        activation: &p::StrategyActivation,
    ) -> p::Result<ArtifactReceipt> {
        candidate.validate()?;
        evaluation.validate()?;
        activation.validate()?;
        if candidate.proposed_version != activation.to
            || evaluation.candidate != activation.to
            || evaluation.evaluation != activation.evaluation
            || evaluation.verdict != p::EvaluationVerdict::Pass
        {
            return Err(p::Error(
                "promotion and activation artifact facts do not align".into(),
            ));
        }
        self.write_value(
            M3ArtifactKind::PromotionActivation,
            serde_json::json!({
                "artifact_type": M3ArtifactKind::PromotionActivation.as_str(),
                "schema_version": 1,
                "candidate": candidate,
                "evaluation": evaluation,
                "activation": activation,
            }),
        )
    }

    pub fn write_rollback(&self, rollback: &p::StrategyRollback) -> p::Result<ArtifactReceipt> {
        rollback.validate()?;
        self.write_value(
            M3ArtifactKind::Rollback,
            serde_json::json!({
                "artifact_type": M3ArtifactKind::Rollback.as_str(),
                "schema_version": 1,
                "rollback": rollback,
            }),
        )
    }

    pub fn write_trace(&self, trace: &TraceManifest) -> p::Result<ArtifactReceipt> {
        trace.validate()?;
        self.write_value(
            M3ArtifactKind::TraceManifest,
            serde_json::json!({
                "artifact_type": M3ArtifactKind::TraceManifest.as_str(),
                "schema_version": trace.schema_version,
                "events": trace.events,
                "checksums": trace.checksums,
                "evaluation": trace.evaluation,
                "active_snapshot": trace.active_snapshot,
            }),
        )
    }

    pub fn verify(&self, path: impl AsRef<Path>) -> p::Result<ArtifactReceipt> {
        let path = path
            .as_ref()
            .canonicalize()
            .map_err(|error| p::Error(format!("failed to resolve M3 artifact: {error}")))?;
        if !path.starts_with(&self.root) || !path.is_file() {
            return Err(p::Error(
                "M3 artifact path escapes its configured root".into(),
            ));
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| p::Error(format!("failed to read M3 artifact: {error}")))?;
        let value = serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|error| p::Error(format!("failed to parse M3 artifact: {error}")))?;
        scan_portable_value(&value)?;
        let kind = value
            .get("artifact_type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| p::Error("M3 artifact has no typed artifact_type".into()))
            .and_then(M3ArtifactKind::parse)?;
        validate_artifact_value(kind, &value)?;
        let canonical = canonical_artifact_bytes(&value)?;
        let digest = fnv64(b"forme-m3-artifact-v1", &canonical);
        let expected_name = format!("{}-{digest}.json", kind.as_str());
        if path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
            return Err(p::Error(
                "M3 artifact filename does not match its content digest".into(),
            ));
        }
        Ok(ArtifactReceipt {
            schema_version: p::SchemaVersion(1),
            kind,
            content_ref: p::ContentRef(format!("artifact:fnv64:{digest}")),
            digest: p::SchemaDigest(format!("fnv64:{digest}")),
            path,
        })
    }

    pub fn verify_complete_set(&self) -> p::Result<Vec<ArtifactReceipt>> {
        let mut paths = std::fs::read_dir(&self.root)
            .map_err(|error| p::Error(format!("failed to list M3 artifact root: {error}")))?
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|error| p::Error(format!("failed to inspect M3 artifact: {error}")))
            })
            .collect::<p::Result<Vec<_>>>()?;
        if paths.iter().any(|path| {
            !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("json")
        }) {
            return Err(p::Error(
                "M3 artifact root contains an unexpected entry".into(),
            ));
        }
        paths.sort();
        if paths.len() != 5 {
            return Err(p::Error(
                "M3 artifact root must contain exactly five typed artifacts".into(),
            ));
        }

        let mut receipts = Vec::new();
        let mut values = BTreeMap::new();
        for path in paths {
            let receipt = self.verify(&path)?;
            let bytes = std::fs::read(&path)
                .map_err(|error| p::Error(format!("failed to read M3 artifact: {error}")))?;
            let value = serde_json::from_slice::<serde_json::Value>(&bytes)
                .map_err(|error| p::Error(format!("failed to parse M3 artifact: {error}")))?;
            if values.insert(receipt.kind.as_str(), value).is_some() {
                return Err(p::Error("M3 artifact kind is duplicated".into()));
            }
            receipts.push(receipt);
        }
        for kind in [
            M3ArtifactKind::ReplayBundle,
            M3ArtifactKind::EvolutionEvaluation,
            M3ArtifactKind::PromotionActivation,
            M3ArtifactKind::Rollback,
            M3ArtifactKind::TraceManifest,
        ] {
            if !values.contains_key(kind.as_str()) {
                return Err(p::Error("M3 artifact set is incomplete".into()));
            }
        }

        validate_artifact_lineage(&values)?;
        Ok(receipts)
    }

    fn write_value(
        &self,
        kind: M3ArtifactKind,
        value: serde_json::Value,
    ) -> p::Result<ArtifactReceipt> {
        scan_portable_value(&value)?;
        validate_artifact_value(kind, &value)?;
        let bytes = canonical_artifact_bytes(&value)?;
        let digest = fnv64(b"forme-m3-artifact-v1", &bytes);
        let path = self.root.join(format!("{}-{digest}.json", kind.as_str()));
        if !path.starts_with(&self.root) {
            return Err(p::Error(
                "M3 artifact path escapes its configured root".into(),
            ));
        }
        if path.exists() {
            let existing = std::fs::read(&path).map_err(|error| {
                p::Error(format!("failed to read existing M3 artifact: {error}"))
            })?;
            let existing = serde_json::from_slice::<serde_json::Value>(&existing)
                .map_err(|error| p::Error(format!("failed to parse M3 artifact: {error}")))?;
            if canonical_artifact_bytes(&existing)? != bytes {
                return Err(p::Error(
                    "M3 artifact digest path is bound to different content".into(),
                ));
            }
        } else {
            std::fs::write(&path, &bytes)
                .map_err(|error| p::Error(format!("failed to write M3 artifact: {error}")))?;
        }
        self.verify(path)
    }
}

fn canonical_artifact_bytes(value: &serde_json::Value) -> p::Result<Vec<u8>> {
    serde_json::to_vec_pretty(value)
        .map_err(|error| p::Error(format!("failed to encode M3 artifact: {error}")))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortableActorClass {
    Owner,
    Agent,
    Subagent,
    External,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableSessionDelta {
    pub schema_version: p::SchemaVersion,
    pub status: p::RunStatus,
    pub source: Option<p::Source>,
    pub session_ref_digest: Option<p::SchemaDigest>,
    pub workspace_digest: Option<p::SchemaDigest>,
    pub wait_reason_digest: Option<p::SchemaDigest>,
    pub resume_ref_digest: Option<p::SchemaDigest>,
    pub stop_reason_digest: Option<p::SchemaDigest>,
    pub result_ref: Option<p::EventId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableTranscriptDelta {
    pub schema_version: p::SchemaVersion,
    pub text_digest: p::SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableEventEnvelope {
    pub schema_version: p::SchemaVersion,
    pub event_id: p::EventId,
    pub run_id: p::RunId,
    pub stream_seq: u64,
    pub turn_id: Option<p::TurnId>,
    pub kind: p::EventKind,
    pub event_schema: p::SchemaVersion,
    pub source: p::Source,
    pub actor_class: PortableActorClass,
    pub trust_tier: p::TrustTier,
    pub caused_by: Option<p::EventId>,
    pub payload_digest: p::SchemaDigest,
    pub session_delta: Option<PortableSessionDelta>,
    pub transcript_delta: Option<PortableTranscriptDelta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayEventRecord {
    pub schema_version: p::SchemaVersion,
    pub envelope: PortableEventEnvelope,
    pub digest: p::SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableReplayBundle {
    pub schema_version: p::SchemaVersion,
    pub manifest: p::ReplayBundle,
    pub events: Vec<ReplayEventRecord>,
    pub digest: p::SchemaDigest,
}

pub fn portable_replay_from_events(
    mut request: p::ReplayRequest,
    events: &[p::Event],
) -> p::Result<PortableReplayBundle> {
    request.validate()?;
    request.source_runs.sort();
    request.event_refs.sort();
    request.cases.sort();
    let expected = request.event_refs.iter().cloned().collect::<BTreeSet<_>>();
    let mut found = BTreeSet::new();
    let mut records = Vec::new();
    for run in &request.source_runs {
        let mut run_events = events
            .iter()
            .filter(|event| event.run_id == *run)
            .collect::<Vec<_>>();
        run_events.sort_by_key(|event| event.stream_seq);
        if run_events.is_empty() {
            return Err(p::Error(format!(
                "replay source run {} has no authoritative events",
                run.0
            )));
        }
        for (index, event) in run_events.into_iter().enumerate() {
            let expected_sequence = u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| p::Error("replay stream sequence is exhausted".into()))?;
            if event.stream_seq != expected_sequence || !expected.contains(&event.event_id) {
                return Err(p::Error(
                    "portable exact replay range is not complete and contiguous".into(),
                ));
            }
            found.insert(event.event_id.clone());
            records.push(portable_record(event)?);
        }
    }
    if events
        .iter()
        .any(|event| !request.source_runs.contains(&event.run_id))
        || found != expected
    {
        return Err(p::Error(
            "replay event refs do not exactly match the authoritative source runs".into(),
        ));
    }
    portable_archive_from_records(request, records)
}

fn portable_archive_from_records(
    request: p::ReplayRequest,
    records: Vec<ReplayEventRecord>,
) -> p::Result<PortableReplayBundle> {
    let digest = bundle_digest(&request, &records)?;
    let manifest = p::ReplayBundle {
        schema_version: p::SchemaVersion(1),
        bundle: request.bundle,
        source_runs: request.source_runs,
        event_refs: records
            .iter()
            .map(|record| record.envelope.event_id.clone())
            .collect(),
        cases: request.cases,
        snapshot: request.snapshot,
        effect_mode: request.effect_mode,
        content_digest: p::SchemaDigest(format!("fnv64:{digest}")),
    };
    manifest.validate()?;
    let archive = PortableReplayBundle {
        schema_version: p::SchemaVersion(1),
        digest: manifest.content_digest.clone(),
        manifest,
        events: records,
    };
    validate_portable_archive(&archive)?;
    Ok(archive)
}

pub struct DeterministicReplayEngine<S: EventStore> {
    store: Arc<S>,
    archives: Mutex<BTreeMap<p::ReplayBundleRef, PortableReplayBundle>>,
}

impl<S> DeterministicReplayEngine<S>
where
    S: EventStore,
{
    pub fn new(store: Arc<S>) -> Self {
        Self {
            store,
            archives: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn portable(&self, bundle: &p::ReplayBundleRef) -> p::Result<PortableReplayBundle> {
        self.archives
            .lock()
            .map_err(|_| p::Error("portable replay archive is unavailable".into()))?
            .get(bundle)
            .cloned()
            .ok_or_else(|| p::Error("portable replay bundle was not built by this engine".into()))
    }

    pub fn exact_portable(archive: &PortableReplayBundle) -> p::Result<p::ExactReplayReport> {
        validate_portable_archive(archive)?;
        let projections_match = deterministic_projection_fold(&archive.events)?;
        let digest =
            replay_report_digest(&archive.manifest, archive.events.len(), projections_match)?;
        let report = p::ExactReplayReport {
            schema_version: p::SchemaVersion(1),
            report: p::ExactReplayReportRef(format!("exact-replay:fnv64:{digest}")),
            bundle: archive.manifest.bundle.clone(),
            event_count: u64::try_from(archive.events.len())
                .map_err(|_| p::Error("portable replay event count is too large".into()))?,
            projections_match,
            history_unchanged: true,
            effect_calls: 0,
            digest: p::SchemaDigest(format!("fnv64:{digest}")),
        };
        report.validate()?;
        Ok(report)
    }

    fn collect_source_events(
        &self,
        source_runs: &[p::RunId],
        expected_refs: &[p::EventId],
    ) -> p::Result<Vec<ReplayEventRecord>> {
        let expected = expected_refs.iter().cloned().collect::<BTreeSet<_>>();
        let mut found = BTreeSet::new();
        let mut records = Vec::new();
        for run in source_runs {
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            if events.is_empty() {
                return Err(p::Error(format!(
                    "replay source run {} has no authoritative events",
                    run.0
                )));
            }
            for (index, event) in events.into_iter().enumerate() {
                let expected_sequence = u64::try_from(index)
                    .ok()
                    .and_then(|value| value.checked_add(1))
                    .ok_or_else(|| p::Error("replay stream sequence is exhausted".into()))?;
                if event.run_id != *run || event.stream_seq != expected_sequence {
                    return Err(p::Error(
                        "replay source stream is not contiguous and run-bound".into(),
                    ));
                }
                if !expected.contains(&event.event_id) {
                    return Err(p::Error(
                        "portable exact replay must cover every event in each source run".into(),
                    ));
                }
                found.insert(event.event_id.clone());
                records.push(portable_record(&event)?);
            }
        }
        if found != expected {
            return Err(p::Error(
                "replay event refs do not exactly match the authoritative source runs".into(),
            ));
        }
        records.sort_by(|left, right| {
            (
                &left.envelope.run_id,
                left.envelope.stream_seq,
                &left.envelope.event_id,
            )
                .cmp(&(
                    &right.envelope.run_id,
                    right.envelope.stream_seq,
                    &right.envelope.event_id,
                ))
        });
        Ok(records)
    }
}

impl<S> ReplayEngine for DeterministicReplayEngine<S>
where
    S: EventStore,
{
    fn build(&self, mut request: p::ReplayRequest) -> p::Result<p::ReplayBundle> {
        request.validate()?;
        request.source_runs.sort();
        request.event_refs.sort();
        request.cases.sort();
        let records = self.collect_source_events(&request.source_runs, &request.event_refs)?;
        let archive = portable_archive_from_records(request, records)?;
        let manifest = archive.manifest.clone();
        let mut archives = self
            .archives
            .lock()
            .map_err(|_| p::Error("portable replay archive is unavailable".into()))?;
        if let Some(existing) = archives.get(&manifest.bundle) {
            if existing != &archive {
                return Err(p::Error(
                    "replay bundle ref is already bound to different content".into(),
                ));
            }
        } else {
            archives.insert(manifest.bundle.clone(), archive);
        }
        Ok(manifest)
    }

    fn exact(&self, bundle: &p::ReplayBundle) -> p::Result<p::ExactReplayReport> {
        bundle.validate()?;
        if bundle.effect_mode != p::EffectMode::ExactReplay {
            return Err(p::Error(
                "exact replay requires the exact-replay effect mode".into(),
            ));
        }
        let archive = self.portable(&bundle.bundle)?;
        if archive.manifest != *bundle {
            return Err(p::Error(
                "exact replay manifest differs from its portable archive".into(),
            ));
        }
        let before = self.collect_source_events(&bundle.source_runs, &bundle.event_refs)?;
        if before != archive.events {
            return Err(p::Error(
                "authoritative replay history changed after bundle construction".into(),
            ));
        }

        let after = self.collect_source_events(&bundle.source_runs, &bundle.event_refs)?;
        let history_unchanged = before == after;
        let mut report = Self::exact_portable(&archive)?;
        report.history_unchanged = history_unchanged;
        report.validate()?;
        Ok(report)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeterministicEvolutionEvaluator;

impl EvolutionEvaluator for DeterministicEvolutionEvaluator {
    fn compare(&self, input: p::EvolutionComparison) -> p::Result<p::EvolutionEvaluation> {
        input.validate()?;
        let verdict = if input
            .hard_invariants
            .iter()
            .any(|result| result.outcome == p::FitnessOutcome::Fail)
        {
            p::EvaluationVerdict::Fail
        } else if input
            .hard_invariants
            .iter()
            .any(|result| result.outcome == p::FitnessOutcome::Unverifiable)
            || input.ground_truth.is_empty()
            || !input.independent_verifier
            || input.self_eval_only
        {
            p::EvaluationVerdict::Unverifiable
        } else if input
            .metrics
            .iter()
            .any(|metric| metric.outcome == p::FitnessOutcome::Fail)
        {
            p::EvaluationVerdict::Fail
        } else if input
            .metrics
            .iter()
            .any(|metric| metric.outcome == p::FitnessOutcome::Unverifiable)
        {
            p::EvaluationVerdict::Unverifiable
        } else {
            p::EvaluationVerdict::Pass
        };
        let evaluation = p::EvolutionEvaluation {
            schema_version: input.schema_version,
            evaluation: input.evaluation,
            bundle: input.bundle,
            baseline: input.baseline,
            candidate: input.candidate,
            case_set_digest: input.case_set_digest,
            holdout_digest: input.holdout_digest,
            metrics: input.metrics,
            hard_invariants: input.hard_invariants,
            ground_truth: input.ground_truth,
            independent_verifier: input.independent_verifier,
            verdict,
        };
        evaluation.validate()?;
        Ok(evaluation)
    }
}

pub(crate) fn validate_portable_archive(archive: &PortableReplayBundle) -> p::Result<()> {
    archive.manifest.validate()?;
    if archive.schema_version.0 == 0
        || archive.events.is_empty()
        || archive.digest != archive.manifest.content_digest
        || archive.events.len() != archive.manifest.event_refs.len()
    {
        return Err(p::Error("portable replay archive is incomplete".into()));
    }
    for record in &archive.events {
        if record.schema_version.0 == 0 {
            return Err(p::Error(
                "portable replay event record is not versioned".into(),
            ));
        }
        validate_portable_envelope(&record.envelope)?;
        let digest = envelope_digest(&record.envelope)?;
        if record.digest.0 != format!("fnv64:{digest}") {
            return Err(p::Error(
                "portable replay event digest does not match its envelope".into(),
            ));
        }
    }
    let event_refs = archive
        .events
        .iter()
        .map(|record| record.envelope.event_id.clone())
        .collect::<Vec<_>>();
    if event_refs != archive.manifest.event_refs {
        return Err(p::Error(
            "portable replay event order differs from its manifest".into(),
        ));
    }
    let mut source_runs = archive
        .events
        .iter()
        .map(|record| record.envelope.run_id.clone())
        .collect::<Vec<_>>();
    source_runs.sort();
    source_runs.dedup();
    if source_runs != archive.manifest.source_runs {
        return Err(p::Error(
            "portable replay source runs differ from its manifest".into(),
        ));
    }
    if !deterministic_projection_fold(&archive.events)? {
        return Err(p::Error(
            "portable replay projection fold is not deterministic".into(),
        ));
    }
    let request = p::ReplayRequest {
        schema_version: archive.manifest.schema_version,
        bundle: archive.manifest.bundle.clone(),
        source_runs: archive.manifest.source_runs.clone(),
        event_refs: archive.manifest.event_refs.clone(),
        cases: archive.manifest.cases.clone(),
        snapshot: archive.manifest.snapshot.clone(),
        effect_mode: archive.manifest.effect_mode,
    };
    let digest = bundle_digest(&request, &archive.events)?;
    if archive.manifest.content_digest.0 != format!("fnv64:{digest}") {
        return Err(p::Error(
            "portable replay bundle digest does not match its contents".into(),
        ));
    }
    Ok(())
}

fn validate_artifact_value(kind: M3ArtifactKind, value: &serde_json::Value) -> p::Result<()> {
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err(p::Error("M3 artifact schema version is unsupported".into()));
    }
    match kind {
        M3ArtifactKind::ReplayBundle => {
            let manifest =
                serde_json::from_value::<p::ReplayBundle>(artifact_field(value, "manifest")?)
                    .map_err(|error| {
                        p::Error(format!("invalid replay manifest artifact: {error}"))
                    })?;
            let events = value
                .get("events")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| p::Error("replay artifact has no event records".into()))?
                .iter()
                .map(|record| {
                    let schema_version = serde_json::from_value::<p::SchemaVersion>(
                        artifact_field(record, "schema_version")?,
                    )
                    .map_err(|error| {
                        p::Error(format!("invalid replay event schema artifact: {error}"))
                    })?;
                    let envelope = serde_json::from_value::<PortableEventEnvelope>(artifact_field(
                        record, "envelope",
                    )?)
                    .map_err(|error| {
                        p::Error(format!("invalid replay event envelope artifact: {error}"))
                    })?;
                    let digest = serde_json::from_value::<p::SchemaDigest>(artifact_field(
                        record, "digest",
                    )?)
                    .map_err(|error| {
                        p::Error(format!("invalid replay event digest artifact: {error}"))
                    })?;
                    Ok(ReplayEventRecord {
                        schema_version,
                        envelope,
                        digest,
                    })
                })
                .collect::<p::Result<Vec<_>>>()?;
            let digest =
                serde_json::from_value::<p::SchemaDigest>(artifact_field(value, "digest")?)
                    .map_err(|error| {
                        p::Error(format!("invalid replay digest artifact: {error}"))
                    })?;
            validate_portable_archive(&PortableReplayBundle {
                schema_version: p::SchemaVersion(1),
                manifest,
                events,
                digest,
            })
        }
        M3ArtifactKind::EvolutionEvaluation => {
            let evaluation = serde_json::from_value::<p::EvolutionEvaluation>(artifact_field(
                value,
                "evaluation",
            )?)
            .map_err(|error| p::Error(format!("invalid evaluation artifact: {error}")))?;
            evaluation.validate()
        }
        M3ArtifactKind::PromotionActivation => {
            let candidate =
                serde_json::from_value::<p::StrategyCandidate>(artifact_field(value, "candidate")?)
                    .map_err(|error| p::Error(format!("invalid candidate artifact: {error}")))?;
            let evaluation = serde_json::from_value::<p::EvolutionEvaluation>(artifact_field(
                value,
                "evaluation",
            )?)
            .map_err(|error| p::Error(format!("invalid evaluation artifact: {error}")))?;
            let activation = serde_json::from_value::<p::StrategyActivation>(artifact_field(
                value,
                "activation",
            )?)
            .map_err(|error| p::Error(format!("invalid activation artifact: {error}")))?;
            candidate.validate()?;
            evaluation.validate()?;
            activation.validate()?;
            if candidate.proposed_version != activation.to
                || evaluation.candidate != activation.to
                || evaluation.evaluation != activation.evaluation
                || evaluation.verdict != p::EvaluationVerdict::Pass
            {
                return Err(p::Error(
                    "promotion and activation artifact facts do not align".into(),
                ));
            }
            Ok(())
        }
        M3ArtifactKind::Rollback => {
            let rollback =
                serde_json::from_value::<p::StrategyRollback>(artifact_field(value, "rollback")?)
                    .map_err(|error| p::Error(format!("invalid rollback artifact: {error}")))?;
            rollback.validate()
        }
        M3ArtifactKind::TraceManifest => {
            let trace = TraceManifest {
                schema_version: p::SchemaVersion(1),
                events: serde_json::from_value(artifact_field(value, "events")?)
                    .map_err(|error| p::Error(format!("invalid trace event refs: {error}")))?,
                checksums: serde_json::from_value(artifact_field(value, "checksums")?)
                    .map_err(|error| p::Error(format!("invalid trace checksums: {error}")))?,
                evaluation: serde_json::from_value(artifact_field(value, "evaluation")?)
                    .map_err(|error| p::Error(format!("invalid trace evaluation ref: {error}")))?,
                active_snapshot: serde_json::from_value(artifact_field(value, "active_snapshot")?)
                    .map_err(|error| p::Error(format!("invalid trace snapshot ref: {error}")))?,
            };
            trace.validate()
        }
    }
}

fn validate_artifact_lineage(values: &BTreeMap<&str, serde_json::Value>) -> p::Result<()> {
    let replay = values
        .get(M3ArtifactKind::ReplayBundle.as_str())
        .expect("required replay artifact was checked");
    let replay_manifest =
        serde_json::from_value::<p::ReplayBundle>(artifact_field(replay, "manifest")?)
            .map_err(|error| p::Error(format!("invalid replay manifest artifact: {error}")))?;
    let replay_checksums = replay
        .get("events")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| p::Error("replay artifact has no event records".into()))?
        .iter()
        .map(|record| {
            let envelope = serde_json::from_value::<PortableEventEnvelope>(artifact_field(
                record, "envelope",
            )?)
            .map_err(|error| {
                p::Error(format!("invalid replay event envelope artifact: {error}"))
            })?;
            let digest =
                serde_json::from_value::<p::SchemaDigest>(artifact_field(record, "digest")?)
                    .map_err(|error| {
                        p::Error(format!("invalid replay event digest artifact: {error}"))
                    })?;
            Ok((envelope.event_id, digest))
        })
        .collect::<p::Result<BTreeMap<_, _>>>()?;

    let evaluation = values
        .get(M3ArtifactKind::EvolutionEvaluation.as_str())
        .expect("required evaluation artifact was checked");
    let evaluation =
        serde_json::from_value::<p::EvolutionEvaluation>(artifact_field(evaluation, "evaluation")?)
            .map_err(|error| p::Error(format!("invalid evaluation artifact: {error}")))?;
    let activation_artifact = values
        .get(M3ArtifactKind::PromotionActivation.as_str())
        .expect("required activation artifact was checked");
    let activation_candidate = serde_json::from_value::<p::StrategyCandidate>(artifact_field(
        activation_artifact,
        "candidate",
    )?)
    .map_err(|error| p::Error(format!("invalid candidate artifact: {error}")))?;
    let activation_evaluation = serde_json::from_value::<p::EvolutionEvaluation>(artifact_field(
        activation_artifact,
        "evaluation",
    )?)
    .map_err(|error| p::Error(format!("invalid activation evaluation artifact: {error}")))?;
    let activation = serde_json::from_value::<p::StrategyActivation>(artifact_field(
        activation_artifact,
        "activation",
    )?)
    .map_err(|error| p::Error(format!("invalid activation artifact: {error}")))?;
    let rollback_artifact = values
        .get(M3ArtifactKind::Rollback.as_str())
        .expect("required rollback artifact was checked");
    let rollback = serde_json::from_value::<p::StrategyRollback>(artifact_field(
        rollback_artifact,
        "rollback",
    )?)
    .map_err(|error| p::Error(format!("invalid rollback artifact: {error}")))?;
    let trace_artifact = values
        .get(M3ArtifactKind::TraceManifest.as_str())
        .expect("required trace artifact was checked");
    let trace = TraceManifest {
        schema_version: p::SchemaVersion(1),
        events: serde_json::from_value(artifact_field(trace_artifact, "events")?)
            .map_err(|error| p::Error(format!("invalid trace event refs: {error}")))?,
        checksums: serde_json::from_value(artifact_field(trace_artifact, "checksums")?)
            .map_err(|error| p::Error(format!("invalid trace checksums: {error}")))?,
        evaluation: serde_json::from_value(artifact_field(trace_artifact, "evaluation")?)
            .map_err(|error| p::Error(format!("invalid trace evaluation ref: {error}")))?,
        active_snapshot: serde_json::from_value(artifact_field(trace_artifact, "active_snapshot")?)
            .map_err(|error| p::Error(format!("invalid trace snapshot ref: {error}")))?,
    };

    if evaluation.bundle != replay_manifest.bundle
        || activation_evaluation != evaluation
        || activation_candidate.proposed_version != activation.to
        || rollback.aggregate != activation.aggregate
        || rollback.domain != activation.domain
        || rollback.scope != activation.scope
        || rollback.failed != activation.to
        || activation.from.as_ref() != Some(&rollback.restored)
        || trace.evaluation != evaluation.evaluation
    {
        return Err(p::Error(
            "M3 artifact set does not describe one evolution lineage".into(),
        ));
    }
    let trace_checksums = trace
        .events
        .iter()
        .cloned()
        .zip(trace.checksums.iter().cloned())
        .collect::<BTreeMap<_, _>>();
    if replay_checksums
        .iter()
        .any(|(event, digest)| trace_checksums.get(event) != Some(digest))
    {
        return Err(p::Error(
            "portable trace does not cover the replay event checksums".into(),
        ));
    }
    Ok(())
}

fn artifact_field(value: &serde_json::Value, field: &str) -> p::Result<serde_json::Value> {
    value
        .get(field)
        .cloned()
        .ok_or_else(|| p::Error(format!("M3 artifact is missing {field}")))
}

fn deterministic_projection_fold(records: &[ReplayEventRecord]) -> p::Result<bool> {
    let mut runs = records
        .iter()
        .map(|record| record.envelope.run_id.clone())
        .collect::<Vec<_>>();
    runs.sort();
    runs.dedup();
    for run in runs {
        let events = records
            .iter()
            .filter(|record| record.envelope.run_id == run)
            .collect::<Vec<_>>();
        let first = fold_run(&events)?;
        let second = fold_run(&events)?;
        if first != second {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PortableSessionState {
    run_id: Option<p::RunId>,
    source: Option<p::Source>,
    status: p::RunStatus,
    session_ref_digest: Option<p::SchemaDigest>,
    workspace_digest: Option<p::SchemaDigest>,
    wait_reason_digest: Option<p::SchemaDigest>,
    resume_ref_digest: Option<p::SchemaDigest>,
    stop_reason_digest: Option<p::SchemaDigest>,
    result_ref: Option<p::EventId>,
    last_stream_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PortableTranscriptEntry {
    event_id: p::EventId,
    run_id: p::RunId,
    turn_id: Option<p::TurnId>,
    stream_seq: u64,
    kind: p::EventKind,
    text_digest: p::SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PortableTranscript {
    run_id: Option<p::RunId>,
    entries: Vec<PortableTranscriptEntry>,
    last_stream_seq: u64,
}

fn fold_run(
    records: &[&ReplayEventRecord],
) -> p::Result<(PortableSessionState, PortableTranscript)> {
    if records.is_empty() {
        return Err(p::Error("portable replay run has no events".into()));
    }
    let mut session = PortableSessionState {
        run_id: None,
        source: None,
        status: p::RunStatus::Accepted,
        session_ref_digest: None,
        workspace_digest: None,
        wait_reason_digest: None,
        resume_ref_digest: None,
        stop_reason_digest: None,
        result_ref: None,
        last_stream_seq: 0,
    };
    let mut transcript = PortableTranscript {
        run_id: None,
        entries: Vec::new(),
        last_stream_seq: 0,
    };
    let run = records[0].envelope.run_id.clone();
    for (index, record) in records.iter().enumerate() {
        let expected = u64::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| p::Error("portable replay sequence is exhausted".into()))?;
        let envelope = &record.envelope;
        if envelope.run_id != run || envelope.stream_seq != expected {
            return Err(p::Error(
                "portable replay run sequence is not contiguous and run-bound".into(),
            ));
        }
        session.run_id = Some(run.clone());
        session.last_stream_seq = envelope.stream_seq;
        transcript.run_id = Some(run.clone());
        transcript.last_stream_seq = envelope.stream_seq;
        if let Some(delta) = &envelope.session_delta {
            session.status = delta.status;
            if let Some(source) = delta.source {
                session.source = Some(source);
            }
            if let Some(value) = &delta.session_ref_digest {
                session.session_ref_digest = Some(value.clone());
            }
            if let Some(value) = &delta.workspace_digest {
                session.workspace_digest = Some(value.clone());
            }
            if let Some(value) = &delta.wait_reason_digest {
                session.wait_reason_digest = Some(value.clone());
            }
            if let Some(value) = &delta.resume_ref_digest {
                session.resume_ref_digest = Some(value.clone());
            }
            if let Some(value) = &delta.stop_reason_digest {
                session.stop_reason_digest = Some(value.clone());
            }
            if let Some(value) = &delta.result_ref {
                session.result_ref = Some(value.clone());
            }
        }
        if let Some(delta) = &envelope.transcript_delta {
            transcript.entries.push(PortableTranscriptEntry {
                event_id: envelope.event_id.clone(),
                run_id: run.clone(),
                turn_id: envelope.turn_id.clone(),
                stream_seq: envelope.stream_seq,
                kind: envelope.kind,
                text_digest: delta.text_digest.clone(),
            });
        }
    }
    Ok((session, transcript))
}

fn bundle_digest(request: &p::ReplayRequest, records: &[ReplayEventRecord]) -> p::Result<String> {
    let identity = serde_json::json!({
        "schema_version": request.schema_version,
        "source_runs": request.source_runs,
        "event_refs": records.iter().map(|record| &record.envelope.event_id).collect::<Vec<_>>(),
        "event_digests": records.iter().map(|record| &record.digest).collect::<Vec<_>>(),
        "cases": request.cases,
        "snapshot": request.snapshot,
        "effect_mode": request.effect_mode,
    });
    let encoded = serde_json::to_vec(&identity)
        .map_err(|error| p::Error(format!("failed to encode replay bundle identity: {error}")))?;
    Ok(fnv64(b"forme-replay-bundle-v1", &encoded))
}

fn envelope_digest(envelope: &PortableEventEnvelope) -> p::Result<String> {
    let encoded = serde_json::to_vec(envelope)
        .map_err(|error| p::Error(format!("failed to encode replay event envelope: {error}")))?;
    Ok(fnv64(b"forme-replay-event-v1", &encoded))
}

fn replay_report_digest(
    bundle: &p::ReplayBundle,
    event_count: usize,
    projections_match: bool,
) -> p::Result<String> {
    let encoded = serde_json::to_vec(&serde_json::json!({
        "bundle": bundle.bundle,
        "content_digest": bundle.content_digest,
        "event_count": event_count,
        "projections_match": projections_match,
        "history_unchanged": true,
        "effect_calls": 0,
    }))
    .map_err(|error| p::Error(format!("failed to encode replay report identity: {error}")))?;
    Ok(fnv64(b"forme-exact-replay-report-v1", &encoded))
}

fn portable_record(event: &p::Event) -> p::Result<ReplayEventRecord> {
    validate_source_event(event)?;
    let envelope = PortableEventEnvelope {
        schema_version: p::SchemaVersion(1),
        event_id: event.event_id.clone(),
        run_id: event.run_id.clone(),
        stream_seq: event.stream_seq,
        turn_id: event.turn_id.clone(),
        kind: event.kind,
        event_schema: event.schema_version,
        source: event.provenance.source,
        actor_class: actor_class(&event.provenance.actor),
        trust_tier: event.provenance.trust_tier,
        caused_by: event.provenance.caused_by.clone(),
        payload_digest: value_digest(b"forme-replay-payload-v1", &event.payload)?,
        session_delta: portable_session_delta(&event.payload)?,
        transcript_delta: portable_transcript_delta(&event.payload)?,
    };
    validate_portable_envelope(&envelope)?;
    let digest = envelope_digest(&envelope)?;
    Ok(ReplayEventRecord {
        schema_version: p::SchemaVersion(1),
        envelope,
        digest: p::SchemaDigest(format!("fnv64:{digest}")),
    })
}

fn validate_source_event(event: &p::Event) -> p::Result<()> {
    event.validate_payload_kind()?;
    if event.schema_version.0 == 0
        || event.event_id.0.trim().is_empty()
        || event.run_id.0.trim().is_empty()
        || event.stream_seq == 0
    {
        return Err(p::Error(
            "portable replay source event is incomplete".into(),
        ));
    }
    let value = serde_json::to_value(event)
        .map_err(|error| p::Error(format!("failed to inspect portable replay event: {error}")))?;
    scan_portable_value(&value)
}

fn actor_class(actor: &p::Actor) -> PortableActorClass {
    match actor {
        p::Actor::Owner => PortableActorClass::Owner,
        p::Actor::Agent => PortableActorClass::Agent,
        p::Actor::Subagent(_) => PortableActorClass::Subagent,
        p::Actor::External(_) => PortableActorClass::External,
        p::Actor::System => PortableActorClass::System,
    }
}

fn portable_session_delta(payload: &p::EventPayload) -> p::Result<Option<PortableSessionDelta>> {
    let mut delta = PortableSessionDelta {
        schema_version: p::SchemaVersion(1),
        status: p::RunStatus::Running,
        source: None,
        session_ref_digest: None,
        workspace_digest: None,
        wait_reason_digest: None,
        resume_ref_digest: None,
        stop_reason_digest: None,
        result_ref: None,
    };
    match payload {
        p::EventPayload::RunAccepted(value) => {
            delta.status = p::RunStatus::Accepted;
            delta.source = Some(value.source);
            delta.session_ref_digest = Some(value_digest(
                b"forme-portable-session-ref-v1",
                &value.session_ref,
            )?);
        }
        p::EventPayload::SessionBound(value) => {
            delta.workspace_digest = Some(value_digest(
                b"forme-portable-workspace-ref-v1",
                &value.workspace,
            )?);
        }
        p::EventPayload::RunComplete(value) => {
            delta.status = p::RunStatus::Complete;
            delta.stop_reason_digest = Some(value_digest(
                b"forme-portable-stop-reason-v1",
                &value.stop_reason,
            )?);
            delta.result_ref = value.result_ref.clone();
        }
        p::EventPayload::RunAborted(value) => {
            delta.status = p::RunStatus::Aborted;
            delta.stop_reason_digest = Some(value_digest(
                b"forme-portable-stop-reason-v1",
                &value.stop_reason,
            )?);
            delta.result_ref = value.result_ref.clone();
        }
        p::EventPayload::RunFailed(value) => {
            delta.status = p::RunStatus::Failed;
            delta.stop_reason_digest = Some(value_digest(
                b"forme-portable-stop-reason-v1",
                &value.stop_reason,
            )?);
            delta.result_ref = value.result_ref.clone();
        }
        p::EventPayload::RunLimited(value) => {
            delta.status = p::RunStatus::Limited;
            delta.stop_reason_digest = Some(value_digest(
                b"forme-portable-stop-reason-v1",
                &value.stop_reason,
            )?);
            delta.result_ref = value.result_ref.clone();
        }
        p::EventPayload::RunWaiting(value) => {
            delta.status = p::RunStatus::Waiting;
            delta.wait_reason_digest = Some(value_digest(
                b"forme-portable-wait-reason-v1",
                &value.wait_reason,
            )?);
            delta.resume_ref_digest = Some(value_digest(
                b"forme-portable-resume-ref-v1",
                &value.resume_ref,
            )?);
        }
        p::EventPayload::RunResumed(value) => {
            delta.wait_reason_digest = Some(value_digest(
                b"forme-portable-wait-reason-v1",
                &value.wait_reason,
            )?);
            delta.resume_ref_digest = Some(value_digest(
                b"forme-portable-resume-ref-v1",
                &value.resume_ref,
            )?);
        }
        p::EventPayload::TurnStarted(_)
        | p::EventPayload::TurnComplete(_)
        | p::EventPayload::ContextBuildStarted(_)
        | p::EventPayload::ContextBuildFinished(_)
        | p::EventPayload::CompactionStarted(_)
        | p::EventPayload::CompactionFinished(_) => {}
        _ => return Ok(None),
    }
    Ok(Some(delta))
}

fn portable_transcript_delta(
    payload: &p::EventPayload,
) -> p::Result<Option<PortableTranscriptDelta>> {
    let (domain, text) = match payload {
        p::EventPayload::RunAccepted(value) => (
            b"forme-portable-input-ref-v1".as_slice(),
            value.input_ref.0.as_bytes(),
        ),
        p::EventPayload::ModelCallDelta(value) => (
            b"forme-portable-model-delta-v1".as_slice(),
            value.delta.as_bytes(),
        ),
        p::EventPayload::ActionOutputDelta(value) => (
            b"forme-portable-action-delta-v1".as_slice(),
            value.delta.as_bytes(),
        ),
        _ => return Ok(None),
    };
    Ok(Some(PortableTranscriptDelta {
        schema_version: p::SchemaVersion(1),
        text_digest: p::SchemaDigest(format!("fnv64:{}", fnv64(domain, text))),
    }))
}

fn value_digest<T>(domain: &[u8], value: &T) -> p::Result<p::SchemaDigest>
where
    T: Serialize + ?Sized,
{
    let bytes = serde_json::to_vec(value)
        .map_err(|error| p::Error(format!("failed to digest portable replay value: {error}")))?;
    Ok(p::SchemaDigest(format!("fnv64:{}", fnv64(domain, &bytes))))
}

fn validate_portable_envelope(envelope: &PortableEventEnvelope) -> p::Result<()> {
    if envelope.schema_version.0 == 0
        || envelope.event_schema.0 == 0
        || envelope.event_id.0.trim().is_empty()
        || envelope.run_id.0.trim().is_empty()
        || envelope.stream_seq == 0
        || !is_fnv64_digest(&envelope.payload_digest)
    {
        return Err(p::Error(
            "portable replay event envelope is incomplete".into(),
        ));
    }
    let expected_session = matches!(
        envelope.kind,
        p::EventKind::RunAccepted
            | p::EventKind::SessionBound
            | p::EventKind::RunComplete
            | p::EventKind::RunAborted
            | p::EventKind::RunFailed
            | p::EventKind::RunLimited
            | p::EventKind::RunWaiting
            | p::EventKind::RunResumed
            | p::EventKind::TurnStarted
            | p::EventKind::TurnComplete
            | p::EventKind::ContextBuildStarted
            | p::EventKind::ContextBuildFinished
            | p::EventKind::CompactionStarted
            | p::EventKind::CompactionFinished
    );
    if expected_session != envelope.session_delta.is_some() {
        return Err(p::Error(
            "portable replay session projection delta does not match its event kind".into(),
        ));
    }
    if let Some(delta) = &envelope.session_delta {
        validate_portable_session_delta(envelope.kind, delta)?;
    }
    let expected_transcript = matches!(
        envelope.kind,
        p::EventKind::RunAccepted | p::EventKind::ModelCallDelta | p::EventKind::ActionOutputDelta
    );
    if expected_transcript != envelope.transcript_delta.is_some() {
        return Err(p::Error(
            "portable replay transcript delta does not match its event kind".into(),
        ));
    }
    if envelope
        .transcript_delta
        .as_ref()
        .is_some_and(|delta| delta.schema_version.0 == 0 || !is_fnv64_digest(&delta.text_digest))
    {
        return Err(p::Error(
            "portable replay transcript delta is incomplete".into(),
        ));
    }
    let value = serde_json::to_value(envelope).map_err(|error| {
        p::Error(format!(
            "failed to inspect portable replay event envelope: {error}"
        ))
    })?;
    scan_portable_value(&value)
}

fn validate_portable_session_delta(
    kind: p::EventKind,
    delta: &PortableSessionDelta,
) -> p::Result<()> {
    let digests = [
        delta.session_ref_digest.as_ref(),
        delta.workspace_digest.as_ref(),
        delta.wait_reason_digest.as_ref(),
        delta.resume_ref_digest.as_ref(),
        delta.stop_reason_digest.as_ref(),
    ];
    if delta.schema_version.0 == 0
        || digests
            .into_iter()
            .flatten()
            .any(|value| !is_fnv64_digest(value))
    {
        return Err(p::Error(
            "portable replay session projection delta is incomplete".into(),
        ));
    }
    let valid_shape = match kind {
        p::EventKind::RunAccepted => {
            delta.status == p::RunStatus::Accepted
                && delta.source.is_some()
                && delta.session_ref_digest.is_some()
        }
        p::EventKind::SessionBound => {
            delta.status == p::RunStatus::Running && delta.workspace_digest.is_some()
        }
        p::EventKind::RunComplete => {
            delta.status == p::RunStatus::Complete && delta.stop_reason_digest.is_some()
        }
        p::EventKind::RunAborted => {
            delta.status == p::RunStatus::Aborted && delta.stop_reason_digest.is_some()
        }
        p::EventKind::RunFailed => {
            delta.status == p::RunStatus::Failed && delta.stop_reason_digest.is_some()
        }
        p::EventKind::RunLimited => {
            delta.status == p::RunStatus::Limited && delta.stop_reason_digest.is_some()
        }
        p::EventKind::RunWaiting => {
            delta.status == p::RunStatus::Waiting
                && delta.wait_reason_digest.is_some()
                && delta.resume_ref_digest.is_some()
        }
        p::EventKind::RunResumed => {
            delta.status == p::RunStatus::Running
                && delta.wait_reason_digest.is_some()
                && delta.resume_ref_digest.is_some()
        }
        p::EventKind::TurnStarted
        | p::EventKind::TurnComplete
        | p::EventKind::ContextBuildStarted
        | p::EventKind::ContextBuildFinished
        | p::EventKind::CompactionStarted
        | p::EventKind::CompactionFinished => delta.status == p::RunStatus::Running,
        _ => false,
    };
    if !valid_shape {
        return Err(p::Error(
            "portable replay session projection delta has an invalid shape".into(),
        ));
    }
    Ok(())
}

fn is_fnv64_digest(value: &p::SchemaDigest) -> bool {
    value
        .0
        .strip_prefix("fnv64:")
        .is_some_and(|hex| hex.len() == 16 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

pub(crate) fn scan_portable_value(value: &serde_json::Value) -> p::Result<()> {
    match value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                let normalized = key.to_ascii_lowercase();
                if matches!(
                    normalized.as_str(),
                    "api_key"
                        | "api-key"
                        | "authorization"
                        | "credential"
                        | "credential_ref"
                        | "password"
                        | "secret"
                        | "secret_ref"
                        | "secrets"
                        | "secret_bindings"
                        | "access_token"
                        | "refresh_token"
                        | "token"
                ) {
                    return Err(p::Error(
                        "portable replay event contains a credential-bearing field".into(),
                    ));
                }
                scan_portable_value(value)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                scan_portable_value(value)?;
            }
        }
        serde_json::Value::String(value) => {
            let normalized = value.to_ascii_lowercase();
            if ["secret:", "credential:", "bearer ", "api_key=", "api-key="]
                .iter()
                .any(|marker| normalized.contains(marker))
            {
                return Err(p::Error(
                    "portable replay event contains a credential marker".into(),
                ));
            }
            if is_private_absolute_path(value) {
                return Err(p::Error(
                    "portable replay event contains a machine-local absolute path".into(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn portable_payload_digest(payload: &p::EventPayload) -> p::Result<p::SchemaDigest> {
    value_digest(b"forme-replay-payload-v1", payload)
}

fn is_private_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || value.starts_with("\\\\")
        || value.starts_with('/')
}

fn fnv64(domain: &[u8], value: &[u8]) -> String {
    const OFFSET: u64 = 14_695_981_039_346_656_037;
    const PRIME: u64 = 1_099_511_628_211;
    let mut hash = OFFSET;
    for byte in domain.iter().chain(value) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}
