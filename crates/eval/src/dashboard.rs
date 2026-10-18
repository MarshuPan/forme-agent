//! Event-derived cognitive dashboard.  The projection is deliberately
//! disposable: the event range is the authority and this module is a pure
//! fold that can be rebuilt after a restart or cache loss.

use std::collections::{BTreeMap, BTreeSet};

use forme_protocol as p;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MetricRatio {
    pub numerator: u64,
    pub denominator: u64,
    pub unverifiable: u64,
}

impl MetricRatio {
    pub const fn empty() -> Self {
        Self {
            numerator: 0,
            denominator: 0,
            unverifiable: 0,
        }
    }

    fn add(&mut self, numerator: u64, denominator: u64) {
        self.numerator = self.numerator.saturating_add(numerator);
        self.denominator = self.denominator.saturating_add(denominator);
    }

    fn mark_unverifiable(&mut self) {
        self.unverifiable = self.unverifiable.saturating_add(1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CognitiveDashboardMetrics {
    pub proposal_adoption: MetricRatio,
    pub proposal_rejection: MetricRatio,
    pub proposal_deferral: MetricRatio,
    pub ask_to_learn_acceptance: MetricRatio,
    pub ask_to_learn_usefulness: MetricRatio,
    pub candidate_resolution: MetricRatio,
    pub verification: MetricRatio,
    pub failures: MetricRatio,
    pub model_attempts: MetricRatio,
    pub model_fallback: MetricRatio,
    pub orchestration: MetricRatio,
    pub candidate_backlog: u64,
    pub interrupt_regret: MetricRatio,
    /// Sum of confidence in thousandths / independently recorded map objects.
    pub map_confidence_millis: MetricRatio,
    pub workspace_supervision: MetricRatio,
    pub pattern_outcomes: BTreeMap<p::OrchestrationPatternRef, MetricRatio>,
    pub model_outcomes: BTreeMap<p::ModelProfileRef, MetricRatio>,
}

impl CognitiveDashboardMetrics {
    fn empty() -> Self {
        Self {
            proposal_adoption: MetricRatio::empty(),
            proposal_rejection: MetricRatio::empty(),
            proposal_deferral: MetricRatio::empty(),
            ask_to_learn_acceptance: MetricRatio::empty(),
            ask_to_learn_usefulness: MetricRatio::empty(),
            candidate_resolution: MetricRatio::empty(),
            verification: MetricRatio::empty(),
            failures: MetricRatio::empty(),
            model_attempts: MetricRatio::empty(),
            model_fallback: MetricRatio::empty(),
            orchestration: MetricRatio::empty(),
            candidate_backlog: 0,
            interrupt_regret: MetricRatio::empty(),
            map_confidence_millis: MetricRatio::empty(),
            workspace_supervision: MetricRatio::empty(),
            pattern_outcomes: BTreeMap::new(),
            model_outcomes: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DashboardWindow {
    pub from: Option<p::Timestamp>,
    pub to: Option<p::Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CognitiveDashboardSnapshot {
    pub schema_version: p::SchemaVersion,
    pub owner_digest: p::SchemaDigest,
    pub workspace: Option<p::WorkspaceRef>,
    pub window: DashboardWindow,
    pub event_count: u64,
    pub cursor: u64,
    /// Per-stream cursor is authoritative; `cursor` is only a display maximum.
    pub stream_heads: BTreeMap<p::RunId, u64>,
    pub metrics: CognitiveDashboardMetrics,
    pub digest: p::SchemaDigest,
}

#[derive(Debug, Clone)]
pub struct CognitiveDashboardProjection;

impl CognitiveDashboardProjection {
    pub fn from_events(
        events: &[p::Event],
        owner_digest: p::SchemaDigest,
        workspace: Option<p::WorkspaceRef>,
        from: Option<p::Timestamp>,
        to: Option<p::Timestamp>,
    ) -> p::Result<CognitiveDashboardSnapshot> {
        if owner_digest.0.trim().is_empty()
            || from.zip(to).is_some_and(|(from, to)| from > to)
            || workspace
                .as_ref()
                .is_some_and(|workspace| workspace.0.trim().is_empty())
        {
            return Err(p::Error("dashboard query is invalid".into()));
        }
        let mut positions = BTreeMap::new();
        for event in events {
            if let Some(existing) = positions.insert((&event.run_id, event.stream_seq), event) {
                if existing != event {
                    return Err(p::Error("dashboard event position conflicts".into()));
                }
            }
        }
        let ordered = positions.into_values().collect::<Vec<_>>();
        let available = ordered
            .iter()
            .copied()
            .filter(|event| to.is_none_or(|t| event.ts_unix_ms <= t))
            .collect::<Vec<_>>();
        let scopes = workspace_scopes(&available);
        let history = available
            .into_iter()
            .filter(|event| {
                workspace
                    .as_ref()
                    .is_none_or(|w| scopes.get(&event.event_id) == Some(w))
            })
            .collect::<Vec<_>>();
        let filtered = history
            .iter()
            .copied()
            .filter(|event| from.is_none_or(|from| event.ts_unix_ms >= from))
            .collect::<Vec<_>>();
        let mut metrics = fold_metrics(&filtered, &history);
        let state_metrics = fold_metrics(&history, &history);
        metrics.candidate_backlog = state_metrics.candidate_backlog;
        metrics.map_confidence_millis = state_metrics.map_confidence_millis;
        metrics.workspace_supervision = state_metrics.workspace_supervision;
        // Resolution and outcome windows may refer to an earlier proposal or
        // route declaration. Use history for identity, the window for counts.
        let (learning, interrupts) = proposal_sets(&history);
        let explicitly_resolved = filtered
            .iter()
            .filter_map(|e| {
                if let p::EventPayload::ProactiveProposalResolved(v) = &e.payload {
                    Some(v.proposal_ref.clone())
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        let mut answered = BTreeSet::new();
        for event in &filtered {
            if let p::EventPayload::ProactiveProposalResolved(v) = &event.payload {
                if learning.contains(&v.proposal_ref) {
                    metrics
                        .ask_to_learn_acceptance
                        .add(u64::from(v.outcome == p::ProposalOutcome::Adopt), 1);
                }
                if interrupts.contains(&v.proposal_ref) {
                    metrics.interrupt_regret.mark_unverifiable();
                }
            }
            if let p::EventPayload::ObservationRecorded(v) = &event.payload {
                if let Some(reply) = &v.learning_reply {
                    if learning.contains(&reply.proposal)
                        && answered.insert(reply.proposal.clone())
                        && !explicitly_resolved.contains(&reply.proposal)
                    {
                        // An owner answer accepts the question; it is not
                        // evidence that the resulting knowledge was useful.
                        metrics.ask_to_learn_acceptance.add(1, 1);
                    }
                }
            }
        }
        let stream_heads = history.iter().fold(BTreeMap::new(), |mut m, e| {
            let v = m.entry(e.run_id.clone()).or_insert(0);
            *v = (*v).max(e.stream_seq);
            m
        });
        let cursor = filtered
            .iter()
            .map(|event| event.stream_seq)
            .max()
            .unwrap_or(0);
        let digest = p::canonical_digest(&(
            &owner_digest,
            &workspace,
            from,
            to,
            history
                .iter()
                .map(|event| {
                    Ok((
                        &event.event_id,
                        event.stream_seq,
                        event.kind,
                        event.ts_unix_ms,
                        p::canonical_digest(&event.payload)?,
                    ))
                })
                .collect::<p::Result<Vec<_>>>()?,
            &metrics,
        ))?;
        Ok(CognitiveDashboardSnapshot {
            schema_version: p::SchemaVersion(1),
            owner_digest,
            workspace,
            window: DashboardWindow { from, to },
            event_count: filtered.len() as u64,
            cursor,
            stream_heads,
            metrics,
            digest,
        })
    }
}

fn explicit_workspace(event: &p::Event) -> Option<p::WorkspaceRef> {
    let scope = match &event.payload {
        p::EventPayload::SessionBound(v) => Some(v.workspace.0.as_str()),
        p::EventPayload::WorkspaceCharterChanged(v) => Some(v.charter.workspace.0.as_str()),
        p::EventPayload::DecisionTraceRecorded(v) => {
            v.orchestration.as_ref().map(|f| f.workspace.0.as_str())
        }
        p::EventPayload::MemoryNodeAppended(v) => Some(v.scope.0.as_str()),
        p::EventPayload::CandidatePromoted(v) => v.stable.as_ref().map(|s| s.scope.0.as_str()),
        p::EventPayload::ProactiveProposalEmitted(v) => {
            v.learning.as_ref().map(|s| s.scope.0.as_str())
        }
        p::EventPayload::CompetenceGateEvaluated(v) => Some(v.scope.0.as_str()),
        p::EventPayload::GoalFramed(v) => v.knowledge.as_ref().map(|s| s.scope.0.as_str()),
        p::EventPayload::ObservationRecorded(v) => Some(v.scope.0.as_str()),
        p::EventPayload::UserAttributeCandidateCreated(v) => Some(v.scope.0.as_str()),
        _ => None,
    };
    scope.map(|s| p::WorkspaceRef(s.into()))
}
fn workspace_scopes(events: &[&p::Event]) -> BTreeMap<p::EventId, p::WorkspaceRef> {
    let runs = events
        .iter()
        .filter_map(|e| {
            if let p::EventPayload::SessionBound(v) = &e.payload {
                Some((e.run_id.clone(), v.workspace.clone()))
            } else {
                None
            }
        })
        .collect::<BTreeMap<_, _>>();
    let mut scopes = events
        .iter()
        .filter_map(|e| {
            explicit_workspace(e)
                .or_else(|| runs.get(&e.run_id).cloned())
                .map(|s| (e.event_id.clone(), s))
        })
        .collect::<BTreeMap<_, _>>();
    loop {
        let before = scopes.len();
        let mut proposals = BTreeMap::new();
        let mut candidates = BTreeMap::new();
        for e in events {
            if let Some(scope) = scopes.get(&e.event_id) {
                if let p::EventPayload::ProactiveProposalEmitted(v) = &e.payload {
                    proposals.insert(v.proposal_ref.clone(), scope.clone());
                }
                let id = match &e.payload {
                    p::EventPayload::CandidateCreated(v) => Some(&v.candidate_id),
                    p::EventPayload::UserAttributeCandidateCreated(v) => Some(&v.candidate_id),
                    _ => candidate_id(&e.payload),
                };
                if let Some(id) = id {
                    candidates.insert(id.clone(), scope.clone());
                }
            }
        }
        for e in events {
            if scopes.contains_key(&e.event_id) {
                continue;
            }
            let mut evidence = Vec::new();
            if let Some(cause) = &e.provenance.caused_by {
                evidence.extend(scopes.get(cause).cloned());
            }
            match &e.payload {
                p::EventPayload::ProactiveProposalResolved(v) => {
                    evidence.extend(proposals.get(&v.proposal_ref).cloned())
                }
                p::EventPayload::CandidateCreated(v) => {
                    evidence.extend(candidates.get(&v.candidate_id).cloned());
                    for r in &v.evidence_refs {
                        evidence.extend(
                            scopes
                                .get(&p::EventId(
                                    r.0.strip_prefix("event:").unwrap_or(&r.0).into(),
                                ))
                                .cloned(),
                        );
                    }
                }
                other => {
                    if let Some(id) = candidate_id(other) {
                        evidence.extend(candidates.get(id).cloned());
                    }
                }
            }
            let unique = evidence.into_iter().collect::<BTreeSet<_>>();
            if unique.len() == 1 {
                scopes.insert(
                    e.event_id.clone(),
                    unique.into_iter().next().expect("one scope"),
                );
            }
        }
        if scopes.len() == before {
            break;
        }
    }
    scopes
}
fn proposal_sets(events: &[&p::Event]) -> (BTreeSet<p::ProposalRef>, BTreeSet<p::ProposalRef>) {
    (
        events
            .iter()
            .filter_map(|e| match &e.payload {
                p::EventPayload::ProactiveProposalEmitted(v) if v.learning.is_some() => {
                    Some(v.proposal_ref.clone())
                }
                _ => None,
            })
            .collect(),
        events
            .iter()
            .filter_map(|e| match &e.payload {
                p::EventPayload::ProactiveProposalEmitted(v)
                    if v.delivery == p::DeliveryMode::Interrupt =>
                {
                    Some(v.proposal_ref.clone())
                }
                _ => None,
            })
            .collect(),
    )
}
fn fold_metrics(events: &[&p::Event], history: &[&p::Event]) -> CognitiveDashboardMetrics {
    let mut metrics = CognitiveDashboardMetrics::empty();
    let mut candidates = BTreeSet::new();
    let mut resolved = BTreeSet::new();
    let mut profiles_by_run = BTreeMap::<p::RunId, BTreeSet<p::ModelProfileRef>>::new();
    let mut projects = BTreeSet::new();
    let mut supervised = BTreeSet::new();
    let mut patterns = BTreeMap::new();
    let mut finishes = BTreeMap::new();
    let window_runs = events
        .iter()
        .filter(|e| matches!(e.payload, p::EventPayload::ModelCallStarted(_)))
        .map(|e| e.run_id.clone())
        .collect::<BTreeSet<_>>();
    for event in history {
        match &event.payload {
            p::EventPayload::ModelCallStarted(v) if window_runs.contains(&event.run_id) => {
                profiles_by_run
                    .entry(event.run_id.clone())
                    .or_default()
                    .insert(v.model_profile.clone());
            }
            p::EventPayload::ModelCallFinished(v) => {
                finishes.insert((event.run_id.clone(), v.call_id.clone()), v);
            }
            p::EventPayload::DecisionTraceRecorded(v) => {
                if let Some(f) = &v.orchestration {
                    if let p::OrchestrationOperation::RouteDeclared(d) = &f.operation {
                        patterns.insert(d.route.clone(), d.pattern.clone());
                    }
                }
            }
            _ => {}
        }
    }
    let mut outcomes = Vec::new();
    let mut map = BTreeMap::new();
    for event in events {
        match &event.payload {
            p::EventPayload::ProactiveProposalResolved(v) => {
                metrics
                    .proposal_adoption
                    .add(u64::from(v.outcome == p::ProposalOutcome::Adopt), 1);
                metrics
                    .proposal_rejection
                    .add(u64::from(v.outcome == p::ProposalOutcome::Reject), 1);
                metrics
                    .proposal_deferral
                    .add(u64::from(v.outcome == p::ProposalOutcome::Defer), 1);
                // Rejection is not a regret label. Without a linked, typed
                // regret observation this stays explicitly unverifiable.
            }
            p::EventPayload::ObservationRecorded(v) if v.learning_reply.is_some() => {
                metrics.ask_to_learn_usefulness.mark_unverifiable()
            }
            p::EventPayload::CandidateCreated(v) => {
                candidates.insert(v.candidate_id.clone());
            }
            p::EventPayload::UserAttributeCandidateCreated(v) => {
                candidates.insert(v.candidate_id.clone());
            }
            p::EventPayload::CandidatePromoted(v) => {
                resolved.insert(v.candidate_id.clone());
                metrics.candidate_resolution.add(1, 1);
                if let Some(s) = &v.stable {
                    if matches!(
                        s.kind.0.as_str(),
                        "frame" | "quality" | "blind-spot" | "resource-relation"
                    ) {
                        map.insert(s.object.clone(), (s.confidence.0 * 1000.0).round() as u64);
                    }
                } else {
                    metrics.map_confidence_millis.mark_unverifiable();
                }
            }
            p::EventPayload::CandidateRejected(_)
            | p::EventPayload::CandidateDowngraded(_)
            | p::EventPayload::CandidateDecayed(_) => {
                if let Some(id) = candidate_id(&event.payload) {
                    resolved.insert(id.clone());
                    metrics.candidate_resolution.add(1, 1);
                }
            }
            p::EventPayload::RetractionEvent(v) => {
                map.remove(&v.target_object);
            }
            p::EventPayload::VerificationFinished(v) => match v.outcome {
                p::VerificationOutcome::Pass => metrics.verification.add(1, 1),
                p::VerificationOutcome::Fail => metrics.verification.add(0, 1),
                p::VerificationOutcome::Unverifiable(_) => metrics.verification.mark_unverifiable(),
            },
            p::EventPayload::FailureEvidenceRecorded(_) => metrics.failures.add(1, 1),
            p::EventPayload::ModelCallStarted(v) => {
                metrics.model_attempts.add(1, 1);
                let metric = metrics
                    .model_outcomes
                    .entry(v.model_profile.clone())
                    .or_insert_with(MetricRatio::empty);
                match finishes.get(&(event.run_id.clone(), v.call_id.clone())) {
                    Some(finished) if finished.model_profile == v.model_profile => {
                        metric.add(u64::from(finished.finish_reason.0 != "provider_error"), 1)
                    }
                    _ => metric.mark_unverifiable(),
                }
            }
            p::EventPayload::DecisionTraceRecorded(v) => {
                if let Some(f) = &v.orchestration {
                    match &f.operation {
                        p::OrchestrationOperation::ProjectDeclared(p)
                        | p::OrchestrationOperation::ProjectRevised { project: p, .. } => {
                            projects.insert(p.reference.clone());
                        }
                        p::OrchestrationOperation::Checkpoint { project, .. } => {
                            supervised.insert(project.clone());
                        }
                        p::OrchestrationOperation::RouteDeclared(d) => {
                            patterns.insert(d.route.clone(), d.pattern.clone());
                        }
                        p::OrchestrationOperation::RouteClosed { route, status } => {
                            outcomes.push((route.clone(), *status));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    metrics.candidate_backlog = candidates.difference(&resolved).count() as u64;
    for value in map.values() {
        metrics.map_confidence_millis.add(*value, 1);
    }
    metrics.workspace_supervision.add(
        projects.intersection(&supervised).count() as u64,
        projects.len() as u64,
    );
    for (route, status) in outcomes {
        let metric = match status {
            p::WorkRouteStatus::Completed => (1, 1, 0),
            p::WorkRouteStatus::OutcomeUnknown => (0, 0, 1),
            _ => (0, 1, 0),
        };
        metrics.orchestration.add(metric.0, metric.1);
        metrics.orchestration.unverifiable += metric.2;
        if let Some(Some(pattern)) = patterns.get(&route) {
            let v = metrics
                .pattern_outcomes
                .entry(pattern.clone())
                .or_insert_with(MetricRatio::empty);
            v.add(metric.0, metric.1);
            v.unverifiable += metric.2;
        }
    }
    metrics.model_fallback.numerator =
        profiles_by_run.values().filter(|p| p.len() > 1).count() as u64;
    metrics.model_fallback.denominator = profiles_by_run.len() as u64;
    metrics
}

fn candidate_id(payload: &p::EventPayload) -> Option<&p::CandidateId> {
    match payload {
        p::EventPayload::CandidatePromoted(payload) => Some(&payload.candidate_id),
        p::EventPayload::CandidateRejected(payload) => Some(&payload.candidate_id),
        p::EventPayload::CandidateDowngraded(payload) => Some(&payload.candidate_id),
        p::EventPayload::CandidateDecayed(payload) => Some(&payload.candidate_id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_window_uses_start_cohort_and_prior_profiles_without_future_finishes() {
        let event = |seq, time, payload| {
            let mut e = p::Event::new(
                p::EventId(format!("event:window:{seq}")),
                p::RunId("run:window".into()),
                None,
                payload,
                p::SchemaVersion(1),
                time,
                p::Provenance {
                    source: p::Source::Internal,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::VerifiedProcess,
                    caused_by: None,
                },
            );
            e.stream_seq = seq;
            e
        };
        let start = |call: &str, profile: &str| {
            p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
                call_id: p::ModelCallId(call.into()),
                model_profile: p::ModelProfileRef(profile.into()),
            })
        };
        let finish = |call: &str, profile: &str| {
            p::EventPayload::ModelCallFinished(p::ModelCallFinishedPayload {
                call_id: p::ModelCallId(call.into()),
                model_profile: p::ModelProfileRef(profile.into()),
                usage: p::ModelUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                },
                finish_reason: p::FinishReason("stop".into()),
            })
        };
        let events = vec![
            event(1, 1, start("old", "primary")),
            event(2, 10, finish("old", "primary")),
            event(3, 11, start("new", "fallback")),
            event(4, 12, finish("new", "fallback")),
        ];
        let query = |to| {
            CognitiveDashboardProjection::from_events(
                &events,
                p::SchemaDigest("owner".into()),
                None,
                Some(10),
                Some(to),
            )
            .unwrap()
        };
        let pending = query(11);
        assert_eq!(pending.metrics.model_attempts.denominator, 1);
        assert!(!pending
            .metrics
            .model_outcomes
            .contains_key(&p::ModelProfileRef("primary".into())));
        assert_eq!(
            pending.metrics.model_outcomes[&p::ModelProfileRef("fallback".into())].unverifiable,
            1
        );
        assert_eq!(
            pending.metrics.model_fallback,
            MetricRatio {
                numerator: 1,
                denominator: 1,
                unverifiable: 0
            }
        );
        let done = query(12);
        assert_eq!(
            done.metrics.model_outcomes[&p::ModelProfileRef("fallback".into())],
            MetricRatio {
                numerator: 1,
                denominator: 1,
                unverifiable: 0
            }
        );
        let mut reversed = events;
        reversed.reverse();
        assert_eq!(
            done,
            CognitiveDashboardProjection::from_events(
                &reversed,
                p::SchemaDigest("owner".into()),
                None,
                Some(10),
                Some(12)
            )
            .unwrap()
        );
    }

    #[test]
    fn projection_is_replayable_and_marks_unverifiable_separately() {
        let run = p::RunId("run:dashboard".into());
        let mut event = p::Event::new(
            p::EventId("event:dashboard".into()),
            run,
            None,
            p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                verifier_kind: p::VerifierKind("deterministic".into()),
                outcome: p::VerificationOutcome::Unverifiable(p::ReasonRef(
                    "missing evidence".into(),
                )),
                against: p::DoneContractRef("done:dashboard".into()),
            }),
            p::SchemaVersion(1),
            10,
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        );
        event.stream_seq = 1;
        let first = CognitiveDashboardProjection::from_events(
            &[event.clone()],
            p::SchemaDigest("digest:owner".into()),
            None,
            None,
            None,
        )
        .unwrap();
        let second = CognitiveDashboardProjection::from_events(
            &[event],
            p::SchemaDigest("digest:owner".into()),
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.metrics.verification.unverifiable, 1);
        assert_eq!(first.metrics.verification.denominator, 0);
    }
}
