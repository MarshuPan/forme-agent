use std::sync::Arc;

use forme_eval::{
    DeterministicEvolutionEvaluator, DeterministicReplayEngine, EvolutionEvaluator,
    M3ArtifactStore, ReplayEngine, TraceManifest,
};
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore, StoreOptions};

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn event(id: &str, run: &p::RunId, payload: p::EventPayload) -> p::Event {
    p::Event::new(
        p::EventId(id.into()),
        run.clone(),
        None,
        payload,
        p::SchemaVersion(1),
        1,
        provenance(),
    )
}

fn evolution_snapshot() -> p::EvolutionSnapshot {
    p::EvolutionSnapshot {
        schema_version: p::SchemaVersion(1),
        snapshot: p::EvolutionSnapshotRef("snapshot:evolution:seed".into()),
        aggregates: vec![p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
            value: 1,
        }],
        strategies: vec![p::ActiveStrategyRef {
            schema_version: p::SchemaVersion(1),
            id: p::ActiveStrategyId("active:loop:seed".into()),
            aggregate: p::EvolutionAggregateRef("evolution:owner:workspace".into()),
            domain: p::StrategyDomain::Loop,
            scope: p::Scope("workspace".into()),
            version: p::StrategyVersionRef("loop:v1".into()),
            spec_ref: p::ContentRef("content:loop:v1".into()),
            spec_digest: p::SchemaDigest("digest:loop:v1".into()),
            activation_event: p::EventId("event:activation:v1".into()),
        }],
        digest: p::SchemaDigest("digest:evolution:seed".into()),
    }
}

fn replay_request(run: p::RunId, event_refs: Vec<p::EventId>) -> p::ReplayRequest {
    p::ReplayRequest {
        schema_version: p::SchemaVersion(1),
        bundle: p::ReplayBundleRef("bundle:s53".into()),
        source_runs: vec![run],
        event_refs,
        cases: vec![p::EvaluationCaseRef("case:s53".into())],
        snapshot: p::ReplaySnapshot {
            schema_version: p::SchemaVersion(1),
            event_schema: p::SchemaVersion(1),
            policy: p::PolicyProfileRef("policy:frozen".into()),
            loop_spec: p::LoopSpecRef("loop:v1".into()),
            model: p::ModelProfileRef("model:recorded".into()),
            tool_schema: p::SchemaDigest("digest:tools".into()),
            driver_profiles: vec![p::DriverProfileRef("driver:recorded".into())],
            evolution: evolution_snapshot(),
            migration_graph_digest: p::SchemaDigest("digest:migrations".into()),
        },
        effect_mode: p::EffectMode::ExactReplay,
    }
}

fn append_replayable_run(store: &SqliteEventStore, run: &p::RunId) -> Vec<p::EventId> {
    let payloads = [
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId("session:s53".into()),
            input_ref: p::InputRef("input:typed-fixture".into()),
            idempotency_key: Some(p::IdempotencyKey("request:s53".into())),
        }),
        p::EventPayload::SessionBound(p::SessionBoundPayload {
            policy_profile: p::PolicyProfileRef("policy:frozen".into()),
            model_profile: p::ModelProfileRef("model:recorded".into()),
            toolset_ref: p::ToolsetRef("toolset:recorded".into()),
            workspace: p::WorkspaceRef("workspace".into()),
            effect_mode: Some(p::EffectMode::LiveGoverned),
            evolution_snapshot: Some(p::EvolutionSnapshotRef("snapshot:evolution:seed".into())),
            federation_snapshot: None,
        }),
        p::EventPayload::TurnStarted(p::TurnStartedPayload { turn_index: 0 }),
        p::EventPayload::ModelCallDelta(p::ModelCallDeltaPayload {
            call_id: p::ModelCallId("model-call:s53".into()),
            delta: "recorded model output that must not enter the portable archive".into(),
        }),
        p::EventPayload::ToolCallProposed(p::ToolCallProposedPayload {
            call_id: p::ToolCallId("tool-call:s53".into()),
            tool: p::ToolRef("browser:click".into()),
            args: serde_json::json!({"selector": "#commit", "origin": "loopback-fixture"}),
        }),
        p::EventPayload::ActionOutputDelta(p::ActionOutputDeltaPayload {
            intent_id: p::ActionId("action:s53".into()),
            backend: p::BackendKind::Browser,
            scope: p::Scope("workspace".into()),
            delta: "untrusted external body that must not enter the portable archive".into(),
            truncated: false,
            trust: p::TrustTier::Untrusted,
            content_ref: Some(p::ContentRef("content:browser-receipt:s53".into())),
            remote_lease: None,
        }),
        p::EventPayload::VerificationFinished(p::VerificationFinishedPayload {
            verifier_kind: p::VerifierKind("deterministic".into()),
            outcome: p::VerificationOutcome::Pass,
            against: p::DoneContractRef("done:s53".into()),
        }),
        p::EventPayload::RunComplete(p::RunCompletePayload {
            stop_reason: p::StopReason("verified".into()),
            result_ref: None,
        }),
    ];
    payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            let id = p::EventId(format!("event:s53:{}", index + 1));
            store
                .append(event(&id.0, run, payload))
                .expect("fixture event appends");
            id
        })
        .collect()
}

