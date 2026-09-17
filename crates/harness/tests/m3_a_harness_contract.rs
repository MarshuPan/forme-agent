use std::sync::Arc;

use forme_coordination::RuleBasedCoordinationReasoner;
use forme_eval::{DeterministicReplayEngine, ReplayEngine};
use forme_execution::ExecutionBackendRegistry;
use forme_harness::{
    AgentHarness, GovernanceConfig, HarnessConfig, M3EvolutionHarness, ReactiveHarness,
};
use forme_models::{
    Cost, ModelCapability, ModelOutput, ModelProfile, ModelResponse, ModelStrength, ModelToolCall,
    RateLimit, ScriptedModelProvider, Url,
};
use forme_protocol as p;
use forme_store::{EventStore, EvolutionEventStore, SqliteEventStore, StoreOptions};

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn event(id: &str, run: &str, payload: p::EventPayload) -> p::Event {
    p::Event::new(
        p::EventId(id.into()),
        p::RunId(run.into()),
        None,
        payload,
        p::SchemaVersion(1),
        1,
        provenance(),
    )
}

fn version(value: u64) -> p::EvolutionAggregateVersion {
    p::EvolutionAggregateVersion {
        schema_version: p::SchemaVersion(1),
        aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
        value,
    }
}

fn candidate(number: u8, baseline: &str, impact: p::EvolutionImpact) -> p::StrategyCandidate {
    p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId(format!("candidate:loop:v{number}")),
        domain: p::StrategyDomain::Loop,
        scope: p::Scope("workspace".into()),
        target_tier: p::StabilityTier::Stable,
        proposed_version: p::StrategyVersionRef(format!("loop:v{number}")),
        baseline: p::StrategyVersionRef(baseline.into()),
        spec_ref: p::ContentRef(format!("content:loop:v{number}")),
        spec_digest: p::SchemaDigest(format!("digest:loop:v{number}")),
        evidence: vec![p::EvidenceRef(format!("evidence:loop:v{number}"))],
        provenance: provenance(),
        impact,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef(baseline.into()),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    }
}

fn seed_active_v1(store: &SqliteEventStore) -> p::EvolutionSnapshot {
    let candidate = candidate(1, "loop:v0", p::EvolutionImpact::Cautious);
    store
        .append(event(
            "event:seed:candidate",
            "run:seed:candidate",
            p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
                candidate_id: candidate.candidate.clone(),
                target: p::CandidateTargetRef("strategy:loop:v1".into()),
                evidence_refs: candidate.evidence.clone(),
                confidence: p::Confidence(1.0),
                provenance: provenance(),
                target_tier: p::StabilityTier::Stable,
                capability_update: None,
                strategy_candidate: Some(candidate.clone()),
            }),
        ))
        .unwrap();
    store
        .append(event(
            "event:seed:evaluation",
            "run:seed:evaluation",
            p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
                evaluation: p::EvolutionEvaluationRef("evaluation:loop:v1".into()),
                baseline: p::StrategyVersionRef("loop:v0".into()),
                candidate: p::StrategyVersionRef("loop:v1".into()),
                verdict: p::EvaluationVerdict::Pass,
                hard_invariants: vec![p::InvariantResultRef("invariant:harness-first".into())],
                ground_truth: vec![p::EvidenceRef("ground-truth:seed".into())],
            }),
        ))
        .unwrap();
    store
        .append(event(
            "event:seed:promotion",
            "run:seed:promotion",
            p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
                candidate_id: candidate.candidate,
                by: p::DecisionActor::Auto,
                reason: p::ReasonRef("seed-reviewed".into()),
            }),
        ))
        .unwrap();
    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace".into());
    let mut activation = event(
        "event:seed:activation",
        "run:seed:activation",
        p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
            activation: p::StrategyActivation {
                schema_version: p::SchemaVersion(1),
                aggregate: aggregate.clone(),
                domain: p::StrategyDomain::Loop,
                scope: p::Scope("workspace".into()),
                from: None,
                to: p::StrategyVersionRef("loop:v1".into()),
                spec_ref: p::ContentRef("content:loop:v1".into()),
                spec_digest: p::SchemaDigest("digest:loop:v1".into()),
                evaluation: p::EvolutionEvaluationRef("evaluation:loop:v1".into()),
                promotion: p::EventId("event:seed:promotion".into()),
                owner_confirmation: None,
                impact: p::EvolutionImpact::Cautious,
                expected_version: version(0),
                committed_version: version(1),
            },
            active_snapshot: p::EvolutionSnapshotRef("pending-preview".into()),
        }),
    );
    let snapshot = store.preview_evolution_snapshot(&activation).unwrap();
    let p::EventPayload::StrategyActivated(payload) = &mut activation.payload else {
        unreachable!()
    };
    payload.active_snapshot = snapshot.snapshot.clone();
    store
        .append_evolution_expected(activation, &aggregate, version(0))
        .unwrap();
    snapshot
}

