use std::collections::{BTreeMap, BTreeSet};

use forme_store::EventStore;

use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub struct CapabilityGrowthEngine;

impl CapabilityGrowthEngine {
    pub fn assess(
        &self,
        reference: p::CapabilityGapRef,
        capability: p::CapabilityRef,
        scope: p::Scope,
        result_evidence: Vec<p::CapabilityResultEvidence>,
        self_confidence: Option<p::Confidence>,
    ) -> p::Result<p::CapabilityGap> {
        if reference.0.trim().is_empty()
            || capability.0.trim().is_empty()
            || scope.0.trim().is_empty()
            || result_evidence.is_empty()
            || result_evidence.iter().any(|evidence| {
                evidence.schema_version.0 == 0
                    || evidence.evidence_ref.0.trim().is_empty()
                    || evidence.observed_at <= 0
            })
            || self_confidence.is_some_and(|confidence| !(0.0..=1.0).contains(&confidence.0))
        {
            return Err(p::Error("capability growth evidence is incomplete".into()));
        }
        let unique = result_evidence
            .iter()
            .map(|evidence| evidence.evidence_ref.clone())
            .collect::<BTreeSet<_>>();
        if unique.len() != result_evidence.len() {
            return Err(p::Error("capability growth evidence is duplicated".into()));
        }

        let passed = result_evidence
            .iter()
            .filter(|evidence| evidence.outcome == p::ResourceEvidenceOutcome::Pass)
            .count();
        let failed = result_evidence
            .iter()
            .filter(|evidence| {
                evidence.outcome == p::ResourceEvidenceOutcome::Fail
                    || evidence.owner_feedback == Some(false)
            })
            .count();
        let unverifiable = result_evidence
            .iter()
            .filter(|evidence| evidence.outcome == p::ResourceEvidenceOutcome::Unverifiable)
            .count();
        let owner_confirmed = result_evidence
            .iter()
            .any(|evidence| evidence.owner_feedback == Some(true));

        let result_ceiling = if passed >= 3 && failed == 0 && unverifiable == 0 && owner_confirmed {
            p::InterventionLevel::L4ActAutonomously
        } else if passed >= 2 && failed == 0 {
            p::InterventionLevel::L3ActWithApproval
        } else if passed >= 1 && failed == 0 {
            p::InterventionLevel::L2Prepare
        } else {
            p::InterventionLevel::L1Suggest
        };
        let self_ceiling = match self_confidence {
            Some(confidence) if confidence.0 < 0.4 => p::InterventionLevel::L2Prepare,
            Some(confidence) if confidence.0 < 0.7 => p::InterventionLevel::L3ActWithApproval,
            _ => p::InterventionLevel::L5HighImpact,
        };

        Ok(p::CapabilityGap {
            schema_version: p::SchemaVersion(1),
            reference,
            capability,
            scope,
            result_evidence,
            self_confidence,
            ceiling: result_ceiling.min(self_ceiling),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongTermContinuationDecision {
    YieldToForeground,
    Continue,
    Replan,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LongTermContinuation {
    pub schema_version: p::SchemaVersion,
    pub decision: LongTermContinuationDecision,
    pub intention: Option<p::IntentionId>,
    pub reason: p::ReasonRef,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LongTermContinuationGuard;

impl LongTermContinuationGuard {
    pub fn decide(
        &self,
        snapshot: &p::GoalLineageSnapshot,
        current_situation: &p::SchemaDigest,
        now: p::Timestamp,
        foreground_active: bool,
        remaining_budget: u64,
    ) -> p::Result<LongTermContinuation> {
        snapshot.goal.validate()?;
        if current_situation.0.trim().is_empty() || now <= 0 {
            return Err(p::Error(
                "long-term continuation context is incomplete".into(),
            ));
        }
        if foreground_active {
            return Ok(continuation(
                LongTermContinuationDecision::YieldToForeground,
                None,
                "foreground run has priority",
            ));
        }
        if snapshot.cancelled || snapshot.revoked {
            return Ok(continuation(
                LongTermContinuationDecision::Stop,
                None,
                "goal or intention was cancelled or revoked",
            ));
        }
        if now >= snapshot.goal.expires_at || remaining_budget == 0 {
            return Ok(continuation(
                LongTermContinuationDecision::Stop,
                None,
                "goal timebox or budget is exhausted",
            ));
        }
        let expected_situation = snapshot
            .checkpoints
            .last()
            .map(|checkpoint| &checkpoint.situation_digest)
            .unwrap_or(&snapshot.goal.situation_digest);
        if expected_situation != current_situation {
            return Ok(continuation(
                LongTermContinuationDecision::Replan,
                None,
                "situation digest changed; the previous route is stale",
            ));
        }
        let terminal = snapshot
            .resolutions
            .iter()
            .filter(|resolution| {
                matches!(
                    resolution.outcome,
                    p::IntentionOutcome::Done
                        | p::IntentionOutcome::Expired
                        | p::IntentionOutcome::Cancelled
                )
            })
            .map(|resolution| resolution.intention.clone())
            .collect::<BTreeSet<_>>();
        let intention = snapshot
            .intentions
            .iter()
            .find(|intention| !terminal.contains(*intention))
            .cloned();
        if let Some(intention) = intention {
            return Ok(continuation(
                LongTermContinuationDecision::Continue,
                Some(intention),
                "continuation remains a governed scheduled run",
            ));
        }
        Ok(continuation(
            LongTermContinuationDecision::Stop,
            None,
            "all goal intentions are terminal",
        ))
    }
}

fn continuation(
    decision: LongTermContinuationDecision,
    intention: Option<p::IntentionId>,
    reason: &str,
) -> LongTermContinuation {
    LongTermContinuation {
        schema_version: p::SchemaVersion(1),
        decision,
        intention,
        reason: p::ReasonRef(reason.into()),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GoalLineageProjector;

impl GoalLineageProjector {
    pub fn rebuild(
        &self,
        events: &[p::Event],
        goal_frame: &p::GoalFrameRef,
    ) -> p::Result<p::GoalLineageSnapshot> {
        if goal_frame.0.trim().is_empty() || events.is_empty() {
            return Err(p::Error(
                "goal lineage rebuild boundary is incomplete".into(),
            ));
        }
        let mut ordered = events.to_vec();
        ordered.sort_by(|left, right| {
            left.run_id
                .cmp(&right.run_id)
                .then_with(|| left.stream_seq.cmp(&right.stream_seq))
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        if ordered.iter().any(|event| event.stream_seq == 0) {
            return Err(p::Error(
                "goal lineage only accepts store-sequenced events".into(),
            ));
        }

        let mut goal = None;
        let mut intentions = BTreeSet::new();
        let mut intention_goal = BTreeMap::new();
        let mut resolutions = Vec::new();
        let mut routes = BTreeSet::new();
        let mut checkpoints = BTreeMap::new();
        let mut aggregate_versions = BTreeMap::<p::RunId, u64>::new();
        let mut cancelled = false;
        let mut revoked = false;

        for event in &ordered {
            aggregate_versions
                .entry(event.run_id.clone())
                .and_modify(|version| *version = (*version).max(event.stream_seq))
                .or_insert(event.stream_seq);
            match &event.payload {
                p::EventPayload::GoalFramed(payload) if &payload.goal_frame == goal_frame => {
                    if let Some(candidate) = &payload.long_term {
                        candidate.validate()?;
                        if candidate.goal_frame != *goal_frame {
                            return Err(p::Error(
                                "long-term goal does not match its GoalFramed event".into(),
                            ));
                        }
                        if goal.as_ref().is_some_and(|existing| existing != candidate) {
                            return Err(p::Error(
                                "long-term goal definition changed without a new identity".into(),
                            ));
                        }
                        goal = Some(candidate.clone());
                    }
                }
                p::EventPayload::ProspectiveIntentionCreated(payload)
                    if payload.goal_frame.as_ref() == Some(goal_frame) =>
                {
                    intentions.insert(payload.intention_id.clone());
                    intention_goal.insert(payload.intention_id.clone(), goal_frame.clone());
                }
                p::EventPayload::ProspectiveIntentionResolved(payload)
                    if intention_goal.get(&payload.intention_id) == Some(goal_frame) =>
                {
                    cancelled |= payload.outcome == p::IntentionOutcome::Cancelled;
                    resolutions.push(p::GoalIntentionResolution {
                        schema_version: p::SchemaVersion(1),
                        intention: payload.intention_id.clone(),
                        outcome: payload.outcome,
                        event_ref: event.event_id.clone(),
                    });
                }
                p::EventPayload::OrchestrationRouteCreated(payload)
                    if payload.goal_frame.as_ref() == Some(goal_frame) =>
                {
                    routes.insert(payload.route.clone());
                    if let Some(checkpoint) = &payload.checkpoint {
                        checkpoint.validate()?;
                        if checkpoint.goal_frame != *goal_frame || checkpoint.route != payload.route
                        {
                            return Err(p::Error(
                                "goal checkpoint does not match its route event".into(),
                            ));
                        }
                        checkpoints.insert(checkpoint.reference.clone(), checkpoint.clone());
                    }
                }
                p::EventPayload::RevocationEvent(payload)
                    if payload.target_object.0 == goal_frame.0 =>
                {
                    revoked = true;
                }
                _ => {}
            }
        }
        let goal = goal.ok_or_else(|| p::Error("long-term goal event is missing".into()))?;
        Ok(p::GoalLineageSnapshot {
            schema_version: p::SchemaVersion(1),
            goal,
            intentions: intentions.into_iter().collect(),
            resolutions,
            routes: routes.into_iter().collect(),
            checkpoints: checkpoints.into_values().collect(),
            aggregate_versions: aggregate_versions
                .into_iter()
                .map(|(aggregate, value)| p::AggregateVersion {
                    schema_version: p::SchemaVersion(1),
                    aggregate,
                    value,
                })
                .collect(),
            cancelled,
            revoked,
        })
    }
}

impl<S> EventSourcedMemory<S>
where
    S: EventStore,
{
    pub fn record_long_term_goal(
        &self,
        goal: p::LongTermGoal,
        provenance: p::Provenance,
    ) -> p::Result<p::GoalFrameRef> {
        goal.validate()?;
        require_trusted_process(&provenance)?;
        self.append_payload(
            provenance,
            p::EventPayload::GoalFramed(p::GoalFramedPayload {
                goal_frame: goal.goal_frame.clone(),
                long_term: Some(goal.clone()),
            }),
        )?;
        Ok(goal.goal_frame)
    }

    pub fn create_goal_schedule(
        &self,
        goal_frame: p::GoalFrameRef,
        command: p::ScheduleCommand,
    ) -> p::Result<p::IntentionId> {
        if goal_frame.0.trim().is_empty() {
            return Err(p::Error("goal schedule has no goal frame".into()));
        }
        command.validate()?;
        let intention = intention_from_protocol(&command.intention);
        validate_intention(&intention)?;
        let binding = command.binding();
        let mut state = self.lock_state()?;
        if let Some(existing) = state.intentions.get(&intention.id) {
            return if same_intention_definition(&existing.intention, &intention)
                && existing.schedule.as_ref() == Some(&binding)
            {
                Ok(intention.id)
            } else {
                Err(p::Error("intention id collision".into()))
            };
        }
        let created_event = self.append_payload(
            intention.provenance.clone(),
            p::EventPayload::ProspectiveIntentionCreated(p::ProspectiveIntentionCreatedPayload {
                intention_id: intention.id.clone(),
                source: intention.source,
                trigger: intention_storage_ref(&intention),
                schedule: Some(binding.clone()),
                goal_frame: Some(goal_frame),
            }),
        )?;
        state.intentions.insert(
            intention.id.clone(),
            IntentionRecord {
                intention: intention.clone(),
                schedule: Some(binding),
                lease_until: None,
                claim_generation: created_event,
            },
        );
        Ok(intention.id)
    }

    pub fn record_goal_checkpoint(
        &self,
        checkpoint: p::GoalCheckpoint,
        pattern_ref: Option<p::OrchestrationPatternRef>,
        provenance: p::Provenance,
    ) -> p::Result<p::GoalCheckpointRef> {
        checkpoint.validate()?;
        require_trusted_process(&provenance)?;
        self.append_payload(
            provenance,
            p::EventPayload::OrchestrationRouteCreated(p::OrchestrationRouteCreatedPayload {
                pattern_ref,
                route: checkpoint.route.clone(),
                goal_frame: Some(checkpoint.goal_frame.clone()),
                checkpoint: Some(checkpoint.clone()),
            }),
        )?;
        Ok(checkpoint.reference)
    }

    pub fn goal_lineage(&self, goal_frame: &p::GoalFrameRef) -> p::Result<p::GoalLineageSnapshot> {
        let events = self
            .store
            .read_run(self.aggregate_run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        GoalLineageProjector.rebuild(&events, goal_frame)
    }

    pub fn propose_capability_update(
        &self,
        proposal: p::CapabilityUpdateProposal,
        provenance: p::Provenance,
    ) -> p::Result<p::CandidateId> {
        proposal.validate()?;
        require_trusted_process(&provenance)?;
        validate_narrow_update(&proposal)?;
        let mut state = self.lock_state()?;
        if let Some(existing) = state.capability_updates.get(&proposal.candidate_id) {
            return if existing == &proposal {
                Ok(proposal.candidate_id)
            } else {
                Err(p::Error("capability update candidate id collision".into()))
            };
        }
        let confidence = proposal.gap.self_confidence.unwrap_or(p::Confidence(0.0));
        self.append_payload(
            provenance.clone(),
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: proposal.candidate_id.clone(),
                target: p::CandidateTargetRef(format!(
                    "capability-update:{}:{}",
                    proposal.gap.capability.0, proposal.gap.scope.0
                )),
                evidence_refs: proposal.evidence_refs.clone(),
                confidence,
                provenance: provenance.clone(),
                target_tier: p::StabilityTier::Working,
                capability_update: Some(proposal.clone()),
                strategy_candidate: None,
            }),
        )?;
        state.candidates.insert(
            proposal.candidate_id.clone(),
            CandidateRecord {
                update: p::CandidateUpdate {
                    schema_version: proposal.schema_version,
                },
                spec: CandidateSpec {
                    schema_version: proposal.schema_version,
                    id: proposal.candidate_id.clone(),
                    target: p::CandidateTargetRef(format!(
                        "capability-update:{}:{}",
                        proposal.gap.capability.0, proposal.gap.scope.0
                    )),
                    evidence_refs: proposal.evidence_refs.clone(),
                    confidence,
                    provenance,
                    target_tier: p::StabilityTier::Working,
                },
                state: CandidateState::Candidate,
                decided_by: None,
            },
        );
        state
            .capability_updates
            .insert(proposal.candidate_id.clone(), proposal.clone());
        Ok(proposal.candidate_id)
    }

    pub fn review_capability_update(
        &self,
        candidate: p::CandidateId,
        decision: p::CandidateReviewDecision,
        owner: p::VerifiedPrincipal,
        reviewed_at: p::Timestamp,
    ) -> p::Result<Option<p::NarrowCapabilityGrant>> {
        if owner.0.trim().is_empty() || reviewed_at <= 0 {
            return Err(p::Error(
                "capability update review is not owner-bound".into(),
            ));
        }
        let proposal = self
            .lock_state()?
            .capability_updates
            .get(&candidate)
            .cloned()
            .ok_or_else(|| p::Error("capability update proposal is not registered".into()))?;
        match decision {
            p::CandidateReviewDecision::Promote => {
                CandidateStore::transition(
                    self,
                    candidate,
                    CandidateState::Promoted,
                    p::Actor::Owner,
                )?;
                Ok(Some(p::NarrowCapabilityGrant {
                    schema_version: p::SchemaVersion(1),
                    proposal: proposal.reference,
                    envelope: proposal.requested_envelope,
                    approved_by: owner,
                    approved_at: reviewed_at,
                    evidence_refs: proposal.evidence_refs,
                }))
            }
            p::CandidateReviewDecision::Reject => {
                CandidateStore::transition(
                    self,
                    candidate,
                    CandidateState::Rejected,
                    p::Actor::Owner,
                )?;
                Ok(None)
            }
            p::CandidateReviewDecision::Downgrade | p::CandidateReviewDecision::Retract => Err(
                p::Error("capability update review only supports promote or reject".into()),
            ),
        }
    }

    pub fn capability_update_proposal(
        &self,
        candidate: &p::CandidateId,
    ) -> Option<p::CapabilityUpdateProposal> {
        self.lock_state()
            .ok()
            .and_then(|state| state.capability_updates.get(candidate).cloned())
    }
}

fn require_trusted_process(provenance: &p::Provenance) -> p::Result<()> {
    if provenance.trust_tier == p::TrustTier::Untrusted
        || matches!(provenance.actor, p::Actor::External(_))
    {
        return Err(p::Error(
            "untrusted content cannot create long-term or capability governance facts".into(),
        ));
    }
    Ok(())
}

fn validate_narrow_update(proposal: &p::CapabilityUpdateProposal) -> p::Result<()> {
    let envelope = &proposal.requested_envelope;
    let gap = &proposal.gap;
    let low_effect = envelope.action_type.iter().all(|action| {
        matches!(
            action,
            p::ActionType::Observe | p::ActionType::Analyze | p::ActionType::Prepare
        )
    });
    if envelope.scope != gap.scope
        || envelope.capability.capabilities.len() != 1
        || envelope.capability.capabilities[0] != gap.capability
        || envelope.capability.permissions.len() > 1
        || envelope.action_type.is_empty()
        || envelope.action_type.len() > 2
        || envelope.risk_limit != p::Risk::Low
        || envelope.timebox.starts_at >= envelope.timebox.expires_at
        || envelope.timebox.max_turns == 0
        || envelope.timebox.max_turns > 8
        || envelope.approval_rule == p::ApprovalRule::Deny
        || (envelope.approval_rule == p::ApprovalRule::Allow
            && (gap.ceiling < p::InterventionLevel::L4ActAutonomously
                || !low_effect
                || !envelope.rollback.required))
        || (envelope.approval_rule == p::ApprovalRule::Ask
            && gap.ceiling < p::InterventionLevel::L3ActWithApproval)
    {
        return Err(p::Error(
            "capability update proposal is not a narrow evidence-bounded envelope".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotColdProjectionPolicy {
    pub schema_version: p::SchemaVersion,
    pub hot_window: p::DurationMs,
    pub retention_window: p::DurationMs,
    pub redact_untrusted_content: bool,
}

impl HotColdProjectionPolicy {
    pub fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0
            || self.hot_window.0 == 0
            || self.retention_window.0 <= self.hot_window.0
            || self.retention_window.0 > i64::MAX as u64
        {
            return Err(p::Error(
                "hot/cold projection policy is incomplete or unbounded".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectiveRecall {
    pub schema_version: p::SchemaVersion,
    pub scope: p::Scope,
    pub include_cold: bool,
    pub limit: usize,
}

impl SelectiveRecall {
    fn validate(&self) -> p::Result<()> {
        if self.schema_version.0 == 0 || self.scope.0.trim().is_empty() || self.limit == 0 {
            return Err(p::Error("selective recall boundary is incomplete".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HotColdMemoryProjector;

impl HotColdMemoryProjector {
    pub fn rebuild(
        &self,
        events: &[p::Event],
        policy: &HotColdProjectionPolicy,
        now: p::Timestamp,
    ) -> p::Result<p::HotColdMemorySnapshot> {
        policy.validate()?;
        if now <= 0 || events.is_empty() {
            return Err(p::Error(
                "hot/cold projection requires events and a valid clock".into(),
            ));
        }

        let mut ordered = events.to_vec();
        ordered.sort_by(|left, right| {
            left.run_id
                .cmp(&right.run_id)
                .then_with(|| left.stream_seq.cmp(&right.stream_seq))
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        if ordered.iter().any(|event| event.stream_seq == 0) {
            return Err(p::Error(
                "hot/cold projection only accepts store-sequenced events".into(),
            ));
        }

        let tombstones = ordered
            .iter()
            .filter_map(tombstone_target)
            .collect::<BTreeSet<_>>();
        let mut aggregate_versions = BTreeMap::<p::RunId, u64>::new();
        let mut entries = Vec::with_capacity(ordered.len());

        for event in ordered {
            aggregate_versions
                .entry(event.run_id.clone())
                .and_modify(|version| *version = (*version).max(event.stream_seq))
                .or_insert(event.stream_seq);

            let mut content_ref = event_content_ref(&event);
            let is_tombstoned = event_object_refs(&event)
                .iter()
                .any(|reference| tombstones.contains(reference));
            let is_redacted = intrinsically_sensitive(event.kind)
                || content_ref
                    .as_ref()
                    .is_some_and(|reference| secret_ref_like(&reference.0))
                || (policy.redact_untrusted_content
                    && event.provenance.trust_tier == p::TrustTier::Untrusted
                    && content_ref.is_some());
            let expired = elapsed_at_least(event.ts_unix_ms, policy.retention_window, now);
            let retention = if is_tombstoned {
                p::MemoryRetentionState::Tombstoned
            } else if expired {
                p::MemoryRetentionState::Expired
            } else if is_redacted {
                p::MemoryRetentionState::Redacted
            } else {
                p::MemoryRetentionState::Active
            };
            if retention != p::MemoryRetentionState::Active {
                content_ref = None;
            }
            let temperature = if elapsed_at_least(event.ts_unix_ms, policy.hot_window, now) {
                p::MemoryTemperature::Cold
            } else {
                p::MemoryTemperature::Hot
            };
            let scope = event_scope(&event);

            entries.push(p::MemoryTierEntry {
                schema_version: p::SchemaVersion(1),
                event_id: event.event_id,
                aggregate: event.run_id,
                stream_seq: event.stream_seq,
                scope,
                temperature,
                retention,
                content_ref,
                occurred_at: event.ts_unix_ms,
            });
        }

        Ok(p::HotColdMemorySnapshot {
            schema_version: p::SchemaVersion(1),
            aggregate_versions: aggregate_versions
                .into_iter()
                .map(|(aggregate, value)| p::AggregateVersion {
                    schema_version: p::SchemaVersion(1),
                    aggregate,
                    value,
                })
                .collect(),
            entries,
            built_at: now,
        })
    }

    pub fn recall(
        &self,
        snapshot: &p::HotColdMemorySnapshot,
        query: &SelectiveRecall,
    ) -> p::Result<Vec<p::MemoryTierEntry>> {
        query.validate()?;
        if snapshot.schema_version.0 == 0
            || snapshot.aggregate_versions.is_empty()
            || snapshot.entries.iter().any(|entry| {
                entry.schema_version.0 == 0 || entry.stream_seq == 0 || entry.occurred_at <= 0
            })
        {
            return Err(p::Error("hot/cold snapshot is incomplete".into()));
        }

        Ok(snapshot
            .entries
            .iter()
            .filter(|entry| entry.retention == p::MemoryRetentionState::Active)
            .filter(|entry| query.include_cold || entry.temperature == p::MemoryTemperature::Hot)
            .filter(|entry| {
                entry
                    .scope
                    .as_ref()
                    .is_some_and(|scope| scope_contains(&query.scope, scope))
            })
            .take(query.limit)
            .cloned()
            .collect())
    }
}

fn elapsed_at_least(occurred_at: p::Timestamp, window: p::DurationMs, now: p::Timestamp) -> bool {
    now.checked_sub(occurred_at)
        .and_then(|elapsed| u64::try_from(elapsed).ok())
        .is_some_and(|elapsed| elapsed >= window.0)
}

fn event_scope(event: &p::Event) -> Option<p::Scope> {
    match &event.payload {
        p::EventPayload::ApprovalRequested(payload) => Some(payload.scope.clone()),
        p::EventPayload::ActionPlanned(payload) => Some(payload.scope.clone()),
        p::EventPayload::ActionStarted(payload) => Some(payload.scope.clone()),
        p::EventPayload::ActionOutputDelta(payload) => Some(payload.scope.clone()),
        p::EventPayload::FailureEvidenceRecorded(payload) => Some(payload.scope.clone()),
        p::EventPayload::ObservationRecorded(payload) => Some(payload.scope.clone()),
        p::EventPayload::CommunicationEventReceived(payload) => Some(payload.scope.clone()),
        p::EventPayload::MemoryNodeAppended(payload) => Some(payload.scope.clone()),
        p::EventPayload::UserAttributeCandidateCreated(payload) => Some(payload.scope.clone()),
        p::EventPayload::GoalFramed(payload) => {
            payload.long_term.as_ref().map(|goal| goal.scope.clone())
        }
        p::EventPayload::CandidateCreated(payload) => payload
            .capability_update
            .as_ref()
            .map(|proposal| proposal.gap.scope.clone()),
        p::EventPayload::SessionBound(payload) => Some(p::Scope(payload.workspace.0.clone())),
        _ => None,
    }
}

fn event_content_ref(event: &p::Event) -> Option<p::ContentRef> {
    match &event.payload {
        p::EventPayload::ActionOutputDelta(payload) => payload.content_ref.clone(),
        p::EventPayload::CommunicationEventReceived(payload) => payload.content_ref.clone(),
        p::EventPayload::MemoryNodeAppended(payload) => Some(payload.content_ref.clone()),
        p::EventPayload::OrchestrationRouteCreated(payload) => payload
            .checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.artifact.clone()),
        _ => None,
    }
}

fn tombstone_target(event: &p::Event) -> Option<String> {
    match &event.payload {
        p::EventPayload::RetractionEvent(payload) => Some(payload.target_object.0.clone()),
        p::EventPayload::RevocationEvent(payload) => Some(payload.target_object.0.clone()),
        _ => None,
    }
}

fn event_object_refs(event: &p::Event) -> Vec<String> {
    let mut refs = vec![event.event_id.0.clone()];
    if let Some(content_ref) = event_content_ref(event) {
        refs.push(content_ref.0);
    }
    match &event.payload {
        p::EventPayload::MemoryNodeAppended(payload) => refs.push(payload.node_id.0.clone()),
        p::EventPayload::CandidateCreated(payload) => {
            refs.push(payload.candidate_id.0.clone());
            refs.push(payload.target.0.clone());
        }
        p::EventPayload::GoalFramed(payload) => refs.push(payload.goal_frame.0.clone()),
        p::EventPayload::ProspectiveIntentionCreated(payload) => {
            refs.push(payload.intention_id.0.clone())
        }
        p::EventPayload::ExternalCommunicationGranted(payload) => {
            refs.push(payload.grant_ref.0.clone())
        }
        p::EventPayload::PluginContributionRegistered(payload) => {
            refs.push(payload.manifest.0.clone())
        }
        _ => {}
    }
    refs
}

fn intrinsically_sensitive(kind: p::EventKind) -> bool {
    matches!(
        kind,
        p::EventKind::RunAccepted
            | p::EventKind::ModelCallDelta
            | p::EventKind::ToolCallProposed
            | p::EventKind::ActionOutputDelta
    )
}

fn secret_ref_like(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "secret:",
        "secretref",
        "credential:",
        "credential_ref",
        "authorization",
        "api_key",
        "api-key",
        "bearer ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn scope_contains(granted: &p::Scope, requested: &p::Scope) -> bool {
    requested.0 == granted.0
        || requested
            .0
            .strip_prefix(&granted.0)
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with(':'))
}
