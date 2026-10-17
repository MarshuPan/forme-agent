use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MaintenanceIssue {
    EvidenceAge,
    Conflict,
}

impl<S: EventStore> CognitiveRuntime<S> {
    /// Additional causes complement the B1 long-unseen path. A fixed graph
    /// supplies explicit, trusted contradiction evidence, never an LLM guess.
    pub fn audit_maintenance_snapshot(
        &self,
        tick: Tick,
        snapshot: &[StableCognition],
        graph: &MemoryGraphSnapshot,
    ) -> p::Result<Vec<p::CandidateId>> {
        let mut candidates = self.audit_stale_snapshot(tick, snapshot)?;
        if tick.schema_version != p::SchemaVersion(1) || tick.stale_after_ms <= 0 {
            return Err(p::Error("invalid cognitive maintenance bounds".into()));
        }
        let _guard = self.lock_command()?;
        for stable in snapshot.iter().filter(|stable| {
            stable.active
                && !matches!(
                    stable.tier,
                    p::StabilityTier::Fixed | p::StabilityTier::Constitutional
                )
        }) {
            if candidates.len() >= 64 {
                break;
            }
            let mut issues = BTreeSet::new();
            let mut refs = stable
                .evidence
                .iter()
                .map(|e| e.reference.clone())
                .collect::<Vec<_>>();
            let unseen = stable
                .last_reproduced_at
                .saturating_add(tick.stale_after_ms)
                <= tick.now;
            if !unseen
                && !stable.evidence.is_empty()
                && stable
                    .evidence
                    .iter()
                    .all(|e| e.observed_at.saturating_add(tick.stale_after_ms) <= tick.now)
            {
                issues.insert(MaintenanceIssue::EvidenceAge);
            }
            for edge in graph.edges.values().filter(|edge| {
                edge.kind == EdgeKind::Contradicts
                    && edge.provenance.trust_tier != p::TrustTier::Untrusted
            }) {
                let Some(from) = graph.nodes.get(&edge.from) else {
                    continue;
                };
                let Some(to) = graph.nodes.get(&edge.to) else {
                    continue;
                };
                if from.provenance.trust_tier == p::TrustTier::Untrusted
                    || to.provenance.trust_tier == p::TrustTier::Untrusted
                    || from.scope.scope != stable.scope.scope
                    || to.scope.scope != stable.scope.scope
                {
                    continue;
                }
                let other = if from.content_ref.0 == stable.object.0 {
                    Some(&to.content_ref.0)
                } else if to.content_ref.0 == stable.object.0 {
                    Some(&from.content_ref.0)
                } else {
                    None
                };
                if other.is_some_and(|object| {
                    snapshot.iter().any(|candidate| {
                        candidate.active
                            && candidate.object.0 == *object
                            && candidate.scope == stable.scope
                    })
                }) {
                    issues.insert(MaintenanceIssue::Conflict);
                    refs.push(p::EvidenceRef(format!("memory-edge:{}", edge.id.0)));
                }
            }
            if issues.is_empty() {
                continue;
            }
            let affected = {
                let state = self.lock_state()?;
                let Some(current) = state.stable.get(&stable.object) else {
                    continue;
                };
                let mut version = current.clone();
                version.active = stable.active;
                if &version != stable || state.retracted.contains(&stable.object) {
                    continue;
                }
                let mut affected = vec![stable.object.clone()];
                affected.extend(Self::transitive_derived(&state, &stable.object));
                affected
            };
            let reasons = issues
                .iter()
                .map(|issue| format!("{issue:?}"))
                .collect::<Vec<_>>();
            let identity = p::canonical_digest(&(tick.now, stable_record(stable), &reasons))?;
            let failure = p::FailureEvidenceRef(format!("memory-maintenance:{}", identity.0));
            self.append_stale_fact(
                &identity,
                tick.now,
                p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
                    failure_ref: failure.clone(),
                    class: p::FailureClass::MemoryMisevolution,
                    impact: if issues.contains(&MaintenanceIssue::Conflict) {
                        p::Impact::High
                    } else {
                        p::Impact::Medium
                    },
                    scope: stable.scope.scope.clone(),
                    related_refs: refs,
                    suggested_fix: Some(p::SuggestedFixRef(format!(
                        "re-evaluate {}",
                        reasons.join(",")
                    ))),
                }),
            )?;
            self.append_stale_fact(
                &identity,
                tick.now,
                p::EventPayload::ReevaluationTaskCreated(p::ReevaluationTaskCreatedPayload {
                    derived_refs: affected,
                    trigger: p::ReevaluationTriggerRef(format!(
                        "maintenance:{}:{}",
                        reasons.join("+"),
                        stable.object.0
                    )),
                }),
            )?;
            let candidate = p::CandidateId(format!("candidate:maintenance:{}", identity.0));
            self.memory.create_candidate_spec(memory::CandidateSpec {
                schema_version: p::SchemaVersion(1),
                id: candidate.clone(),
                target: p::CandidateTargetRef(format!("downgrade:{}", stable.object.0)),
                evidence_refs: vec![p::EvidenceRef(failure.0)],
                confidence: p::Confidence(0.4),
                provenance: system_provenance(),
                target_tier: p::StabilityTier::Working,
            })?;
            candidates.push(candidate);
        }
        Ok(candidates)
    }
}
