//! Goal-dependent learning: unknown knowledge alone never justifies a question.
use crate::{
    CognitionSnapshot, CommunicationPurpose, DeliveryMode, ImpulseSource, InterventionLevel,
    Observation, ProposalIntent, StableCognition, StableCognitionKind,
};
use forme_protocol as p;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeGap {
    pub record: p::AskToLearnRecord,
    pub existing_confidence: p::Confidence,
}

impl KnowledgeGap {
    pub fn observation(&self) -> Observation {
        Observation {
            schema_version: p::SchemaVersion(1),
            source: p::Source::Internal,
            scope: self.record.scope.clone(),
            grant_ref: None,
            authorized: true,
            seed: vec![self.record.seed()],
            signal: ImpulseSource::Gap,
            estimated_value: match self.record.impact {
                p::Impact::High => 90,
                p::Impact::Medium => 65,
                p::Impact::Low => 20,
            },
            urgency: 50,
            requested_level: InterventionLevel::L1Suggest,
            delivery: DeliveryMode::Hitchhike,
            proposal_intent: ProposalIntent::Communication(CommunicationPurpose::AskToLearn),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LearningSnapshot {
    pub goals: BTreeMap<p::GoalFrameRef, p::GoalKnowledge>,
    pub proposals: BTreeMap<p::ProposalRef, (p::AskToLearnRecord, p::EventId)>,
    pub resolved: BTreeMap<p::ProposalRef, p::ProposalOutcome>,
    pub replies: BTreeMap<p::ProposalRef, p::LearningReplyRecord>,
}

impl LearningSnapshot {
    pub fn rebuild(events: &[p::Event]) -> Self {
        let mut snapshot = Self::default();
        let terminal: BTreeSet<_> = events
            .iter()
            .filter(|event| {
                matches!(
                    event.payload,
                    p::EventPayload::RunComplete(_)
                        | p::EventPayload::RunAborted(_)
                        | p::EventPayload::RunFailed(_)
                        | p::EventPayload::RunLimited(_)
                )
            })
            .map(|event| &event.run_id)
            .collect();
        for event in events {
            match &event.payload {
                p::EventPayload::GoalFramed(payload)
                    if !terminal.contains(&event.run_id) || payload.long_term.is_some() =>
                {
                    if let Some(knowledge) = &payload.knowledge {
                        snapshot
                            .goals
                            .insert(payload.goal_frame.clone(), knowledge.clone());
                    }
                }
                p::EventPayload::ProactiveProposalEmitted(payload) => {
                    if let Some(record) = &payload.learning {
                        snapshot.proposals.insert(
                            payload.proposal_ref.clone(),
                            (record.clone(), event.event_id.clone()),
                        );
                    }
                }
                p::EventPayload::ProactiveProposalResolved(payload) => {
                    snapshot
                        .resolved
                        .insert(payload.proposal_ref.clone(), payload.outcome);
                }
                p::EventPayload::ObservationRecorded(payload) => {
                    if let Some(reply) = &payload.learning_reply {
                        snapshot
                            .replies
                            .insert(reply.proposal.clone(), reply.clone());
                    }
                }
                _ => {}
            }
        }
        snapshot
    }

    pub fn origin_seen(&self, origin: &p::SchemaDigest) -> bool {
        self.proposals
            .values()
            .any(|(record, _)| &record.origin == origin)
    }

    pub fn pending(&self) -> impl Iterator<Item = (&p::ProposalRef, &p::AskToLearnRecord)> {
        self.proposals
            .iter()
            .filter(|(id, _)| !self.resolved.contains_key(*id) && !self.replies.contains_key(*id))
            .map(|(id, (record, _))| (id, record))
    }

    pub fn gaps(&self, stable: &[StableCognition]) -> p::Result<Vec<KnowledgeGap>> {
        let mut gaps = BTreeMap::new();
        for (goal, knowledge) in &self.goals {
            if !knowledge.allow_questions {
                continue;
            }
            for requirement in &knowledge.requirements {
                let confidence = stable
                    .iter()
                    .filter(|item| item.active && item.scope.scope == knowledge.scope)
                    .filter(|item| match &requirement.target {
                        p::LearningTarget::UserAttribute(attribute) => {
                            item.kind == StableCognitionKind::UserAttribute
                                && item.object.0
                                    == format!(
                                        "user-attribute:{}:{}",
                                        attribute.0, knowledge.scope.0
                                    )
                        }
                        p::LearningTarget::CognitiveMap(object) => &item.object == object,
                    })
                    .map(|item| item.confidence.0)
                    .fold(0.0_f32, f32::max);
                if confidence >= requirement.minimum_confidence.0 {
                    continue;
                }
                let mut record = p::AskToLearnRecord {
                    schema_version: p::SchemaVersion(1),
                    goal: goal.clone(),
                    scope: knowledge.scope.clone(),
                    target: requirement.target.clone(),
                    reason: requirement.reason.clone(),
                    impact: requirement.impact,
                    origin: p::SchemaDigest(String::new()),
                };
                record.origin = record.expected_origin()?;
                if !self.origin_seen(&record.origin) {
                    gaps.entry(record.origin.0.clone()).or_insert(KnowledgeGap {
                        record,
                        existing_confidence: p::Confidence(confidence),
                    });
                }
            }
        }
        Ok(gaps.into_values().collect())
    }
}

pub trait KnowledgeGapDetector {
    fn detect(
        &self,
        snapshot: &CognitionSnapshot,
        learning: &LearningSnapshot,
        stable: &[StableCognition],
    ) -> p::Result<Vec<KnowledgeGap>>;
}

pub struct GoalKnowledgeGapDetector;
impl KnowledgeGapDetector for GoalKnowledgeGapDetector {
    fn detect(
        &self,
        _snapshot: &CognitionSnapshot,
        learning: &LearningSnapshot,
        stable: &[StableCognition],
    ) -> p::Result<Vec<KnowledgeGap>> {
        learning.gaps(stable)
    }
}

impl<S: forme_store::EventStore> crate::CognitiveRuntime<S> {
    /// Only the trusted runtime calls this after authenticating and persisting
    /// the reply. A retry repairs an interrupted candidate chain, never promotes.
    pub fn propose_learning_reply(
        &self,
        ask: &p::AskToLearnRecord,
        reply: &p::LearningReplyRecord,
        evidence: p::EvidenceRef,
        observed_at: p::Timestamp,
    ) -> p::Result<p::CandidateId> {
        ask.validate()?;
        reply.validate()?;
        let id = p::CandidateId(format!(
            "candidate:learning:{}",
            p::canonical_digest(reply)?.0
        ));
        let provenance = p::Provenance {
            source: p::Source::UserTurn,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        };
        match &ask.target {
            p::LearningTarget::UserAttribute(attribute) => {
                let _command = self.lock_command()?;
                self.memory.create_user_attribute_candidate(
                    forme_memory::UserAttributeCandidate {
                        schema_version: p::SchemaVersion(1),
                        candidate_id: id.clone(),
                        attribute: attribute.clone(),
                        value: reply.value.clone(),
                        evidence: vec![evidence.clone()],
                        confidence: p::Confidence(0.4),
                        first_observed_at: observed_at,
                        last_updated_at: observed_at,
                        stability: p::StabilityTier::Working,
                        scope: ask.scope.clone(),
                        conflicts: Vec::new(),
                        feedback: Vec::new(),
                        provenance: provenance.clone(),
                        evidence_priority: forme_memory::EvidencePriority::Process,
                    },
                )?;
                self.memory
                    .create_candidate_spec(forme_memory::CandidateSpec {
                        schema_version: p::SchemaVersion(1),
                        id: id.clone(),
                        target: p::CandidateTargetRef(format!(
                            "user-attribute:{}:{}",
                            attribute.0, ask.scope.0
                        )),
                        evidence_refs: vec![evidence],
                        confidence: p::Confidence(0.4),
                        provenance,
                        target_tier: p::StabilityTier::Working,
                    })?;
            }
            p::LearningTarget::CognitiveMap(object) => {
                use crate::CognitiveMapStore;
                self.propose_update(crate::CognitiveMapUpdateProposal {
                    schema_version: p::SchemaVersion(1),
                    candidate_id: Some(id.clone()),
                    scope: crate::MapScope {
                        schema_version: p::SchemaVersion(1),
                        scope: ask.scope.clone(),
                    },
                    kind: crate::MapUpdateKind::Frame,
                    confidence: p::Confidence(0.4),
                    evidence: vec![evidence.clone()],
                    frame: Some(p::JudgmentFrameRef(object.0.clone())),
                    quality: None,
                    blindspot: None,
                    resource: None,
                    reflection_inputs: vec![evidence],
                    statement: reply.value.0.clone(),
                    provenance,
                })?;
            }
        }
        Ok(id)
    }
}
