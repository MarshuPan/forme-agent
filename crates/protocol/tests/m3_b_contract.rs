use forme_protocol as p;
use serde_json::json;

fn compatibility() -> p::StrategyRuntimeCompatibility {
    p::StrategyRuntimeCompatibility {
        schema_version: p::SchemaVersion(1),
        minimum_runtime_schema: p::SchemaVersion(1),
        event_schema: p::SchemaVersion(1),
        model_profile: None,
        tool_schema: Some(p::SchemaDigest("tool-schema:v1".into())),
        backend_schema: Some(p::SchemaDigest("backend-schema:v1".into())),
    }
}

fn loop_spec() -> p::LoopStrategySpec {
    p::LoopStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("loop:m3-b:v1".into()),
        scope: p::Scope("workspace:m3-b".into()),
        content_ref: p::ContentRef("spec:loop:m3-b:v1".into()),
        content_digest: p::SchemaDigest("digest:loop:m3-b:v1".into()),
        compatibility: compatibility(),
        phases: vec![
            p::LoopPhase::Context,
            p::LoopPhase::Deliberate,
            p::LoopPhase::Policy,
            p::LoopPhase::Approval,
            p::LoopPhase::Execute,
            p::LoopPhase::Checkpoint,
            p::LoopPhase::Verify,
            p::LoopPhase::Finish,
        ],
        triggers: vec![p::LoopTrigger::Reactive, p::LoopTrigger::CheckpointResume],
        checkpoint_cadence_turns: 2,
        verification_cadence_turns: 1,
        budget: p::LoopBudgetProfile {
            schema_version: p::SchemaVersion(1),
            max_turns: 8,
            max_tokens: 8_000,
            max_wall_time_ms: 60_000,
            max_cost_microunits: 50_000,
            max_tool_calls: 4,
        },
        failure_fallback: p::LoopFailureFallback::PreserveCheckpoint,
    }
}

fn coordination_spec() -> p::CoordinationStrategySpec {
    p::CoordinationStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("coordination:m3-b:v1".into()),
        scope: p::Scope("workspace:m3-b".into()),
        content_ref: p::ContentRef("spec:coordination:m3-b:v1".into()),
        content_digest: p::SchemaDigest("digest:coordination:m3-b:v1".into()),
        compatibility: compatibility(),
        applicability: p::CoordinationApplicabilitySpec {
            schema_version: p::SchemaVersion(1),
            scale: p::CoordinationTaskScale::MultiStage,
            decomposability: p::CoordinationDecomposability::Sequential,
            verifiability: p::CoordinationVerifiability::IndependentReview,
        },
        patterns: vec![p::CoordinationPatternSpec {
            schema_version: p::SchemaVersion(1),
            pattern: p::OrchestrationPatternRef("pattern:review".into()),
            topology: p::CoordinationTopology::ReviewGate,
        }],
        mode: p::CoordinationMode::Multi,
        role_weights: vec![
            p::CoordinationRoleWeight {
                schema_version: p::SchemaVersion(1),
                role: p::RoleRef("role:worker".into()),
                weight_basis_points: 6_000,
            },
            p::CoordinationRoleWeight {
                schema_version: p::SchemaVersion(1),
                role: p::RoleRef("role:reviewer".into()),
                weight_basis_points: 4_000,
            },
        ],
        max_subagents: 2,
        checkpoint_topology: p::CheckpointTopology::ReviewGate,
    }
}

fn selection_spec() -> p::SelectionStrategySpec {
    p::SelectionStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("selection:m3-b:v1".into()),
        scope: p::Scope("workspace:m3-b".into()),
        content_ref: p::ContentRef("spec:selection:m3-b:v1".into()),
        content_digest: p::SchemaDigest("digest:selection:m3-b:v1".into()),
        compatibility: compatibility(),
        target: p::SelectionTarget::Capability,
        weights: vec![
            p::SelectionFeatureWeight {
                schema_version: p::SchemaVersion(1),
                feature: p::SelectionFeature::VerifiedSuccess,
                weight_basis_points: 5_000,
            },
            p::SelectionFeatureWeight {
                schema_version: p::SchemaVersion(1),
                feature: p::SelectionFeature::FailurePenalty,
                weight_basis_points: 3_000,
            },
            p::SelectionFeatureWeight {
                schema_version: p::SchemaVersion(1),
                feature: p::SelectionFeature::Compatibility,
                weight_basis_points: 2_000,
            },
        ],
        tie_breaker: p::SelectionTieBreaker::StableIdentity,
        fallback: p::SelectionFallback::NoSelection,
        max_results: 4,
    }
}

