#[path = "support/brain_evolution.rs"]
mod evolution;
#[path = "support/core_brain.rs"]
mod support;
use forme_cognition as c;
use forme_context::{ContextBudget, ContextHydrationRequest, ContextHydrator};
use forme_harness::{AgentHarness, GatewayControl};
use forme_protocol as p;
use forme_store::EventStore;
use support::*;

fn loop_spec(
    workspace: &p::WorkspaceRef,
    version: &str,
    turns: u32,
) -> forme_harness::ProductionStrategySpec {
    forme_harness::ProductionStrategySpec::Loop(p::LoopStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef(version.into()),
        scope: p::Scope(workspace.0.clone()),
        content_ref: p::ContentRef("unsealed".into()),
        content_digest: p::SchemaDigest("unsealed".into()),
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
            p::LoopPhase::Verify,
            p::LoopPhase::Checkpoint,
            p::LoopPhase::Finish,
        ],
        triggers: vec![p::LoopTrigger::Reactive],
        checkpoint_cadence_turns: 1,
        verification_cadence_turns: 1,
        budget: p::LoopBudgetProfile {
            schema_version: p::SchemaVersion(1),
            max_turns: turns,
            max_tokens: 4096,
            max_wall_time_ms: 10_000,
            max_cost_microunits: 10_000,
            max_tool_calls: 1,
        },
        failure_fallback: p::LoopFailureFallback::Stop,
    })
}

#[test]
fn production_m3_resolves_immutable_bodies_binds_activated_versions_and_recovers() {
    let f = Fixture::new();
    let runtime = f.build();
    let h = runtime.harness();
    let seed_run = h.submit_run(request("no-active-policy")).unwrap();
    assert_eq!(h.wait(seed_run).unwrap().status, p::RunStatus::Complete);
    drop(h);
    let seed = runtime
        .initialize_strategy_seed(&f.owner(), p::StrategyDomain::Loop, &f.config.workspace)
        .unwrap();
    for domain in [
        p::StrategyDomain::Coordination,
        p::StrategyDomain::CapabilitySelection,
        p::StrategyDomain::ModelSelection,
        p::StrategyDomain::BackendSelection,
        p::StrategyDomain::ModelAdaptation,
    ] {
        let seeded = runtime
            .initialize_strategy_seed(&f.owner(), domain, &f.config.workspace)
            .unwrap();
        let before = events(&runtime).len();
        assert_eq!(
            seeded,
            runtime
                .initialize_strategy_seed(&f.owner(), domain, &f.config.workspace)
                .unwrap()
        );
        assert_eq!(before, events(&runtime).len());
    }
    assert!(runtime
        .initialize_strategy_seed(
            &p::VerifiedPrincipal("other".into()),
            p::StrategyDomain::Loop,
            &f.config.workspace
        )
        .is_err());
    // Initial baselines describe built-in behavior; they do not require
    // fabricated model outcome evidence or change the normal execution path.
    let baseline_run = runtime
        .harness()
        .submit_run(request("seed-baselines"))
        .unwrap();
    assert_eq!(
        runtime.harness().wait(baseline_run).unwrap().status,
        p::RunStatus::Complete
    );
    let body = runtime
        .register_strategy_spec(
            &f.owner(),
            loop_spec(&f.config.workspace, "loop:owned-v2", 1),
        )
        .unwrap();
    assert_eq!(
        body,
        runtime
            .register_strategy_spec(&f.owner(), body.clone())
            .unwrap()
    );
    assert!(runtime
        .register_strategy_spec(
            &f.owner(),
            loop_spec(&f.config.workspace, "loop:owned-v2", 2)
        )
        .is_err());
    assert!(runtime
        .register_strategy_spec(&p::VerifiedPrincipal("outsider".into()), body.clone())
        .is_err());
    let c = evolution::candidate(
        &body,
        p::StrategyDomain::Loop,
        seed.version().clone(),
        vec![p::EvidenceRef("owned-independent-result".into())],
    );
    let mut wrong_domain = c.clone();
    wrong_domain.domain = p::StrategyDomain::Coordination;
    let before_rejection = events(&runtime).len();
    assert!(
        forme_harness::EvolutionGatewayControl::record_strategy_candidate(
            runtime.harness().as_ref(),
            p::RunId("wrong-domain".into()),
            wrong_domain,
        )
        .is_err()
    );
    assert_eq!(before_rejection, events(&runtime).len());
    evolution::activate(&runtime, &c, false);
    let run = runtime
        .harness()
        .submit_run(request("activated-v2"))
        .unwrap();
    assert_eq!(
        runtime.harness().wait(run.clone()).unwrap().status,
        p::RunStatus::Complete
    );
    assert!(events(&runtime).iter().any(|e| e.run_id == run
        && matches!(&e.payload,
        p::EventPayload::DecisionTraceRecorded(p) if p.rationale.0.contains("loop:owned-v2"))));
    let digest = body.digest().clone();
    drop(runtime);
    let runtime = f.build();
    let run = runtime
        .harness()
        .submit_run(request("reopened-active-v2"))
        .unwrap();
    assert_eq!(
        runtime.harness().wait(run.clone()).unwrap().status,
        p::RunStatus::Complete
    );
    assert_eq!(
        runtime
            .register_strategy_spec(&f.owner(), body.clone())
            .unwrap()
            .digest(),
        &digest
    );
    assert!(events(&runtime).iter().any(|e| e.run_id == run
        && matches!(&e.payload,
        p::EventPayload::SessionBound(p) if p.evolution_snapshot.is_some())));
    use forme_harness::EvolutionGatewayControl;
    runtime
        .harness()
        .rollback_strategy(
            p::RunId("owner:rollback-seed".into()),
            p::EvolutionAggregateRef(format!("evolution:Loop:{}", f.config.workspace.0)),
            p::StrategyDomain::Loop,
            p::Scope(f.config.workspace.0.clone()),
            seed.version().clone(),
            vec![p::EvidenceRef("owner:rollback-request".into())],
            p::InFlightDisposition::KeepPinned,
            Some(p::OwnerControlRef("owner:reviewed-rollback".into())),
        )
        .unwrap();
    let run = runtime
        .harness()
        .submit_run(request("rolled-back-to-compiled-seed"))
        .unwrap();
    assert_eq!(
        runtime.harness().wait(run).unwrap().status,
        p::RunStatus::Complete
    );
}

