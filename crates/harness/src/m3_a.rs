use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use forme_cognition::{
    ConservativeStrategyEvolutionGovernor, EvolutionDecision, StrategyEvolutionGovernor,
};
use forme_eval::{DeterministicEvolutionEvaluator, EvolutionEvaluator, ReplayEngine};
use forme_protocol as p;
use forme_store::{EventStore, EvolutionEventStore, EvolutionProjection, SqliteEventStore};

#[derive(Debug, Clone, PartialEq)]
pub struct EvolutionEvaluationResult {
    pub evaluation: p::EvolutionEvaluation,
    pub event_id: p::EventId,
    pub decision: EvolutionDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvolutionActivationResult {
    pub append: p::ExpectedAppend,
    pub snapshot: p::EvolutionSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactReplayAuditResult {
    pub report: p::ExactReplayReport,
    pub events: Vec<p::EventId>,
}

pub struct M3EvolutionHarness {
    store: SqliteEventStore,
    evaluator: Arc<dyn EvolutionEvaluator + Send + Sync>,
    governor: Arc<dyn StrategyEvolutionGovernor + Send + Sync>,
    sequence: AtomicU64,
    auto_activation_paused: AtomicBool,
}

impl M3EvolutionHarness {
    pub fn new(store: SqliteEventStore) -> Self {
        Self {
            store,
            evaluator: Arc::new(DeterministicEvolutionEvaluator),
            governor: Arc::new(ConservativeStrategyEvolutionGovernor),
            sequence: AtomicU64::new(1),
            auto_activation_paused: AtomicBool::new(false),
        }
    }

    pub fn with_evaluator(mut self, evaluator: Arc<dyn EvolutionEvaluator + Send + Sync>) -> Self {
        self.evaluator = evaluator;
        self
    }

    pub fn with_governor(
        mut self,
        governor: Arc<dyn StrategyEvolutionGovernor + Send + Sync>,
    ) -> Self {
        self.governor = governor;
        self
    }

    pub fn set_auto_activation_paused(&self, paused: bool) {
        self.auto_activation_paused.store(paused, Ordering::SeqCst);
    }

    pub fn auto_activation_paused(&self) -> bool {
        self.auto_activation_paused.load(Ordering::SeqCst)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn exact_replay<R: ReplayEngine + ?Sized>(
        &self,
        run: p::RunId,
        session: p::SessionId,
        workspace: p::WorkspaceRef,
        toolset: p::ToolsetRef,
        engine: &R,
        bundle: &p::ReplayBundle,
    ) -> p::Result<ExactReplayAuditResult> {
        bundle.validate()?;
        if run.0.trim().is_empty()
            || session.0.trim().is_empty()
            || workspace.0.trim().is_empty()
            || toolset.0.trim().is_empty()
            || bundle.effect_mode != p::EffectMode::ExactReplay
        {
            return Err(p::Error("exact replay audit context is incomplete".into()));
        }
        if self.store.read_run(run.clone()).next().is_some() {
            return Err(p::Error("exact replay audit run already exists".into()));
        }
        let report = engine.exact(bundle)?;
        report.validate()?;
        let done = p::DoneContractRef(format!("done:exact-replay:{}", bundle.bundle.0));
        let events = vec![
            self.append(
                run.clone(),
                verified_provenance(),
                p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                    source: p::Source::Replay,
                    session_ref: session,
                    input_ref: p::InputRef(bundle.bundle.0.clone()),
                    idempotency_key: None,
                }),
            )?,
            self.append(
                run.clone(),
                verified_provenance(),
                p::EventPayload::SessionBound(p::SessionBoundPayload {
                    policy_profile: bundle.snapshot.policy.clone(),
                    model_profile: bundle.snapshot.model.clone(),
                    toolset_ref: toolset,
                    workspace,
                    effect_mode: Some(p::EffectMode::ExactReplay),
                    evolution_snapshot: Some(bundle.snapshot.evolution.snapshot.clone()),
                    federation_snapshot: None,
                }),
            )?,
            self.append(
                run.clone(),
                verified_provenance(),
                p::EventPayload::VerificationStarted(p::VerificationStartedPayload {
                    verifier_kind: p::VerifierKind("exact-replay".into()),
                    against: done.clone(),
                }),
            )?,
            self.append(
                run.clone(),
                verified_provenance(),
                p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                    verifier_kind: p::VerifierKind("exact-replay".into()),
                    outcome: p::VerificationOutcome::Pass,
                    against: done,
                }),
            )?,
            self.append(
                run,
                verified_provenance(),
                p::EventPayload::RunComplete(p::RunCompletePayload {
                    stop_reason: p::StopReason("exact_replay_verified".into()),
                    result_ref: Some(p::EventId(report.report.0.clone())),
                }),
            )?,
        ];
        Ok(ExactReplayAuditResult { report, events })
    }

    pub fn record_candidate(
        &self,
        run: p::RunId,
        candidate: p::StrategyCandidate,
    ) -> p::Result<p::EventId> {
        candidate.validate()?;
        if !trusted_evolution_provenance(&candidate.provenance) {
            return Err(p::Error(
                "strategy candidate requires harness-verified or owner provenance".into(),
            ));
        }
        self.append(
            run,
            candidate.provenance.clone(),
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: candidate.candidate.clone(),
                target: p::CandidateTargetRef(format!(
                    "strategy:{}:{}",
                    domain_name(candidate.domain),
                    candidate.proposed_version.0
                )),
                evidence_refs: candidate.evidence.clone(),
                confidence: p::Confidence(1.0),
                provenance: candidate.provenance.clone(),
                target_tier: p::StabilityTier::Stable,
                capability_update: None,
                strategy_candidate: Some(candidate),
            }),
        )
    }

    pub fn evaluate(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        comparison: p::EvolutionComparison,
    ) -> p::Result<EvolutionEvaluationResult> {
        if comparison.candidate != candidate.proposed_version
            || comparison.baseline != candidate.baseline
        {
            return Err(p::Error(
                "evolution comparison does not match its strategy candidate".into(),
            ));
        }
        let recorded = self
            .store
            .strategy_candidate(&candidate.candidate)?
            .ok_or_else(|| p::Error("strategy candidate is not recorded".into()))?;
        if recorded != *candidate {
            return Err(p::Error(
                "recorded strategy candidate differs from evaluation input".into(),
            ));
        }
        let done = p::DoneContractRef(format!(
            "done:evolution-evaluation:{}",
            comparison.evaluation.0
        ));
        self.append(
            run.clone(),
            verified_provenance(),
            p::EventPayload::VerificationStarted(p::VerificationStartedPayload {
                verifier_kind: p::VerifierKind("evolution-ground-truth".into()),
                against: done.clone(),
            }),
        )?;
        let evaluation = match self.evaluator.compare(comparison) {
            Ok(evaluation) => evaluation,
            Err(error) => {
                let _ = self.append(
                    run,
                    verified_provenance(),
                    p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                        verifier_kind: p::VerifierKind("evolution-ground-truth".into()),
                        outcome: p::VerificationOutcome::Fail,
                        against: done,
                    }),
                );
                return Err(error);
            }
        };
        self.append(
            run.clone(),
            verified_provenance(),
            p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
                verifier_kind: p::VerifierKind("evolution-ground-truth".into()),
                outcome: if evaluation.verdict == p::EvaluationVerdict::Pass {
                    p::VerificationOutcome::Pass
                } else {
                    p::VerificationOutcome::Fail
                },
                against: done,
            }),
        )?;
        let event_id = self.append(
            run,
            verified_provenance(),
            p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
                evaluation: evaluation.evaluation.clone(),
                baseline: evaluation.baseline.clone(),
                candidate: evaluation.candidate.clone(),
                verdict: evaluation.verdict,
                hard_invariants: evaluation
                    .hard_invariants
                    .iter()
                    .map(|result| result.reference.clone())
                    .collect(),
                ground_truth: evaluation.ground_truth.clone(),
            }),
        )?;
        let current = self.store.snapshot(candidate.scope.clone())?;
        let decision = self.governor.decide(candidate, &evaluation, &current);
        Ok(EvolutionEvaluationResult {
            evaluation,
            event_id,
            decision,
        })
    }

    pub fn promote(
        &self,
        run: p::RunId,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
    ) -> p::Result<p::EventId> {
        if evaluation.verdict != p::EvaluationVerdict::Pass
            || evaluation.candidate != candidate.proposed_version
            || evaluation.baseline != candidate.baseline
        {
            return Err(p::Error(
                "only a matching passing evaluation may promote a strategy".into(),
            ));
        }
        let recorded = self
            .store
            .strategy_candidate(&candidate.candidate)?
            .ok_or_else(|| p::Error("strategy candidate is not recorded".into()))?;
        if recorded != *candidate {
            return Err(p::Error(
                "recorded strategy candidate differs from promotion input".into(),
            ));
        }
        let current = self.store.snapshot(candidate.scope.clone())?;
        if !matches!(
            self.governor.decide(candidate, evaluation, &current),
            EvolutionDecision::Promote | EvolutionDecision::NeedOwner
        ) {
            return Err(p::Error(
                "strategy governor did not permit stable promotion".into(),
            ));
        }
        self.append(
            run,
            verified_provenance(),
            p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                candidate_id: candidate.candidate.clone(),
                by: p::DecisionActor::Auto,
                reason: p::ReasonRef("independent ground-truth evaluation passed".into()),
            }),
        )
    }

    pub fn activate(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        candidate: &p::StrategyCandidate,
        evaluation: &p::EvolutionEvaluation,
        promotion: p::EventId,
        owner_confirmation: Option<p::OwnerControlRef>,
    ) -> p::Result<EvolutionActivationResult> {
        candidate.validate()?;
        evaluation.validate()?;
        if evaluation.verdict != p::EvaluationVerdict::Pass
            || evaluation.evaluation.0.trim().is_empty()
            || evaluation.candidate != candidate.proposed_version
            || evaluation.baseline != candidate.baseline
        {
            return Err(p::Error(
                "activation requires the candidate's passing evaluation".into(),
            ));
        }
        let owner_required = matches!(
            candidate.impact,
            p::EvolutionImpact::Bounded | p::EvolutionImpact::Expansive
        );
        if candidate.impact == p::EvolutionImpact::Constitutional
            || (owner_required && owner_confirmation.is_none())
            || (owner_confirmation.is_none() && self.auto_activation_paused())
        {
            return Err(p::Error(
                "activation impact requires owner confirmation or is constitutional".into(),
            ));
        }
        let stable = self
            .store
            .stable_strategy(
                candidate.domain,
                &candidate.scope,
                &candidate.proposed_version,
            )?
            .ok_or_else(|| p::Error("activation target is not stable".into()))?;
        if stable.candidate != *candidate || stable.promotion_event != promotion {
            return Err(p::Error(
                "activation target differs from its stable promotion".into(),
            ));
        }
        let from = self
            .store
            .active_for(&aggregate, candidate.domain, &candidate.scope)?
            .map(|active| active.version);
        if from
            .as_ref()
            .is_some_and(|active| *active != candidate.baseline)
        {
            return Err(p::Error(
                "candidate baseline is not the currently active strategy".into(),
            ));
        }
        let expected = self.store.evolution_version(&aggregate)?;
        let committed = expected.next()?;
        let provenance = if owner_confirmation.is_some() {
            owner_provenance()
        } else {
            verified_provenance()
        };
        let event_id = self.next_event_id(&run, "activate");
        let mut event = p::Event::new(
            event_id,
            run,
            None,
            p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
                activation: p::StrategyActivation {
                    schema_version: p::SchemaVersion(1),
                    aggregate: aggregate.clone(),
                    domain: candidate.domain,
                    scope: candidate.scope.clone(),
                    from,
                    to: candidate.proposed_version.clone(),
                    spec_ref: candidate.spec_ref.clone(),
                    spec_digest: candidate.spec_digest.clone(),
                    evaluation: evaluation.evaluation.clone(),
                    promotion,
                    owner_confirmation,
                    impact: candidate.impact,
                    expected_version: expected.clone(),
                    committed_version: committed,
                },
                active_snapshot: p::EvolutionSnapshotRef("pending-preview".into()),
            }),
            p::SchemaVersion(1),
            super::now_ms(),
            provenance,
        );
        let snapshot = self.store.preview_evolution_snapshot(&event)?;
        let p::EventPayload::StrategyActivated(payload) = &mut event.payload else {
            unreachable!()
        };
        payload.active_snapshot = snapshot.snapshot.clone();
        let append = self
            .store
            .append_evolution_expected(event, &aggregate, expected)?;
        if append.status != p::ExpectedAppendStatus::Applied {
            return Err(p::Error(
                "strategy activation lost its expected-version race".into(),
            ));
        }
        Ok(EvolutionActivationResult { append, snapshot })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rollback(
        &self,
        run: p::RunId,
        aggregate: p::EvolutionAggregateRef,
        domain: p::StrategyDomain,
        scope: p::Scope,
        restored: p::StrategyVersionRef,
        triggers: Vec<p::EvidenceRef>,
        in_flight: p::InFlightDisposition,
        owner_confirmation: Option<p::OwnerControlRef>,
    ) -> p::Result<EvolutionActivationResult> {
        if triggers.is_empty() || triggers.iter().any(|item| item.0.trim().is_empty()) {
            return Err(p::Error("strategy rollback has no trigger evidence".into()));
        }
        if owner_confirmation
            .as_ref()
            .is_some_and(|confirmation| confirmation.0.trim().is_empty())
        {
            return Err(p::Error(
                "strategy rollback owner confirmation is empty".into(),
            ));
        }
        if owner_confirmation.is_none() {
            let control_events = self
                .store
                .read_run(run.clone())
                .collect::<p::Result<Vec<_>>>()?;
            if triggers.iter().any(|trigger| {
                !control_events
                    .iter()
                    .any(|event| event_records_rollback_trigger(event, trigger))
            }) {
                return Err(p::Error(
                    "automatic strategy rollback requires recorded regression evidence".into(),
                ));
            }
        }
        let current = self
            .store
            .active_for(&aggregate, domain, &scope)?
            .ok_or_else(|| p::Error("strategy rollback has no active version".into()))?;
        if current.version == restored {
            return Err(p::Error(
                "strategy rollback target is already active".into(),
            ));
        }
        let stable = self
            .store
            .stable_strategy(domain, &scope, &restored)?
            .ok_or_else(|| p::Error("strategy rollback target is not stable".into()))?;
        let expected = self.store.evolution_version(&aggregate)?;
        let committed = expected.next()?;
        let event_id = self.next_event_id(&run, "rollback");
        let mut event = p::Event::new(
            event_id,
            run,
            None,
            p::EventPayload::StrategyRolledBack(p::StrategyRolledBackPayload {
                rollback: p::StrategyRollback {
                    schema_version: p::SchemaVersion(1),
                    aggregate: aggregate.clone(),
                    domain,
                    scope,
                    failed: current.version,
                    restored,
                    restored_spec_ref: stable.candidate.spec_ref,
                    restored_spec_digest: stable.candidate.spec_digest,
                    triggers,
                    expected_version: expected.clone(),
                    committed_version: committed,
                    in_flight,
                    external_effects_reverted: p::HistoricalFalse,
                },
                active_snapshot: p::EvolutionSnapshotRef("pending-preview".into()),
            }),
            p::SchemaVersion(1),
            super::now_ms(),
            if owner_confirmation.is_some() {
                owner_provenance()
            } else {
                verified_provenance()
            },
        );
        let snapshot = self.store.preview_evolution_snapshot(&event)?;
        let p::EventPayload::StrategyRolledBack(payload) = &mut event.payload else {
            unreachable!()
        };
        payload.active_snapshot = snapshot.snapshot.clone();
        let append = self
            .store
            .append_evolution_expected(event, &aggregate, expected)?;
        if append.status != p::ExpectedAppendStatus::Applied {
            return Err(p::Error(
                "strategy rollback lost its expected-version race".into(),
            ));
        }
        Ok(EvolutionActivationResult { append, snapshot })
    }

    fn append(
        &self,
        run: p::RunId,
        provenance: p::Provenance,
        payload: p::EventPayload,
    ) -> p::Result<p::EventId> {
        let event_id = self.next_event_id(&run, "record");
        self.store.append(p::Event::new(
            event_id,
            run,
            None,
            payload,
            p::SchemaVersion(1),
            super::now_ms(),
            provenance,
        ))
    }

    fn next_event_id(&self, run: &p::RunId, operation: &str) -> p::EventId {
        p::EventId(format!(
            "m3-control:{}:{operation}:{}",
            run.0,
            self.sequence.fetch_add(1, Ordering::SeqCst)
        ))
    }
}

