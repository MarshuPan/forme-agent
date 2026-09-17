use forme_protocol as p;
use serde::de::DeserializeOwned;

fn text<T: DeserializeOwned>(value: &str) -> T {
    serde_json::from_value(serde_json::Value::String(value.to_owned())).unwrap()
}

fn version(value: u64) -> p::EvolutionAggregateVersion {
    p::EvolutionAggregateVersion {
        schema_version: p::SchemaVersion(1),
        aggregate: text("evolution:owner:workspace"),
        value,
    }
}

fn provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn snapshot() -> p::EvolutionSnapshot {
    p::EvolutionSnapshot {
        schema_version: p::SchemaVersion(1),
        snapshot: text("evolution-snapshot:1"),
        aggregates: vec![version(1)],
        strategies: vec![p::ActiveStrategyRef {
            schema_version: p::SchemaVersion(1),
            id: text("active:loop:workspace"),
            aggregate: text("evolution:owner:workspace"),
            domain: p::StrategyDomain::Loop,
            scope: text("workspace"),
            version: text("loop:v1"),
            spec_ref: text("content:loop:v1"),
            spec_digest: text("digest:loop:v1"),
            activation_event: text("event:activate:v1"),
        }],
        digest: text("digest:evolution-snapshot:1"),
    }
}

fn replay_snapshot() -> p::ReplaySnapshot {
    p::ReplaySnapshot {
        schema_version: p::SchemaVersion(1),
        event_schema: p::SchemaVersion(1),
        policy: text("policy:1"),
        loop_spec: text("loop:v1"),
        model: text("model:deterministic"),
        tool_schema: text("digest:tools:1"),
        driver_profiles: vec![text("driver:recorded:1")],
        evolution: snapshot(),
        migration_graph_digest: text("digest:migrations:1"),
    }
}

fn metric() -> p::FitnessMetric {
    p::FitnessMetric {
        schema_version: p::SchemaVersion(1),
        dimension: p::FitnessDimension::Quality,
        outcome: p::FitnessOutcome::Pass,
        measured: Some(10_000),
        unit: p::FitnessUnit::BasisPoints,
        evidence: vec![text("evidence:ground-truth:1")],
    }
}

fn invariant() -> p::InvariantResult {
    p::InvariantResult {
        schema_version: p::SchemaVersion(1),
        reference: text("invariant:harness-first"),
        name: "harness_first".into(),
        outcome: p::FitnessOutcome::Pass,
        evidence: vec![text("evidence:invariant:1")],
    }
}

fn activation(impact: p::EvolutionImpact) -> p::StrategyActivation {
    p::StrategyActivation {
        schema_version: p::SchemaVersion(1),
        aggregate: text("evolution:owner:workspace"),
        domain: p::StrategyDomain::Loop,
        scope: text("workspace"),
        from: Some(text("loop:v1")),
        to: text("loop:v2"),
        spec_ref: text("content:loop:v2"),
        spec_digest: text("digest:loop:v2"),
        evaluation: text("evaluation:loop:v2"),
        promotion: text("event:promote:v2"),
        owner_confirmation: None,
        impact,
        expected_version: version(1),
        committed_version: version(2),
    }
}

fn rollback() -> p::StrategyRollback {
    p::StrategyRollback {
        schema_version: p::SchemaVersion(1),
        aggregate: text("evolution:owner:workspace"),
        domain: p::StrategyDomain::Loop,
        scope: text("workspace"),
        failed: text("loop:v2"),
        restored: text("loop:v1"),
        restored_spec_ref: text("content:loop:v1"),
        restored_spec_digest: text("digest:loop:v1"),
        triggers: vec![text("evidence:regression:1")],
        expected_version: version(2),
        committed_version: version(3),
        in_flight: p::InFlightDisposition::KeepPinned,
        external_effects_reverted: p::HistoricalFalse,
    }
}

#[test]
fn m2_taxonomy_is_a_strict_prefix_of_m3() {
    assert_eq!(p::EventKind::ALL.len(), 99);
    assert_eq!(p::EventKind::ALL[85], p::EventKind::ComplianceCheckResult);
    assert_eq!(
        &p::EventKind::ALL[86..89],
        &[
            p::EventKind::EvolutionEvaluationRecorded,
            p::EventKind::StrategyActivated,
            p::EventKind::StrategyRolledBack,
        ]
    );
}

#[test]
fn m3_payloads_round_trip_and_keep_kind_identity() {
    let payloads = [
        p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
            evaluation: text("evaluation:loop:v2"),
            baseline: text("loop:v1"),
            candidate: text("loop:v2"),
            verdict: p::EvaluationVerdict::Pass,
            hard_invariants: vec![text("invariant:harness-first")],
            ground_truth: vec![text("evidence:ground-truth:1")],
        }),
        p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
            activation: activation(p::EvolutionImpact::Cautious),
            active_snapshot: text("evolution-snapshot:2"),
        }),
        p::EventPayload::StrategyRolledBack(p::StrategyRolledBackPayload {
            rollback: rollback(),
            active_snapshot: text("evolution-snapshot:3"),
        }),
    ];

    for (payload, expected_kind) in payloads.into_iter().zip(p::EventKind::ALL[86..89].iter()) {
        let encoded = serde_json::to_vec(&payload).unwrap();
        let decoded: p::EventPayload = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(&decoded, &payload);
        assert_eq!(&decoded.kind(), expected_kind);
    }

    let event = p::Event::new(
        text("event:mismatch"),
        text("run:control"),
        None,
        payloads_for_mismatch(),
        p::SchemaVersion(1),
        1,
        provenance(),
    );
    let mut encoded = serde_json::to_value(event).unwrap();
    encoded["kind"] = serde_json::json!("CandidatePromoted");
    let decoded: p::Event = serde_json::from_value(encoded).unwrap();
    assert!(decoded.validate_payload_kind().is_err());
}