fn profile() -> ModelProfile {
    ModelProfile {
        schema_version: p::SchemaVersion(1),
        provider: p::ProviderId("m3-harness-test".into()),
        model: "m3-harness-model".into(),
        base_url: Url::parse("https://models.invalid/v1").unwrap(),
        capability: ModelCapability {
            schema_version: p::SchemaVersion(1),
            context_window: 16_384,
            tool_use: true,
            strength: ModelStrength::Standard,
        },
        cost: Cost {
            schema_version: p::SchemaVersion(1),
            input_microunits_per_million: 1,
            output_microunits_per_million: 1,
        },
        rate_limit: RateLimit {
            schema_version: p::SchemaVersion(1),
            requests_per_minute: 60,
            tokens_per_minute: 100_000,
        },
        credential_ref: p::CredentialRef("credential-ref:test".into()),
    }
}

fn final_response(value: &str) -> ModelResponse {
    ModelResponse {
        schema_version: p::SchemaVersion(1),
        output: ModelOutput::Final(value.into()),
        usage: p::ModelUsage {
            input_tokens: 2,
            output_tokens: 1,
        },
        finish_reason: p::FinishReason("stop".into()),
    }
}

fn request(source: p::Source, suffix: &str) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source,
        session: p::SessionRef(format!("session:m3:{suffix}")),
        agent_profile: p::AgentProfileRef("agent:m3".into()),
        input: p::RunInput(format!("m3 fixture {suffix}")),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey(format!("request:m3:{suffix}"))),
    }
}

fn comparison(candidate: &p::StrategyCandidate) -> p::EvolutionComparison {
    p::EvolutionComparison {
        schema_version: p::SchemaVersion(1),
        evaluation: p::EvolutionEvaluationRef("evaluation:loop:v2".into()),
        bundle: p::ReplayBundleRef("bundle:loop:v2".into()),
        baseline: candidate.baseline.clone(),
        candidate: candidate.proposed_version.clone(),
        case_set_digest: p::SchemaDigest("digest:train:v2".into()),
        holdout_digest: p::SchemaDigest("digest:holdout:v2".into()),
        metrics: vec![p::FitnessMetric {
            schema_version: p::SchemaVersion(1),
            dimension: p::FitnessDimension::Quality,
            outcome: p::FitnessOutcome::Pass,
            measured: Some(10_000),
            unit: p::FitnessUnit::BasisPoints,
            evidence: vec![p::EvidenceRef("ground-truth:v2".into())],
        }],
        hard_invariants: vec![p::InvariantResult {
            schema_version: p::SchemaVersion(1),
            reference: p::InvariantResultRef("invariant:harness-first".into()),
            name: "harness_first".into(),
            outcome: p::FitnessOutcome::Pass,
            evidence: vec![p::EvidenceRef("evidence:harness-first:v2".into())],
        }],
        ground_truth: vec![p::EvidenceRef("ground-truth:v2".into())],
        independent_verifier: true,
        self_eval_only: false,
    }
}

fn regression_comparison(candidate: &p::StrategyCandidate) -> p::EvolutionComparison {
    let mut comparison = comparison(candidate);
    comparison.evaluation = p::EvolutionEvaluationRef("evaluation:loop:v2:regression".into());
    comparison.metrics[0].outcome = p::FitnessOutcome::Fail;
    comparison.metrics[0].evidence = vec![p::EvidenceRef("evidence:regression:v2".into())];
    comparison.hard_invariants[0].outcome = p::FitnessOutcome::Fail;
    comparison.hard_invariants[0].evidence = vec![p::EvidenceRef("evidence:regression:v2".into())];
    comparison.ground_truth = vec![p::EvidenceRef("evidence:regression:v2".into())];
    comparison
}