fn request(key: &str) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef(format!("session:{key}")),
        agent_profile: p::AgentProfileRef("agent:core-b".into()),
        input: p::RunInput("Prepare a scoped project report".into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey(key.into())),
    }
}

fn charter(runtime: &forme_harness::ProductionRuntime, f: &Fixture, workspace: &p::WorkspaceRef) {
    let change = runtime
        .prepare_workspace_charter(
            &f.owner(),
            p::WorkspaceCharterRecord {
                schema_version: p::SchemaVersion(1),
                workspace: workspace.clone(),
                version: 1,
                goals: vec![],
                constraints: vec![p::Constraint("max-nodes:8".into())],
                prohibitions: vec![],
                done_contract: None,
                review_cadence: Some(p::DurationMs(60_000)),
                actor: p::Actor::Owner,
                digest: p::SchemaDigest(String::new()),
            },
            0,
        )
        .unwrap();
    runtime
        .apply_workspace_charter(
            &f.owner(),
            p::IdempotencyKey(format!("charter:{}", workspace.0)),
            change,
        )
        .unwrap();
}

fn project(workspace: &p::WorkspaceRef) -> p::WorkspaceProject {
    p::WorkspaceProject {
        schema_version: p::SchemaVersion(1),
        reference: format!("project:{}", workspace.0),
        workspace: workspace.clone(),
        goal: p::GoalRef(format!("goal:{}", workspace.0)),
        done: p::DoneContractRef(format!("done:{}", workspace.0)),
        summary: "Return ground-truth".into(),
        signature: p::TaskSignature {
            schema_version: p::SchemaVersion(1),
            partition: p::TaskPartition::Whole,
            dependency: p::TaskDependency::Independent,
            verification: p::TaskVerification::SelfCheck,
            clarity: p::TaskClarity::Clear,
            extent: p::TaskExtent::Single,
            reversibility: p::TaskReversibility::LowRiskReversible,
            anchors: vec![],
        },
        requirements: vec![p::RouteRequirement::OutputContains("ground-truth".into())],
        scope: p::Scope(workspace.0.clone()),
        capabilities: vec![],
        model: p::ModelProfileRef("model:core-b".into()),
        budget_units: 8,
        node_units: 2,
        cadence_ms: 60_000,
        deadline: None,
    }
}

