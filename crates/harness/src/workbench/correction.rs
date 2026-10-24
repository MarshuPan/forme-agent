//! Exact owner feedback intake; no model, tool, direct stable write or retry.
use super::correction_receipts::{candidate_id, event_id, key, CorrectionFact, PREFIX};
use super::*;

struct Source {
    candidate: p::CandidateId,
    state: p::CandidateReviewState,
    object: p::ObjectRef,
    scope: p::Scope,
    heads: forme_store::TickEventCursor,
    events: Vec<p::Event>,
}

impl ProductionRuntime {
    pub fn correct_workbench_cognition(
        &self,
        owner: &p::VerifiedPrincipal,
        command: p::WorkbenchCorrectionCommand,
    ) -> p::Result<p::WorkbenchCorrectionReceipt> {
        let _data = self.data_read_guard()?;
        self.require_brain_owner(owner)?;
        command.validate()?;
        let harness = self.harness();
        let lock = harness.session_lock(&p::SessionId("workbench-owner-corrections".into()))?;
        let _guard = lock
            .lock()
            .map_err(|_| p::Error("owner correction queue is unavailable".into()))?;
        if let Some(receipt) = self.previous_correction(owner, &command)? {
            return Ok(receipt);
        }
        let first = self.correction_source(&command)?;
        let review_target = p::canonical_digest(&(&command.run, &first.candidate))?;
        let target_lock = harness.session_lock(&p::SessionId(format!(
            "workbench-review-target:{}",
            review_target.0
        )))?;
        let _target_guard = target_lock
            .lock()
            .map_err(|_| p::Error("owner correction target is unavailable".into()))?;
        let source = self.correction_source(&command)?;
        if super::review::pending_target(&source.events, &review_target) {
            return Err(p::Error(
                "candidate has an unresolved review; inspect the original receipt".into(),
            ));
        }
        let key = key(owner, &command.command)?;
        let binding = p::canonical_digest(&(owner, &command))?;
        let aggregate = self.services().aggregates.memory_cognition.clone();
        let store = self.services().store.clone();
        let protection = store
            .content_protection()?
            .ok_or_else(|| p::Error("owner correction protection is unavailable".into()))?;
        let (_, body) = protection.seal_record(
            command.statement.as_bytes(),
            &source.scope,
            &format!("owner-correction-body:{}", key.0),
            &p::canonical_digest(&("owner-correction", &key))?,
        )?;
        let material = crate::production_artifact_store::material_receipt_record(
            &protection,
            &body,
            command.statement.as_bytes(),
        )?;
        let provenance = p::Provenance {
            source: p::Source::OwnerControl,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: Some(command.event.clone()),
        };
        let at = now_ms();
        let make =
            |run: p::RunId, stage: &str, payload: p::EventPayload, provenance: p::Provenance| {
                p::Event::new(
                    event_id(&key, stage),
                    run,
                    None,
                    payload,
                    p::SchemaVersion(1),
                    at,
                    provenance,
                )
            };
        let mut events = Vec::new();
        let mut dependent = Vec::new();
        if source.state == p::CandidateReviewState::Promoted {
            let retraction = make(
                command.run.clone(),
                "retract",
                p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                    target_object: source.object.clone(),
                    evidence_lineage: p::LineageRef(format!("owner-correction:{}", key.0)),
                }),
                provenance.clone(),
            );
            let cause = retraction.event_id.clone();
            events.push(retraction);
            events.push(make(
                command.run.clone(),
                "reevaluate",
                p::EventPayload::ReevaluationTaskCreated(p::ReevaluationTaskCreatedPayload {
                    derived_refs: lineage(&source.events, &source.object),
                    trigger: p::ReevaluationTriggerRef(format!("owner-correction:{}", key.0)),
                }),
                p::Provenance {
                    source: p::Source::Internal,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::VerifiedProcess,
                    caused_by: Some(cause),
                },
            ));
        } else {
            events.push(make(
                command.run.clone(),
                "downgrade",
                p::EventPayload::CandidateDowngraded(p::CandidateDowngradedPayload {
                    candidate_id: source.candidate.clone(),
                    by: p::DecisionActor::User,
                    reason: p::ReasonRef(format!("owner-correction:{}", key.0)),
                }),
                provenance.clone(),
            ));
        }
        let observation = make(
            aggregate.clone(),
            "observation",
            p::EventPayload::ObservationRecorded(p::ObservationRecordedPayload {
                source: p::Source::OwnerControl,
                scope: source.scope.clone(),
                grant_ref: None,
                learning_reply: None,
            }),
            provenance.clone(),
        );
        let evidence = p::EvidenceRef(observation.event_id.0.clone());
        dependent.push(observation.event_id.clone());
        events.push(observation);
        let memory = make(
            aggregate.clone(),
            "memory",
            p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                node_id: p::NodeId(format!("owner-correction-memory:{}", key.0)),
                kind: p::MemoryNodeType("reflection".into()),
                content_ref: body.content_ref.clone(),
                tier: p::StabilityTier::Working,
                confidence: p::Confidence(0.4),
                scope: source.scope.clone(),
                resting_activation: p::RestingActivation(0.0),
                recency: p::Recency(at),
            }),
            provenance.clone(),
        );
        dependent.push(memory.event_id.clone());
        events.push(memory);
        let candidate = candidate_id(&key);
        for (index, payload) in forme_cognition::owner_correction_candidate(
            candidate.clone(),
            source.scope,
            source.object.clone(),
            command.statement.clone(),
            evidence,
            provenance.clone(),
        )?
        .into_iter()
        .enumerate()
        {
            let event = make(
                aggregate.clone(),
                &format!("candidate-{index}"),
                payload,
                provenance.clone(),
            );
            dependent.push(event.event_id.clone());
            events.push(event);
        }
        let conflict = make(
            aggregate.clone(),
            "conflict",
            p::EventPayload::CandidateConflictDetected(p::CandidateConflictDetectedPayload {
                candidate_id: candidate.clone(),
                conflict_with: source.candidate,
                kind: p::ConflictKind("owner-correction".into()),
            }),
            provenance.clone(),
        );
        dependent.push(conflict.event_id.clone());
        events.push(conflict);
        let fact = CorrectionFact {
            schema_version: p::SchemaVersion(1),
            command: command.command.clone(),
            binding,
            target: p::canonical_digest(&source.object)?,
            candidate,
        };
        let encoded = serde_json::to_string(&fact)
            .map_err(|_| p::Error("owner correction receipt encoding failed".into()))?;
        events.push(make(
            aggregate,
            "receipt",
            p::EventPayload::MemoryMaintenanceApplied(p::MemoryMaintenanceAppliedPayload {
                deltas_ref: p::MemoryDeltasRef(format!("{PREFIX}{encoded}")),
            }),
            provenance,
        ));
        let streams: BTreeSet<_> = events.iter().map(|e| e.run_id.clone()).collect();
        let expected = streams
            .into_iter()
            .map(|aggregate| p::AggregateVersion {
                schema_version: p::SchemaVersion(1),
                value: source.heads.0.get(&aggregate).copied().unwrap_or(0),
                aggregate,
            })
            .collect();
        let result =
            store.append_owner_correction(expected, events, vec![body, material], dependent);
        if let Err(error) = result {
            return self.previous_correction(owner, &command)?.ok_or(error);
        }
        self.services().memory.refresh_projection()?;
        self.services().cognition.refresh_projection()?;
        self.previous_correction(owner, &command)?
            .ok_or_else(|| p::Error("owner correction commit outcome is unknown".into()))
    }

    fn correction_source(&self, command: &p::WorkbenchCorrectionCommand) -> p::Result<Source> {
        let store = &self.services().store;
        let heads = store.run_heads()?;
        let (rows, _) = self.workbench_items(&p::WorkbenchQuery {
            schema_version: p::SchemaVersion(1),
            section: p::WorkbenchSection::Cognition,
            scope: None,
            offset: 0,
            limit: 100,
            snapshot: Some(command.snapshot.clone()),
        })?;
        let kind = match command.kind {
            p::WorkbenchCorrectionKind::Candidate => p::WorkbenchItemKind::Candidate,
            p::WorkbenchCorrectionKind::Stable => p::WorkbenchItemKind::Stable,
        };
        let row = rows
            .iter()
            .find(|row| {
                row.run == command.run
                    && row.id == command.id
                    && row.event == command.event
                    && row.kind == kind
            })
            .ok_or_else(|| p::Error("owner correction source is unavailable".into()))?;
        if row.tier.is_none_or(|tier| {
            matches!(
                tier,
                p::StabilityTier::Fixed | p::StabilityTier::Constitutional
            )
        }) {
            return Err(p::Error(
                "owner correction cannot modify this stability tier".into(),
            ));
        }
        let scope = row
            .scope
            .clone()
            .ok_or_else(|| p::Error("owner correction source scope is unavailable".into()))?;
        let events = self.owner_brain_events_at(None)?;
        let run_events: Vec<_> = events
            .iter()
            .filter(|e| e.run_id == command.run)
            .cloned()
            .collect();
        let candidate = if kind == p::WorkbenchItemKind::Candidate {
            p::CandidateId(row.id.clone())
        } else {
            if row.state != "Active" {
                return Err(p::Error("owner correction source is not active".into()));
            }
            run_events
                .iter()
                .find_map(|event| match &event.payload {
                    p::EventPayload::CandidatePromoted(v) if event.event_id == row.event => {
                        Some(v.candidate_id.clone())
                    }
                    _ => None,
                })
                .ok_or_else(|| {
                    p::Error("owner correction promotion source is unavailable".into())
                })?
        };
        let snapshot = crate::candidate_snapshot(&candidate, &run_events)?;
        let object = match snapshot.state {
            p::CandidateReviewState::Candidate => {
                p::ObjectRef(format!("candidate:{}", candidate.0))
            }
            p::CandidateReviewState::Promoted => {
                let promoted = run_events.iter().rev().find(|event|
                    matches!(&event.payload, p::EventPayload::CandidatePromoted(v) if v.candidate_id == candidate))
                    .ok_or_else(|| p::Error("owner correction promotion is unavailable".into()))?;
                let stable = rows
                    .iter()
                    .find(|r| {
                        r.kind == p::WorkbenchItemKind::Stable
                            && r.run == command.run
                            && r.event == promoted.event_id
                            && r.state == "Active"
                    })
                    .ok_or_else(|| {
                        p::Error("owner correction stable source is unavailable".into())
                    })?;
                if matches!(
                    stable.tier,
                    Some(p::StabilityTier::Fixed | p::StabilityTier::Constitutional)
                ) {
                    return Err(p::Error(
                        "owner correction cannot modify this stability tier".into(),
                    ));
                }
                p::ObjectRef(stable.id.clone())
            }
            _ => return Err(p::Error("owner correction source state is stale".into())),
        };
        if store.run_heads()? != heads {
            return Err(p::Error("owner correction source version is stale".into()));
        }
        Ok(Source {
            candidate,
            state: snapshot.state,
            object,
            scope,
            heads,
            events,
        })
    }
}
