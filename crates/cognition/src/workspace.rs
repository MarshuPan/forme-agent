//! Owner-wide attention is a bounded event projection, never another brain.
use crate::{AgentWorkspaceItem, AgentWorkspaceItemKind as Kind, AgentWorkspaceProjection};
use forme_protocol as p;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_AGENT_WORKSPACE_CAPACITY: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceQuery {
    pub schema_version: p::SchemaVersion,
    pub scope: Option<p::Scope>,
    pub capacity: usize,
    pub now: p::Timestamp,
    pub config_version: p::Version,
}

pub fn workspace_scope_contains(parent: &p::Scope, child: &p::Scope) -> bool {
    parent.0 == "*"
        || parent == child
        || child
            .0
            .strip_prefix(&parent.0)
            .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with('/'))
}

impl AgentWorkspaceProjection {
    /// Input must be the owner authority's complete snapshot, not just the
    /// requesting run. Cross-stream causality is joined by ids, never clocks.
    pub fn rebuild_scoped(events: &[p::Event], query: &AgentWorkspaceQuery) -> p::Result<Self> {
        if query.schema_version != p::SchemaVersion(1)
            || query.config_version.0 == 0
            || query.capacity > MAX_AGENT_WORKSPACE_CAPACITY
            || query
                .scope
                .as_ref()
                .is_some_and(|scope| scope.0.trim().is_empty())
        {
            return Err(p::Error("owner attention query is invalid".into()));
        }
        let mut sorted = events.iter().collect::<Vec<_>>();
        sorted.sort_by(|a, b| {
            a.run_id
                .cmp(&b.run_id)
                .then(a.stream_seq.cmp(&b.stream_seq))
        });
        let mut cursor = BTreeMap::<p::RunId, u64>::new();
        let mut seen = BTreeMap::new();
        let mut scope_by_run = BTreeMap::new();
        let mut candidate_scope = BTreeMap::new();
        let mut terminal = BTreeSet::new();
        let mut closed_candidates = BTreeSet::new();
        let mut closed_intentions = BTreeSet::new();
        let learning = crate::LearningSnapshot::rebuild(events);
        for event in &sorted {
            let position = (event.run_id.clone(), event.stream_seq);
            if let Some(previous) = seen.insert(position, event.event_id.clone()) {
                if previous != event.event_id {
                    return Err(p::Error(
                        "attention snapshot has conflicting stream positions".into(),
                    ));
                }
            }
            cursor
                .entry(event.run_id.clone())
                .and_modify(|head| *head = (*head).max(event.stream_seq))
                .or_insert(event.stream_seq);
            match &event.payload {
                p::EventPayload::SessionBound(payload) => {
                    scope_by_run
                        .insert(event.run_id.clone(), p::Scope(payload.workspace.0.clone()));
                }
                p::EventPayload::RunComplete(_)
                | p::EventPayload::RunFailed(_)
                | p::EventPayload::RunAborted(_)
                | p::EventPayload::RunLimited(_) => {
                    terminal.insert(event.run_id.clone());
                }
                p::EventPayload::UserAttributeCandidateCreated(payload) => {
                    candidate_scope.insert(payload.candidate_id.clone(), payload.scope.clone());
                }
                p::EventPayload::CandidatePromoted(payload) => {
                    closed_candidates.insert(payload.candidate_id.clone());
                }
                p::EventPayload::CandidateRejected(payload) => {
                    closed_candidates.insert(payload.candidate_id.clone());
                }
                p::EventPayload::CandidateDowngraded(payload) => {
                    closed_candidates.insert(payload.candidate_id.clone());
                }
                p::EventPayload::CandidateDecayed(payload) => {
                    closed_candidates.insert(payload.candidate_id.clone());
                }
                p::EventPayload::ProspectiveIntentionResolved(payload)
                    if payload.outcome != p::IntentionOutcome::Fired =>
                {
                    closed_intentions.insert(payload.intention_id.clone());
                }
                _ => {}
            }
        }
        let mut items = BTreeMap::<String, AgentWorkspaceItem>::new();
        let mut latest_scope = BTreeMap::new();
        let mut last_impulse = BTreeMap::new();
        let mut proposal_impulses = BTreeMap::new();
        let mut charters = BTreeMap::new();
        for event in sorted {
            let base_scope = scope_by_run
                .get(&event.run_id)
                .cloned()
                .unwrap_or(p::Scope("owner".into()));
            let mut add = |id: String, kind, value, urgency, scope: p::Scope| {
                let mut item = crate::workspace_item(event, id.clone(), kind, value, urgency);
                item.scope = scope;
                items.insert(id, item);
            };
            match &event.payload {
                p::EventPayload::RunAccepted(_) if !terminal.contains(&event.run_id) => add(
                    format!("run:{}", event.run_id.0),
                    Kind::Run,
                    100,
                    100,
                    base_scope,
                ),
                p::EventPayload::GoalFramed(payload) => {
                    if payload
                        .long_term
                        .as_ref()
                        .is_some_and(|goal| goal.expires_at <= query.now)
                    {
                        continue;
                    }
                    if terminal.contains(&event.run_id) && payload.long_term.is_none() {
                        continue;
                    }
                    let scope = payload
                        .knowledge
                        .as_ref()
                        .map(|k| k.scope.clone())
                        .or_else(|| payload.long_term.as_ref().map(|g| g.scope.clone()))
                        .unwrap_or(base_scope);
                    add(
                        format!("goal:{}", payload.goal_frame.0),
                        Kind::Goal,
                        95,
                        90,
                        scope,
                    );
                }
                p::EventPayload::ObservationRecorded(payload) => {
                    latest_scope.insert(event.run_id.clone(), payload.scope.clone());
                }
                p::EventPayload::ImpulseRaised(payload) => {
                    let id = format!("impulse:{}", event.event_id.0);
                    last_impulse.insert(event.run_id.clone(), id.clone());
                    let (value, urgency) = if payload.source == p::ImpulseSource::Commitment {
                        (100, 100)
                    } else {
                        (80, 75)
                    };
                    add(
                        id,
                        Kind::Impulse,
                        value,
                        urgency,
                        latest_scope
                            .get(&event.run_id)
                            .cloned()
                            .unwrap_or(base_scope),
                    );
                }
                p::EventPayload::ProactiveProposalEmitted(payload) => {
                    if let Some(id) = last_impulse.get(&event.run_id) {
                        proposal_impulses.insert(payload.proposal_ref.clone(), id.clone());
                    }
                }
                p::EventPayload::ProspectiveIntentionCreated(payload)
                    if !closed_intentions.contains(&payload.intention_id) =>
                {
                    let trigger = payload.trigger.0.split('|').next().unwrap_or("");
                    let due = trigger
                        .strip_prefix("at:")
                        .and_then(|at| at.parse::<i64>().ok())
                        .is_some_and(|at| at <= query.now);
                    let expired = payload
                        .trigger
                        .0
                        .split('|')
                        .find_map(|part| part.strip_prefix("expires="))
                        .and_then(|at| at.parse::<i64>().ok())
                        .is_some_and(|at| at <= query.now);
                    if !due || expired {
                        continue;
                    }
                    let learning_scope = payload
                        .intention_id
                        .0
                        .strip_prefix("learning-deferred:")
                        .and_then(|id| learning.proposals.get(&p::ProposalRef(id.into())))
                        .map(|(record, _)| record.scope.clone());
                    let scope = payload
                        .schedule
                        .as_ref()
                        .map(|schedule| schedule.envelope.scope.clone())
                        .or(learning_scope)
                        .unwrap_or(base_scope);
                    add(
                        format!("intention:{}", payload.intention_id.0),
                        Kind::Intention,
                        100,
                        100,
                        scope,
                    );
                }
                p::EventPayload::CandidateCreated(payload)
                    if !closed_candidates.contains(&payload.candidate_id) =>
                {
                    let scope = candidate_scope
                        .get(&payload.candidate_id)
                        .cloned()
                        .or_else(|| {
                            crate::parse_cognitive_target(&payload.target)
                                .map(|(scope, _, _)| scope)
                        })
                        .unwrap_or(base_scope);
                    add(
                        format!("candidate:{}", payload.candidate_id.0),
                        Kind::Candidate,
                        60,
                        50,
                        scope,
                    );
                }
                p::EventPayload::WorkspaceCharterChanged(payload) => {
                    let key = payload.charter.workspace.clone();
                    if charters.get(&key).is_none_or(
                        |(version, _, _): &(u64, &p::Event, p::WorkspaceCharterRecord)| {
                            *version < payload.charter.version
                        },
                    ) {
                        charters.insert(
                            key,
                            (payload.charter.version, event, payload.charter.clone()),
                        );
                    }
                }
                _ => {}
            }
        }
        for (proposal, id) in proposal_impulses {
            if learning.resolved.contains_key(&proposal) || learning.replies.contains_key(&proposal)
                || events.iter().any(|event|matches!(&event.payload,p::EventPayload::ProactiveProposalResolved(p) if p.proposal_ref==proposal)) {
                items.remove(&id);
            }
        }
        for (workspace, (_, event, charter)) in charters {
            if charter.review_cadence.is_some_and(|cadence| {
                event
                    .ts_unix_ms
                    .saturating_add(i64::try_from(cadence.0).unwrap_or(i64::MAX))
                    <= query.now
            }) {
                let id = format!("workspace-review:{}", workspace.0);
                let mut item =
                    crate::workspace_item(event, id.clone(), Kind::WorkspaceReview, 75, 80);
                item.scope = p::Scope(workspace.0);
                items.insert(id, item);
            }
        }
        // Filter before competing for the bounded attention slots.
        let mut items = items
            .into_values()
            .filter(|item| {
                query
                    .scope
                    .as_ref()
                    .is_none_or(|scope| workspace_scope_contains(scope, &item.scope))
            })
            .collect::<Vec<_>>();
        items.sort_by(|a, b| {
            (u128::from(b.value) * u128::from(b.urgency))
                .cmp(&(u128::from(a.value) * u128::from(a.urgency)))
                .then(b.urgency.cmp(&a.urgency))
                .then(b.stream_seq.cmp(&a.stream_seq))
                .then(a.run.cmp(&b.run))
                .then(a.id.cmp(&b.id))
        });
        items.truncate(query.capacity);
        let material = items
            .iter()
            .map(|i| {
                (
                    &i.id,
                    format!("{:?}", i.kind),
                    i.value,
                    i.urgency,
                    &i.scope,
                    &i.event_ref,
                    &i.run,
                    i.stream_seq,
                )
            })
            .collect::<Vec<_>>();
        let digest = p::canonical_digest(&(
            query.schema_version,
            &query.scope,
            query.capacity,
            query.now,
            query.config_version,
            cursor,
            material,
        ))?;
        Ok(Self {
            schema_version: p::SchemaVersion(1),
            snapshot_ref: p::AgentWorkspaceSnapshotRef(format!("agent-workspace:{}", digest.0)),
            capacity: query.capacity,
            items,
        })
    }
}