fn assert_subsequence(actual: &[p::EventKind], expected: &[p::EventKind]) {
    let mut cursor = 0;
    for expected_kind in expected {
        let offset = actual[cursor..]
            .iter()
            .position(|kind| kind == expected_kind)
            .unwrap_or_else(|| panic!("missing {expected_kind:?} after event index {cursor}"));
        cursor += offset + 1;
    }
}

fn bound_snapshot(events: &[p::Event]) -> p::EvolutionSnapshotRef {
    events
        .iter()
        .find_map(|event| match &event.payload {
            p::EventPayload::SessionBound(payload) => payload.evolution_snapshot.clone(),
            _ => None,
        })
        .expect("M3 run binds an evolution snapshot")
}

#[test]
fn s53_exact_replay_writes_a_separate_effect_free_audit_run() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let snapshot = seed_active_v1(&store);
    let source_run = p::RunId("run:m3:replay-source".into());
    let payloads = [
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId("session:m3:replay-source".into()),
            input_ref: p::InputRef("input:m3:replay-source".into()),
            idempotency_key: None,
        }),
        p::EventPayload::SessionBound(p::SessionBoundPayload {
            policy_profile: p::PolicyProfileRef("policy:m3:replay".into()),
            model_profile: p::ModelProfileRef("model:m3:replay".into()),
            toolset_ref: p::ToolsetRef("toolset:m3:replay".into()),
            workspace: p::WorkspaceRef("workspace".into()),
            effect_mode: Some(p::EffectMode::LiveGoverned),
            evolution_snapshot: Some(snapshot.snapshot.clone()),
            federation_snapshot: None,
        }),
        p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
            verifier_kind: p::VerifierKind("recorded".into()),
            outcome: p::VerificationOutcome::Pass,
            against: p::DoneContractRef("done:recorded".into()),
        }),
        p::EventPayload::RunComplete(p::RunCompletePayload {
            stop_reason: p::StopReason("verified".into()),
            result_ref: None,
        }),
    ];
    let event_refs = payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            let id = p::EventId(format!("event:m3:replay-source:{}", index + 1));
            store.append(event(&id.0, &source_run.0, payload)).unwrap();
            id
        })
        .collect::<Vec<_>>();
    let source_before = store
        .read_run(source_run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let engine = DeterministicReplayEngine::new(Arc::new(store.clone()));
    let bundle = engine
        .build(p::ReplayRequest {
            schema_version: p::SchemaVersion(1),
            bundle: p::ReplayBundleRef("bundle:m3:replay-audit".into()),
            source_runs: vec![source_run.clone()],
            event_refs,
            cases: vec![p::EvaluationCaseRef("case:m3:replay-audit".into())],
            snapshot: p::ReplaySnapshot {
                schema_version: p::SchemaVersion(1),
                event_schema: p::SchemaVersion(1),
                policy: p::PolicyProfileRef("policy:m3:replay".into()),
                loop_spec: p::LoopSpecRef("loop:v1".into()),
                model: p::ModelProfileRef("model:m3:replay".into()),
                tool_schema: p::SchemaDigest("digest:toolset:m3:replay".into()),
                driver_profiles: vec![p::DriverProfileRef("driver:recorded".into())],
                evolution: snapshot,
                migration_graph_digest: p::SchemaDigest("digest:migrations:m3".into()),
            },
            effect_mode: p::EffectMode::ExactReplay,
        })
        .unwrap();
    let audit_run = p::RunId("run:m3:replay-audit".into());
    let audit = M3EvolutionHarness::new(store.clone())
        .exact_replay(
            audit_run.clone(),
            p::SessionId("session:m3:replay-audit".into()),
            p::WorkspaceRef("workspace".into()),
            p::ToolsetRef("toolset:m3:replay".into()),
            &engine,
            &bundle,
        )
        .unwrap();
    assert!(audit.report.projections_match);
    assert!(audit.report.history_unchanged);
    assert_eq!(audit.report.effect_calls, 0);
    assert_eq!(audit.events.len(), 5);
    assert_eq!(
        store
            .read_run(source_run)
            .collect::<p::Result<Vec<_>>>()
            .unwrap(),
        source_before
    );
    let audit_events = store
        .read_run(audit_run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        audit_events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ]
    );
    assert!(matches!(
        &audit_events[0].payload,
        p::EventPayload::RunAccepted(payload) if payload.source == p::Source::Replay
    ));
    assert!(matches!(
        &audit_events[1].payload,
        p::EventPayload::SessionBound(payload)
            if payload.effect_mode == Some(p::EffectMode::ExactReplay)
                && payload.evolution_snapshot.is_some()
    ));

    let generic = ReactiveHarness::new(
        SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap(),
        Arc::new(
            ScriptedModelProvider::new(profile(), vec![final_response("must not be called")])
                .unwrap(),
        ),
        Arc::new(ExecutionBackendRegistry::default()),
        GovernanceConfig::default(),
        HarnessConfig::for_model(&profile()),
    )
    .unwrap();
    assert!(generic
        .submit_run(request(p::Source::Replay, "generic-replay"))
        .is_err());
}

