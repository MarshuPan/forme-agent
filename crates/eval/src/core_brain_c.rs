//! Portable C-wave evidence. Only self-authored controlled fixtures are exportable.
//! The verifier never opens an owner store or calls a model/driver.
use std::collections::{BTreeMap, BTreeSet};

use forme_protocol as p;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreBrainCCase {
    Patterns,
    Conflict,
    Recovery,
    Isolation,
    Supervision,
    Learning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreBrainCEventWitness {
    pub schema_version: p::SchemaVersion,
    pub run: p::RunId,
    pub seq: u64,
    pub id: p::EventId,
    pub kind: p::EventKind,
    pub digest: p::SchemaDigest,
    pub event: Option<p::Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreBrainCFileEvidence {
    pub schema_version: p::SchemaVersion,
    pub route: p::ExecutionRouteRef,
    pub node: String,
    pub attempt: u32,
    pub relative_path: String,
    pub digest: p::SchemaDigest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreBrainCArtifact {
    pub schema_version: p::SchemaVersion,
    pub kind: String,
    pub case: CoreBrainCCase,
    pub evidence_grade: String,
    pub source_commit: String,
    pub composition_digest: p::SchemaDigest,
    pub taxonomy_digest: p::SchemaDigest,
    pub charter_stream: p::RunId,
    pub model_profiles: BTreeSet<p::ModelProfileRef>,
    pub model_bindings: BTreeMap<p::ModelProfileRef, p::ModelProfileRef>,
    pub stream_heads: BTreeMap<p::RunId, u64>,
    pub witnesses: Vec<CoreBrainCEventWitness>,
    pub online_ledger_digest: p::SchemaDigest,
    pub files: Vec<CoreBrainCFileEvidence>,
    pub digest: p::SchemaDigest,
}

impl CoreBrainCArtifact {
    #[allow(clippy::too_many_arguments)]
    pub fn capture(
        case: CoreBrainCCase,
        source_commit: String,
        composition_digest: p::SchemaDigest,
        charter_stream: p::RunId,
        model_bindings: BTreeMap<p::ModelProfileRef, p::ModelProfileRef>,
        events: &[p::Event],
        online: &p::OrchestrationLedger,
        files: Vec<CoreBrainCFileEvidence>,
    ) -> p::Result<Self> {
        let mut ordered = events.iter().collect::<Vec<_>>();
        ordered.sort_by(|a, b| {
            a.run_id
                .cmp(&b.run_id)
                .then(a.stream_seq.cmp(&b.stream_seq))
        });
        let mut heads = BTreeMap::new();
        let mut witnesses = Vec::new();
        for e in ordered {
            heads.insert(e.run_id.clone(), e.stream_seq);
            witnesses.push(CoreBrainCEventWitness {
                schema_version: p::SchemaVersion(1),
                run: e.run_id.clone(),
                seq: e.stream_seq,
                id: e.event_id.clone(),
                kind: e.kind,
                digest: p::canonical_digest(e)?,
                event: portable(e).then(|| e.clone()),
            });
        }
        let mut artifact = Self {
            schema_version: p::SchemaVersion(1),
            kind: "forme-core-brain-c".into(),
            case,
            evidence_grade: "controlled-http-real-runtime-sqlite-files".into(),
            source_commit,
            composition_digest,
            taxonomy_digest: p::canonical_digest(&p::EventKind::ALL.to_vec())?,
            charter_stream,
            model_profiles: model_bindings.values().cloned().collect(),
            model_bindings,
            stream_heads: heads,
            witnesses,
            online_ledger_digest: p::canonical_digest(online)?,
            files,
            digest: p::SchemaDigest(String::new()),
        };
        artifact.digest = artifact.content_digest()?;
        artifact.validate()?;
        Ok(artifact)
    }
    fn content_digest(&self) -> p::Result<p::SchemaDigest> {
        let mut copy = self.clone();
        copy.digest = p::SchemaDigest(String::new());
        p::canonical_digest(&copy)
    }
    pub fn validate(&self) -> p::Result<p::OrchestrationLedger> {
        let fail = || p::Error("C-wave artifact binding, replay or evidence check failed".into());
        if self.schema_version != p::SchemaVersion(1)
            || self.kind != "forme-core-brain-c"
            || self.evidence_grade != "controlled-http-real-runtime-sqlite-files"
            || self.source_commit.len() != 40
            || !self.source_commit.bytes().all(|b| b.is_ascii_hexdigit())
            || !valid_digest(&self.composition_digest)
            || !valid_digest(&self.online_ledger_digest)
            || self.taxonomy_digest != p::canonical_digest(&p::EventKind::ALL.to_vec())?
            || p::EventKind::ALL.len() != 99
            || self.digest != self.content_digest()?
            || self.witnesses.is_empty()
            || self.witnesses.len() > 16_384
            || self.files.len() > 128
            || self.model_profiles.is_empty()
            || self.model_profiles != self.model_bindings.values().cloned().collect()
        {
            return Err(p::Error(
                "C-wave artifact header or digest is invalid".into(),
            ));
        }
        let bytes = serde_json::to_vec(self).map_err(|_| fail())?;
        if bytes.len() > 8_388_608 {
            return Err(fail());
        }
        reject_private(&serde_json::from_slice(&bytes).map_err(|_| fail())?)?;
        let mut heads = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut ledger = p::OrchestrationLedger::default();
        let mut included = Vec::new();
        let mut previous: Option<(&p::RunId, u64)> = None;
        for w in &self.witnesses {
            if w.schema_version != p::SchemaVersion(1)
                || !valid_digest(&w.digest)
                || !ids.insert(w.id.clone())
                || w.seq != heads.get(&w.run).copied().unwrap_or(0) + 1
                || previous.is_some_and(|(r, s)| (r, s) >= (&w.run, w.seq))
            {
                return Err(p::Error(format!(
                    "C-wave witness sequence or identity is invalid at {} {:?}",
                    w.seq, w.kind
                )));
            }
            previous = Some((&w.run, w.seq));
            heads.insert(w.run.clone(), w.seq);
            // All durable orchestration facts in its sole aggregate must be present.
            if w.run == self.charter_stream
                && matches!(
                    w.kind,
                    p::EventKind::DecisionTraceRecorded | p::EventKind::WorkspaceCharterChanged
                )
                && w.event.is_none()
            {
                return Err(p::Error("C-wave charter control payload is missing".into()));
            }
            if mandatory_kind(w.kind) && w.event.is_none() {
                return Err(p::Error(
                    "C-wave mandatory runtime payload is missing".into(),
                ));
            }
            if let Some(e) = &w.event {
                if !portable(e)
                    || e.schema_version != p::SchemaVersion(1)
                    || e.run_id != w.run
                    || e.stream_seq != w.seq
                    || e.event_id != w.id
                    || e.kind != w.kind
                    || p::canonical_digest(e)? != w.digest
                {
                    return Err(p::Error(
                        "C-wave event does not match its portable witness".into(),
                    ));
                }
                e.validate_payload_kind()?;
                if let p::EventPayload::DecisionTraceRecorded(t) = &e.payload {
                    if t.orchestration.is_some()
                        && (e.run_id != self.charter_stream
                            || e.provenance.trust_tier == p::TrustTier::Untrusted)
                    {
                        return Err(p::Error("C-wave orchestration authority is invalid".into()));
                    }
                }
                if e.run_id == self.charter_stream {
                    ledger.fold(e)?;
                }
                if let p::EventPayload::ModelCallStarted(c) = &e.payload {
                    if !self.model_profiles.contains(&c.model_profile) {
                        return Err(p::Error(
                            "C-wave event model profile is unregistered".into(),
                        ));
                    }
                }
                included.push(e);
            }
        }
        if heads != self.stream_heads || ledger.routes.is_empty() {
            return Err(p::Error(
                "C-wave stream range or route coverage differs".into(),
            ));
        }
        if p::canonical_digest(&ledger)? != self.online_ledger_digest {
            return Err(p::Error(
                "C-wave replay differs from the online ledger".into(),
            ));
        }
        for route in ledger.routes.values() {
            let reserved: u64 = route.nodes.values().map(|n| n.reserved).sum();
            if route.remaining + route.spent + reserved != route.declaration.budget_units
                || route.checkpointed && (!route.closed || route.has_inflight())
            {
                return Err(fail());
            }
            for (id, node) in &route.nodes {
                let spec = route.node_spec(id).ok_or_else(fail)?;
                let runs = (1..=node.attempts)
                    .map(|a| route.declaration.node_run_id(id, a))
                    .collect::<BTreeSet<_>>();
                let calls = included
                    .iter()
                    .filter(|e| {
                        runs.contains(&e.run_id) && e.kind == p::EventKind::ModelCallStarted
                    })
                    .count() as u64;
                let expected_profile = self.model_bindings.get(&spec.model).ok_or_else(fail)?;
                if included.iter().any(|e|runs.contains(&e.run_id) && matches!(&e.payload,p::EventPayload::ModelCallStarted(c) if &c.model_profile!=expected_profile)) {return Err(fail())}
                if calls > node.consumed + node.reserved
                    || (node.reserved == 0 && calls != node.consumed)
                {
                    return Err(p::Error(format!(
                        "C-wave node accounting differs: calls={calls}, consumed={}, reserved={}",
                        node.consumed, node.reserved
                    )));
                }
                if node.status == p::WorkNodeStatus::Done {
                    let run = route.declaration.node_run_id(id, node.attempts);
                    if !included
                        .iter()
                        .any(|e| e.run_id == run && e.kind == p::EventKind::RunComplete)
                        || node.verification != p::VerificationOutcome::Pass
                    {
                        return Err(fail());
                    }
                    for req in &spec.requirements {
                        match req {
                            p::RouteRequirement::OutputContains(text)
                                if !node.summary.contains(text) =>
                            {
                                return Err(fail())
                            }
                            p::RouteRequirement::ActionSucceeded
                                if !included.iter().any(|e| {
                                    e.run_id == run && e.kind == p::EventKind::ActionCompleted
                                }) =>
                            {
                                return Err(fail())
                            }
                            p::RouteRequirement::FileDigest {
                                relative_path,
                                digest,
                            } if !self.files.iter().any(|f| {
                                f.route == route.declaration.route
                                    && f.node == *id
                                    && f.attempt == node.attempts
                                    && f.relative_path == *relative_path
                                    && f.digest.0 == *digest
                            }) =>
                            {
                                return Err(fail())
                            }
                            _ => {}
                        }
                    }
                }
            }
            if route.closed {
                let episode = format!("route-episode:{}", route.declaration.route.0);
                if !included.iter().any(|e|matches!(&e.payload,p::EventPayload::MemoryNodeAppended(n) if n.node_id.0==episode))
                    || !included.iter().any(|e|matches!(&e.payload,p::EventPayload::CandidateCreated(c) if c.candidate_id.0==format!("route-strategy:{}",route.declaration.route.0)
                        && c.strategy_candidate.as_ref().is_some_and(|s|s.domain==p::StrategyDomain::Coordination&&s.evidence.contains(&p::EvidenceRef(episode.clone()))))) {return Err(fail())}
            }
        }
        for file in &self.files {
            if file.schema_version != p::SchemaVersion(1)
                || file.relative_path.is_empty()
                || file
                    .relative_path
                    .split(['/', '\\'])
                    .any(|s| s.is_empty() || s == "." || s == "..")
                || file.relative_path.contains(':')
                || !valid_digest(&file.digest)
                || !ledger.routes.get(&file.route).is_some_and(|r| {
                    r.nodes
                        .get(&file.node)
                        .is_some_and(|n| n.attempts == file.attempt)
                })
            {
                return Err(fail());
            }
        }
        if self.witnesses.iter().any(|w| {
            matches!(
                w.kind,
                p::EventKind::CandidatePromoted | p::EventKind::StrategyActivated
            )
        }) {
            return Err(fail());
        }
        self.validate_case(&ledger, &included)?;
        Ok(ledger)
    }
    fn validate_case(
        &self,
        ledger: &p::OrchestrationLedger,
        events: &[&p::Event],
    ) -> p::Result<()> {
        let routes = ledger.routes.values().collect::<Vec<_>>();
        let ok = match self.case {
            CoreBrainCCase::Patterns => {
                let names = routes
                    .iter()
                    .filter_map(|r| r.declaration.pattern.as_ref().map(|p| p.0.as_str()))
                    .collect::<BTreeSet<_>>();
                names
                    == [
                        "pattern:single-run",
                        "pattern:retrieve-then-answer",
                        "pattern:tool-loop",
                        "pattern:clarify-then-act",
                        "pattern:generator-evaluator",
                        "pattern:parallel-research-fanout",
                        "pattern:planner-builder-evaluator-integrator",
                        "pattern:long-running-harness",
                    ]
                    .into_iter()
                    .collect()
                    && routes.len() == 8
                    && routes.iter().all(|r| r.closed && r.checkpointed)
                    && events
                        .iter()
                        .filter(|e| e.kind == p::EventKind::SubagentSpawned)
                        .count()
                        == routes
                            .iter()
                            .map(|r| r.declaration.nodes.len())
                            .sum::<usize>()
            }
            CoreBrainCCase::Conflict => {
                routes
                    .iter()
                    .any(|r| r.status == p::WorkRouteStatus::Conflict && !r.conflicts.is_empty())
                    && routes
                        .iter()
                        .any(|r| r.status == p::WorkRouteStatus::Cancelled)
            }
            CoreBrainCCase::Recovery => {
                routes.iter().any(|r| {
                    r.status == p::WorkRouteStatus::OutcomeUnknown
                        && r.has_inflight()
                        && !r.checkpointed
                }) && events
                    .iter()
                    .filter(|e| e.kind == p::EventKind::ActionOutcomeUnknown)
                    .count()
                    == 1
                    && !events.iter().any(|e| {
                        matches!(
                            e.kind,
                            p::EventKind::ModelCallStarted | p::EventKind::ActionCompleted
                        )
                    })
            }
            CoreBrainCCase::Isolation => {
                self.files.len() == 2
                    && self.files[0].node != self.files[1].node
                    && events
                        .iter()
                        .filter(|e| e.kind == p::EventKind::ActionStarted)
                        .count()
                        == 2
                    && events
                        .iter()
                        .filter(|e| e.kind == p::EventKind::ActionCompleted)
                        .count()
                        == 2
                    && events
                        .iter()
                        .filter(|e| e.kind == p::EventKind::ApprovalResolved)
                        .count()
                        == 2
                    && routes
                        .iter()
                        .all(|r| r.status == p::WorkRouteStatus::Completed)
            }
            CoreBrainCCase::Supervision => {
                routes.len() >= 2
                    && routes
                        .iter()
                        .any(|r| r.closed && r.checkpointed && r.declaration.nodes.is_empty())
                    && routes.iter().any(|r| {
                        !r.closed
                            && !r.declaration.nodes.is_empty()
                            && r.nodes.values().all(|n| n.attempts == 0)
                    })
            }
            CoreBrainCCase::Learning => {
                routes
                    .iter()
                    .any(|r| r.status == p::WorkRouteStatus::Completed)
                    && routes
                        .iter()
                        .any(|r| r.status == p::WorkRouteStatus::Failed)
                    && events
                        .iter()
                        .any(|e| e.kind == p::EventKind::FailureDigestUpdated)
            }
        };
        if ok {
            Ok(())
        } else {
            Err(p::Error(
                "C-wave case lacks its required runtime evidence".into(),
            ))
        }
    }
}

fn portable(e: &p::Event) -> bool {
    match &e.payload {
        p::EventPayload::DecisionTraceRecorded(t) => t.orchestration.is_some(),
        p::EventPayload::MemoryNodeAppended(_)
        | p::EventPayload::MemoryEdgeAppended(_)
        | p::EventPayload::CandidateCreated(_)
        | p::EventPayload::FailureEvidenceRecorded(_)
        | p::EventPayload::FailureDigestUpdated(_) => e.event_id.0.starts_with("route-learning:"),
        _ => mandatory_kind(e.kind),
    }
}
fn mandatory_kind(kind: p::EventKind) -> bool {
    matches!(
        kind,
        p::EventKind::WorkspaceCharterChanged
            | p::EventKind::ModelCallStarted
            | p::EventKind::ModelCallFinished
            | p::EventKind::RunComplete
            | p::EventKind::RunFailed
            | p::EventKind::RunAborted
            | p::EventKind::RunLimited
            | p::EventKind::ActionStarted
            | p::EventKind::ActionCompleted
            | p::EventKind::ActionOutcomeUnknown
            | p::EventKind::ApprovalResolved
            | p::EventKind::SubagentSpawned
    )
}
fn valid_digest(d: &p::SchemaDigest) -> bool {
    d.0.strip_prefix("sha256:")
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn reject_private(value: &serde_json::Value) -> p::Result<()> {
    match value {
        serde_json::Value::String(s) => {
            let lower = s.to_ascii_lowercase();
            let drive = lower.as_bytes().windows(3).any(|w| {
                w[0].is_ascii_alphabetic() && w[1] == b':' && matches!(w[2], b'/' | b'\\')
            });
            if drive
                || [
                    "http://",
                    "https://",
                    "bearer ",
                    "api_key",
                    "api-key",
                    "credential_env",
                    "secretref:",
                    "secret:",
                    "sk-",
                    "/users/",
                    "/home/",
                    "/tmp/",
                    "\\\\",
                    "forme_core_b_test_key",
                ]
                .iter()
                .any(|p| lower.contains(p))
            {
                return Err(p::Error(
                    "C-wave artifact contains a credential, endpoint or private path marker".into(),
                ));
            }
        }
        serde_json::Value::Array(v) => {
            for x in v {
                reject_private(x)?
            }
        }
        serde_json::Value::Object(v) => {
            for (k, x) in v {
                reject_private(&serde_json::Value::String(k.clone()))?;
                reject_private(x)?
            }
        }
        _ => {}
    }
    Ok(())
}