#[test]
fn s53_portable_exact_replay_is_deterministic_read_only_and_effect_free() {
    let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    let run = p::RunId("run:s53".into());
    let refs = append_replayable_run(&store, &run);
    let engine = DeterministicReplayEngine::new(store.clone());

    let first = engine
        .build(replay_request(run.clone(), refs.clone()))
        .unwrap();
    let second = engine
        .build(replay_request(run.clone(), refs.clone()))
        .unwrap();
    assert_eq!(first, second);
    let before = store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let first_report = engine.exact(&first).unwrap();
    let second_report = engine.exact(&second).unwrap();
    assert_eq!(first_report, second_report);
    assert!(first_report.projections_match);
    assert!(first_report.history_unchanged);
    assert_eq!(first_report.effect_calls, 0);
    assert_eq!(first_report.event_count, refs.len() as u64);
    assert_eq!(
        store.read_run(run).collect::<p::Result<Vec<_>>>().unwrap(),
        before
    );

    let portable = engine.portable(&first.bundle).unwrap();
    let encoded = serde_json::to_string(&portable.events).unwrap();
    assert!(!encoded.contains("recorded model output"));
    assert!(!encoded.contains("#commit"));
    assert!(!encoded.contains("untrusted external body"));
    assert!(portable
        .events
        .iter()
        .any(|record| record.envelope.kind == p::EventKind::ModelCallDelta));
    assert!(portable
        .events
        .iter()
        .any(|record| record.envelope.kind == p::EventKind::ToolCallProposed));
    assert!(portable
        .events
        .iter()
        .any(|record| record.envelope.kind == p::EventKind::ActionOutputDelta));
    assert_eq!(
        DeterministicReplayEngine::<SqliteEventStore>::exact_portable(&portable).unwrap(),
        first_report
    );
}

#[test]
fn s53_portable_replay_rejects_partial_secret_and_private_path_inputs() {
    for (suffix, payload) in [
        (
            "secret",
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::UserTurn,
                session_ref: p::SessionId("session:secret".into()),
                input_ref: p::InputRef("secret:provider-key".into()),
                idempotency_key: None,
            }),
        ),
        (
            "path",
            p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                node_id: p::NodeId("node:path".into()),
                kind: p::MemoryNodeType("artifact".into()),
                content_ref: p::ContentRef("C:\\Users\\owner\\private.txt".into()),
                tier: p::StabilityTier::Working,
                confidence: p::Confidence(0.5),
                scope: p::Scope("workspace".into()),
                resting_activation: p::RestingActivation(0.1),
                recency: p::Recency(1),
            }),
        ),
        (
            "unix-path",
            p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
                node_id: p::NodeId("node:unix-path".into()),
                kind: p::MemoryNodeType("artifact".into()),
                content_ref: p::ContentRef("/tmp/owner/private.txt".into()),
                tier: p::StabilityTier::Working,
                confidence: p::Confidence(0.5),
                scope: p::Scope("workspace".into()),
                resting_activation: p::RestingActivation(0.1),
                recency: p::Recency(1),
            }),
        ),
        (
            "credential-field",
            p::EventPayload::ToolCallProposed(p::ToolCallProposedPayload {
                call_id: p::ToolCallId("call:credential".into()),
                tool: p::ToolRef("tool:request".into()),
                args: serde_json::json!({"authorization": "redacted"}),
            }),
        ),
    ] {
        let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
        let run = p::RunId(format!("run:s53:{suffix}"));
        let event_id = p::EventId(format!("event:s53:{suffix}"));
        store
            .append(event(&event_id.0, &run, payload))
            .expect("negative fixture is authoritative but not portable");
        let engine = DeterministicReplayEngine::new(store);
        assert!(engine.build(replay_request(run, vec![event_id])).is_err());
    }

    let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    let run = p::RunId("run:s53:partial".into());
    let refs = append_replayable_run(&store, &run);
    let engine = DeterministicReplayEngine::new(store);
    assert!(engine
        .build(replay_request(run, refs[..refs.len() - 1].to_vec()))
        .is_err());
}

