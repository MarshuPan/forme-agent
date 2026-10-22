//! Safe, bounded owner projections; no model/tool invocation or event writes.
use super::*;
use forme_cognition::{GraphQuery, MemoryGraph};

pub(super) fn project(
    runtime: &ProductionRuntime,
    events: &[p::Event],
    section: p::WorkbenchSection,
) -> p::Result<Vec<p::WorkbenchItem>> {
    match section {
        p::WorkbenchSection::Memory => Ok(memory(runtime, events)),
        p::WorkbenchSection::Cognition => cognition(runtime, events),
        p::WorkbenchSection::Inbox => Ok(inbox(events)),
    }
}

fn memory(runtime: &ProductionRuntime, events: &[p::Event]) -> Vec<p::WorkbenchItem> {
    let services = runtime.services();
    let sources: BTreeMap<_, _> = events
        .iter()
        .filter_map(|event| match &event.payload {
            p::EventPayload::MemoryNodeAppended(payload) => Some((&payload.node_id, event)),
            _ => None,
        })
        .collect();
    services
        .memory
        .query(GraphQuery::default())
        .into_iter()
        .filter_map(|node| {
            let source = sources.get(&node.id)?;
            let mut row = item(source, node.id.0, p::WorkbenchItemKind::Memory);
            row.scope = Some(node.scope.scope);
            row.state = format!("{:?}", node.kind);
            row.summary = format!("{:?} / {:?}", node.scope.slice, node.kind);
            row.content = Some(node.content_ref);
            row.tier = Some(node.tier);
            row.confidence = Some(node.confidence);
            references(
                &mut row,
                vec![p::EvidenceRef(source.event_id.0.clone())],
                Vec::new(),
            );
            Some(row)
        })
        .collect()
}

fn cognition(runtime: &ProductionRuntime, events: &[p::Event]) -> p::Result<Vec<p::WorkbenchItem>> {
    let services = runtime.services();
    let mut rows = Vec::new();
    let mut runs = BTreeMap::<&p::RunId, Vec<p::Event>>::new();
    for event in events {
        runs.entry(&event.run_id).or_default().push(event.clone());
    }
    for event in events {
        match &event.payload {
            p::EventPayload::CandidateCreated(payload) => {
                let snapshot =
                    crate::candidate_snapshot(&payload.candidate_id, &runs[&event.run_id])?;
                let mut row = item(
                    event,
                    payload.candidate_id.0.clone(),
                    p::WorkbenchItemKind::Candidate,
                );
                row.state = format!("{:?}", snapshot.state);
                row.summary = payload.target.0.chars().take(4096).collect();
                row.tier = Some(payload.target_tier);
                row.confidence = Some(payload.confidence);
                row.scope = events.iter().find_map(|e| match &e.payload {
                    p::EventPayload::UserAttributeCandidateCreated(p)
                        if p.candidate_id == payload.candidate_id =>
                    {
                        Some(p.scope.clone())
                    }
                    p::EventPayload::CandidatePromoted(p)
                        if p.candidate_id == payload.candidate_id =>
                    {
                        p.stable.as_ref().map(|s| s.scope.clone())
                    }
                    _ => None,
                });
                if row.scope.is_none() {
                    row.scope = payload
                        .target
                        .0
                        .strip_prefix("cognitive-map|")
                        .and_then(|target| target.split_once('|'))
                        .map(|(scope, _)| p::Scope(scope.replace("%7C", "|").replace("%25", "%")));
                }
                row.reviews = allowed(snapshot.state);
                let target = p::canonical_digest(&(&event.run_id, &payload.candidate_id))?;
                if super::review::pending_target(events, &target) {
                    row.reviews.clear();
                    row.state = "ReviewUnknown".into();
                }
                references(
                    &mut row,
                    snapshot.evidence,
                    lineage(events, &p::ObjectRef(snapshot.target.0)),
                );
                rows.push(row);
            }
            p::EventPayload::ReevaluationTaskCreated(payload) => {
                let mut row = item(
                    event,
                    format!("reevaluation:{}", event.event_id.0),
                    p::WorkbenchItemKind::Reevaluation,
                );
                let pending = payload
                    .derived_refs
                    .iter()
                    .any(|object| !services.cognition.is_object_usable(object));
                row.state = if pending { "Pending" } else { "Reevaluated" }.into();
                row.summary = payload.trigger.0.clone();
                references(
                    &mut row,
                    vec![p::EvidenceRef(event.event_id.0.clone())],
                    payload.derived_refs.clone(),
                );
                rows.push(row);
            }
            _ => {}
        }
    }
    for stable in services.cognition.stable_objects() {
        let source = stable_source(events, &stable.object).ok_or_else(|| {
            p::Error("stable cognition source is unavailable; verify the brain trace".into())
        })?;
        let mut row = item(
            source,
            stable.object.0.clone(),
            p::WorkbenchItemKind::Stable,
        );
        row.scope = Some(stable.scope.scope);
        row.state = if stable.active && services.cognition.is_object_usable(&stable.object) {
            "Active"
        } else {
            "Inactive"
        }
        .into();
        row.summary = stable.statement;
        row.tier = Some(stable.tier);
        row.confidence = Some(stable.confidence);
        references(
            &mut row,
            stable.evidence.into_iter().map(|e| e.reference).collect(),
            lineage(events, &stable.object),
        );
        rows.push(row);
    }
    Ok(rows)
}

