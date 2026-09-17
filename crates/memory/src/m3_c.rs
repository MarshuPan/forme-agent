use std::collections::{BTreeMap, BTreeSet};

use forme_protocol as p;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyMemoryCandidateState {
    Candidate,
    Stable,
    Rejected,
    Downgraded,
    Decayed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyMemoryCandidateProjection {
    pub schema_version: p::SchemaVersion,
    pub candidate: p::CandidateId,
    pub domain: p::StrategyDomain,
    pub scope: p::Scope,
    pub version: p::StrategyVersionRef,
    pub baseline: p::StrategyVersionRef,
    pub evidence: Vec<p::EvidenceRef>,
    pub conflicts: Vec<p::CandidateId>,
    pub state: StrategyMemoryCandidateState,
    pub active: bool,
    pub trusted_lineage: bool,
    pub created_at: p::Timestamp,
    pub last_changed_at: p::Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StrategyMemoryLineageEdge {
    pub evidence: p::EvidenceRef,
    pub candidate: p::CandidateId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyMemoryRecommendation {
    Reevaluate {
        candidate: p::CandidateId,
        triggers: Vec<p::EvidenceRef>,
    },
    Downgrade {
        candidate: p::CandidateId,
        reason: p::ReasonRef,
    },
    Rollback {
        domain: p::StrategyDomain,
        scope: p::Scope,
        failed: p::StrategyVersionRef,
        restored: p::StrategyVersionRef,
        triggers: Vec<p::EvidenceRef>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyMemorySnapshot {
    pub schema_version: p::SchemaVersion,
    pub candidates: Vec<StrategyMemoryCandidateProjection>,
    pub lineage: Vec<StrategyMemoryLineageEdge>,
    pub retracted_evidence: Vec<p::EvidenceRef>,
    pub recommendations: Vec<StrategyMemoryRecommendation>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StrategyMemoryProjector;

impl StrategyMemoryProjector {
    pub fn rebuild(
        &self,
        events: &[p::Event],
        spec: &p::StrategyMemorySpec,
        now: p::Timestamp,
    ) -> p::Result<StrategyMemorySnapshot> {
        spec.validate()?;
        if now <= 0 || events.iter().any(|event| event.stream_seq == 0) {
            return Err(p::Error(
                "strategy memory projection requires sequenced events and a valid clock".into(),
            ));
        }

        let mut candidates = BTreeMap::<p::CandidateId, StrategyMemoryCandidateProjection>::new();
        let mut lineage = BTreeSet::<StrategyMemoryLineageEdge>::new();
        let mut retracted = BTreeSet::<p::EvidenceRef>::new();
        let mut reevaluated = BTreeSet::<p::CandidateId>::new();
        let mut active_versions =
            BTreeMap::<(p::StrategyDomain, p::Scope), p::StrategyVersionRef>::new();

        for event in events {
            match &event.payload {
                p::EventPayload::CandidateCreated(payload) => {
                    let Some(candidate) = payload.strategy_candidate.as_ref() else {
                        continue;
                    };
                    candidate.validate()?;
                    if payload.candidate_id != candidate.candidate
                        || payload.evidence_refs != candidate.evidence
                        || payload.target_tier != candidate.target_tier
                        || payload.provenance != candidate.provenance
                    {
                        return Err(p::Error(
                            "strategy memory candidate envelope does not match its event payload"
                                .into(),
                        ));
                    }
                    if candidate.domain != p::StrategyDomain::StrategyMemory
                        || candidate.scope != spec.envelope.scope
                    {
                        continue;
                    }
                    if candidate.schema_version != spec.envelope.schema_version {
                        return Err(p::Error(
                            "strategy memory candidate requires an explicit additive schema reader"
                                .into(),
                        ));
                    }
                    if candidate.evidence.len() > usize::from(spec.maximum_derived_edges) {
                        return Err(p::Error(
                            "strategy memory candidate exceeds its bounded evidence lineage".into(),
                        ));
                    }
                    let trusted = authoritative(&event.provenance);
                    let projected = StrategyMemoryCandidateProjection {
                        schema_version: candidate.schema_version,
                        candidate: candidate.candidate.clone(),
                        domain: candidate.domain,
                        scope: candidate.scope.clone(),
                        version: candidate.proposed_version.clone(),
                        baseline: candidate.baseline.clone(),
                        evidence: candidate.evidence.clone(),
                        conflicts: Vec::new(),
                        state: StrategyMemoryCandidateState::Candidate,
                        active: false,
                        trusted_lineage: trusted,
                        created_at: event.ts_unix_ms,
                        last_changed_at: event.ts_unix_ms,
                    };
                    if let Some(existing) = candidates.get(&candidate.candidate) {
                        if existing != &projected {
                            return Err(p::Error(
                                "strategy memory candidate id maps to different content".into(),
                            ));
                        }
                    } else {
                        if trusted {
                            for evidence in &candidate.evidence {
                                lineage.insert(StrategyMemoryLineageEdge {
                                    evidence: evidence.clone(),
                                    candidate: candidate.candidate.clone(),
                                });
                            }
                        }
                        candidates.insert(candidate.candidate.clone(), projected);
                    }
                }
                p::EventPayload::CandidateConflictDetected(payload)
                    if authoritative(&event.provenance) =>
                {
                    add_conflict(
                        &mut candidates,
                        &payload.candidate_id,
                        &payload.conflict_with,
                        spec.maximum_derived_edges,
                    )?;
                    add_conflict(
                        &mut candidates,
                        &payload.conflict_with,
                        &payload.candidate_id,
                        spec.maximum_derived_edges,
                    )?;
                }
                p::EventPayload::CandidatePromoted(payload) if authoritative(&event.provenance) => {
                    transition(
                        &mut candidates,
                        &payload.candidate_id,
                        StrategyMemoryCandidateState::Stable,
                        event.ts_unix_ms,
                    );
                }
                p::EventPayload::CandidateRejected(payload) if authoritative(&event.provenance) => {
                    transition(
                        &mut candidates,
                        &payload.candidate_id,
                        StrategyMemoryCandidateState::Rejected,
                        event.ts_unix_ms,
                    );
                }
                p::EventPayload::CandidateDowngraded(payload)
                    if authoritative(&event.provenance) =>
                {
                    transition(
                        &mut candidates,
                        &payload.candidate_id,
                        StrategyMemoryCandidateState::Downgraded,
                        event.ts_unix_ms,
                    );
                }
                p::EventPayload::CandidateDecayed(payload) if authoritative(&event.provenance) => {
                    transition(
                        &mut candidates,
                        &payload.candidate_id,
                        StrategyMemoryCandidateState::Decayed,
                        event.ts_unix_ms,
                    );
                }
                p::EventPayload::RetractionEvent(payload) if authoritative(&event.provenance) => {
                    retracted.insert(p::EvidenceRef(payload.target_object.0.clone()));
                }
                p::EventPayload::ReevaluationTaskCreated(payload)
                    if authoritative(&event.provenance) =>
                {
                    for derived in &payload.derived_refs {
                        reevaluated.insert(p::CandidateId(derived.0.clone()));
                    }
                }
                p::EventPayload::StrategyActivated(payload) if authoritative(&event.provenance) => {
                    payload.activation.validate()?;
                    if payload.active_snapshot.0.trim().is_empty() {
                        return Err(p::Error(
                            "strategy memory activation has no active snapshot".into(),
                        ));
                    }
                    if payload.activation.domain == p::StrategyDomain::StrategyMemory
                        && payload.activation.scope == spec.envelope.scope
                    {
                        active_versions.insert(
                            (payload.activation.domain, payload.activation.scope.clone()),
                            payload.activation.to.clone(),
                        );
                    }
                }
                p::EventPayload::StrategyRolledBack(payload)
                    if authoritative(&event.provenance) =>
                {
                    payload.rollback.validate()?;
                    if payload.active_snapshot.0.trim().is_empty() {
                        return Err(p::Error(
                            "strategy memory rollback has no active snapshot".into(),
                        ));
                    }
                    if payload.rollback.domain == p::StrategyDomain::StrategyMemory
                        && payload.rollback.scope == spec.envelope.scope
                    {
                        active_versions.insert(
                            (payload.rollback.domain, payload.rollback.scope.clone()),
                            payload.rollback.restored.clone(),
                        );
                    }
                }
                _ => {}
            }
        }

        for candidate in candidates.values_mut() {
            candidate.active = active_versions
                .get(&(candidate.domain, candidate.scope.clone()))
                .is_some_and(|version| version == &candidate.version);
        }

        let mut recommendations = Vec::new();
        for candidate in candidates.values() {
            if !candidate.trusted_lineage
                || matches!(
                    candidate.state,
                    StrategyMemoryCandidateState::Rejected
                        | StrategyMemoryCandidateState::Downgraded
                        | StrategyMemoryCandidateState::Decayed
                )
            {
                continue;
            }
            let retracted_triggers = candidate
                .evidence
                .iter()
                .filter(|evidence| retracted.contains(*evidence))
                .cloned()
                .collect::<Vec<_>>();
            let stale = elapsed_at_least(
                candidate.last_changed_at,
                spec.envelope.evidence_policy.decay_after,
                now,
            );
            let conflicted = !candidate.conflicts.is_empty();
            if retracted_triggers.is_empty() && !stale && !conflicted {
                continue;
            }

            let mut triggers = retracted_triggers;
            if stale {
                triggers.push(p::EvidenceRef(format!(
                    "freshness-expired:{}",
                    candidate.candidate.0
                )));
            }
            if conflicted {
                triggers.extend(
                    candidate
                        .conflicts
                        .iter()
                        .map(|conflict| p::EvidenceRef(format!("conflict:{}", conflict.0))),
                );
            }
            triggers.sort();
            triggers.dedup();
            if !reevaluated.contains(&candidate.candidate) {
                recommendations.push(StrategyMemoryRecommendation::Reevaluate {
                    candidate: candidate.candidate.clone(),
                    triggers: triggers.clone(),
                });
            }
            if candidate.state == StrategyMemoryCandidateState::Stable || candidate.active {
                recommendations.push(StrategyMemoryRecommendation::Downgrade {
                    candidate: candidate.candidate.clone(),
                    reason: p::ReasonRef(if retracted.contains_any(&candidate.evidence) {
                        "strategy evidence was retracted".into()
                    } else if conflicted {
                        "strategy candidate conflicts with retained evidence".into()
                    } else {
                        "strategy evidence is stale".into()
                    }),
                });
            }
            if candidate.active && spec.rollback_on_active_evidence_loss {
                recommendations.push(StrategyMemoryRecommendation::Rollback {
                    domain: candidate.domain,
                    scope: candidate.scope.clone(),
                    failed: candidate.version.clone(),
                    restored: spec.envelope.rollback_policy.known_good.clone(),
                    triggers,
                });
            }
        }

        let mut candidates = candidates.into_values().collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.candidate.cmp(&right.candidate));
        Ok(StrategyMemorySnapshot {
            schema_version: p::SchemaVersion(1),
            candidates,
            lineage: lineage.into_iter().collect(),
            retracted_evidence: retracted.into_iter().collect(),
            recommendations,
        })
    }
}

trait ContainsEvidence {
    fn contains_any(&self, evidence: &[p::EvidenceRef]) -> bool;
}

impl ContainsEvidence for BTreeSet<p::EvidenceRef> {
    fn contains_any(&self, evidence: &[p::EvidenceRef]) -> bool {
        evidence.iter().any(|item| self.contains(item))
    }
}

fn add_conflict(
    candidates: &mut BTreeMap<p::CandidateId, StrategyMemoryCandidateProjection>,
    candidate: &p::CandidateId,
    conflict: &p::CandidateId,
    maximum_derived_edges: u16,
) -> p::Result<()> {
    if let Some(candidate) = candidates.get_mut(candidate) {
        if !candidate.conflicts.contains(conflict) {
            if candidate.conflicts.len() >= usize::from(maximum_derived_edges) {
                return Err(p::Error(
                    "strategy memory conflict graph exceeds its bounded edge limit".into(),
                ));
            }
            candidate.conflicts.push(conflict.clone());
            candidate.conflicts.sort();
        }
    }
    Ok(())
}

fn transition(
    candidates: &mut BTreeMap<p::CandidateId, StrategyMemoryCandidateProjection>,
    candidate: &p::CandidateId,
    state: StrategyMemoryCandidateState,
    changed_at: p::Timestamp,
) {
    if let Some(candidate) = candidates.get_mut(candidate) {
        candidate.state = state;
        candidate.last_changed_at = changed_at;
    }
}

fn authoritative(provenance: &p::Provenance) -> bool {
    matches!(
        (&provenance.actor, provenance.trust_tier),
        (p::Actor::Owner, p::TrustTier::OwnerInput)
            | (p::Actor::System, p::TrustTier::VerifiedProcess)
    )
}

fn elapsed_at_least(occurred_at: p::Timestamp, window: p::DurationMs, now: p::Timestamp) -> bool {
    now.checked_sub(occurred_at)
        .and_then(|elapsed| u64::try_from(elapsed).ok())
        .is_some_and(|elapsed| elapsed >= window.0)
}
