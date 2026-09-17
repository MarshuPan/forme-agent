use forme_loop::{apply_loop_strategy, Budget, InMemoryLoopRegistry, LoopRegistry, RunCtx};
use forme_protocol as p;

fn spec(version: &str, turns: u32, tokens: u64) -> p::LoopStrategySpec {
    p::LoopStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef(version.into()),
        scope: p::Scope("workspace:m3-b".into()),
        content_ref: p::ContentRef(format!("spec:{version}")),
        content_digest: p::SchemaDigest(format!("digest:{version}")),
        compatibility: p::StrategyRuntimeCompatibility {
            schema_version: p::SchemaVersion(1),
            minimum_runtime_schema: p::SchemaVersion(1),
            event_schema: p::SchemaVersion(1),
            model_profile: None,
            tool_schema: None,
            backend_schema: None,
        },
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
        triggers: vec![p::LoopTrigger::Reactive],
        checkpoint_cadence_turns: 1,
        verification_cadence_turns: 1,
        budget: p::LoopBudgetProfile {
            schema_version: p::SchemaVersion(1),
            max_turns: turns,
            max_tokens: tokens,
            max_wall_time_ms: 10_000,
            max_cost_microunits: 10_000,
            max_tool_calls: 2,
        },
        failure_fallback: p::LoopFailureFallback::Stop,
    }
}

fn hard_budget() -> Budget {
    Budget {
        schema_version: p::SchemaVersion(1),
        tokens: Some(10_000),
        wall_time: Some(p::DurationMs(20_000)),
        cost_microunits: Some(20_000),
        tool_calls: Some(4),
        max_turns: 10,
    }
}

fn context(run: &p::RunId) -> forme_context::RunCtx {
    let scope = p::Scope("workspace:m3-b".into());
    forme_context::RunCtx {
        schema_version: p::SchemaVersion(1),
        run: run.clone(),
        session: p::SessionId("session:m3-b".into()),
        scope: scope.clone(),
        selected_skills: Vec::new(),
        brain_call: false,
        sources: forme_context::ContextSources::empty(
            scope,
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        ),
    }
}

#[test]
fn s58_loop_registry_is_immutable_seeded_and_only_narrows_runtime_budget() {
    let v1 = spec("loop:m3-b:v1", 8, 8_000);
    let v2 = spec("loop:m3-b:v2", 6, 6_000);
    let registry = InMemoryLoopRegistry::with_seed(v1.clone()).unwrap();
    registry.register(v2.clone()).unwrap();
    assert_eq!(registry.seed_version(), &v1.version);
    assert_eq!(registry.resolve(&v1.version).unwrap(), v1);
    assert_eq!(registry.resolve(&v2.version).unwrap(), v2);

    let narrowed = apply_loop_strategy(&v2, &hard_budget()).unwrap();
    assert_eq!(narrowed.max_turns, 6);
    assert_eq!(narrowed.tokens, Some(6_000));
    let mut wider = v2.clone();
    wider.budget.max_turns = 11;
    assert!(apply_loop_strategy(&wider, &hard_budget()).is_err());

    let mut rebound = v2.clone();
    rebound.content_digest = p::SchemaDigest("digest:mutated".into());
    assert!(registry.register(rebound).is_err());
}

#[test]
fn s58_run_binds_one_loop_version_and_cannot_hot_swap() {
    let run = p::RunId("run:m3-b-loop-pin".into());
    let mut ctx = RunCtx::new(
        p::SessionId("session:m3-b-loop-pin".into()),
        p::RunInput("bounded task".into()),
        context(&run),
        forme_context::ContextBudget {
            schema_version: p::SchemaVersion(1),
            max_tokens: 10_000,
            reserve: 500,
        },
        hard_budget(),
    )
    .unwrap();
    let v1 = spec("loop:m3-b:v1", 8, 8_000);
    let v2 = spec("loop:m3-b:v2", 6, 6_000);
    ctx.bind_strategy(v1.clone()).unwrap();
    assert_eq!(ctx.bound_strategy().unwrap().version, v1.version);
    assert!(ctx.bind_strategy(v2).is_err());
    assert_eq!(ctx.bound_strategy().unwrap().version, v1.version);
}