#[test]
fn one_owner_brain_runs_distinct_workspace_projects_and_replays_both() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let runtime = f.build();
    let a = p::WorkspaceRef("workspace:independent-a".into());
    let b = p::WorkspaceRef("workspace:independent-b".into());
    assert!(runtime
        .submit_workspace_run(&f.owner(), &a, request("not-registered"))
        .is_err());
    for workspace in [&a, &b] {
        charter(&runtime, &f, workspace);
        let p = project(workspace);
        runtime
            .register_project(
                &f.owner(),
                p::IdempotencyKey(format!("register:{}", workspace.0)),
                p.clone(),
            )
            .unwrap();
        let run = runtime
            .submit_workspace_run(&f.owner(), workspace, request("same-command"))
            .unwrap();
        assert_eq!(
            runtime.harness().wait(run.clone()).unwrap().status,
            p::RunStatus::Complete
        );
        assert_eq!(
            runtime
                .submit_workspace_run(&f.owner(), workspace, request("same-command"))
                .unwrap(),
            run
        );
        assert!(events(&runtime).iter().any(|e| e.run_id == run
            && matches!(&e.payload,
            p::EventPayload::SessionBound(p) if p.workspace == *workspace)));
        let route = runtime
            .plan_project_route(
                &f.owner(),
                p::IdempotencyKey(format!("plan:{}", workspace.0)),
                &p.reference,
                p.signature,
                p::canonical_digest(&workspace.0).unwrap(),
                now(),
            )
            .unwrap();
        let result = runtime
            .execute_project_route(&f.owner(), &route.route)
            .unwrap();
        assert_eq!(result.status, p::WorkRouteStatus::Completed);
    }
    let a_request = runtime
        .submit_workspace_run(&f.owner(), &a, request("same-command"))
        .unwrap();
    let b_request = runtime
        .submit_workspace_run(&f.owner(), &b, request("same-command"))
        .unwrap();
    assert_ne!(a_request, b_request);
    let aggregate = runtime.services().aggregates.memory_cognition.clone();
    let mut parallel = project(&a);
    parallel.reference = "project:parallel-a".into();
    parallel.signature.partition = p::TaskPartition::Parallel;
    parallel.signature.verification = p::TaskVerification::Environment;
    parallel.signature.clarity = p::TaskClarity::Explore;
    parallel.signature.extent = p::TaskExtent::MultiStage;
    runtime
        .register_project(
            &f.owner(),
            p::IdempotencyKey("parallel-project".into()),
            parallel.clone(),
        )
        .unwrap();
    let route = runtime
        .plan_project_route(
            &f.owner(),
            p::IdempotencyKey("parallel-plan".into()),
            &parallel.reference,
            parallel.signature,
            p::SchemaDigest("parallel-input".into()),
            now(),
        )
        .unwrap();
    assert_eq!(route.nodes.len(), 3);
    assert_eq!(
        runtime
            .execute_project_route(&f.owner(), &route.route)
            .unwrap()
            .status,
        p::WorkRouteStatus::Completed
    );
    let ledger = runtime.orchestration_state(&f.owner()).unwrap();
    assert_eq!(ledger.projects.len(), 3);
    assert_eq!(ledger.routes.len(), 3);
    assert!(runtime
        .submit_workspace_run(
            &p::VerifiedPrincipal("someone-else".into()),
            &a,
            request("wrong-owner")
        )
        .is_err());
    drop(runtime);
    let runtime = f.build();
    assert_eq!(runtime.services().aggregates.memory_cognition, aggregate);
    assert_eq!(runtime.orchestration_state(&f.owner()).unwrap(), ledger);
    assert_eq!(
        runtime
            .submit_workspace_run(&f.owner(), &b, request("same-command"))
            .unwrap(),
        b_request
    );
}