#[test]
fn s56_cautious_activation_is_automatic_but_never_authorizes() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    seed_active_v1(&store);
    let control = M3EvolutionHarness::new(store.clone());
    let run = p::RunId("run:m3:cautious-auto".into());
    let cautious = candidate(2, "loop:v1", p::EvolutionImpact::Cautious);
    control
        .record_candidate(run.clone(), cautious.clone())
        .unwrap();
    let evaluated = control
        .evaluate(run.clone(), &cautious, comparison(&cautious))
        .unwrap();
    assert_eq!(
        evaluated.decision,
        forme_cognition::EvolutionDecision::Promote
    );
    let promotion = control
        .promote(run.clone(), &cautious, &evaluated.evaluation)
        .unwrap();
    let activated = control
        .activate(
            run.clone(),
            p::EvolutionAggregateRef("evolution:owner:workspace".into()),
            &cautious,
            &evaluated.evaluation,
            promotion,
            None,
        )
        .unwrap();
    assert_eq!(activated.snapshot.strategies[0].version.0, "loop:v2");
    let events = store.read_run(run).collect::<p::Result<Vec<_>>>().unwrap();
    assert_subsequence(
        &events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        &[
            p::EventKind::CandidateCreated,
            p::EventKind::EvolutionEvaluationRecorded,
            p::EventKind::CandidatePromoted,
            p::EventKind::StrategyActivated,
        ],
    );
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::StrategyActivated(payload)
            if payload.activation.impact == p::EvolutionImpact::Cautious
                && payload.activation.owner_confirmation.is_none()
    )));
    assert!(!events.iter().any(|event| matches!(
        event.kind,
        p::EventKind::ApprovalResolved
            | p::EventKind::AutonomyEnvelopeSet
            | p::EventKind::ExternalCommunicationGranted
    )));
}