fn model_spec() -> p::ModelAdaptationSpec {
    p::ModelAdaptationSpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("model-adaptation:m3-b:v1".into()),
        scope: p::Scope("workspace:m3-b".into()),
        content_ref: p::ContentRef("spec:model-adaptation:m3-b:v1".into()),
        content_digest: p::SchemaDigest("digest:model-adaptation:m3-b:v1".into()),
        compatibility: compatibility(),
        predicate: p::ModelCapabilityPredicate {
            schema_version: p::SchemaVersion(1),
            minimum_context_window: 8_192,
            requires_tool_use: true,
            minimum_strength: p::ModelStrengthBand::Standard,
        },
        scaffold: p::ModelScaffoldProfile {
            schema_version: p::SchemaVersion(1),
            externalized_steps: 3,
            verification_passes: 2,
            checkpoint_cadence_steps: 1,
        },
    }
}

#[test]
fn m3_b_specs_are_versioned_strict_and_round_trip_without_new_events() {
    assert_eq!(p::EventKind::ALL.len(), 99);
    for value in [
        serde_json::to_value(loop_spec()).unwrap(),
        serde_json::to_value(coordination_spec()).unwrap(),
        serde_json::to_value(selection_spec()).unwrap(),
        serde_json::to_value(model_spec()).unwrap(),
    ] {
        assert_eq!(value["schema_version"], json!(1));
    }
    let loop_round_trip: p::LoopStrategySpec =
        serde_json::from_value(serde_json::to_value(loop_spec()).unwrap()).unwrap();
    let coordination_round_trip: p::CoordinationStrategySpec =
        serde_json::from_value(serde_json::to_value(coordination_spec()).unwrap()).unwrap();
    let selection_round_trip: p::SelectionStrategySpec =
        serde_json::from_value(serde_json::to_value(selection_spec()).unwrap()).unwrap();
    let model_round_trip: p::ModelAdaptationSpec =
        serde_json::from_value(serde_json::to_value(model_spec()).unwrap()).unwrap();
    assert_eq!(loop_round_trip, loop_spec());
    assert_eq!(coordination_round_trip, coordination_spec());
    assert_eq!(selection_round_trip, selection_spec());
    assert_eq!(model_round_trip, model_spec());
    loop_round_trip.validate().unwrap();
    coordination_round_trip.validate().unwrap();
    selection_round_trip.validate().unwrap();
    model_round_trip.validate().unwrap();
}

#[test]
fn m3_b_forbidden_governance_fields_and_unknown_extensions_fail_closed() {
    let fixtures = [
        (
            serde_json::to_value(loop_spec()).unwrap(),
            "approval_disabled",
        ),
        (
            serde_json::to_value(coordination_spec()).unwrap(),
            "permission",
        ),
        (
            serde_json::to_value(selection_spec()).unwrap(),
            "autonomy_envelope",
        ),
        (
            serde_json::to_value(model_spec()).unwrap(),
            "high_impact_trace_disabled",
        ),
    ];
    for (mut value, field) in fixtures {
        value
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!(true));
        assert!(serde_json::from_value::<p::LoopStrategySpec>(value.clone()).is_err());
        assert!(serde_json::from_value::<p::CoordinationStrategySpec>(value.clone()).is_err());
        assert!(serde_json::from_value::<p::SelectionStrategySpec>(value.clone()).is_err());
        assert!(serde_json::from_value::<p::ModelAdaptationSpec>(value).is_err());
    }
}

#[test]
fn m3_b_zero_duplicate_overflow_and_governance_removal_are_rejected() {
    let mut loop_zero = loop_spec();
    loop_zero.budget.max_turns = 0;
    assert!(loop_zero.validate().is_err());
    let mut loop_without_approval = loop_spec();
    loop_without_approval
        .phases
        .retain(|phase| *phase != p::LoopPhase::Approval);
    assert!(loop_without_approval.validate().is_err());
    let mut coordination_duplicate = coordination_spec();
    coordination_duplicate
        .role_weights
        .push(coordination_duplicate.role_weights[0].clone());
    assert!(coordination_duplicate.validate().is_err());
    let mut coordination_unbounded = coordination_spec();
    coordination_unbounded.max_subagents = u16::MAX;
    assert!(coordination_unbounded.validate().is_err());
    let mut selection_overflow = selection_spec();
    selection_overflow.weights[0].weight_basis_points = 10_000;
    assert!(selection_overflow.validate().is_err());
    let mut model_unbounded = model_spec();
    model_unbounded.scaffold.externalized_steps = u16::MAX;
    assert!(model_unbounded.validate().is_err());
}