fn route_candidate(
    runtime: &forme_harness::ProductionRuntime,
    route: &p::ExecutionRouteRef,
) -> p::StrategyCandidate {
    runtime
        .services()
        .store
        .strategy_candidate(&p::CandidateId(format!("route-strategy:{}", route.0)))
        .unwrap()
        .unwrap()
}

#[test]
fn route_history_changes_selection_only_after_governed_activation_and_replays() {
    let f = Fixture::new();
    let runtime = f.build();
    charter(&runtime, &f, &f.config.workspace);
    let mut spec = project(&f.config.workspace);
    spec.signature.verification = p::TaskVerification::Environment;
    runtime
        .register_project(
            &f.owner(),
            p::IdempotencyKey("fitness-project".into()),
            spec.clone(),
        )
        .unwrap();
    runtime
        .initialize_strategy_seed(
            &f.owner(),
            p::StrategyDomain::Coordination,
            &f.config.workspace,
        )
        .unwrap();
    let plan = |runtime: &forme_harness::ProductionRuntime, key: &str| {
        runtime
            .plan_project_route(
                &f.owner(),
                p::IdempotencyKey(key.into()),
                &spec.reference,
                spec.signature.clone(),
                p::SchemaDigest(key.into()),
                now(),
            )
            .unwrap_or_else(|e| panic!("{key}: {e:?}"))
    };
    let failed = plan(&runtime, "failed-route");
    assert_eq!(
        failed.pattern.as_ref().unwrap().0,
        "pattern:retrieve-then-answer"
    );
    assert_eq!(
        runtime
            .execute_project_route(&f.owner(), &failed.route)
            .unwrap()
            .status,
        p::WorkRouteStatus::Failed
    );
    *f.answer.lock().unwrap() = "ground-truth".into();
    let success = plan(&runtime, "unactivated-success");
    assert_eq!(
        success.scores, failed.scores,
        "unactivated failure cannot change ranking"
    );
    assert_eq!(
        runtime
            .execute_project_route(&f.owner(), &success.route)
            .unwrap()
            .status,
        p::WorkRouteStatus::Completed
    );
    let candidate = route_candidate(&runtime, &failed.route);
    evolution::activate(&runtime, &candidate, true);
    let changed = plan(&runtime, "activated-failure");
    assert_eq!(changed.pattern.as_ref().unwrap().0, "pattern:single-run");
    let score = |route: &p::RouteDeclaration, name: &str| {
        route
            .scores
            .iter()
            .find(|s| s.pattern.0 == name)
            .unwrap()
            .fitness
    };
    assert!(
        score(&changed, "pattern:retrieve-then-answer")
            < score(&failed, "pattern:retrieve-then-answer")
    );
    assert_eq!(
        runtime
            .execute_project_route(&f.owner(), &changed.route)
            .unwrap()
            .status,
        p::WorkRouteStatus::Completed
    );
    let unchanged = plan(&runtime, "new-unactivated-outcome");
    assert_eq!(unchanged.scores, changed.scores);
    runtime
        .execute_project_route(&f.owner(), &unchanged.route)
        .unwrap();
    // An independently reviewed successful episode can become the evidence of
    // a new policy. The comparison here is a controlled evaluator fixture.
    let success_candidate = route_candidate(&runtime, &success.route);
    let protection = runtime
        .services()
        .store
        .content_protection()
        .unwrap()
        .unwrap();
    let protected = protection
        .lifecycle_service((*runtime.services().store).clone())
        .protected_content(&f.owner(), &success_candidate.spec_ref)
        .unwrap()
        .unwrap();
    let bytes = protection
        .content_service()
        .decrypt(&protected.envelope.binding(), &protected.envelope)
        .unwrap();
    let mut body: forme_harness::ProductionStrategySpec = serde_json::from_slice(&bytes).unwrap();
    if let forme_harness::ProductionStrategySpec::Coordination(s) = &mut body {
        s.version = p::StrategyVersionRef("coordination:successful-evidence-v2".into());
    } else {
        panic!("route candidate has no executable coordination body");
    }
    let body = runtime.register_strategy_spec(&f.owner(), body).unwrap();
    let upgrade = evolution::candidate(
        &body,
        p::StrategyDomain::Coordination,
        candidate.proposed_version,
        success_candidate.evidence,
    );
    evolution::activate(&runtime, &upgrade, false);
    let improved = plan(&runtime, "activated-success");
    assert_eq!(
        improved.pattern.as_ref().unwrap().0,
        "pattern:retrieve-then-answer"
    );
    assert!(
        score(&improved, "pattern:retrieve-then-answer")
            > score(&failed, "pattern:retrieve-then-answer")
    );
    runtime
        .execute_project_route(&f.owner(), &improved.route)
        .unwrap();
    // Neither unrelated workspace outcomes nor a different signature may
    // inherit the activated score adjustment.
    let other = p::WorkspaceRef("workspace:other-fitness".into());
    charter(&runtime, &f, &other);
    let mut foreign = project(&other);
    foreign.signature = spec.signature.clone();
    runtime
        .register_project(
            &f.owner(),
            p::IdempotencyKey("foreign-fitness".into()),
            foreign.clone(),
        )
        .unwrap();
    let isolated = runtime
        .plan_project_route(
            &f.owner(),
            p::IdempotencyKey("foreign-plan".into()),
            &foreign.reference,
            foreign.signature,
            p::SchemaDigest("foreign".into()),
            now(),
        )
        .unwrap();
    assert_eq!(isolated.scores, failed.scores);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(
        plan(&runtime, "after-restart-fitness").scores,
        improved.scores
    );
}