fn stable_source<'a>(events: &'a [p::Event], object: &p::ObjectRef) -> Option<&'a p::Event> {
    events.iter().rev().find(|event| {
        let p::EventPayload::CandidatePromoted(payload) = &event.payload else {
            return false;
        };
        if let Some(record) = &payload.stable {
            return &record.object == object;
        }
        // Legacy map promotions did not carry a stable record. Resolve their
        // source from the original proposal, never invent an event or hide it.
        events.iter().any(|event| match &event.payload {
            p::EventPayload::CognitiveMapUpdateProposed(p)
                if p.candidate_id == payload.candidate_id =>
            {
                p.frame.as_ref().is_some_and(|v| v.0 == object.0)
                    || p.quality.as_ref().is_some_and(|v| v.0 == object.0)
                    || p.blindspot.as_ref().is_some_and(|v| v.0 == object.0)
                    || p.resource.as_ref().is_some_and(|v| v.0 == object.0)
            }
            _ => false,
        })
    })
}

fn allowed(state: p::CandidateReviewState) -> Vec<p::CandidateReviewDecision> {
    use p::CandidateReviewDecision as D;
    use p::CandidateReviewState as S;
    match state {
        S::Candidate => vec![D::Promote, D::Reject, D::Downgrade],
        S::Downgraded => vec![D::Promote],
        S::Promoted => vec![D::Downgrade, D::Retract],
        _ => Vec::new(),
    }
}

fn inbox(events: &[p::Event]) -> Vec<p::WorkbenchItem> {
    let learning = forme_cognition::LearningSnapshot::rebuild(events);
    let resolved: BTreeMap<_, _> = events
        .iter()
        .filter_map(|event| match &event.payload {
            p::EventPayload::ProactiveProposalResolved(payload) => {
                Some((&payload.proposal_ref, payload.outcome))
            }
            _ => None,
        })
        .collect();
    let mut rows = Vec::new();
    for event in events {
        match &event.payload {
            p::EventPayload::ProactiveProposalEmitted(payload) => {
                let mut row = item(
                    event,
                    payload.proposal_ref.0.clone(),
                    if payload.learning.is_some() {
                        p::WorkbenchItemKind::AskToLearn
                    } else {
                        p::WorkbenchItemKind::Suggestion
                    },
                );
                row.state = if learning.replies.contains_key(&payload.proposal_ref) {
                    "Answered".into()
                } else {
                    resolved
                        .get(&payload.proposal_ref)
                        .map_or("Pending".into(), |outcome| format!("{outcome:?}"))
                };
                row.summary = payload.learning.as_ref().map_or_else(
                    || payload.proposal_kind.0.clone(),
                    |ask| ask.reason.0.clone(),
                );
                row.scope = payload.learning.as_ref().map(|ask| ask.scope.clone());
                row.learning = payload.learning.clone();
                references(
                    &mut row,
                    vec![p::EvidenceRef(event.event_id.0.clone())],
                    Vec::new(),
                );
                rows.push(row);
            }
            p::EventPayload::FailureEvidenceRecorded(payload) => {
                let mut row = item(
                    event,
                    payload.failure_ref.0.clone(),
                    p::WorkbenchItemKind::Failure,
                );
                row.scope = Some(payload.scope.clone());
                row.summary = format!("{:?} / {:?}", payload.class, payload.impact);
                references(&mut row, payload.related_refs.clone(), Vec::new());
                rows.push(row);
            }
            _ => {}
        }
    }
    rows
}