fn payloads_for_mismatch() -> p::EventPayload {
    p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
        activation: activation(p::EvolutionImpact::Cautious),
        active_snapshot: text("evolution-snapshot:2"),
    })
}

#[test]
fn legacy_optional_fields_decode_as_absent() {
    let session: p::SessionBoundPayload = serde_json::from_value(serde_json::json!({
        "policy_profile": "policy:legacy",
        "model_profile": "model:legacy",
        "toolset_ref": "toolset:legacy",
        "workspace": "workspace:legacy"
    }))
    .unwrap();
    assert_eq!(session.effect_mode, None);
    assert_eq!(session.evolution_snapshot, None);

    let candidate: p::CandidateCreatedPayload = serde_json::from_value(serde_json::json!({
        "candidate_id": "candidate:legacy",
        "target": "target:legacy",
        "evidence_refs": [],
        "confidence": 0.5,
        "provenance": {
            "source": "Internal",
            "actor": "System",
            "trust_tier": "VerifiedProcess",
            "caused_by": null
        },
        "target_tier": "Working"
    }))
    .unwrap();
    assert_eq!(candidate.strategy_candidate, None);

    let trace: p::DecisionTraceRecordedPayload = serde_json::from_value(serde_json::json!({
        "trace_ref": "trace:legacy",
        "refs": {"map": null, "user": null, "agent_self": null, "trust": null, "failure": []},
        "rationale": "legacy",
        "workspace_snapshot": "workspace-snapshot:legacy"
    }))
    .unwrap();
    assert_eq!(trace.evolution_snapshot, None);
}

#[test]
fn replay_and_evaluation_contracts_fail_closed() {
    let request = p::ReplayRequest {
        schema_version: p::SchemaVersion(1),
        bundle: text("bundle:1"),
        source_runs: vec![text("run:source:1")],
        event_refs: vec![text("event:source:1")],
        cases: vec![text("case:1")],
        snapshot: replay_snapshot(),
        effect_mode: p::EffectMode::ExactReplay,
    };
    request.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<p::ReplayRequest>(serde_json::to_value(&request).unwrap())
            .unwrap(),
        request
    );

    let mut live = request.clone();
    live.effect_mode = p::EffectMode::LiveGoverned;
    assert!(live.validate().is_err());
    let mut zero = request.clone();
    zero.schema_version = p::SchemaVersion(0);
    assert!(zero.validate().is_err());
    assert!(serde_json::from_value::<p::FitnessUnit>(serde_json::json!("Points")).is_err());

    let comparison = p::EvolutionComparison {
        schema_version: p::SchemaVersion(1),
        evaluation: text("evaluation:1"),
        bundle: text("bundle:1"),
        baseline: text("loop:v1"),
        candidate: text("loop:v2"),
        case_set_digest: text("digest:train"),
        holdout_digest: text("digest:holdout"),
        metrics: vec![metric()],
        hard_invariants: vec![invariant()],
        ground_truth: vec![text("evidence:ground-truth:1")],
        independent_verifier: true,
        self_eval_only: false,
    };
    comparison.validate().unwrap();

    let mut leaked = comparison.clone();
    leaked.holdout_digest = leaked.case_set_digest.clone();
    assert!(leaked.validate().is_err());
    let mut negative = metric();
    negative.measured = Some(-1);
    assert!(negative.validate().is_err());
}

#[test]
fn activation_and_rollback_enforce_version_owner_and_historical_boundaries() {
    activation(p::EvolutionImpact::Cautious).validate().unwrap();

    let mut bounded = activation(p::EvolutionImpact::Bounded);
    assert!(bounded.validate().is_err());
    bounded.owner_confirmation = Some(text("owner-control:1"));
    bounded.validate().unwrap();

    let mut constitutional = activation(p::EvolutionImpact::Constitutional);
    constitutional.owner_confirmation = Some(text("owner-control:1"));
    assert!(constitutional.validate().is_err());

    let mut skipped = activation(p::EvolutionImpact::Cautious);
    skipped.committed_version.value = skipped.expected_version.value + 2;
    assert!(skipped.validate().is_err());

    let mut cross_aggregate = rollback();
    cross_aggregate.committed_version.aggregate = text("evolution:other");
    assert!(cross_aggregate.validate().is_err());
    rollback().validate().unwrap();

    assert!(serde_json::from_value::<p::HistoricalFalse>(serde_json::json!(true)).is_err());
    assert_eq!(
        serde_json::to_value(p::HistoricalFalse).unwrap(),
        serde_json::json!(false)
    );
    let exhausted = p::EvolutionAggregateVersion {
        value: u64::MAX,
        ..version(0)
    };
    assert!(exhausted.next().is_err());
}