#[test]
fn production_observes_all_three_models_without_self_promoting_or_duplicate_recovery() {
    let f = Fixture::new();
    let runtime = f.build();
    let h = runtime.harness();
    let run = h.submit_run(request("three-models")).unwrap();
    assert_eq!(h.wait(run.clone()).unwrap().status, p::RunStatus::Complete);
    let services = runtime.services();
    let records = services.memory.candidate_records();
    assert!(records
        .iter()
        .any(|r| r.spec.id.0 == format!("run-user:{}", run.0)));
    assert!(records
        .iter()
        .any(|r| r.spec.target.0.starts_with("agent-self|")));
    assert!(records
        .iter()
        .any(|r| r.spec.target.0.starts_with("partnership|")));
    assert!(services.cognition.stable_objects().is_empty());
    let trace = services
        .store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let before = events(&runtime).len();
    services
        .temporal_models
        .observe_terminal_run(&trace)
        .unwrap();
    assert_eq!(before, events(&runtime).len());
    // Merely acknowledging the observation cannot replace the evidence gate.
    let candidate = records
        .iter()
        .find(|r| r.spec.target.0.starts_with("agent-self|"))
        .unwrap();
    assert!(GatewayControl::review_candidate(
        h.as_ref(),
        p::CandidateReviewCommand {
            schema_version: p::SchemaVersion(1),
            run: services.aggregates.memory_cognition.clone(),
            candidate: candidate.spec.id.clone(),
            expected_state: p::CandidateReviewState::Candidate,
            decision: p::CandidateReviewDecision::Promote,
            actor: p::Actor::Owner,
            evidence: candidate.spec.evidence_refs.clone(),
            retraction: None,
        }
    )
    .is_err());
    assert!(services.cognition.stable_objects().is_empty());
    let inputs = services
        .competence_inputs(
            &p::Scope(f.config.workspace.0.clone()),
            Some(&p::CapabilityRef("owner-dialogue".into())),
        )
        .unwrap();
    assert!(
        inputs.capability_evidence.is_empty(),
        "a verified final string is not verified ability"
    );
    drop(services);
    drop(h);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(runtime.services().memory.candidate_records(), records);
}