fn metric(dimension: p::FitnessDimension, outcome: p::FitnessOutcome) -> p::FitnessMetric {
    p::FitnessMetric {
        schema_version: p::SchemaVersion(1),
        dimension,
        outcome,
        measured: Some(1),
        unit: p::FitnessUnit::Count,
        evidence: vec![p::EvidenceRef(format!("evidence:{dimension:?}"))],
    }
}

fn comparison() -> p::EvolutionComparison {
    p::EvolutionComparison {
        schema_version: p::SchemaVersion(1),
        evaluation: p::EvolutionEvaluationRef("evaluation:s55".into()),
        bundle: p::ReplayBundleRef("bundle:s55".into()),
        baseline: p::StrategyVersionRef("strategy:v1".into()),
        candidate: p::StrategyVersionRef("strategy:v2".into()),
        case_set_digest: p::SchemaDigest("digest:train".into()),
        holdout_digest: p::SchemaDigest("digest:holdout".into()),
        metrics: vec![
            metric(p::FitnessDimension::Quality, p::FitnessOutcome::Pass),
            metric(p::FitnessDimension::Cost, p::FitnessOutcome::Pass),
        ],
        hard_invariants: vec![p::InvariantResult {
            schema_version: p::SchemaVersion(1),
            reference: p::InvariantResultRef("invariant:harness-first".into()),
            name: "harness_first".into(),
            outcome: p::FitnessOutcome::Pass,
            evidence: vec![p::EvidenceRef("evidence:harness-first".into())],
        }],
        ground_truth: vec![p::EvidenceRef("ground-truth:s55".into())],
        independent_verifier: true,
        self_eval_only: false,
    }
}

#[test]
fn s55_ground_truth_and_hard_invariants_dominate_self_score_and_cost() {
    let evaluator = DeterministicEvolutionEvaluator;
    assert_eq!(
        evaluator.compare(comparison()).unwrap().verdict,
        p::EvaluationVerdict::Pass
    );

    let mut hard_failure = comparison();
    hard_failure.hard_invariants[0].outcome = p::FitnessOutcome::Fail;
    hard_failure.metrics[1].measured = Some(0);
    assert_eq!(
        evaluator.compare(hard_failure).unwrap().verdict,
        p::EvaluationVerdict::Fail
    );

    let mut no_ground_truth = comparison();
    no_ground_truth.ground_truth.clear();
    assert_eq!(
        evaluator.compare(no_ground_truth).unwrap().verdict,
        p::EvaluationVerdict::Unverifiable
    );

    let mut self_eval = comparison();
    self_eval.self_eval_only = true;
    assert_eq!(
        evaluator.compare(self_eval).unwrap().verdict,
        p::EvaluationVerdict::Unverifiable
    );

    let mut metric_failure = comparison();
    metric_failure.metrics[0].outcome = p::FitnessOutcome::Fail;
    assert_eq!(
        evaluator.compare(metric_failure).unwrap().verdict,
        p::EvaluationVerdict::Fail
    );

    let mut leaked_holdout = comparison();
    leaked_holdout.holdout_digest = leaked_holdout.case_set_digest.clone();
    assert!(evaluator.compare(leaked_holdout).is_err());
}