#[test]
fn s56_s57_candidate_promotion_activation_run_pinning_and_rollback_are_separate() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let seed = seed_active_v1(&store);
    let model = Arc::new(
        ScriptedModelProvider::new(
            profile(),
            vec![
                final_response("v1 result"),
                final_response("v2 result"),
                final_response("restored result"),
            ],
        )
        .unwrap(),
    );
    let harness = ReactiveHarness::new(
        store.clone(),
        model,
        Arc::new(ExecutionBackendRegistry::default()),
        GovernanceConfig::default(),
        HarnessConfig::for_model(&profile()),
    )
    .unwrap()
    .with_coordination(Arc::new(RuleBasedCoordinationReasoner));

    let old_run = harness
        .submit_run(request(p::Source::UserTurn, "old"))
        .unwrap();
    let old_events = harness.stream_events(old_run.clone()).events();
    assert_eq!(bound_snapshot(&old_events), seed.snapshot);

    let control = M3EvolutionHarness::new(store.clone());
    let control_run = p::RunId("run:m3:control:v2".into());
    let v2 = candidate(2, "loop:v1", p::EvolutionImpact::Bounded);
    control
        .record_candidate(control_run.clone(), v2.clone())
        .unwrap();
    let evaluated = control
        .evaluate(control_run.clone(), &v2, comparison(&v2))
        .unwrap();
    assert_eq!(
        evaluated.decision,
        forme_cognition::EvolutionDecision::NeedOwner
    );
    let promotion = control
        .promote(control_run.clone(), &v2, &evaluated.evaluation)
        .unwrap();
    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace".into());
    assert_eq!(
        store
            .active_for(&aggregate, p::StrategyDomain::Loop, &v2.scope)
            .unwrap()
            .unwrap()
            .version
            .0,
        "loop:v1"
    );
    assert!(control
        .activate(
            control_run.clone(),
            aggregate.clone(),
            &v2,
            &evaluated.evaluation,
            promotion.clone(),
            None,
        )
        .is_err());
    assert!(!store
        .read_run(control_run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
        .iter()
        .any(|event| event.kind == p::EventKind::StrategyActivated));

    let activated = control
        .activate(
            control_run.clone(),
            aggregate.clone(),
            &v2,
            &evaluated.evaluation,
            promotion,
            Some(p::OwnerControlRef("owner-control:v2".into())),
        )
        .unwrap();
    let new_run = harness
        .submit_run(request(p::Source::UserTurn, "new"))
        .unwrap();
    let new_events = harness.stream_events(new_run).events();
    assert_eq!(bound_snapshot(&new_events), activated.snapshot.snapshot);
    assert_ne!(bound_snapshot(&old_events), bound_snapshot(&new_events));
    assert!(new_events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::DecisionTraceRecorded(payload)
            if payload.evolution_snapshot.as_ref() == Some(&activated.snapshot.snapshot)
    )));

    assert!(control
        .rollback(
            control_run.clone(),
            aggregate.clone(),
            p::StrategyDomain::Loop,
            v2.scope.clone(),
            p::StrategyVersionRef("loop:v1".into()),
            vec![p::EvidenceRef("evidence:not-recorded".into())],
            p::InFlightDisposition::KeepPinned,
            None,
        )
        .is_err());
    assert!(control
        .rollback(
            control_run.clone(),
            aggregate.clone(),
            p::StrategyDomain::Loop,
            v2.scope.clone(),
            p::StrategyVersionRef("loop:v1".into()),
            vec![p::EvidenceRef("evidence:not-recorded".into())],
            p::InFlightDisposition::KeepPinned,
            Some(p::OwnerControlRef(" ".into())),
        )
        .is_err());
    store
        .append(event(
            "event:regression:v2",
            &control_run.0,
            p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
                failure_ref: p::FailureEvidenceRef("evidence:regression:v2".into()),
                class: p::FailureClass::VerificationFailure,
                impact: p::Impact::High,
                scope: v2.scope.clone(),
                related_refs: vec![p::EvidenceRef("loop:v2".into())],
                suggested_fix: Some(p::SuggestedFixRef("restore known-good loop:v1".into())),
            }),
        ))
        .unwrap();
    let regression = control
        .evaluate(control_run.clone(), &v2, regression_comparison(&v2))
        .unwrap();
    assert_eq!(
        regression.decision,
        forme_cognition::EvolutionDecision::Rollback
    );

    let rolled_back = control
        .rollback(
            control_run.clone(),
            aggregate.clone(),
            p::StrategyDomain::Loop,
            v2.scope.clone(),
            p::StrategyVersionRef("loop:v1".into()),
            vec![p::EvidenceRef("evidence:regression:v2".into())],
            p::InFlightDisposition::KeepPinned,
            None,
        )
        .unwrap();
    let restored_run = harness
        .submit_run(request(p::Source::UserTurn, "restored"))
        .unwrap();
    assert_eq!(
        bound_snapshot(&harness.stream_events(restored_run).events()),
        rolled_back.snapshot.snapshot
    );
    assert_eq!(rolled_back.snapshot.strategies[0].version.0, "loop:v1");
    assert_eq!(bound_snapshot(&old_events), seed.snapshot);

    let control_events = store
        .read_run(control_run)
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    assert_subsequence(
        &control_events
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        &[
            p::EventKind::CandidateCreated,
            p::EventKind::EvolutionEvaluationRecorded,
            p::EventKind::CandidatePromoted,
            p::EventKind::StrategyActivated,
            p::EventKind::FailureEvidenceRecorded,
            p::EventKind::EvolutionEvaluationRecorded,
            p::EventKind::StrategyRolledBack,
        ],
    );
    assert!(control_events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::StrategyRolledBack(payload)
            if serde_json::to_value(payload).unwrap()["rollback"]["external_effects_reverted"]
                == serde_json::json!(false)
    )));
    assert!(!control_events.iter().any(|event| matches!(
        event.kind,
        p::EventKind::ApprovalResolved
            | p::EventKind::AutonomyEnvelopeSet
            | p::EventKind::ExternalCommunicationGranted
    )));
}

