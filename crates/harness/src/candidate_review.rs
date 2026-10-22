//! Owner candidate transitions retain the frozen GatewayControl contract.
use super::*;

impl ReactiveHarness {
    pub(crate) fn review_candidate_bound(
        &self,
        command: p::CandidateReviewCommand,
    ) -> p::Result<()> {
        command.validate()?;
        let initial_events = self
            .store
            .read_run(command.run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let session = if self
            .brain_services
            .as_ref()
            .is_some_and(|s| s.aggregates.memory_cognition == command.run)
        {
            p::SessionId(format!("brain-review:{}", command.run.0))
        } else {
            run_session(&initial_events)?
        };
        let session_lock = self.session_lock(&session)?;
        let _session_guard = session_lock
            .lock()
            .map_err(|_| p::Error("session queue is unavailable".into()))?;
        let events = self
            .store
            .read_run(command.run.clone())
            .collect::<p::Result<Vec<_>>>()?;
        let candidate = candidate_snapshot(&command.candidate, &events)?;
        if candidate.state != command.expected_state {
            return Err(p::Error("candidate review expected state is stale".into()));
        }
        if !command
            .evidence
            .iter()
            .all(|evidence| evidence_is_resolved(evidence, &candidate, &events))
        {
            return Err(p::Error(
                "candidate review evidence is not in the run trace".into(),
            ));
        }
        let reason = p::ReasonRef(format!(
            "owner review with {} trace evidence reference(s)",
            command.evidence.len()
        ));
        let provenance = p::Provenance {
            source: p::Source::UserTurn,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        };
        match command.decision {
            p::CandidateReviewDecision::Promote
                if matches!(
                    candidate.state,
                    p::CandidateReviewState::Candidate | p::CandidateReviewState::Downgraded
                ) =>
            {
                let decision = if let Some(services) = self
                    .brain_services
                    .as_ref()
                    .filter(|services| services.aggregates.memory_cognition == command.run)
                {
                    services
                        .temporal_models
                        .review_candidate(command.candidate, true)?
                } else {
                    let store = Arc::new(self.store.clone());
                    let memory =
                        cognition::open_persistent_memory(store.clone(), command.run.clone())?;
                    let runtime = Arc::new(cognition::CognitiveRuntime::open(
                        store,
                        memory.clone(),
                        command.run,
                    )?);
                    cognition::TemporalModels::new(memory, runtime)
                        .review_candidate(command.candidate, true)?
                };
                if decision != cognition::GovernanceDecision::Promote {
                    return Err(p::Error(format!("candidate remains governed: {decision:?}; owner confirmation does not replace evidence")));
                }
            }
            p::CandidateReviewDecision::Reject
                if candidate.state == p::CandidateReviewState::Candidate =>
            {
                self.append(
                    command.run,
                    None,
                    provenance,
                    p::EventPayload::CandidateRejected(p::CandidateRejectedPayload {
                        candidate_id: command.candidate,
                        by: p::DecisionActor::User,
                        reason,
                    }),
                )?;
            }
            p::CandidateReviewDecision::Downgrade
                if matches!(
                    candidate.state,
                    p::CandidateReviewState::Candidate | p::CandidateReviewState::Promoted
                ) =>
            {
                self.append(
                    command.run,
                    None,
                    provenance,
                    p::EventPayload::CandidateDowngraded(p::CandidateDowngradedPayload {
                        candidate_id: command.candidate,
                        by: p::DecisionActor::User,
                        reason,
                    }),
                )?;
            }
            p::CandidateReviewDecision::Retract
                if candidate.state == p::CandidateReviewState::Promoted =>
            {
                let retraction = command
                    .retraction
                    .ok_or_else(|| p::Error("candidate retraction is missing".into()))?;
                if retraction.target.0 != candidate.target.0 {
                    return Err(p::Error(
                        "candidate retraction targets another stable object".into(),
                    ));
                }
                let retraction_event = self.append(
                    command.run.clone(),
                    None,
                    provenance,
                    p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                        target_object: retraction.target,
                        evidence_lineage: retraction.lineage,
                    }),
                )?;
                self.append(
                    command.run,
                    None,
                    p::Provenance {
                        source: p::Source::Internal,
                        actor: p::Actor::System,
                        trust_tier: p::TrustTier::VerifiedProcess,
                        caused_by: Some(retraction_event),
                    },
                    p::EventPayload::ReevaluationTaskCreated(p::ReevaluationTaskCreatedPayload {
                        derived_refs: retraction.derived_refs,
                        trigger: p::ReevaluationTriggerRef(format!(
                            "candidate-retraction:{}",
                            command.candidate.0
                        )),
                    }),
                )?;
            }
            _ => return Err(p::Error("candidate review transition is invalid".into())),
        }
        if let Some(services) = &self.brain_services {
            services.memory.refresh_projection()?;
            services.cognition.refresh_projection()?;
        }
        Ok(())
    }
}