#[test]
fn m3_artifacts_are_typed_content_addressed_secret_free_and_offline_verifiable() {
    let store = Arc::new(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    let run = p::RunId("run:m3:artifacts".into());
    let refs = append_replayable_run(&store, &run);
    let engine = DeterministicReplayEngine::new(store);
    let bundle = engine.build(replay_request(run, refs.clone())).unwrap();
    let portable = engine.portable(&bundle.bundle).unwrap();
    let mut artifact_comparison = comparison();
    artifact_comparison.bundle = bundle.bundle.clone();
    let evaluation = DeterministicEvolutionEvaluator
        .compare(artifact_comparison)
        .unwrap();
    let candidate = p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId("candidate:artifact:v2".into()),
        domain: p::StrategyDomain::Loop,
        scope: p::Scope("workspace".into()),
        target_tier: p::StabilityTier::Stable,
        proposed_version: p::StrategyVersionRef("strategy:v2".into()),
        baseline: p::StrategyVersionRef("strategy:v1".into()),
        spec_ref: p::ContentRef("content:strategy:v2".into()),
        spec_digest: p::SchemaDigest("digest:strategy:v2".into()),
        evidence: vec![p::EvidenceRef("ground-truth:s55".into())],
        provenance: provenance(),
        impact: p::EvolutionImpact::Bounded,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef("strategy:v1".into()),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    };
    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace".into());
    let activation = p::StrategyActivation {
        schema_version: p::SchemaVersion(1),
        aggregate: aggregate.clone(),
        domain: p::StrategyDomain::Loop,
        scope: p::Scope("workspace".into()),
        from: Some(p::StrategyVersionRef("strategy:v1".into())),
        to: p::StrategyVersionRef("strategy:v2".into()),
        spec_ref: candidate.spec_ref.clone(),
        spec_digest: candidate.spec_digest.clone(),
        evaluation: evaluation.evaluation.clone(),
        promotion: p::EventId("event:promotion:v2".into()),
        owner_confirmation: Some(p::OwnerControlRef("owner-control:v2".into())),
        impact: p::EvolutionImpact::Bounded,
        expected_version: p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: aggregate.clone(),
            value: 1,
        },
        committed_version: p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: aggregate.clone(),
            value: 2,
        },
    };
    let rollback = p::StrategyRollback {
        schema_version: p::SchemaVersion(1),
        aggregate: aggregate.clone(),
        domain: p::StrategyDomain::Loop,
        scope: p::Scope("workspace".into()),
        failed: p::StrategyVersionRef("strategy:v2".into()),
        restored: p::StrategyVersionRef("strategy:v1".into()),
        restored_spec_ref: p::ContentRef("content:strategy:v1".into()),
        restored_spec_digest: p::SchemaDigest("digest:strategy:v1".into()),
        triggers: vec![p::EvidenceRef("evidence:regression:v2".into())],
        expected_version: p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate: aggregate.clone(),
            value: 2,
        },
        committed_version: p::EvolutionAggregateVersion {
            schema_version: p::SchemaVersion(1),
            aggregate,
            value: 3,
        },
        in_flight: p::InFlightDisposition::KeepPinned,
        external_effects_reverted: p::HistoricalFalse,
    };
    let trace = TraceManifest {
        schema_version: p::SchemaVersion(1),
        events: refs,
        checksums: portable
            .events
            .iter()
            .map(|record| record.digest.clone())
            .collect(),
        evaluation: evaluation.evaluation.clone(),
        active_snapshot: p::EvolutionSnapshotRef("snapshot:active:v2".into()),
    };

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("forme-m3-artifacts-{}-{nonce}", std::process::id()));
    let writer = M3ArtifactStore::open(&root).unwrap();
    let receipts = vec![
        writer.write_replay(&portable).unwrap(),
        writer.write_evaluation(&evaluation).unwrap(),
        writer
            .write_activation(&candidate, &evaluation, &activation)
            .unwrap(),
        writer.write_rollback(&rollback).unwrap(),
        writer.write_trace(&trace).unwrap(),
    ];
    let verifier = M3ArtifactStore::open(&root).unwrap();
    for receipt in &receipts {
        assert_eq!(verifier.verify(&receipt.path).unwrap(), *receipt);
        let content = std::fs::read_to_string(&receipt.path).unwrap();
        assert!(!content.contains("secret:"));
        assert!(!content.contains("C:\\Users\\"));
    }
    assert_eq!(verifier.verify_complete_set().unwrap().len(), 5);
    let line_ending_variant = receipts[0].path.clone();
    let canonical = std::fs::read_to_string(&line_ending_variant).unwrap();
    std::fs::write(&line_ending_variant, canonical.replace('\n', "\r\n")).unwrap();
    assert_eq!(
        verifier.verify(&line_ending_variant).unwrap().digest,
        receipts[0].digest
    );
    assert_eq!(verifier.verify_complete_set().unwrap().len(), 5);
    let unexpected = root.join("unexpected.txt");
    std::fs::write(&unexpected, b"not part of the typed artifact set").unwrap();
    assert!(verifier.verify_complete_set().is_err());
    std::fs::remove_file(unexpected).unwrap();

    let forbidden = TraceManifest {
        events: vec![p::EventId("secret:provider-key".into())],
        checksums: vec![p::SchemaDigest("digest:safe".into())],
        ..trace.clone()
    };
    assert!(writer.write_trace(&forbidden).is_err());

    let tampered = receipts[0].path.clone();
    std::fs::write(&tampered, b"{}").unwrap();
    assert!(verifier.verify(&tampered).is_err());
    let outside = std::env::temp_dir().join(format!("forme-m3-outside-{nonce}.json"));
    std::fs::write(&outside, b"{}").unwrap();
    assert!(verifier.verify(&outside).is_err());
    let _ = std::fs::remove_file(outside);
    let _ = std::fs::remove_dir_all(root);
}