fn event_records_rollback_trigger(event: &p::Event, trigger: &p::EvidenceRef) -> bool {
    match &event.payload {
        p::EventPayload::FailureEvidenceRecorded(payload) => payload.failure_ref.0 == trigger.0,
        p::EventPayload::EvolutionEvaluationRecorded(payload)
            if payload.verdict != p::EvaluationVerdict::Pass =>
        {
            payload
                .ground_truth
                .iter()
                .any(|reference| reference == trigger)
                || payload
                    .hard_invariants
                    .iter()
                    .any(|reference| reference.0 == trigger.0)
        }
        p::EventPayload::RetractionEvent(payload) => {
            payload.target_object.0 == trigger.0 || payload.evidence_lineage.0 == trigger.0
        }
        p::EventPayload::RevocationEvent(payload) => {
            payload.target_object.0 == trigger.0 || payload.evidence_lineage.0 == trigger.0
        }
        _ => false,
    }
}

fn domain_name(domain: p::StrategyDomain) -> &'static str {
    match domain {
        p::StrategyDomain::Loop => "loop",
        p::StrategyDomain::Coordination => "coordination",
        p::StrategyDomain::CapabilitySelection => "capability-selection",
        p::StrategyDomain::ModelSelection => "model-selection",
        p::StrategyDomain::BackendSelection => "backend-selection",
        p::StrategyDomain::ModelAdaptation => "model-adaptation",
        p::StrategyDomain::StrategyMemory => "strategy-memory",
        p::StrategyDomain::AgentSelf => "agent-self",
        p::StrategyDomain::Partnership => "partnership",
        p::StrategyDomain::TrustDelegation => "trust-delegation",
        p::StrategyDomain::Proactivity => "proactivity",
        p::StrategyDomain::Communication => "communication",
    }
}

fn verified_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn owner_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::UserTurn,
        actor: p::Actor::Owner,
        trust_tier: p::TrustTier::OwnerInput,
        caused_by: None,
    }
}

fn trusted_evolution_provenance(provenance: &p::Provenance) -> bool {
    matches!(
        (&provenance.actor, provenance.trust_tier),
        (p::Actor::Owner, p::TrustTier::OwnerInput)
            | (p::Actor::System, p::TrustTier::VerifiedProcess)
    )
}
