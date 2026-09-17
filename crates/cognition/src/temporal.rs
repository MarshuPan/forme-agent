use std::sync::Arc;

use forme_memory as memory;
use forme_protocol as p;
use forme_store::EventStore;

use crate::{
    ChangeDirection, CognitiveRuntime, GovernanceCandidate, GovernanceDecision, GovernanceEvidence,
    MapScope, StableCognition, StableCognitionKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalEvidence {
    pub schema_version: p::SchemaVersion,
    pub reference: p::EvidenceRef,
    pub observed_at: p::Timestamp,
    pub verified_process: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserObservation {
    pub schema_version: p::SchemaVersion,
    pub attribute: p::UserAttributeRef,
    pub value: p::UserAttributeValueRef,
    pub evidence: Vec<TemporalEvidence>,
    pub confidence: p::Confidence,
    pub scope: p::Scope,
    pub time_scale: memory::TimeScale,
    pub feedback: Vec<p::FeedbackRef>,
    pub provenance: p::Provenance,
    pub evidence_priority: memory::EvidencePriority,
}

pub trait UserModel {
    fn query(
        &self,
        key: p::UserAttributeRef,
        scope: p::Scope,
    ) -> Option<memory::UserModelAttribute>;
    fn observe(&self, observation: UserObservation) -> p::Result<p::CandidateId>;
    fn import_historical(&self, evidence: memory::ImportedHistoricalEvidence) -> p::Result<()>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSelfAttribute {
    pub schema_version: p::SchemaVersion,
    pub capability: p::CapabilityRef,
    pub reliability: p::Reliability,
    pub gap: Option<String>,
    pub evidence: Vec<TemporalEvidence>,
    pub confidence: p::Confidence,
    pub stability: p::StabilityTier,
    pub scope: p::Scope,
    pub provenance: p::Provenance,
    pub initial_profile: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfObservation {
    pub schema_version: p::SchemaVersion,
    pub capability: p::CapabilityRef,
    pub reliability: p::Reliability,
    pub gap: Option<String>,
    pub evidence: Vec<TemporalEvidence>,
    pub confidence: p::Confidence,
    pub scope: p::Scope,
    pub time_scale: memory::TimeScale,
    pub provenance: p::Provenance,
    pub initial_profile: bool,
}

pub trait AgentSelfModel {
    fn capability(&self, key: p::CapabilityRef) -> Option<AgentSelfAttribute>;
    fn observe(&self, observation: SelfObservation) -> p::Result<p::CandidateId>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationState {
    None,
    ApprovalRequired,
    Bounded,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PartnershipState {
    pub schema_version: p::SchemaVersion,
    pub scope: p::Scope,
    pub complement: Vec<String>,
    pub delegation_state: DelegationState,
    pub evidence: Vec<TemporalEvidence>,
    pub stability: p::StabilityTier,
    pub last_updated_at: p::Timestamp,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PartnershipObservation {
    pub schema_version: p::SchemaVersion,
    pub scope: p::Scope,
    pub complement: Vec<String>,
    pub delegation_state: DelegationState,
    pub evidence: Vec<TemporalEvidence>,
    pub time_scale: memory::TimeScale,
    pub provenance: p::Provenance,
}

pub trait PartnershipModel {
    fn state(&self, scope: p::Scope) -> PartnershipState;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    Untrusted,
    Observed,
    Established,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrustProfile {
    pub schema_version: p::SchemaVersion,
    pub scope: p::Scope,
    pub trust_level: TrustLevel,
    pub delegation_state: DelegationState,
    pub evidence: Vec<TemporalEvidence>,
    pub last_updated_at: p::Timestamp,
    pub stability: p::StabilityTier,
    pub provenance: p::Provenance,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrustObservation {
    pub schema_version: p::SchemaVersion,
    pub scope: p::Scope,
    pub trust_level: TrustLevel,
    pub delegation_state: DelegationState,
    pub evidence: Vec<TemporalEvidence>,
    pub time_scale: memory::TimeScale,
    pub provenance: p::Provenance,
    pub evidence_priority: memory::EvidencePriority,
}

pub struct TemporalModels<S: EventStore> {
    memory: Arc<memory::EventSourcedMemory<S>>,
    cognition: Arc<CognitiveRuntime<S>>,
    tier_map: memory::TimeScaleTierMap,
}

impl<S> TemporalModels<S>
where
    S: EventStore,
{
    pub fn new(
        memory: Arc<memory::EventSourcedMemory<S>>,
        cognition: Arc<CognitiveRuntime<S>>,
    ) -> Self {
        Self {
            memory,
            cognition,
            tier_map: memory::TimeScaleTierMap,
        }
    }

    pub fn review_user_candidate(
        &self,
        id: p::CandidateId,
        owner_confirmed: bool,
    ) -> p::Result<GovernanceDecision> {
        let candidate = self
            .memory
            .user_candidate(&id)
            .ok_or_else(|| p::Error("user candidate is not registered".into()))?;
        let evidence = temporal_evidence_from_user(&candidate);
        self.cognition.govern_and_apply(GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: id,
            direction: ChangeDirection::ConfidenceOrAutonomyIncreasing,
            confidence: candidate.confidence,
            evidence: governance_evidence(&evidence),
            impact: p::Impact::Medium,
            provenance: candidate.provenance.clone(),
            conflicts: candidate.conflicts.clone(),
            owner_confirmed,
            target: StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef(format!(
                    "user-attribute:{}:{}",
                    candidate.attribute.0, candidate.scope.0
                )),
                scope: map_scope(candidate.scope.clone()),
                kind: StableCognitionKind::UserAttribute,
                statement: format!("{} = {}", candidate.attribute.0, candidate.value.0),
                tier: candidate.stability,
                confidence: candidate.confidence,
                evidence: governance_evidence(&evidence),
                last_reproduced_at: candidate.last_updated_at,
                active: false,
            },
        })
    }

    pub fn review_agent_self_candidate(
        &self,
        id: p::CandidateId,
        owner_confirmed: bool,
    ) -> p::Result<GovernanceDecision> {
        let attribute = self
            .candidate_records()
            .find_map(|record| {
                (record.spec.id == id)
                    .then(|| parse_agent_self(&record))
                    .flatten()
            })
            .ok_or_else(|| p::Error("agent-self candidate is not registered".into()))?;
        self.cognition
            .govern_and_apply(governance_for_agent(id, attribute, owner_confirmed))
    }

    pub fn propose_partnership(
        &self,
        observation: PartnershipObservation,
    ) -> p::Result<p::CandidateId> {
        validate_partnership(&observation)?;
        let tier = bounded_tier(
            self.tier_map.tier(observation.time_scale),
            &observation.evidence,
        );
        let last_updated_at = latest_time(&observation.evidence);
        let state = PartnershipState {
            schema_version: observation.schema_version,
            scope: observation.scope,
            complement: observation.complement,
            delegation_state: observation.delegation_state,
            evidence: observation.evidence,
            stability: tier,
            last_updated_at,
            provenance: observation.provenance,
        };
        self.create_semantic_candidate(
            encode_partnership(&state),
            state.evidence.clone(),
            p::Confidence(if state.delegation_state == DelegationState::None {
                0.6
            } else {
                0.8
            }),
            state.stability,
            state.provenance,
        )
    }

    pub fn review_partnership_candidate(
        &self,
        id: p::CandidateId,
        owner_confirmed: bool,
    ) -> p::Result<GovernanceDecision> {
        let state = self
            .candidate_records()
            .find_map(|record| {
                (record.spec.id == id)
                    .then(|| parse_partnership(&record))
                    .flatten()
            })
            .ok_or_else(|| p::Error("partnership candidate is not registered".into()))?;
        let confidence = if state.delegation_state == DelegationState::None {
            p::Confidence(0.6)
        } else {
            p::Confidence(0.8)
        };
        self.cognition.govern_and_apply(GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: id,
            direction: if state.delegation_state == DelegationState::None {
                ChangeDirection::CautionIncreasing
            } else {
                ChangeDirection::ConfidenceOrAutonomyIncreasing
            },
            confidence,
            evidence: governance_evidence(&state.evidence),
            impact: if state.delegation_state == DelegationState::None {
                p::Impact::Low
            } else {
                p::Impact::High
            },
            provenance: state.provenance.clone(),
            conflicts: Vec::new(),
            owner_confirmed,
            target: StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef(format!("partnership:{}", state.scope.0)),
                scope: map_scope(state.scope.clone()),
                kind: StableCognitionKind::Partnership,
                statement: state.complement.join("; "),
                tier: state.stability,
                confidence,
                evidence: governance_evidence(&state.evidence),
                last_reproduced_at: state.last_updated_at,
                active: false,
            },
        })
    }

    pub fn propose_trust(&self, observation: TrustObservation) -> p::Result<p::CandidateId> {
        validate_trust(&observation)?;
        if observation.evidence_priority == memory::EvidencePriority::Imported {
            return Err(p::Error(
                "imported historical evidence cannot propose trust or delegation".into(),
            ));
        }
        let tier = bounded_tier(
            self.tier_map.tier(observation.time_scale),
            &observation.evidence,
        );
        let last_updated_at = latest_time(&observation.evidence);
        let profile = TrustProfile {
            schema_version: observation.schema_version,
            scope: observation.scope,
            trust_level: observation.trust_level,
            delegation_state: observation.delegation_state,
            evidence: observation.evidence,
            last_updated_at,
            stability: tier,
            provenance: observation.provenance,
        };
        self.create_semantic_candidate(
            encode_trust(&profile),
            profile.evidence.clone(),
            trust_confidence(profile.trust_level),
            profile.stability,
            profile.provenance,
        )
    }

    pub fn review_trust_candidate(
        &self,
        id: p::CandidateId,
        owner_confirmed: bool,
    ) -> p::Result<GovernanceDecision> {
        let profile = self
            .candidate_records()
            .find_map(|record| {
                (record.spec.id == id)
                    .then(|| parse_trust(&record))
                    .flatten()
            })
            .ok_or_else(|| p::Error("trust candidate is not registered".into()))?;
        let confidence = trust_confidence(profile.trust_level);
        self.cognition.govern_and_apply(GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: id,
            direction: if profile.trust_level == TrustLevel::Untrusted {
                ChangeDirection::CautionIncreasing
            } else {
                ChangeDirection::ConfidenceOrAutonomyIncreasing
            },
            confidence,
            evidence: governance_evidence(&profile.evidence),
            impact: if profile.delegation_state == DelegationState::None {
                p::Impact::Medium
            } else {
                p::Impact::High
            },
            provenance: profile.provenance.clone(),
            conflicts: Vec::new(),
            owner_confirmed,
            target: StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef(format!("trust:{}", profile.scope.0)),
                scope: map_scope(profile.scope.clone()),
                kind: StableCognitionKind::Trust,
                statement: format!(
                    "trust={:?}; delegation={:?}",
                    profile.trust_level, profile.delegation_state
                ),
                tier: profile.stability,
                confidence,
                evidence: governance_evidence(&profile.evidence),
                last_reproduced_at: profile.last_updated_at,
                active: false,
            },
        })
    }

    pub fn trust_profile(&self, scope: p::Scope) -> Option<TrustProfile> {
        self.candidate_records()
            .filter(|record| record.state == memory::CandidateState::Promoted)
            .filter_map(|record| parse_trust(&record))
            .filter(|profile| profile.scope == scope)
            .filter(|profile| {
                self.cognition
                    .is_object_usable(&p::ObjectRef(format!("trust:{}", profile.scope.0)))
            })
            .max_by_key(|profile| profile.last_updated_at)
    }

    fn create_semantic_candidate(
        &self,
        target: p::CandidateTargetRef,
        evidence: Vec<TemporalEvidence>,
        confidence: p::Confidence,
        target_tier: p::StabilityTier,
        provenance: p::Provenance,
    ) -> p::Result<p::CandidateId> {
        let id = self.memory.reserve_candidate_id();
        self.memory.create_candidate_spec(memory::CandidateSpec {
            schema_version: p::SchemaVersion(1),
            id,
            target,
            evidence_refs: evidence
                .into_iter()
                .map(|evidence| evidence.reference)
                .collect(),
            confidence,
            provenance,
            target_tier,
        })
    }

    fn candidate_records(&self) -> impl Iterator<Item = memory::CandidateRecord> {
        self.memory.candidate_records().into_iter()
    }
}

impl<S> UserModel for TemporalModels<S>
where
    S: EventStore,
{
    fn query(
        &self,
        key: p::UserAttributeRef,
        scope: p::Scope,
    ) -> Option<memory::UserModelAttribute> {
        let attribute = self.memory.user_attribute(&key, &scope)?;
        self.cognition
            .is_object_usable(&p::ObjectRef(format!(
                "user-attribute:{}:{}",
                key.0, scope.0
            )))
            .then_some(attribute)
    }

    fn observe(&self, observation: UserObservation) -> p::Result<p::CandidateId> {
        validate_user_observation(&observation)?;
        if observation.evidence_priority == memory::EvidencePriority::Imported {
            return Err(p::Error(
                "historical evidence must enter through import_historical".into(),
            ));
        }
        let tier = bounded_tier(
            self.tier_map.tier(observation.time_scale),
            &observation.evidence,
        );
        let candidate_id = self.memory.reserve_candidate_id();
        self.memory
            .create_user_attribute_candidate(memory::UserAttributeCandidate {
                schema_version: observation.schema_version,
                candidate_id,
                attribute: observation.attribute,
                value: observation.value,
                evidence: observation
                    .evidence
                    .iter()
                    .map(|evidence| evidence.reference.clone())
                    .collect(),
                confidence: observation.confidence,
                first_observed_at: earliest_time(&observation.evidence),
                last_updated_at: latest_time(&observation.evidence),
                stability: tier,
                scope: observation.scope,
                conflicts: Vec::new(),
                feedback: observation.feedback,
                provenance: observation.provenance,
                evidence_priority: observation.evidence_priority,
            })
    }

    fn import_historical(&self, evidence: memory::ImportedHistoricalEvidence) -> p::Result<()> {
        self.memory.import_historical(evidence)
    }
}

impl<S> AgentSelfModel for TemporalModels<S>
where
    S: EventStore,
{
    fn capability(&self, key: p::CapabilityRef) -> Option<AgentSelfAttribute> {
        self.candidate_records()
            .filter(|record| record.state == memory::CandidateState::Promoted)
            .filter_map(|record| parse_agent_self(&record))
            .filter(|attribute| attribute.capability == key)
            .filter(|attribute| {
                self.cognition.is_object_usable(&p::ObjectRef(format!(
                    "agent-self:{}",
                    attribute.capability.0
                )))
            })
            .max_by_key(|attribute| latest_time(&attribute.evidence))
    }

    fn observe(&self, observation: SelfObservation) -> p::Result<p::CandidateId> {
        validate_self_observation(&observation)?;
        let tier = if observation.initial_profile {
            p::StabilityTier::Working
        } else {
            bounded_tier(
                self.tier_map.tier(observation.time_scale),
                &observation.evidence,
            )
        };
        let attribute = AgentSelfAttribute {
            schema_version: observation.schema_version,
            capability: observation.capability,
            reliability: observation.reliability,
            gap: observation.gap,
            evidence: observation.evidence,
            confidence: observation.confidence,
            stability: tier,
            scope: observation.scope,
            provenance: observation.provenance,
            initial_profile: observation.initial_profile,
        };
        self.create_semantic_candidate(
            encode_agent_self(&attribute),
            attribute.evidence.clone(),
            attribute.confidence,
            attribute.stability,
            attribute.provenance,
        )
    }
}

impl<S> PartnershipModel for TemporalModels<S>
where
    S: EventStore,
{
    fn state(&self, scope: p::Scope) -> PartnershipState {
        self.candidate_records()
            .filter(|record| record.state == memory::CandidateState::Promoted)
            .filter_map(|record| parse_partnership(&record))
            .filter(|state| state.scope == scope)
            .filter(|state| {
                self.cognition
                    .is_object_usable(&p::ObjectRef(format!("partnership:{}", state.scope.0)))
            })
            .max_by_key(|state| state.last_updated_at)
            .unwrap_or_else(|| PartnershipState {
                schema_version: p::SchemaVersion(1),
                scope,
                complement: Vec::new(),
                delegation_state: DelegationState::None,
                evidence: Vec::new(),
                stability: p::StabilityTier::Ephemeral,
                last_updated_at: 0,
                provenance: system_provenance(),
            })
    }
}

fn governance_for_agent(
    id: p::CandidateId,
    attribute: AgentSelfAttribute,
    owner_confirmed: bool,
) -> GovernanceCandidate {
    GovernanceCandidate {
        schema_version: p::SchemaVersion(1),
        candidate_id: id,
        direction: ChangeDirection::ConfidenceOrAutonomyIncreasing,
        confidence: attribute.confidence,
        evidence: governance_evidence(&attribute.evidence),
        impact: p::Impact::Medium,
        provenance: attribute.provenance.clone(),
        conflicts: Vec::new(),
        owner_confirmed,
        target: StableCognition {
            schema_version: p::SchemaVersion(1),
            object: p::ObjectRef(format!("agent-self:{}", attribute.capability.0)),
            scope: map_scope(attribute.scope.clone()),
            kind: StableCognitionKind::AgentSelf,
            statement: format!(
                "capability={}; reliability={}",
                attribute.capability.0, attribute.reliability.0
            ),
            tier: attribute.stability,
            confidence: attribute.confidence,
            evidence: governance_evidence(&attribute.evidence),
            last_reproduced_at: latest_time(&attribute.evidence),
            active: false,
        },
    }
}

fn temporal_evidence_from_user(
    candidate: &memory::UserAttributeCandidate,
) -> Vec<TemporalEvidence> {
    candidate
        .evidence
        .iter()
        .enumerate()
        .map(|(index, reference)| TemporalEvidence {
            schema_version: p::SchemaVersion(1),
            reference: reference.clone(),
            observed_at: if index == 0 {
                candidate.first_observed_at
            } else {
                candidate.last_updated_at
            },
            verified_process: candidate.evidence_priority == memory::EvidencePriority::Process,
        })
        .collect()
}

fn governance_evidence(evidence: &[TemporalEvidence]) -> Vec<GovernanceEvidence> {
    evidence
        .iter()
        .map(|evidence| GovernanceEvidence {
            schema_version: evidence.schema_version,
            reference: evidence.reference.clone(),
            observed_at: evidence.observed_at,
            verified_process: evidence.verified_process,
        })
        .collect()
}

fn bounded_tier(requested: p::StabilityTier, evidence: &[TemporalEvidence]) -> p::StabilityTier {
    let timepoints = evidence
        .iter()
        .map(|evidence| evidence.observed_at)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if requested == p::StabilityTier::Stable && (evidence.len() < 2 || timepoints < 2) {
        p::StabilityTier::Working
    } else {
        requested
    }
}

fn validate_user_observation(observation: &UserObservation) -> p::Result<()> {
    if observation.schema_version.0 == 0
        || observation.attribute.0.trim().is_empty()
        || observation.value.0.trim().is_empty()
        || observation.scope.0.trim().is_empty()
        || observation.evidence.is_empty()
        || !observation.confidence.0.is_finite()
    {
        return Err(p::Error("user observation is incomplete".into()));
    }
    Ok(())
}

fn validate_self_observation(observation: &SelfObservation) -> p::Result<()> {
    if observation.schema_version.0 == 0
        || observation.capability.0.trim().is_empty()
        || observation.reliability.0.trim().is_empty()
        || observation.scope.0.trim().is_empty()
        || observation.evidence.is_empty()
        || !observation.confidence.0.is_finite()
    {
        return Err(p::Error("agent-self observation is incomplete".into()));
    }
    Ok(())
}

fn validate_partnership(observation: &PartnershipObservation) -> p::Result<()> {
    if observation.schema_version.0 == 0
        || observation.scope.0.trim().is_empty()
        || observation.complement.is_empty()
        || observation.evidence.is_empty()
    {
        return Err(p::Error("partnership observation is incomplete".into()));
    }
    Ok(())
}

fn validate_trust(observation: &TrustObservation) -> p::Result<()> {
    if observation.schema_version.0 == 0
        || observation.scope.0.trim().is_empty()
        || observation.evidence.is_empty()
    {
        return Err(p::Error("trust observation is incomplete".into()));
    }
    Ok(())
}

fn map_scope(scope: p::Scope) -> MapScope {
    MapScope {
        schema_version: p::SchemaVersion(1),
        scope,
    }
}

fn earliest_time(evidence: &[TemporalEvidence]) -> p::Timestamp {
    evidence
        .iter()
        .map(|evidence| evidence.observed_at)
        .min()
        .unwrap_or(0)
}

fn latest_time(evidence: &[TemporalEvidence]) -> p::Timestamp {
    evidence
        .iter()
        .map(|evidence| evidence.observed_at)
        .max()
        .unwrap_or(0)
}

fn trust_confidence(level: TrustLevel) -> p::Confidence {
    p::Confidence(match level {
        TrustLevel::Untrusted => 0.2,
        TrustLevel::Observed => 0.6,
        TrustLevel::Established => 0.9,
    })
}

fn encode_evidence(evidence: &[TemporalEvidence]) -> String {
    evidence
        .iter()
        .map(|evidence| {
            format!(
                "{}@{}@{}",
                escape(&evidence.reference.0),
                evidence.observed_at,
                u8::from(evidence.verified_process)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn decode_evidence(value: &str) -> Option<Vec<TemporalEvidence>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    value
        .split(',')
        .map(|item| {
            let mut fields = item.rsplitn(3, '@');
            let verified = fields.next()? == "1";
            let observed_at = fields.next()?.parse().ok()?;
            let reference = fields.next()?;
            Some(TemporalEvidence {
                schema_version: p::SchemaVersion(1),
                reference: p::EvidenceRef(unescape(reference)),
                observed_at,
                verified_process: verified,
            })
        })
        .collect()
}

fn encode_agent_self(attribute: &AgentSelfAttribute) -> p::CandidateTargetRef {
    p::CandidateTargetRef(format!(
        "agent-self|{}|{}|{}|{}|{}|{}|{}|{}",
        escape(&attribute.capability.0),
        escape(&attribute.reliability.0),
        escape(attribute.gap.as_deref().unwrap_or("")),
        escape(&attribute.scope.0),
        attribute.confidence.0,
        tier_name(attribute.stability),
        encode_evidence(&attribute.evidence),
        u8::from(attribute.initial_profile)
    ))
}

fn parse_agent_self(record: &memory::CandidateRecord) -> Option<AgentSelfAttribute> {
    let fields = fields(&record.spec.target, "agent-self", 8)?;
    Some(AgentSelfAttribute {
        schema_version: record.spec.schema_version,
        capability: p::CapabilityRef(unescape(fields[0])),
        reliability: p::Reliability(unescape(fields[1])),
        gap: {
            let gap = unescape(fields[2]);
            (!gap.is_empty()).then_some(gap)
        },
        scope: p::Scope(unescape(fields[3])),
        confidence: p::Confidence(fields[4].parse().ok()?),
        stability: parse_tier(fields[5])?,
        evidence: decode_evidence(fields[6])?,
        provenance: record.spec.provenance.clone(),
        initial_profile: fields[7] == "1",
    })
}

fn encode_partnership(state: &PartnershipState) -> p::CandidateTargetRef {
    p::CandidateTargetRef(format!(
        "partnership|{}|{}|{}|{}|{}|{}",
        escape(&state.scope.0),
        state
            .complement
            .iter()
            .map(|value| escape(value))
            .collect::<Vec<_>>()
            .join(","),
        delegation_name(state.delegation_state),
        state.last_updated_at,
        tier_name(state.stability),
        encode_evidence(&state.evidence)
    ))
}

fn parse_partnership(record: &memory::CandidateRecord) -> Option<PartnershipState> {
    let fields = fields(&record.spec.target, "partnership", 6)?;
    Some(PartnershipState {
        schema_version: record.spec.schema_version,
        scope: p::Scope(unescape(fields[0])),
        complement: if fields[1].is_empty() {
            Vec::new()
        } else {
            fields[1].split(',').map(unescape).collect()
        },
        delegation_state: parse_delegation(fields[2])?,
        last_updated_at: fields[3].parse().ok()?,
        stability: parse_tier(fields[4])?,
        evidence: decode_evidence(fields[5])?,
        provenance: record.spec.provenance.clone(),
    })
}

fn encode_trust(profile: &TrustProfile) -> p::CandidateTargetRef {
    p::CandidateTargetRef(format!(
        "trust|{}|{}|{}|{}|{}|{}",
        escape(&profile.scope.0),
        trust_name(profile.trust_level),
        delegation_name(profile.delegation_state),
        profile.last_updated_at,
        tier_name(profile.stability),
        encode_evidence(&profile.evidence)
    ))
}

fn parse_trust(record: &memory::CandidateRecord) -> Option<TrustProfile> {
    let fields = fields(&record.spec.target, "trust", 6)?;
    Some(TrustProfile {
        schema_version: record.spec.schema_version,
        scope: p::Scope(unescape(fields[0])),
        trust_level: parse_trust_level(fields[1])?,
        delegation_state: parse_delegation(fields[2])?,
        last_updated_at: fields[3].parse().ok()?,
        stability: parse_tier(fields[4])?,
        evidence: decode_evidence(fields[5])?,
        provenance: record.spec.provenance.clone(),
    })
}

fn fields<'a>(
    target: &'a p::CandidateTargetRef,
    prefix: &str,
    expected: usize,
) -> Option<Vec<&'a str>> {
    let value = target.0.strip_prefix(&format!("{prefix}|"))?;
    let fields = value.split('|').collect::<Vec<_>>();
    (fields.len() == expected).then_some(fields)
}

fn tier_name(tier: p::StabilityTier) -> &'static str {
    match tier {
        p::StabilityTier::Fixed => "fixed",
        p::StabilityTier::Constitutional => "constitutional",
        p::StabilityTier::Stable => "stable",
        p::StabilityTier::Working => "working",
        p::StabilityTier::Session => "session",
        p::StabilityTier::Ephemeral => "ephemeral",
    }
}

fn parse_tier(value: &str) -> Option<p::StabilityTier> {
    match value {
        "fixed" => Some(p::StabilityTier::Fixed),
        "constitutional" => Some(p::StabilityTier::Constitutional),
        "stable" => Some(p::StabilityTier::Stable),
        "working" => Some(p::StabilityTier::Working),
        "session" => Some(p::StabilityTier::Session),
        "ephemeral" => Some(p::StabilityTier::Ephemeral),
        _ => None,
    }
}

fn delegation_name(state: DelegationState) -> &'static str {
    match state {
        DelegationState::None => "none",
        DelegationState::ApprovalRequired => "approval-required",
        DelegationState::Bounded => "bounded",
    }
}

fn parse_delegation(value: &str) -> Option<DelegationState> {
    match value {
        "none" => Some(DelegationState::None),
        "approval-required" => Some(DelegationState::ApprovalRequired),
        "bounded" => Some(DelegationState::Bounded),
        _ => None,
    }
}

fn trust_name(level: TrustLevel) -> &'static str {
    match level {
        TrustLevel::Untrusted => "untrusted",
        TrustLevel::Observed => "observed",
        TrustLevel::Established => "established",
    }
}

fn parse_trust_level(value: &str) -> Option<TrustLevel> {
    match value {
        "untrusted" => Some(TrustLevel::Untrusted),
        "observed" => Some(TrustLevel::Observed),
        "established" => Some(TrustLevel::Established),
        _ => None,
    }
}

fn escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('|', "%7C")
        .replace(',', "%2C")
        .replace('@', "%40")
}

fn unescape(value: &str) -> String {
    value
        .replace("%40", "@")
        .replace("%2C", ",")
        .replace("%7C", "|")
        .replace("%25", "%")
}

fn system_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}
