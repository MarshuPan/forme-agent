//! Owner-level cognitive observations and live, scope-bound competence reads.
use crate::{now_ms, BrainServices, ReactiveHarness};
use forme_cognition as c;
use forme_protocol as p;
use forme_store::{EventStore, TickEventCursor};

impl BrainServices {
    pub fn competence_inputs(
        &self,
        scope: &p::Scope,
        capability: Option<&p::CapabilityRef>,
    ) -> p::Result<c::CompetenceInputs> {
        self.competence_inputs_at(scope, capability, now_ms(), None)
    }

    pub(crate) fn competence_inputs_at(
        &self,
        scope: &p::Scope,
        capability: Option<&p::CapabilityRef>,
        now: p::Timestamp,
        cursor: Option<&TickEventCursor>,
    ) -> p::Result<c::CompetenceInputs> {
        let stable = match cursor {
            Some(cursor) => self
                .cognition
                .stable_snapshot_at(cursor.position(&self.aggregates.memory_cognition))?,
            None => self.cognition.stable_objects(),
        };
        let usable = stable
            .iter()
            .filter(|s| s.active && p::charter_scope_contains(&s.scope.scope, scope));
        let mut inputs = c::CompetenceInputs::default();
        let mut maps = Vec::new();
        for object in usable {
            match object.kind {
                c::StableCognitionKind::Frame
                | c::StableCognitionKind::Quality
                | c::StableCognitionKind::BlindSpot
                | c::StableCognitionKind::ResourceRelation => maps.push(object),
                c::StableCognitionKind::AgentSelf
                    if capability
                        .is_some_and(|cap| object.object.0 == format!("agent-self:{}", cap.0)) =>
                {
                    inputs.self_model = Some(c::AgentSelfInput {
                        schema_version: p::SchemaVersion(1),
                        reference: p::AgentSelfModelRef(object.object.0.clone()),
                        confidence: object.confidence,
                    });
                }
                c::StableCognitionKind::Trust => {
                    let ceiling =
                        if object.confidence.0 < 0.8 || object.statement.contains("Untrusted") {
                            c::InterventionLevel::L1Suggest
                        } else if object.statement.contains("Established")
                            && object.statement.contains("Bounded")
                        {
                            c::InterventionLevel::L4Autonomous
                        } else {
                            c::InterventionLevel::L3ActWithApproval
                        };
                    inputs.trust = Some(c::TrustInput {
                        schema_version: p::SchemaVersion(1),
                        reference: p::TrustProfileRef(object.object.0.clone()),
                        ceiling,
                    });
                }
                _ => {}
            }
        }
        if !maps.is_empty() {
            inputs.map_confidence = Some(c::MapConfidenceInput {
                schema_version: p::SchemaVersion(1),
                reference: p::MapConfidenceRef(format!("map-confidence:{}", scope.0)),
                value: p::Confidence(
                    maps.iter().map(|s| s.confidence.0).sum::<f32>() / maps.len() as f32,
                ),
            });
        }
        let cutoff = now.saturating_sub(7 * 24 * 60 * 60 * 1000);
        for run in self.store.run_ids()? {
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            let mut tool = None;
            let mut completed = None;
            let mut run_scope = None;
            let mut verified_turns = std::collections::BTreeSet::new();
            for e in events {
                if cursor.is_some_and(|cursor| e.stream_seq > cursor.position(&run)) {
                    break;
                }
                if e.ts_unix_ms < cutoff || e.ts_unix_ms > now {
                    continue;
                }
                match &e.payload {
                    p::EventPayload::SessionBound(bound) => {
                        run_scope = Some(p::Scope(bound.workspace.0.clone()))
                    }
                    p::EventPayload::ToolCallProposed(proposed) => {
                        tool = Some((p::CapabilityRef(proposed.tool.0.clone()), e.turn_id.clone()));
                        completed = None;
                    }
                    p::EventPayload::ActionCompleted(_) => completed = Some(e.turn_id.clone()),
                    p::EventPayload::ActionFailed(_) | p::EventPayload::ActionOutcomeUnknown(_) => {
                        completed = None
                    }
                    p::EventPayload::VerificationFinished(result)
                        if capability.is_some()
                            && run_scope
                                .as_ref()
                                .is_some_and(|s| p::charter_scope_contains(s, scope))
                            && tool.as_ref().is_some_and(|(c, turn)| {
                                Some(c) == capability && turn == &e.turn_id
                            })
                            && completed.as_ref() == Some(&e.turn_id) =>
                    {
                        // Text-final verification and repeated verifier passes are not new ability evidence.
                        if !verified_turns.insert(e.turn_id.clone()) {
                            continue;
                        }
                        inputs.verification.push(c::VerificationEvidenceInput {
                            schema_version: p::SchemaVersion(1),
                            reference: p::EvidenceRef(e.event_id.0.clone()),
                            passed: result.outcome == p::VerificationOutcome::Pass,
                        });
                        inputs.capability_evidence.push(c::CapabilityEvidenceInput {
                            schema_version: p::SchemaVersion(1),
                            reference: p::CapabilityEvidenceRef(e.event_id.0.clone()),
                            verified_success: result.outcome == p::VerificationOutcome::Pass,
                            reliability: p::Confidence(
                                if result.outcome == p::VerificationOutcome::Pass {
                                    0.9
                                } else {
                                    0.0
                                },
                            ),
                        });
                    }
                    p::EventPayload::FailureEvidenceRecorded(failure)
                        if p::charter_scope_contains(&failure.scope, scope)
                            && (capability.is_none()
                                || tool.as_ref().is_some_and(|(c, _)| Some(c) == capability)
                                || tool.is_none()) =>
                    {
                        inputs.failure.push(c::FailureEvidenceInput {
                            schema_version: p::SchemaVersion(1),
                            reference: failure.failure_ref.clone(),
                            impact: failure.impact,
                        });
                    }
                    _ => {}
                }
            }
        }
        inputs.failure.sort_by(|a, b| a.reference.cmp(&b.reference));
        inputs.failure.dedup_by(|a, b| a.reference == b.reference);
        Ok(inputs)
    }
}

impl ReactiveHarness {
    pub(crate) fn refresh_competence(
        &self,
        scope: &p::Scope,
        capability: Option<&p::CapabilityRef>,
    ) -> p::Result<c::CompetenceInputs> {
        match &self.brain_services {
            Some(services) => services.competence_inputs(scope, capability),
            None => Ok(self.competence_snapshot.clone()),
        }
    }
}