fn tool_response() -> ModelResponse {
    let intent = p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:simulation".into()),
        source: p::Source::Simulation,
        goal: p::GoalRef("counterfactual".into()),
        backend_hint: p::BackendKind::Shell,
        capability_ref: p::CapabilityRef("capability:shell".into()),
        action_type: p::ActionType::Execute,
        scope: p::Scope("workspace".into()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("none".into()),
        parameters: p::ActionParameters::Shell {
            program: "not-registered".into(),
            args: vec!["mutate".into()],
            cwd: None,
            network: true,
        },
        requested_permissions: vec![p::PermissionRef("shell:execute".into())],
        requested_at: 1,
        estimated_output_bytes: 1,
        estimated_duration: p::DurationMs(1),
    };
    ModelResponse {
        schema_version: p::SchemaVersion(1),
        output: ModelOutput::Tool(Box::new(ModelToolCall {
            schema_version: p::SchemaVersion(1),
            call_id: p::ToolCallId("tool-call:simulation".into()),
            tool: p::ToolRef("not-registered".into()),
            arguments: serde_json::json!({"mutation": true}),
            intent: Some(intent),
        })),
        usage: p::ModelUsage {
            input_tokens: 2,
            output_tokens: 2,
        },
        finish_reason: p::FinishReason("tool_calls".into()),
    }
}

#[test]
fn s54_simulation_denies_before_backend_planning_or_execution() {
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    seed_active_v1(&store);
    let harness = ReactiveHarness::new(
        store,
        Arc::new(ScriptedModelProvider::new(profile(), vec![tool_response()]).unwrap()),
        Arc::new(ExecutionBackendRegistry::default()),
        GovernanceConfig::default(),
        HarnessConfig::for_model(&profile()),
    )
    .unwrap();
    let simulated_candidate = candidate(2, "loop:v1", p::EvolutionImpact::Cautious);
    let mut simulation_request = request(p::Source::Simulation, "simulation");
    simulation_request.idempotency_key = None;
    let run = harness
        .submit_evolution_simulation(
            simulation_request,
            simulated_candidate.clone(),
            comparison(&simulated_candidate),
        )
        .unwrap();
    let result = harness.wait(run.clone()).unwrap();
    assert_eq!(result.status, p::RunStatus::Complete);
    let events = harness.stream_events(run).events();
    let kinds = events.iter().map(|event| event.kind).collect::<Vec<_>>();
    assert_subsequence(
        &kinds,
        &[
            p::EventKind::RunAccepted,
            p::EventKind::SessionBound,
            p::EventKind::ToolCallProposed,
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::ActionDenied,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::EvolutionEvaluationRecorded,
            p::EventKind::RunComplete,
        ],
    );
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::SessionBound(payload)
            if payload.effect_mode == Some(p::EffectMode::CounterfactualDeny)
                && payload.evolution_snapshot.is_some()
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::ActionDenied(payload)
            if payload.reason.0 == "simulation_effect_denied"
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        p::EventPayload::EvolutionEvaluationRecorded(payload)
            if payload.verdict == p::EvaluationVerdict::Pass
                && payload.candidate == simulated_candidate.proposed_version
    )));
    assert!(!kinds.iter().any(|kind| matches!(
        kind,
        p::EventKind::ActionPlanned
            | p::EventKind::ActionStarted
            | p::EventKind::ActionCompleted
            | p::EventKind::CapabilityEvidenceRecorded
            | p::EventKind::FailureEvidenceRecorded
            | p::EventKind::RunAborted
    )));
}
