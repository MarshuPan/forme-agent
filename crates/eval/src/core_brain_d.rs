//! Closed, reproducible task evidence. A model's claim that it succeeded is
//! never a workflow proof; selectors below inspect captured events/files/state.
use forme_protocol as p;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProductGoldenCase {
    ColdDialogue,
    ComplexTools,
    LongWorkspace,
    MultiAgent,
    AskToLearn,
    FailureRecoordination,
}
impl ProductGoldenCase {
    pub const ALL: [Self; 6] = [
        Self::ColdDialogue,
        Self::ComplexTools,
        Self::LongWorkspace,
        Self::MultiAgent,
        Self::AskToLearn,
        Self::FailureRecoordination,
    ];
    pub fn contract(self) -> Vec<ProductCriterion> {
        let mut c = vec![ProductCriterion::new(
            "answer",
            ProductSelector::FinalOutput,
            ProductPredicate::Exact("47".into()),
            "ground-truth:13+17+17=47",
        )];
        let mut event = |id: &str, kind, count| {
            c.push(ProductCriterion::new(
                id,
                ProductSelector::EventCount(kind),
                ProductPredicate::AtLeast(count),
                "ground-truth:runtime-event-contract",
            ))
        };
        match self {
            Self::ColdDialogue => event("completed", p::EventKind::RunComplete, 1),
            Self::ComplexTools => {
                event("file-driver", p::EventKind::ActionCompleted, 1);
                c.push(ProductCriterion::new(
                    "file",
                    ProductSelector::File("result.txt".into()),
                    ProductPredicate::Exact("47\n".into()),
                    "ground-truth:result-file-bytes",
                ));
            }
            Self::LongWorkspace => {
                c.push(ProductCriterion::new(
                    "restart",
                    ProductSelector::Restart,
                    ProductPredicate::AtLeast(1),
                    "ground-truth:sqlite-restart-equality",
                ));
                c.push(ProductCriterion::new(
                    "checkpoints",
                    ProductSelector::Checkpoints,
                    ProductPredicate::AtLeast(2),
                    "ground-truth:two-independent-project-segments",
                ));
            }
            Self::MultiAgent => {
                event("children", p::EventKind::SubagentResultReturned, 2);
                c.push(ProductCriterion::new(
                    "route",
                    ProductSelector::CompletedRoutes,
                    ProductPredicate::AtLeast(1),
                    "ground-truth:verified-node-ledger",
                ));
            }
            Self::AskToLearn => {
                event("question", p::EventKind::ProactiveProposalEmitted, 1);
                event("reply", p::EventKind::ObservationRecorded, 1);
                event("candidate", p::EventKind::UserAttributeCandidateCreated, 1);
            }
            Self::FailureRecoordination => {
                c.push(ProductCriterion::new(
                    "failure",
                    ProductSelector::FailedRoutes,
                    ProductPredicate::AtLeast(1),
                    "ground-truth:injected-missing-file-requirement",
                ));
                c.push(ProductCriterion::new(
                    "recoordination",
                    ProductSelector::CompletedRoutes,
                    ProductPredicate::AtLeast(1),
                    "ground-truth:revised-project-success",
                ));
            }
        }
        c
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductSelector {
    FinalOutput,
    EventCount(p::EventKind),
    File(String),
    Restart,
    Checkpoints,
    CompletedRoutes,
    FailedRoutes,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductPredicate {
    Exact(String),
    AtLeast(u64),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCriterion {
    pub schema_version: p::SchemaVersion,
    pub id: String,
    pub selector: ProductSelector,
    pub predicate: ProductPredicate,
    pub tolerance: u64,
    pub ground_truth: String,
}
impl ProductCriterion {
    fn new(id: &str, selector: ProductSelector, predicate: ProductPredicate, truth: &str) -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            id: id.into(),
            selector,
            predicate,
            tolerance: 0,
            ground_truth: truth.into(),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductEvidenceGrade {
    LiveConfigured,
    Controlled,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductVerdict {
    Pass,
    Fail,
    Unverifiable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductFileEvidence {
    pub schema_version: p::SchemaVersion,
    pub relative_path: String,
    pub bytes: Vec<u8>,
    pub digest: p::SchemaDigest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductRestartEvidence {
    pub schema_version: p::SchemaVersion,
    pub owner_before: p::SchemaDigest,
    pub owner_after: p::SchemaDigest,
    pub state_before: p::SchemaDigest,
    pub state_after: p::SchemaDigest,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductAttempt {
    pub schema_version: p::SchemaVersion,
    pub output: Option<String>,
    pub model_profiles: BTreeSet<p::ModelProfileRef>,
    pub events: Vec<p::Event>,
    pub witnesses: Vec<crate::CoreBrainCEventWitness>,
    pub stream_heads: BTreeMap<p::RunId, u64>,
    pub files: Vec<ProductFileEvidence>,
    pub restart: Option<ProductRestartEvidence>,
    pub latency_ms: u64,
    pub usage: p::ModelUsage,
    pub cost_microunits: Option<u64>,
    /// Fixed diagnostic codes only; never a raw provider error or endpoint.
    pub unavailable: Option<String>,
}
impl ProductAttempt {
    pub fn empty() -> Self {
        Self {
            schema_version: p::SchemaVersion(1),
            output: None,
            model_profiles: BTreeSet::new(),
            events: Vec::new(),
            witnesses: Vec::new(),
            stream_heads: BTreeMap::new(),
            files: Vec::new(),
            restart: None,
            latency_ms: 0,
            usage: p::ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
            },
            cost_microunits: None,
            unavailable: None,
        }
    }
    /// Preserve every stream position without exporting private input/deltas.
    pub fn capture_events(&mut self, events: &[p::Event]) -> p::Result<()> {
        let mut ordered = events.iter().collect::<Vec<_>>();
        ordered.sort_by(|a, b| {
            a.run_id
                .cmp(&b.run_id)
                .then(a.stream_seq.cmp(&b.stream_seq))
        });
        self.events.clear();
        self.witnesses.clear();
        self.stream_heads.clear();
        self.model_profiles.clear();
        for e in ordered {
            self.stream_heads.insert(e.run_id.clone(), e.stream_seq);
            self.witnesses.push(crate::CoreBrainCEventWitness {
                schema_version: p::SchemaVersion(1),
                run: e.run_id.clone(),
                seq: e.stream_seq,
                id: e.event_id.clone(),
                kind: e.kind,
                digest: p::canonical_digest(e)?,
                event: None,
            });
            if portable_product_event(e) {
                self.events.push(e.clone());
            }
            if let p::EventPayload::ModelCallStarted(v) = &e.payload {
                self.model_profiles.insert(v.model_profile.clone());
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductTaskEvidence {
    pub schema_version: p::SchemaVersion,
    pub case: ProductGoldenCase,
    pub contract: Vec<ProductCriterion>,
    pub forme: Vec<ProductAttempt>,
    pub direct_model: Vec<ProductAttempt>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreBrainDArtifact {
    pub schema_version: p::SchemaVersion,
    pub kind: String,
    pub source_commit: String,
    pub grade: ProductEvidenceGrade,
    pub taxonomy_digest: p::SchemaDigest,
    pub configuration_digest: Option<p::SchemaDigest>,
    pub tasks: Vec<ProductTaskEvidence>,
    pub digest: p::SchemaDigest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductTaskScore {
    pub case: ProductGoldenCase,
    pub quality_passes: u64,
    pub quality_trials: u64,
    pub baseline_quality_passes: u64,
    pub baseline_trials: u64,
    pub workflow_passes: u64,
    pub workflow_trials: u64,
    pub unverifiable: u64,
    pub consistent: bool,
    pub baseline_consistent: bool,
    pub latency_ms: u64,
    pub baseline_latency_ms: u64,
    pub cost_microunits: Option<u64>,
    pub baseline_cost_microunits: Option<u64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub interrupt_count: u64,
    pub hard_invariants: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductClosureScore {
    pub verdict: ProductVerdict,
    pub tasks: Vec<ProductTaskScore>,
}
impl CoreBrainDArtifact {
    pub fn unavailable(source_commit: String) -> p::Result<Self> {
        let tasks = ProductGoldenCase::ALL
            .into_iter()
            .map(|case| ProductTaskEvidence {
                schema_version: p::SchemaVersion(1),
                case,
                contract: case.contract(),
                forme: vec![],
                direct_model: vec![],
            })
            .collect();
        let mut a = Self {
            schema_version: p::SchemaVersion(1),
            kind: "forme-core-brain-d".into(),
            source_commit,
            grade: ProductEvidenceGrade::Unavailable,
            taxonomy_digest: p::canonical_digest(&p::EventKind::ALL.to_vec())?,
            configuration_digest: None,
            tasks,
            digest: p::SchemaDigest(String::new()),
        };
        a.seal()?;
        Ok(a)
    }
    pub fn seal(&mut self) -> p::Result<()> {
        self.digest = p::SchemaDigest(String::new());
        self.digest = p::canonical_digest(self)?;
        Ok(())
    }
    pub fn verify(&self) -> p::Result<ProductClosureScore> {
        let mut copy = self.clone();
        copy.seal()?;
        if self.schema_version.0 != 1
            || self.kind != "forme-core-brain-d"
            || copy.digest != self.digest
            || self.taxonomy_digest != p::canonical_digest(&p::EventKind::ALL.to_vec())?
            || !matches!(self.source_commit.len(), 40 | 64)
            || !self.source_commit.bytes().all(|b| b.is_ascii_hexdigit())
            || self.tasks.len() != 6
        {
            return Err(p::Error(
                "D artifact identity, taxonomy or digest is invalid".into(),
            ));
        }
        let text = serde_json::to_string(self)
            .map_err(|_| p::Error("D artifact encoding failed".into()))?;
        if [
            "http://",
            "https://",
            "authorization:",
            "bearer ",
            "secret:",
            "credential:",
            ":\\\\",
            "/home/",
            "/Users/",
            "/private/",
        ]
        .iter()
        .any(|m| text.to_ascii_lowercase().contains(&m.to_ascii_lowercase()))
        {
            return Err(p::Error(
                "D artifact contains private transport or path data".into(),
            ));
        }
        if let Some(d) = &self.configuration_digest {
            p::sha256_digest_bytes(d)?;
        }
        if self.grade == ProductEvidenceGrade::Unavailable
            && self
                .tasks
                .iter()
                .any(|t| !t.forme.is_empty() || !t.direct_model.is_empty())
        {
            return Err(p::Error(
                "unavailable evidence cannot contain a completed trial".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut scores = Vec::new();
        let mut failed = false;
        let mut unknown = self.grade != ProductEvidenceGrade::LiveConfigured
            || self.configuration_digest.is_none();
        for task in &self.tasks {
            if task.schema_version.0 != 1
                || !seen.insert(task.case)
                || task.contract != task.case.contract()
                || task.forme.len() > 8
                || task.direct_model.len() > 8
            {
                return Err(p::Error("D task set or frozen selectors changed".into()));
            }
            let mut score = ProductTaskScore {
                case: task.case,
                quality_passes: 0,
                quality_trials: 0,
                baseline_quality_passes: 0,
                baseline_trials: 0,
                workflow_passes: 0,
                workflow_trials: 0,
                unverifiable: 0,
                consistent: true,
                baseline_consistent: true,
                latency_ms: 0,
                baseline_latency_ms: 0,
                cost_microunits: Some(0),
                baseline_cost_microunits: Some(0),
                input_tokens: 0,
                output_tokens: 0,
                interrupt_count: 0,
                hard_invariants: true,
            };
            if task.forme.len() < 2 || task.direct_model.len() < 2 {
                unknown = true;
                score.unverifiable += 1;
            }
            for (baseline, attempts) in [(false, &task.forme), (true, &task.direct_model)] {
                let mut outputs = BTreeSet::new();
                for attempt in attempts {
                    let ledger = validate_attempt(attempt, baseline)?;
                    if attempt.unavailable.is_some()
                        || attempt.output.is_none()
                        || attempt.model_profiles.is_empty()
                    {
                        unknown = true;
                        score.unverifiable += 1;
                        continue;
                    }
                    outputs.insert(
                        attempt
                            .output
                            .as_deref()
                            .unwrap_or_default()
                            .trim()
                            .to_owned(),
                    );
                    let quality = attempt.output.as_deref().is_some_and(|s| s.trim() == "47");
                    if baseline {
                        score.baseline_trials += 1;
                        score.baseline_quality_passes += u64::from(quality);
                        score.baseline_latency_ms =
                            score.baseline_latency_ms.saturating_add(attempt.latency_ms);
                        score.baseline_cost_microunits =
                            sum_cost(score.baseline_cost_microunits, attempt.cost_microunits);
                    } else {
                        score.quality_trials += 1;
                        score.quality_passes += u64::from(quality);
                        score.workflow_trials += 1;
                        let workflow = task
                            .contract
                            .iter()
                            .all(|c| criterion_passes(c, attempt, &ledger));
                        score.workflow_passes += u64::from(workflow);
                        score.latency_ms = score.latency_ms.saturating_add(attempt.latency_ms);
                        score.cost_microunits =
                            sum_cost(score.cost_microunits, attempt.cost_microunits);
                        score.input_tokens = score
                            .input_tokens
                            .saturating_add(attempt.usage.input_tokens);
                        score.output_tokens = score
                            .output_tokens
                            .saturating_add(attempt.usage.output_tokens);
                        score.interrupt_count+=attempt.events.iter().filter(|e|matches!(&e.payload,p::EventPayload::ProactiveProposalEmitted(v) if v.delivery==p::DeliveryMode::Interrupt)).count() as u64;
                        if !workflow || !quality {
                            failed = true;
                        }
                    }
                }
                if baseline {
                    score.baseline_consistent = outputs.len() <= 1;
                } else {
                    score.consistent = outputs.len() <= 1;
                }
            }
            scores.push(score);
        }
        Ok(ProductClosureScore {
            verdict: if failed {
                ProductVerdict::Fail
            } else if unknown {
                ProductVerdict::Unverifiable
            } else {
                ProductVerdict::Pass
            },
            tasks: scores,
        })
    }
}
fn sum_cost(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    a.zip(b).map(|(a, b)| a.saturating_add(b))
}
fn validate_attempt(a: &ProductAttempt, baseline: bool) -> p::Result<p::OrchestrationLedger> {
    if a.schema_version.0 != 1
        || a.events.len() > 50_000
        || a.witnesses.len() > 50_000
        || a.output.as_ref().is_some_and(|s| s.len() > 65_536)
        || a.unavailable.as_ref().is_some_and(|s| {
            s.is_empty()
                || s.len() > 96
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        })
    {
        return Err(p::Error("D attempt schema or bound is invalid".into()));
    }
    let mut heads = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut witnesses = BTreeMap::new();
    let mut previous = None;
    for w in &a.witnesses {
        p::sha256_digest_bytes(&w.digest)?;
        if w.schema_version.0 != 1
            || w.event.is_some()
            || w.seq != heads.get(&w.run).copied().unwrap_or(0) + 1
            || !ids.insert(w.id.clone())
            || previous.is_some_and(|(r, s)| (r, s) >= (&w.run, w.seq))
        {
            return Err(p::Error(
                "D full stream witness has a gap or duplicate".into(),
            ));
        }
        heads.insert(w.run.clone(), w.seq);
        previous = Some((&w.run, w.seq));
        witnesses.insert((&w.run, w.seq), w);
    }
    if heads != a.stream_heads
        || a.events.len()
            != a.witnesses
                .iter()
                .filter(|w| portable_product_kind(w.kind))
                .count()
    {
        return Err(p::Error(
            "D stream heads or required portable event set changed".into(),
        ));
    }
    let mut ledger = p::OrchestrationLedger::default();
    let mut positions = BTreeMap::new();
    let mut calls = BTreeMap::new();
    let mut finished = BTreeSet::new();
    let mut planned = BTreeMap::new();
    let mut unknown = BTreeSet::new();
    let mut approvals = BTreeMap::new();
    let mut granted = BTreeSet::new();
    let mut actual_profiles = BTreeSet::new();
    let mut actual_usage = p::ModelUsage {
        input_tokens: 0,
        output_tokens: 0,
    };
    let mut events = a.events.iter().collect::<Vec<_>>();
    events.sort_by(|a, b| {
        a.run_id
            .cmp(&b.run_id)
            .then(a.stream_seq.cmp(&b.stream_seq))
    });
    for e in &events {
        if e.schema_version.0 != 1
            || e.stream_seq == 0
            || e.kind != e.payload.kind()
            || positions
                .insert((&e.run_id, e.stream_seq), e.event_id.clone())
                .is_some()
            || !portable_product_event(e)
        {
            return Err(p::Error("D event witness is invalid or unsafe".into()));
        }
        let w = witnesses
            .get(&(&e.run_id, e.stream_seq))
            .ok_or_else(|| p::Error("D event has no full stream witness".into()))?;
        if w.id != e.event_id || w.kind != e.kind || w.digest != p::canonical_digest(e)? {
            return Err(p::Error(
                "D event changed independently of its witness".into(),
            ));
        }
        match &e.payload {
            p::EventPayload::ModelCallStarted(v) => {
                if calls
                    .insert(v.call_id.clone(), v.model_profile.clone())
                    .is_some()
                {
                    return Err(p::Error("D repeated a model attempt identity".into()));
                }
                actual_profiles.insert(v.model_profile.clone());
            }
            p::EventPayload::ModelCallFinished(v) => {
                if calls.get(&v.call_id) != Some(&v.model_profile)
                    || !finished.insert(v.call_id.clone())
                {
                    return Err(p::Error(
                        "D model finish changed its actual provider".into(),
                    ));
                }
                actual_usage.input_tokens = actual_usage
                    .input_tokens
                    .saturating_add(v.usage.input_tokens);
                actual_usage.output_tokens = actual_usage
                    .output_tokens
                    .saturating_add(v.usage.output_tokens);
                if v.finish_reason.0 == "provider_error" && a.cost_microunits.is_some() {
                    return Err(p::Error(
                        "D failed provider cost is not independently known".into(),
                    ));
                }
            }
            p::EventPayload::ApprovalRequested(v) => {
                approvals.insert(v.approval_id.clone(), v);
            }
            p::EventPayload::ApprovalResolved(v) if v.outcome == p::ApprovalOutcome::Granted => {
                granted.insert(v.approval_id.clone());
            }
            p::EventPayload::ActionPlanned(v) => {
                if unknown.contains(&v.intent_id) {
                    return Err(p::Error("D attempted to retry an unknown effect".into()));
                }
                planned.insert(v.intent_id.clone(), v);
            }
            p::EventPayload::ActionStarted(v) => {
                let plan = planned
                    .get(&v.intent_id)
                    .ok_or_else(|| p::Error("D driver started without a final plan".into()))?;
                if plan.backend != v.backend
                    || plan.scope != v.scope
                    || unknown.contains(&v.intent_id)
                {
                    return Err(p::Error(
                        "D driver changed its plan or repeated an unknown effect".into(),
                    ));
                }
                if let Some(id) = &plan.approval_ref {
                    let request = approvals
                        .get(id)
                        .ok_or_else(|| p::Error("D action lacks its approval request".into()))?;
                    if !granted.contains(id)
                        || request.plan_digest.as_ref() != Some(&plan.plan_digest)
                        || request.scope != v.scope
                        || e.ts_unix_ms > request.expires_at
                    {
                        return Err(p::Error(
                            "D action approval changed plan, scope or validity".into(),
                        ));
                    }
                }
            }
            p::EventPayload::ActionOutcomeUnknown(v) => {
                unknown.insert(v.intent_id.clone());
            }
            _ => {}
        }
        ledger.fold(e)?;
    }
    if !baseline && a.unavailable.is_none() && (actual_profiles!=a.model_profiles||calls.len()!=finished.len()||a.usage!=actual_usage||!events.iter().any(|e|matches!(&e.payload,p::EventPayload::SessionBound(v) if v.composition_manifest.is_some()))||calls.is_empty()) {return Err(p::Error("D task lacks complete formal runtime/model/usage evidence".into()));}
    let mut paths = BTreeSet::new();
    for f in &a.files {
        if f.schema_version.0 != 1
            || f.relative_path != "result.txt"
            || !paths.insert(&f.relative_path)
            || f.digest != p::sha256_content_digest(&f.bytes)
            || f.bytes.len() > 65536
        {
            return Err(p::Error("D file selector or digest is invalid".into()));
        }
    }
    if let Some(r) = &a.restart {
        if r.schema_version.0 != 1
            || r.owner_before != r.owner_after
            || r.state_before != r.state_after
        {
            return Err(p::Error("D SQLite restart did not reconcile".into()));
        }
    }
    Ok(ledger)
}
pub fn portable_product_event(e: &p::Event) -> bool {
    portable_product_kind(e.kind)
}
fn portable_product_kind(kind: p::EventKind) -> bool {
    matches!(
        kind,
        p::EventKind::SessionBound
            | p::EventKind::ModelCallStarted
            | p::EventKind::ModelCallFinished
            | p::EventKind::ActionPlanned
            | p::EventKind::ActionStarted
            | p::EventKind::ActionCompleted
            | p::EventKind::ActionFailed
            | p::EventKind::ActionOutcomeUnknown
            | p::EventKind::ApprovalRequested
            | p::EventKind::ApprovalResolved
            | p::EventKind::VerificationFinished
            | p::EventKind::RunComplete
            | p::EventKind::RunFailed
            | p::EventKind::SubagentResultReturned
            | p::EventKind::DecisionTraceRecorded
            | p::EventKind::ProactiveProposalEmitted
            | p::EventKind::ObservationRecorded
            | p::EventKind::UserAttributeCandidateCreated
            | p::EventKind::WorkspaceCharterChanged
            | p::EventKind::DataLifecycleApplied
    )
}
fn criterion_passes(c: &ProductCriterion, a: &ProductAttempt, l: &p::OrchestrationLedger) -> bool {
    match (&c.selector, &c.predicate) {
        (ProductSelector::FinalOutput, ProductPredicate::Exact(expected)) => {
            a.output.as_ref().is_some_and(|v| v.trim() == expected)
        }
        (ProductSelector::File(path), ProductPredicate::Exact(expected)) => a
            .files
            .iter()
            .any(|f| &f.relative_path == path && f.bytes == expected.as_bytes()),
        (ProductSelector::EventCount(kind), ProductPredicate::AtLeast(n)) => {
            a.events.iter().filter(|e| e.kind == *kind).count() as u64 >= *n
        }
        (selector, ProductPredicate::AtLeast(n)) => {
            let count = match selector {
                ProductSelector::Restart => u64::from(a.restart.is_some()),
                ProductSelector::Checkpoints => {
                    l.routes.values().filter(|r| r.checkpointed).count() as u64
                }
                ProductSelector::CompletedRoutes => l
                    .routes
                    .values()
                    .filter(|r| r.closed && r.status == p::WorkRouteStatus::Completed)
                    .count() as u64,
                ProductSelector::FailedRoutes => l
                    .routes
                    .values()
                    .filter(|r| r.closed && r.status == p::WorkRouteStatus::Failed)
                    .count() as u64,
                _ => 0,
            };
            count >= *n
        }
        _ => false,
    }
}
