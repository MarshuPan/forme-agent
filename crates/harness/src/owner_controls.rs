//! Owner control and durable, session-scoped approval read projections.
use super::*;

impl ReactiveHarness {
    pub fn pending_approvals(&self, session: p::SessionId) -> p::Result<Vec<ApprovalRequest>> {
        Ok(self
            .pending_approval_records(&session)?
            .into_iter()
            .map(|(_, request)| request)
            .collect())
    }

    // Reading an outstanding approval must not require an in-memory RunHandle,
    // replan an action, or append a new fact. Both public read surfaces share
    // this projection so a service restart cannot hide an owner's decision.
    pub(super) fn pending_approval_records(
        &self,
        session: &p::SessionId,
    ) -> p::Result<Vec<(p::RunId, ApprovalRequest)>> {
        let mut pending = BTreeMap::new();
        let now = now_ms();
        for run in self.store.run_ids()? {
            let events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            let belongs = events.iter().any(|event| {
                matches!(
                    &event.payload, p::EventPayload::RunAccepted(p) if &p.session_ref == session
                )
            });
            if !belongs
                || events.iter().any(|event| {
                    matches!(
                        event.kind,
                        p::EventKind::RunComplete
                            | p::EventKind::RunAborted
                            | p::EventKind::RunFailed
                            | p::EventKind::RunLimited
                    )
                })
            {
                continue;
            }
            let mut requests = BTreeMap::new();
            for event in &events {
                match &event.payload {
                    p::EventPayload::ApprovalRequested(request) => {
                        requests.insert(request.approval_id.clone(), request);
                    }
                    p::EventPayload::ApprovalResolved(resolution) => {
                        requests.remove(&resolution.approval_id);
                    }
                    _ => {}
                }
            }
            for (id, request) in requests {
                if request.expires_at <= now {
                    continue;
                }
                let request = ApprovalRequest {
                    schema_version: p::SchemaVersion(1),
                    approval_id: id.clone(),
                    session: session.clone(),
                    action_summary: request.action_summary.0.clone(),
                    risk_level: request.risk,
                    scope: request.scope.clone(),
                    requested_permissions: request.requested_permissions.clone(),
                    normalized_action_digest: request.normalized_action_digest.clone().ok_or_else(
                        || p::Error("persisted approval has no action binding".into()),
                    )?,
                    affected_resources: request.affected_resources.clone(),
                    rollback_boundary: request.rollback_boundary.clone(),
                    expires_at: request.expires_at,
                    choices: request.choices.clone(),
                    plan_digest: request
                        .plan_digest
                        .clone()
                        .ok_or_else(|| p::Error("persisted approval has no plan binding".into()))?,
                    policy_version: request.policy_version.ok_or_else(|| {
                        p::Error("persisted approval has no policy binding".into())
                    })?,
                    tool_schema_version: request.tool_schema_version.ok_or_else(|| {
                        p::Error("persisted approval has no schema binding".into())
                    })?,
                };
                pending.insert((run.clone(), id), request);
            }
        }
        for child in self.child_executors()? {
            for (run, request) in child.pending_approval_records(session)? {
                pending
                    .entry((run, request.approval_id.clone()))
                    .or_insert(request);
            }
        }
        Ok(pending
            .into_iter()
            .map(|((run, _), request)| (run, request))
            .collect())
    }

    pub fn state(&self, run: p::RunId) -> p::Result<LoopState> {
        Ok(self
            .run_handle(&run)?
            .record
            .lock()
            .map_err(|_| p::Error("run state is unavailable".into()))?
            .loop_ctx
            .state
            .clone())
    }

    pub fn steer(&self, command: SteeringCommand) -> p::Result<p::EventId> {
        let (run, payload) = match command {
            SteeringCommand::CorrectMisunderstanding {
                run,
                target,
                lineage,
            }
            | SteeringCommand::Forget {
                run,
                target,
                lineage,
            } => (
                run,
                p::EventPayload::RetractionEvent(p::RetractionEventPayload {
                    target_object: target,
                    evidence_lineage: lineage,
                }),
            ),
            SteeringCommand::AdjustProactivity { run, scope, level } => (
                run,
                p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                    trace_ref: p::DecisionTraceRef(format!(
                        "steering:proactivity:{}:{level:?}",
                        scope.0
                    )),
                    refs: p::DecisionRefs {
                        map: None,
                        user: None,
                        agent_self: None,
                        trust: None,
                        failure: Vec::new(),
                    },
                    rationale: p::Rationale("owner adjusted proactivity".into()),
                    workspace_snapshot: p::AgentWorkspaceSnapshotRef("steering".into()),
                    resource_graph_snapshot: None,
                    evolution_snapshot: None,
                    federation_snapshot: None,
                    orchestration: None,
                }),
            ),
            SteeringCommand::ResolveCandidate {
                run,
                candidate,
                resolution,
            } => {
                let reason = p::ReasonRef("owner steering resolution".into());
                let payload = match resolution {
                    CandidateResolution::Promote => {
                        p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                            candidate_id: candidate,
                            by: p::DecisionActor::User,
                            reason,
                            stable: None,
                        })
                    }
                    CandidateResolution::Reject => {
                        p::EventPayload::CandidateRejected(p::CandidateRejectedPayload {
                            candidate_id: candidate,
                            by: p::DecisionActor::User,
                            reason,
                        })
                    }
                    CandidateResolution::Downgrade => {
                        p::EventPayload::CandidateDowngraded(p::CandidateDowngradedPayload {
                            candidate_id: candidate,
                            by: p::DecisionActor::User,
                            reason,
                        })
                    }
                };
                (run, payload)
            }
            SteeringCommand::SetDelegation { run, envelope } => (
                run,
                p::EventPayload::AutonomyEnvelopeSet(p::AutonomyEnvelopeSetPayload { envelope }),
            ),
        };
        self.run_handle(&run)?;
        self.append(
            run,
            None,
            p::Provenance {
                source: p::Source::UserTurn,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            payload,
        )
    }
}