#[test]
fn scoped_stable_statements_reach_the_model_and_retraction_removes_them() {
    use c::{CognitiveMapStore, EvolutionGovernor};
    let f = Fixture::new();
    let runtime = f.build();
    let services = runtime.services();
    let scope = p::Scope(f.config.workspace.0.clone());
    let candidate = services
        .cognition
        .propose_update(c::CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: c::MapScope {
                schema_version: p::SchemaVersion(1),
                scope: scope.clone(),
            },
            kind: c::MapUpdateKind::BlindSpot,
            confidence: p::Confidence(0.5),
            evidence: vec![p::EvidenceRef("external-verified-process".into())],
            frame: None,
            quality: None,
            blindspot: Some(p::BlindSpotModelRef("blindspot:assumption".into())),
            resource: None,
            reflection_inputs: vec![],
            statement: "Verify units before comparing project measurements".into(),
            provenance: p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        })
        .unwrap();
    let evidence = vec![c::GovernanceEvidence {
        schema_version: p::SchemaVersion(1),
        reference: p::EvidenceRef("external-verified-process".into()),
        observed_at: now(),
        verified_process: true,
    }];
    services
        .cognition
        .govern_and_apply(c::GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: candidate,
            direction: c::ChangeDirection::CautionIncreasing,
            confidence: p::Confidence(0.5),
            evidence: evidence.clone(),
            impact: p::Impact::Low,
            provenance: p::Provenance {
                source: p::Source::OwnerControl,
                actor: p::Actor::Owner,
                trust_tier: p::TrustTier::OwnerInput,
                caused_by: None,
            },
            conflicts: vec![],
            owner_confirmed: true,
            target: c::StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef("blindspot:assumption".into()),
                scope: c::MapScope {
                    schema_version: p::SchemaVersion(1),
                    scope: scope.clone(),
                },
                kind: c::StableCognitionKind::BlindSpot,
                statement: "Verify units before comparing project measurements".into(),
                tier: p::StabilityTier::Stable,
                confidence: p::Confidence(0.5),
                evidence,
                last_reproduced_at: now(),
                active: false,
            },
        })
        .unwrap();
    let hydrate = || {
        services
            .context_hydrator
            .hydrate(&ContextHydrationRequest {
                schema_version: p::SchemaVersion(1),
                run: p::RunId("new-context".into()),
                session: p::SessionId("s".into()),
                workspace: f.config.workspace.clone(),
                scope: scope.clone(),
                brain_call: true,
                selected_skills: vec![],
                model: p::ModelProfileRef("model:core-b".into()),
                budget: ContextBudget {
                    schema_version: p::SchemaVersion(1),
                    max_tokens: 32768,
                    reserve: 1024,
                },
            })
            .unwrap()
    };
    assert!(format!("{:?}", hydrate()).contains("Verify units before comparing"));
    let h = runtime.harness();
    let run = h.submit_run(request("stable-context")).unwrap();
    h.wait(run).unwrap();
    assert!(f
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|r| r.contains("Verify units before comparing")));
    services.cognition.on_retraction(c::RetractionEvent {
        schema_version: p::SchemaVersion(1),
        target_object: p::ObjectRef("blindspot:assumption".into()),
        evidence_lineage: p::LineageRef("owner-correction".into()),
        provenance: p::Provenance {
            source: p::Source::OwnerControl,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    });
    assert!(!format!("{:?}", hydrate()).contains("Verify units before comparing"));
    assert!(services
        .competence_inputs(&scope, None)
        .unwrap()
        .map_confidence
        .is_none());
}
