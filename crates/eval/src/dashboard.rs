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
    pub ask_to_learn_acceptance: MetricRatio,
    pub ask_to_learn_usefulness: MetricRatio,
    pub candidate_resolution: MetricRatio,
    pub verification: MetricRatio,
    pub failures: MetricRatio,
    pub model_attempts: MetricRatio,
    pub model_fallback: MetricRatio,
    pub orchestration: MetricRatio,
    pub candidate_backlog: u64,
}

impl CognitiveDashboardMetrics {
    fn empty() -> Self {
        Self {
            proposal_adoption: MetricRatio::empty(),
            ask_to_learn_acceptance: MetricRatio::empty(),
            ask_to_learn_usefulness: MetricRatio::empty(),
            candidate_resolution: MetricRatio::empty(),
            verification: MetricRatio::empty(),
            failures: MetricRatio::empty(),
            model_attempts: MetricRatio::empty(),
            model_fallback: MetricRatio::empty(),
            orchestration: MetricRatio::empty(),
            candidate_backlog: 0,
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
        let filtered = events
            .iter()
            .filter(|event| from.is_none_or(|from| event.ts_unix_ms >= from))
            .filter(|event| to.is_none_or(|to| event.ts_unix_ms <= to))
            .filter(|event| workspace_matches(event, workspace.as_ref()))
            .collect::<Vec<_>>();
        let metrics = fold_metrics(&filtered);
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
            filtered
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
            metrics,
            digest,
        })
    }
}

fn workspace_matches(event: &p::Event, workspace: Option<&p::WorkspaceRef>) -> bool {
    let Some(workspace) = workspace else {
        return true;
    };
    match &event.payload {
        p::EventPayload::SessionBound(payload) => &payload.workspace == workspace,
        p::EventPayload::WorkspaceCharterChanged(payload) => {
            &payload.charter.workspace == workspace
        }
        p::EventPayload::OrchestrationRouteCreated(payload) => payload
            .goal_frame
            .as_ref()
            .is_some_and(|goal| goal.0.contains(&workspace.0)),
        p::EventPayload::GoalFramed(_) => true,
        _ => true,
    }
}

fn fold_metrics(events: &[&p::Event]) -> CognitiveDashboardMetrics {
    let mut metrics = CognitiveDashboardMetrics::empty();
    let mut learning_proposals = BTreeSet::new();
    let mut candidates = BTreeSet::new();
    let mut attempts_by_run = BTreeMap::<p::RunId, u64>::new();
    let mut fallback_runs = BTreeSet::new();
    let mut routes = 0_u64;
    let mut route_outcomes = 0_u64;

    for event in events {
        match &event.payload {
            p::EventPayload::ProactiveProposalEmitted(payload) => {
                if payload.learning.is_some() {
                    learning_proposals.insert(payload.proposal_ref.clone());
                }
            }
            p::EventPayload::ProactiveProposalResolved(payload) => {
                metrics
                    .proposal_adoption
                    .add(u64::from(payload.outcome == p::ProposalOutcome::Adopt), 1);
                if learning_proposals.contains(&payload.proposal_ref) {
                    metrics
                        .ask_to_learn_acceptance
                        .add(u64::from(payload.outcome == p::ProposalOutcome::Adopt), 1);
                }
            }
            p::EventPayload::ObservationRecorded(payload) => {
                if payload.learning_reply.is_some() {
                    metrics.ask_to_learn_usefulness.add(1, 1);
                }
            }
            p::EventPayload::CandidateCreated(payload) => {
                candidates.insert(payload.candidate_id.clone());
            }
            p::EventPayload::CandidatePromoted(_)
            | p::EventPayload::CandidateRejected(_)
            | p::EventPayload::CandidateDowngraded(_)
            | p::EventPayload::CandidateDecayed(_) => {
                let candidate = candidate_id(&event.payload).expect("candidate event id");
                metrics.candidate_resolution.add(1, 1);
                candidates.remove(candidate);
            }
            p::EventPayload::VerificationFinished(payload) => match payload.outcome {
                p::VerificationOutcome::Pass => metrics.verification.add(1, 1),
                p::VerificationOutcome::Fail => metrics.verification.add(0, 1),
                p::VerificationOutcome::Unverifiable(_) => metrics.verification.mark_unverifiable(),
            },
            p::EventPayload::FailureEvidenceRecorded(_) => metrics.failures.add(1, 1),
            p::EventPayload::ModelCallStarted(_) => {
                let count = attempts_by_run.entry(event.run_id.clone()).or_default();
                *count = count.saturating_add(1);
                metrics.model_attempts.add(1, 1);
            }
            p::EventPayload::DecisionTraceRecorded(payload)
                if payload.rationale.0.starts_with("model selection") =>
            {
                if payload.rationale.0.contains("fallback_count=")
                    && !payload.rationale.0.contains("fallback_count=0")
                {
                    fallback_runs.insert(event.run_id.clone());
                }
            }
            p::EventPayload::OrchestrationRouteCreated(_) => routes = routes.saturating_add(1),
            p::EventPayload::SubagentResultReturned(payload) => {
                route_outcomes = route_outcomes.saturating_add(1);
                metrics
                    .orchestration
                    .add(u64::from(payload.status == p::RunStatus::Complete), 1);
            }
            _ => {}
        }
    }
    metrics.candidate_backlog = candidates.len() as u64;
    metrics.orchestration.denominator = metrics
        .orchestration
        .denominator
        .saturating_add(routes.saturating_sub(route_outcomes));
    metrics.model_fallback.numerator = fallback_runs.len() as u64;
    metrics.model_fallback.denominator = attempts_by_run.len() as u64;
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
