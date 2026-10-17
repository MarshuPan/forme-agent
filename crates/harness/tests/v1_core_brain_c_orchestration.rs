#[path = "support/core_brain.rs"]
mod support;
use forme_harness::*;
use forme_protocol as p;
use forme_store::EventStore;
use std::sync::atomic::Ordering;
use support::*;

fn export_artifact(
    runtime: &ProductionRuntime,
    case: forme_eval::CoreBrainCCase,
    file: &str,
    files: Vec<forme_eval::CoreBrainCFileEvidence>,
) {
    let Some(directory) = std::env::var_os("FORME_CORE_C_ARTIFACT_DIR") else {
        return;
    };
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let source = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    let a = forme_eval::CoreBrainCArtifact::capture(
        case,
        source,
        runtime.manifest().digest.clone(),
        runtime.services().aggregates.charter.clone(),
        runtime
            .manifest()
            .model_profiles
            .iter()
            .map(|reference| {
                (
                    reference.clone(),
                    runtime
                        .services()
                        .model_registry
                        .get(reference)
                        .unwrap()
                        .profile()
                        .profile_ref(),
                )
            })
            .collect(),
        &events(runtime),
        &runtime
            .orchestration_state(&p::VerifiedPrincipal(runtime.owner().0.clone()))
            .unwrap(),
        files,
    )
    .unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        std::path::PathBuf::from(directory).join(file),
        serde_json::to_vec_pretty(&a).unwrap(),
    )
    .unwrap();
}

fn signature() -> p::TaskSignature {
    p::TaskSignature {
        schema_version: p::SchemaVersion(1),
        partition: p::TaskPartition::Whole,
        dependency: p::TaskDependency::Independent,
        verification: p::TaskVerification::SelfCheck,
        clarity: p::TaskClarity::Clear,
        extent: p::TaskExtent::Single,
        reversibility: p::TaskReversibility::LowRiskReversible,
        anchors: vec![],
    }
}
fn project(f: &Fixture) -> p::WorkspaceProject {
    p::WorkspaceProject {
        schema_version: p::SchemaVersion(1),
        reference: "project:review".into(),
        workspace: f.config.workspace.clone(),
        goal: p::GoalRef("goal:project".into()),
        done: p::DoneContractRef("done:project".into()),
        summary: "Return the independently specified fixture result".into(),
        signature: signature(),
        requirements: vec![p::RouteRequirement::OutputContains("ground-truth".into())],
        scope: p::Scope(f.config.workspace.0.clone()),
        capabilities: vec![],
        model: p::ModelProfileRef("model:core-b".into()),
        budget_units: 8,
        node_units: 2,
        cadence_ms: 60_000,
        deadline: None,
    }
}
fn setup(f: &Fixture) -> ProductionRuntime {
    setup_project(f, project(f))
}
fn setup_project(f: &Fixture, spec: p::WorkspaceProject) -> ProductionRuntime {
    let runtime = f.build();
    let charter = p::WorkspaceCharterRecord {
        schema_version: p::SchemaVersion(1),
        workspace: f.config.workspace.clone(),
        version: 1,
        goals: vec![],
        constraints: vec![p::Constraint("max-nodes:8".into())],
        prohibitions: vec![],
        done_contract: None,
        review_cadence: Some(p::DurationMs(60_000)),
        actor: p::Actor::Owner,
        digest: p::SchemaDigest(String::new()),
    };
    let change = runtime
        .prepare_workspace_charter(&f.owner(), charter, 0)
        .unwrap();
    runtime
        .apply_workspace_charter(
            &f.owner(),
            p::IdempotencyKey("setup-charter".into()),
            change,
        )
        .unwrap();
    runtime
        .register_project(&f.owner(), p::IdempotencyKey("project-1".into()), spec)
        .unwrap();
    runtime
}
fn plan(
    runtime: &ProductionRuntime,
    f: &Fixture,
    sig: p::TaskSignature,
    key: &str,
) -> p::RouteDeclaration {
    runtime
        .plan_project_route(
            &f.owner(),
            p::IdempotencyKey(key.into()),
            "project:review",
            sig,
            p::canonical_digest(&key).unwrap(),
            now(),
        )
        .unwrap()
}

fn raw_fact(
    runtime: &ProductionRuntime,
    command: &str,
    revision: u64,
    operation: p::OrchestrationOperation,
) -> p::Result<p::EventId> {
    let services = runtime.services();
    let fact = p::OrchestrationFact::seal(
        runtime.config().workspace.clone(),
        p::IdempotencyKey(command.into()),
        revision,
        operation,
    )?;
    services.store.append(p::Event::new(
        p::EventId(format!("raw:{command}")),
        services.aggregates.charter.clone(),
        None,
        p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
            trace_ref: p::DecisionTraceRef(format!("trace:{command}")),
            refs: p::DecisionRefs {
                map: None,
                user: None,
                agent_self: None,
                trust: None,
                failure: vec![],
            },
            rationale: p::Rationale("self-authored store adversarial test".into()),
            workspace_snapshot: p::AgentWorkspaceSnapshotRef("snapshot:test".into()),
            resource_graph_snapshot: None,
            evolution_snapshot: None,
            federation_snapshot: None,
            orchestration: Some(fact),
        }),
        p::SchemaVersion(1),
        now(),
        p::Provenance {
            source: p::Source::Internal,
            actor: p::Actor::System,
            trust_tier: p::TrustTier::VerifiedProcess,
            caused_by: None,
        },
    ))
}

#[test]
fn c16_unknown_claim_survives_restart_holds_budget_and_never_calls_a_driver_again() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let route = plan(&runtime, &f, signature(), "unknown");
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    raw_fact(
        &runtime,
        "claim-crash",
        state.revision,
        p::OrchestrationOperation::NodeStarted {
            route: route.route.clone(),
            node: "main".into(),
            intent: route.main.intent.clone(),
        },
    )
    .unwrap();
    drop(runtime);
    let restarted = f.build();
    let outcome = restarted
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_eq!(outcome.status, p::WorkRouteStatus::OutcomeUnknown);
    assert_eq!(outcome.nodes["main"].reserved, 2);
    assert_eq!(outcome.remaining, 6);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(restarted
        .retry_project_node(
            &f.owner(),
            route.route.clone(),
            "main".into(),
            p::ActionId("must-not-run".into()),
            p::IdempotencyKey("blind-retry".into())
        )
        .is_err());
    assert!(restarted
        .cancel_project_route(
            &f.owner(),
            &route.route,
            p::IdempotencyKey("erase-unknown".into())
        )
        .is_err());
    assert_eq!(
        restarted
            .execute_project_route(&f.owner(), &route.route)
            .unwrap(),
        outcome
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn c16_store_cas_reserves_parent_budget_once_and_rejects_forged_success() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let mut sig = signature();
    sig.partition = p::TaskPartition::Parallel;
    sig.verification = p::TaskVerification::Environment;
    sig.clarity = p::TaskClarity::Explore;
    sig.extent = p::TaskExtent::MultiStage;
    let route = plan(&runtime, &f, sig, "parallel");
    assert_eq!(route.nodes.len(), 3);
    let revision = runtime.orchestration_state(&f.owner()).unwrap().revision;
    let winners = std::thread::scope(|scope| {
        let handles = route.nodes[..2]
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let r = &runtime;
                let route = &route;
                scope.spawn(move || {
                    raw_fact(
                        r,
                        &format!("race-{i}"),
                        revision,
                        p::OrchestrationOperation::NodeStarted {
                            route: route.route.clone(),
                            node: n.id.clone(),
                            intent: n.intent.clone(),
                        },
                    )
                    .is_ok()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|won| *won)
            .count()
    });
    assert_eq!(winners, 1);
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    let record = &state.routes[&route.route];
    assert_eq!(record.remaining, 6);
    assert_eq!(record.nodes.values().map(|n| n.reserved).sum::<u64>(), 2);
    let id = record
        .nodes
        .iter()
        .find(|(_, n)| n.status == p::WorkNodeStatus::Running)
        .unwrap()
        .0
        .clone();
    let head = runtime
        .services()
        .store
        .run_head(&runtime.services().aggregates.charter)
        .unwrap();
    assert!(raw_fact(
        &runtime,
        "fake-pass",
        state.revision,
        p::OrchestrationOperation::NodeFinished {
            route: route.route.clone(),
            node: id.clone(),
            status: p::WorkNodeStatus::Done,
            consumed: 0,
            result: Some(p::ResultRef("self-report".into())),
            summary: "I am correct".into(),
            verification: p::VerificationOutcome::Pass,
            evidence: vec![],
            effects_absent: true
        }
    )
    .is_err());
    assert!(raw_fact(
        &runtime,
        "overspend",
        state.revision,
        p::OrchestrationOperation::NodeFinished {
            route: route.route.clone(),
            node: id,
            status: p::WorkNodeStatus::Failed,
            consumed: 3,
            result: None,
            summary: String::new(),
            verification: p::VerificationOutcome::Fail,
            evidence: vec![],
            effects_absent: true
        }
    )
    .is_err());
    assert_eq!(
        runtime
            .services()
            .store
            .run_head(&runtime.services().aggregates.charter)
            .unwrap(),
        head
    );
}

#[test]
fn c16_parent_cancel_cascades_pending_work_and_preserves_real_conflicts() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let route = plan(&runtime, &f, signature(), "cancelled");
    runtime
        .cancel_project_route(&f.owner(), &route.route, p::IdempotencyKey("cancel".into()))
        .unwrap();
    let out = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_eq!(out.status, p::WorkRouteStatus::Cancelled);
    assert_eq!(out.remaining, 8);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    let mut sig = signature();
    sig.partition = p::TaskPartition::Sequential;
    sig.dependency = p::TaskDependency::Interdependent;
    sig.verification = p::TaskVerification::IndependentReview;
    sig.extent = p::TaskExtent::MultiStage;
    let route = plan(&runtime, &f, sig, "conflict");
    *f.answer.lock().unwrap() =
        r#"{"answer":"ground-truth","conflicts":[{"kind":"fact","nodes":["node-0","node-1"]}]}"#
            .into();
    let out = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_eq!(out.status, p::WorkRouteStatus::Conflict);
    assert_eq!(out.conflicts.len(), 1);
    assert_eq!(out.conflicts[0].kind, p::ResultConflictKind::Fact);
    assert_eq!(out.conflicts[0].nodes, vec!["node-0", "node-1"]);
    assert!(out
        .nodes
        .values()
        .all(|n| n.verification == p::VerificationOutcome::Pass));
    assert_eq!(
        runtime.orchestration_state(&f.owner()).unwrap().routes[&route.route].conflicts,
        out.conflicts
    );
    export_artifact(
        &runtime,
        forme_eval::CoreBrainCCase::Conflict,
        "conflict.json",
        vec![],
    );
}

#[test]
fn c16_failure_retry_requires_new_intent_and_only_reclaims_unused_units() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let runtime = setup(&f);
    let route = plan(&runtime, &f, signature(), "retry");
    let revision = runtime.orchestration_state(&f.owner()).unwrap().revision;
    raw_fact(
        &runtime,
        "start-once",
        revision,
        p::OrchestrationOperation::NodeStarted {
            route: route.route.clone(),
            node: "main".into(),
            intent: route.main.intent.clone(),
        },
    )
    .unwrap();
    raw_fact(
        &runtime,
        "fail-once",
        revision + 1,
        p::OrchestrationOperation::NodeFinished {
            route: route.route.clone(),
            node: "main".into(),
            status: p::WorkNodeStatus::Failed,
            consumed: 1,
            result: None,
            summary: "no effect".into(),
            verification: p::VerificationOutcome::Fail,
            evidence: vec![p::EvidenceRef("pre-effect:test".into())],
            effects_absent: true,
        },
    )
    .unwrap();
    assert_eq!(
        runtime.orchestration_state(&f.owner()).unwrap().routes[&route.route].remaining,
        7
    );
    assert!(runtime
        .retry_project_node(
            &f.owner(),
            route.route.clone(),
            "main".into(),
            route.main.intent.clone(),
            p::IdempotencyKey("same-intent".into())
        )
        .is_err());
    runtime
        .retry_project_node(
            &f.owner(),
            route.route.clone(),
            "main".into(),
            p::ActionId("new-safe-attempt".into()),
            p::IdempotencyKey("retry-once".into()),
        )
        .unwrap();
    let out = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_eq!(out.status, p::WorkRouteStatus::Completed);
    assert_eq!(out.spent, 2);
    assert_eq!(out.remaining, 6);
    assert_eq!(out.nodes["main"].attempts, 2);
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn c17_tick_plans_next_segment_after_checkpoint_and_never_executes_or_hot_swaps() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let runtime = setup(&f);
    let t = now() + 100;
    let report = runtime.cognitive_tick_at(t).unwrap();
    assert!(matches!(report,CognitiveTickOutcome::Ran(r) if !r.failed()));
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    assert_eq!(state.routes.len(), 1);
    let first = state.routes.values().next().unwrap().declaration.clone();
    assert!(first.nodes.is_empty());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    let mut changed = project(&f);
    changed.signature.partition = p::TaskPartition::Sequential;
    changed.signature.dependency = p::TaskDependency::Interdependent;
    changed.signature.verification = p::TaskVerification::IndependentReview;
    changed.signature.extent = p::TaskExtent::MultiStage;
    runtime
        .revise_project(
            &f.owner(),
            p::IdempotencyKey("project-complexity".into()),
            changed.clone(),
            p::canonical_digest(&project(&f)).unwrap(),
        )
        .unwrap();
    let _ = runtime.cognitive_tick_at(t + 60_000).unwrap();
    assert_eq!(
        runtime
            .orchestration_state(&f.owner())
            .unwrap()
            .routes
            .len(),
        1
    );
    runtime
        .execute_project_route(&f.owner(), &first.route)
        .unwrap();
    let before_calls = f.calls.load(Ordering::SeqCst);
    *f.answer.lock().unwrap() = "[]".into();
    let outcome = runtime.cognitive_tick_at(t + 120_000).unwrap();
    assert!(matches!(outcome,CognitiveTickOutcome::Ran(r) if !r.failed()));
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    assert_eq!(state.routes.len(), 2);
    assert!(state.checkpoints.contains_key("project:review"));
    let second = state
        .routes
        .values()
        .find(|r| r.declaration.route != first.route)
        .unwrap();
    assert_eq!(second.declaration.nodes.len(), 2);
    assert_eq!(state.routes[&first.route].declaration.nodes.len(), 0);
    // A relevance baseline can run during a cognitive tick; no child/model
    // call belonging to the new route is admitted by supervision itself.
    assert!(f.calls.load(Ordering::SeqCst) >= before_calls);
    assert!(!events(&runtime).iter().any(|e| e
        .run_id
        .0
        .starts_with(&format!("child:{}", second.declaration.parent.0))));
    drop(runtime);
    let restarted = f.build();
    assert_eq!(restarted.orchestration_state(&f.owner()).unwrap(), state);
    let _ = restarted.cognitive_tick_at(t + 180_000).unwrap();
    assert_eq!(
        restarted
            .orchestration_state(&f.owner())
            .unwrap()
            .routes
            .len(),
        2
    );
    export_artifact(
        &restarted,
        forme_eval::CoreBrainCCase::Supervision,
        "supervision.json",
        vec![],
    );
}

#[test]
fn c16_formal_direct_route_is_idempotent_verified_and_rebuilds_after_restart() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let runtime = setup(&f);
    let route = plan(&runtime, &f, signature(), "route-direct");
    assert!(route.nodes.is_empty());
    let result = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_eq!(
        result.status,
        p::WorkRouteStatus::Completed,
        "{:?}",
        result.nodes
    );
    assert!(result.closed);
    assert_eq!(result.spent, 1);
    assert_eq!(result.remaining, 7);
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime
            .execute_project_route(&f.owner(), &route.route)
            .unwrap(),
        result
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    let stream = runtime.services().aggregates.charter.clone();
    let ledger = runtime.orchestration_state(&f.owner()).unwrap();
    runtime
        .services()
        .store
        .rebuild_orchestration(&stream)
        .unwrap();
    assert_eq!(runtime.orchestration_state(&f.owner()).unwrap(), ledger);
    drop(runtime);
    let restarted = f.build();
    assert_eq!(restarted.orchestration_state(&f.owner()).unwrap(), ledger);
    assert_eq!(
        restarted
            .execute_project_route(&f.owner(), &route.route)
            .unwrap(),
        result
    );
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn c16_formal_dag_runs_in_distinct_real_directories_with_dependency_context() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let runtime = setup(&f);
    let mut sig = signature();
    sig.partition = p::TaskPartition::Sequential;
    sig.dependency = p::TaskDependency::Interdependent;
    sig.verification = p::TaskVerification::IndependentReview;
    sig.extent = p::TaskExtent::MultiStage;
    let route = plan(&runtime, &f, sig, "route-dag");
    assert_eq!(route.nodes.len(), 2);
    let result = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_eq!(
        result.status,
        p::WorkRouteStatus::Completed,
        "{:?}; {:?}",
        result.nodes,
        events(&runtime)
            .iter()
            .filter_map(|e| if e.kind == p::EventKind::RunFailed {
                Some(&e.payload)
            } else {
                None
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(result.spent, 2);
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    let a = runtime
        .project_node_directory(&route, &route.nodes[0].id, 1)
        .unwrap();
    let b = runtime
        .project_node_directory(&route, &route.nodes[1].id, 1)
        .unwrap();
    assert_ne!(a, b);
    assert!(a.is_dir() && b.is_dir());
    std::fs::write(a.join("private.txt"), b"local-child-state").unwrap();
    assert!(!b.join("private.txt").exists());
    let all = events(&runtime);
    assert_eq!(
        all.iter()
            .filter(|e| e.kind == p::EventKind::SubagentSpawned)
            .count(),
        2
    );
    assert!(all.iter().any(|e|matches!(&e.payload,p::EventPayload::RunAccepted(p) if p.source==p::Source::Subagent && p.input_ref.0.contains("Verified dependency"))));
    let mut replay = p::OrchestrationLedger::default();
    for e in runtime
        .services()
        .store
        .read_run(runtime.services().aggregates.charter.clone())
    {
        replay.fold(&e.unwrap()).unwrap();
    }
    assert_eq!(replay, runtime.orchestration_state(&f.owner()).unwrap());
}

#[test]
fn c18_success_and_failure_form_episodes_lineage_and_only_strategy_candidates() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let runtime = setup(&f);
    let route = plan(&runtime, &f, signature(), "success");
    assert_eq!(
        runtime
            .execute_project_route(&f.owner(), &route.route)
            .unwrap()
            .status,
        p::WorkRouteStatus::Completed
    );
    *f.answer.lock().unwrap() = "I declare success without evidence".into();
    let route2 = plan(&runtime, &f, signature(), "failure");
    assert_eq!(
        runtime
            .execute_project_route(&f.owner(), &route2.route)
            .unwrap()
            .status,
        p::WorkRouteStatus::Failed
    );
    let all = events(&runtime);
    for r in [&route, &route2] {
        assert!(all.iter().any(|e|matches!(&e.payload,p::EventPayload::MemoryNodeAppended(p) if p.node_id.0==format!("route-episode:{}",r.route.0)&&p.kind.0=="episodic")));
        assert!(all.iter().any(|e|matches!(&e.payload,p::EventPayload::CandidateCreated(p) if p.strategy_candidate.as_ref().is_some_and(|s|s.domain==p::StrategyDomain::Coordination && s.candidate.0==format!("route-strategy:{}",r.route.0)))));
    }
    assert!(all
        .iter()
        .any(|e| e.kind == p::EventKind::FailureDigestUpdated));
    assert!(!all.iter().any(|e| matches!(
        e.kind,
        p::EventKind::CandidatePromoted | p::EventKind::StrategyActivated
    )));
    let before = all.len();
    let checkpoint = runtime
        .orchestration_state(&f.owner())
        .unwrap()
        .checkpoints
        .clone();
    runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_eq!(
        runtime.orchestration_state(&f.owner()).unwrap().checkpoints,
        checkpoint
    );
    runtime
        .execute_project_route(&f.owner(), &route2.route)
        .unwrap();
    assert_eq!(events(&runtime).len(), before);
    let graph = runtime.services().memory.graph_snapshot_at(None).unwrap();
    let candidates = runtime.services().memory.candidate_records();
    assert!(candidates
        .iter()
        .any(|c| format!("{c:?}").contains("route-strategy:")));
    drop(runtime);
    let restarted = f.build();
    assert_eq!(
        format!("{graph:?}"),
        format!(
            "{:?}",
            restarted.services().memory.graph_snapshot_at(None).unwrap()
        )
    );
    assert_eq!(
        format!("{candidates:?}"),
        format!("{:?}", restarted.services().memory.candidate_records())
    );
    assert_eq!(
        restarted.services().orchestration.index()[0]
            .pattern
            .fitness,
        p::Confidence(0.5)
    );
    export_artifact(
        &restarted,
        forme_eval::CoreBrainCCase::Learning,
        "learning.json",
        vec![],
    );
}

fn review_signature() -> p::TaskSignature {
    let mut s = signature();
    s.partition = p::TaskPartition::Sequential;
    s.dependency = p::TaskDependency::Interdependent;
    s.verification = p::TaskVerification::IndependentReview;
    s.extent = p::TaskExtent::MultiStage;
    s
}

#[test]
fn c13_charter_references_and_live_resource_tightening_fail_without_admission() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let mut charter = runtime
        .workspace_charter(&f.owner(), &f.config.workspace)
        .unwrap()
        .unwrap();
    charter.goals = vec![p::GoalRef("foreign:undeclared".into())];
    let change = runtime
        .prepare_workspace_charter(&f.owner(), charter.clone(), 1)
        .unwrap();
    let head = runtime
        .services()
        .store
        .run_head(&runtime.services().aggregates.charter)
        .unwrap();
    assert!(runtime
        .apply_workspace_charter(&f.owner(), p::IdempotencyKey("bad-ref".into()), change)
        .is_err());
    assert_eq!(
        head,
        runtime
            .services()
            .store
            .run_head(&runtime.services().aggregates.charter)
            .unwrap()
    );
    charter.goals = vec![p::GoalRef("goal:project".into())];
    charter.done_contract = Some(p::DoneContractRef("done:project".into()));
    charter.constraints = vec![p::Constraint("max-nodes:1".into())];
    let change = runtime
        .prepare_workspace_charter(&f.owner(), charter, 1)
        .unwrap();
    std::thread::scope(|s| {
        let threads = (0..4)
            .map(|_| {
                let runtime = &runtime;
                let change = change.clone();
                let owner = f.owner();
                s.spawn(move || {
                    runtime.apply_workspace_charter(
                        &owner,
                        p::IdempotencyKey("same-charter".into()),
                        change,
                    )
                })
            })
            .collect::<Vec<_>>();
        let ids = threads
            .into_iter()
            .map(|t| t.join().unwrap().unwrap())
            .collect::<Vec<_>>();
        assert!(ids.iter().all(|id| id == &ids[0]));
    });
    assert!(runtime
        .plan_project_route(
            &f.owner(),
            p::IdempotencyKey("blocked-dag".into()),
            "project:review",
            review_signature(),
            p::canonical_digest(&"input").unwrap(),
            now()
        )
        .is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(!events(&runtime)
        .iter()
        .any(|e| e.kind == p::EventKind::SubagentSpawned));
}

#[test]
fn c15_clarification_blocks_admission_and_planning_retries_keep_the_original_segment() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let mut sig = signature();
    sig.clarity = p::TaskClarity::Clarify;
    let route = plan(&runtime, &f, sig.clone(), "clarify");
    assert_eq!(
        route.pattern.as_ref().unwrap().0,
        "pattern:clarify-then-act"
    );
    let head = events(&runtime).len();
    let duplicate = runtime
        .plan_project_route(
            &f.owner(),
            p::IdempotencyKey("clarify".into()),
            "project:review",
            sig.clone(),
            p::canonical_digest(&"clarify").unwrap(),
            now() + 1000,
        )
        .unwrap();
    assert_eq!(duplicate, route);
    assert_eq!(head, events(&runtime).len());
    assert!(runtime
        .execute_project_route(&f.owner(), &route.route)
        .is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    assert!(
        runtime.orchestration_state(&f.owner()).unwrap().routes[&route.route]
            .nodes
            .values()
            .all(|n| n.attempts == 0)
    );
    sig.clarity = p::TaskClarity::Clear;
    assert!(runtime
        .plan_project_route(
            &f.owner(),
            p::IdempotencyKey("clarify".into()),
            "project:review",
            sig,
            p::canonical_digest(&"clarify").unwrap(),
            now()
        )
        .is_err());
}

#[test]
fn c16_concurrent_execute_has_one_driver_and_live_parent_cancel_reaches_children() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    f.delay_ms.store(250, Ordering::SeqCst);
    let runtime = setup(&f);
    let route = plan(&runtime, &f, signature(), "concurrent");
    std::thread::scope(|s| {
        let owner = f.owner();
        let runtime = &runtime;
        let route = &route.route;
        let first = s.spawn(move || runtime.execute_project_route(&owner, route));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while f.calls.load(Ordering::SeqCst) == 0 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(
            !runtime
                .execute_project_route(&f.owner(), route)
                .unwrap()
                .closed
        );
        first.join().unwrap().unwrap();
    });
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    let route = plan(&runtime, &f, review_signature(), "live-cancel");
    std::thread::scope(|s| {
        let owner = f.owner();
        let runtime = &runtime;
        let route = &route.route;
        let first = s.spawn(move || runtime.execute_project_route(&owner, route));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while f.calls.load(Ordering::SeqCst) < 2 {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        runtime
            .cancel_project_route(
                &f.owner(),
                route,
                p::IdempotencyKey("live-cancel-command".into()),
            )
            .unwrap();
        let out = first.join().unwrap().unwrap();
        assert_eq!(out.status, p::WorkRouteStatus::Cancelled);
        assert!(out.nodes.values().all(|n| matches!(
            n.status,
            p::WorkNodeStatus::Done | p::WorkNodeStatus::Cancelled
        )));
    });
    assert_eq!(
        events(&runtime)
            .iter()
            .filter(|e| e.kind == p::EventKind::SubagentSpawned)
            .count(),
        1
    );
}

fn file_fixture() -> Fixture {
    let mut f = Fixture::new();
    f.config.capabilities.file.enabled = true;
    f.config.capabilities.file.roots = vec![f
        .config
        .store_path
        .parent()
        .unwrap()
        .to_string_lossy()
        .into_owned()];
    f.config.models[0].tool_use = true;
    f.config.models[0].tool_schema_versions = vec![1];
    *f.answer.lock().unwrap() = "ground-truth".into();
    f
}
fn file_project(f: &Fixture) -> p::WorkspaceProject {
    let mut p = project(f);
    p.capabilities = vec![p::CapabilityRef("capability:file".into())];
    p.node_units = 3;
    p.requirements.push(p::RouteRequirement::FileDigest {
        relative_path: "result.txt".into(),
        digest: p::sha256_content_digest(b"isolated-result").0,
    });
    p
}
fn file_call(path: &str) -> serde_json::Value {
    let parameters = p::ActionParameters::File {
        operation: p::FileOperation::Write,
        path: path.into(),
        content: Some(b"isolated-result".to_vec()),
    };
    serde_json::json!({"choices":[{"message":{"tool_calls":[{"id":"write-once","type":"function",
        "function":{"name":forme_models::ToolDefinition::catalog_alias("capability:file").unwrap(),"arguments":serde_json::to_string(&parameters).unwrap()}}]},
        "finish_reason":"tool_calls"}],"usage":{"prompt_tokens":20,"completion_tokens":4}})
}
fn grant(f: &Fixture, request: &forme_approval::ApprovalRequest) -> ApprovalGrant {
    ApprovalGrant {
        schema_version: p::SchemaVersion(1),
        approval_id: request.approval_id.clone(),
        outcome: p::ApprovalOutcome::Granted,
        granted_scope: forme_approval::GrantScope::OneShot,
        approver: f.owner(),
        bound_plan_digest: request.plan_digest.clone(),
        normalized_action_digest: request.normalized_action_digest.clone(),
        affected_resources: request.affected_resources.clone(),
        policy_version: request.policy_version,
        tool_schema_version: request.tool_schema_version,
        nonce: p::Nonce(format!("nonce:{}", request.approval_id.0)),
        use_by: request.expires_at,
    }
}
fn child_approval(runtime: &ProductionRuntime) -> (p::RunId, forme_approval::ApprovalRequest) {
    for e in events(runtime).iter().rev() {
        if let p::EventPayload::RunAccepted(p) = &e.payload {
            if p.source == p::Source::Subagent {
                let requests = runtime
                    .harness()
                    .pending_approvals(p.session_ref.clone())
                    .unwrap();
                if let Some(request) = requests.first() {
                    return (e.run_id.clone(), request.clone());
                }
            }
        }
    }
    panic!("no live child approval")
}

#[test]
fn c16_real_child_file_tools_wait_resume_and_keep_sibling_directories_isolated() {
    let f = file_fixture();
    let runtime = setup_project(&f, file_project(&f));
    let route = plan(&runtime, &f, review_signature(), "file-work");
    f.responses
        .lock()
        .unwrap()
        .push_back(file_call("result.txt"));
    let waiting = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert!(waiting.has_inflight());
    let a = runtime
        .project_node_directory(&route, &route.nodes[0].id, 1)
        .unwrap();
    let b = runtime
        .project_node_directory(&route, &route.nodes[1].id, 1)
        .unwrap();
    assert!(!a.join("result.txt").exists());
    let (run, request) = child_approval(&runtime);
    runtime
        .harness()
        .resume(run, ResumeInput::Approval(grant(&f, &request)))
        .unwrap();
    assert_eq!(
        std::fs::read(a.join("result.txt")).unwrap(),
        b"isolated-result"
    );
    assert!(!b.join("result.txt").exists());
    f.responses
        .lock()
        .unwrap()
        .push_back(file_call("result.txt"));
    assert!(runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap()
        .has_inflight());
    let (run, request) = child_approval(&runtime);
    runtime
        .harness()
        .resume(run, ResumeInput::Approval(grant(&f, &request)))
        .unwrap();
    let out = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_eq!(out.status, p::WorkRouteStatus::Completed);
    assert_eq!(out.spent, 4);
    assert_eq!(
        std::fs::read(b.join("result.txt")).unwrap(),
        b"isolated-result"
    );
    let all = events(&runtime);
    assert_eq!(
        all.iter()
            .filter(|e| e.kind == p::EventKind::ActionStarted)
            .count(),
        2
    );
    let requests = f.requests.lock().unwrap();
    for r in requests.iter() {
        let body = r.split("\r\n\r\n").nth(1).unwrap();
        let body: serde_json::Value = serde_json::from_str(body).unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        let alias = tools[0]["function"]["name"].as_str().unwrap();
        assert_eq!(
            alias,
            forme_models::ToolDefinition::catalog_alias("capability:file").unwrap()
        );
        assert!(
            alias.len() <= 64
                && alias
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        );
        assert!(!body["messages"]
            .to_string()
            .contains("single owner brain for owner-digest"));
    }
    let files = [(&route.nodes[0].id, a), (&route.nodes[1].id, b)]
        .into_iter()
        .map(|(node, dir)| forme_eval::CoreBrainCFileEvidence {
            schema_version: p::SchemaVersion(1),
            route: route.route.clone(),
            node: node.clone(),
            attempt: 1,
            relative_path: "result.txt".into(),
            digest: p::sha256_content_digest(&std::fs::read(dir.join("result.txt")).unwrap()),
        })
        .collect();
    export_artifact(
        &runtime,
        forme_eval::CoreBrainCCase::Isolation,
        "isolation.json",
        files,
    );
}

#[test]
fn c16_waiting_child_approval_survives_restart_without_repeating_model_or_effect() {
    let f = file_fixture();
    let runtime = setup_project(&f, file_project(&f));
    let route = plan(&runtime, &f, review_signature(), "file-restart");
    f.responses
        .lock()
        .unwrap()
        .push_back(file_call("result.txt"));
    runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    let (run, request) = child_approval(&runtime);
    let head = events(&runtime).len();
    drop(runtime);
    let runtime = f.build();
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    assert_eq!(head, events(&runtime).len());
    let (recovered, restored) = child_approval(&runtime);
    assert_eq!(recovered, run);
    assert_eq!(request, restored);
    runtime
        .harness()
        .resume(run.clone(), ResumeInput::Approval(grant(&f, &request)))
        .unwrap();
    assert!(runtime
        .harness()
        .resume(run, ResumeInput::Approval(grant(&f, &request)))
        .is_err());
    assert_eq!(
        events(&runtime)
            .iter()
            .filter(|e| e.kind == p::EventKind::ActionStarted)
            .count(),
        1,
        "{:?}",
        events(&runtime)
            .iter()
            .filter(|e| matches!(
                e.kind,
                p::EventKind::ActionDenied | p::EventKind::FailureEvidenceRecorded
            ))
            .collect::<Vec<_>>()
    );
    runtime
        .cancel_project_route(
            &f.owner(),
            &route.route,
            p::IdempotencyKey("end-test".into()),
        )
        .unwrap();
    runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
}

#[test]
fn c13_changed_charter_overrides_a_waiting_child_approval_before_the_driver() {
    let f = file_fixture();
    let runtime = setup_project(&f, file_project(&f));
    let route = plan(&runtime, &f, review_signature(), "charter-approval");
    f.responses
        .lock()
        .unwrap()
        .push_back(file_call("result.txt"));
    runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    let (run, request) = child_approval(&runtime);
    let mut charter = runtime
        .workspace_charter(&f.owner(), &f.config.workspace)
        .unwrap()
        .unwrap();
    charter.prohibitions = vec![p::Constraint("deny-capability:capability:file".into())];
    let change = runtime
        .prepare_workspace_charter(&f.owner(), charter, 1)
        .unwrap();
    runtime
        .apply_workspace_charter(&f.owner(), p::IdempotencyKey("tighten".into()), change)
        .unwrap();
    runtime
        .harness()
        .resume(run, ResumeInput::Approval(grant(&f, &request)))
        .unwrap();
    assert!(!events(&runtime)
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
    assert!(!runtime
        .project_node_directory(&route, &route.nodes[0].id, 1)
        .unwrap()
        .join("result.txt")
        .exists());
}

#[test]
fn c16_parent_file_targets_and_cross_directory_aliases_cannot_escape_child_roots() {
    let f = file_fixture();
    let runtime = setup_project(&f, file_project(&f));
    let route = plan(&runtime, &f, review_signature(), "outside-file");
    let outside = f
        .config
        .store_path
        .parent()
        .unwrap()
        .join("outside-result.txt");
    f.responses
        .lock()
        .unwrap()
        .push_back(file_call(outside.to_str().unwrap()));
    let out = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_ne!(out.status, p::WorkRouteStatus::Completed);
    assert!(!outside.exists());
    assert!(!events(&runtime)
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
    #[cfg(windows)]
    {
        let a = runtime
            .project_node_directory(&route, &route.nodes[0].id, 1)
            .unwrap();
        let b = runtime
            .project_node_directory(&route, &route.nodes[1].id, 1)
            .unwrap();
        std::fs::remove_dir(&b).unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&b)
            .arg(&a)
            .output()
            .unwrap();
        assert!(status.status.success());
        assert!(runtime
            .project_node_directory(&route, &route.nodes[1].id, 1)
            .is_err());
        std::fs::remove_dir(b).unwrap();
    }
}

#[test]
fn c14_all_eight_patterns_reach_the_formal_runtime_with_correct_child_counts() {
    use forme_coordination as c;
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let runtime = setup(&f);
    for entry in runtime.services().orchestration.index() {
        let s = entry.pattern.signature;
        let sig = p::TaskSignature {
            schema_version: p::SchemaVersion(1),
            partition: match s.decomposability {
                c::Decomposability::Whole => p::TaskPartition::Whole,
                c::Decomposability::Sequential => p::TaskPartition::Sequential,
                c::Decomposability::Parallel => p::TaskPartition::Parallel,
                c::Decomposability::Iterative => p::TaskPartition::Iterative,
            },
            dependency: match s.dependency {
                c::DependencyShape::Independent => p::TaskDependency::Independent,
                c::DependencyShape::Interdependent => p::TaskDependency::Interdependent,
            },
            verification: match s.verifiability {
                c::Verifiability::SelfCheck => p::TaskVerification::SelfCheck,
                c::Verifiability::Environment => p::TaskVerification::Environment,
                c::Verifiability::IndependentReview => p::TaskVerification::IndependentReview,
            },
            clarity: match s.clarity {
                c::GoalClarity::Clear => p::TaskClarity::Clear,
                c::GoalClarity::Clarify => p::TaskClarity::Clarify,
                c::GoalClarity::Explore => p::TaskClarity::Explore,
            },
            extent: match s.scale {
                c::TaskScale::Single => p::TaskExtent::Single,
                c::TaskScale::MultiStage => p::TaskExtent::MultiStage,
                c::TaskScale::LongRunning => p::TaskExtent::LongRunning,
            },
            reversibility: match s.reversibility {
                c::Reversibility::LowRiskReversible => p::TaskReversibility::LowRiskReversible,
                c::Reversibility::HighRiskIrreversible => {
                    p::TaskReversibility::HighRiskIrreversible
                }
            },
            anchors: vec![],
        };
        let route = plan(&runtime, &f, sig, &entry.pattern.reference.0);
        assert_eq!(route.pattern.as_ref(), Some(&entry.pattern.reference));
        let before = events(&runtime)
            .iter()
            .filter(|e| e.kind == p::EventKind::SubagentSpawned)
            .count();
        if route.signature.clarity == p::TaskClarity::Clarify {
            assert!(route.nodes.is_empty());
            assert!(runtime
                .execute_project_route(&f.owner(), &route.route)
                .is_err());
            runtime
                .cancel_project_route(
                    &f.owner(),
                    &route.route,
                    p::IdempotencyKey("end-clarify".into()),
                )
                .unwrap();
            runtime
                .execute_project_route(&f.owner(), &route.route)
                .unwrap();
        } else {
            let out = runtime
                .execute_project_route(&f.owner(), &route.route)
                .unwrap();
            assert_eq!(
                out.status,
                p::WorkRouteStatus::Completed,
                "{} {:?}",
                entry.pattern.reference.0,
                out.nodes
            );
            assert_eq!(out.spent, route.nodes.len().max(1) as u64);
        }
        let spawned = events(&runtime)
            .iter()
            .filter(|e| e.kind == p::EventKind::SubagentSpawned)
            .count()
            - before;
        assert_eq!(spawned, route.nodes.len());
    }
    assert_eq!(
        runtime
            .orchestration_state(&f.owner())
            .unwrap()
            .routes
            .len(),
        8
    );
    export_artifact(
        &runtime,
        forme_eval::CoreBrainCCase::Patterns,
        "patterns.json",
        vec![],
    );
}

#[test]
fn c17_no_change_cadence_deadline_and_failed_outcome_use_the_checkpoint_snapshot() {
    use forme_coordination::WorkspaceSupervisor;
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let mut spec = project(&f);
    spec.deadline = Some(now() + 10_000);
    let runtime = setup_project(&f, spec.clone());
    let route = plan(&runtime, &f, signature(), "quiet-project");
    runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    let charter = runtime
        .workspace_charter(&f.owner(), &f.config.workspace)
        .unwrap()
        .unwrap();
    let all = events(&runtime);
    let at = state.checkpoints["project:review"].at;
    assert!(WorkspaceSupervisor::reviews(&state, &charter, &all, at + 1)
        .unwrap()
        .is_empty());
    let due =
        WorkspaceSupervisor::reviews(&state, &charter, &all, spec.deadline.unwrap() + 1).unwrap();
    assert!(due[0].reasons.contains(&"deadline".into()));
    let due = WorkspaceSupervisor::reviews(&state, &charter, &all, at + 60_001).unwrap();
    assert!(due[0].reasons.contains(&"cadence".into()));
    *f.answer.lock().unwrap() = "unsupported self-report".into();
    let failed = plan(&runtime, &f, signature(), "failed-project");
    runtime
        .execute_project_route(&f.owner(), &failed.route)
        .unwrap();
    let reviews = WorkspaceSupervisor::reviews(
        &runtime.orchestration_state(&f.owner()).unwrap(),
        &charter,
        &events(&runtime),
        now() + 1,
    )
    .unwrap();
    assert_eq!(
        reviews[0].signature.verification,
        p::TaskVerification::IndependentReview
    );
    assert!(reviews[0]
        .reasons
        .contains(&"independent-review-after-failure".into()));
    *f.answer.lock().unwrap() = "[]".into();
    let before = f.calls.load(Ordering::SeqCst);
    let report = runtime.cognitive_tick_at(now() + 1000).unwrap();
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    let next = state
        .routes
        .values()
        .find(|r| !r.closed)
        .unwrap_or_else(|| panic!("{report:?}"));
    assert_eq!(next.declaration.nodes.len(), 2);
    assert!(f.calls.load(Ordering::SeqCst) >= before);
    assert!(!events(&runtime).iter().any(|e| {
        e.kind == p::EventKind::ModelCallStarted
            && (e.run_id == next.declaration.parent
                || e.run_id
                    .0
                    .starts_with(&format!("child:{}:", next.declaration.parent.0)))
    }));
    assert!(!events(&runtime)
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
}

#[test]
fn c16_dependency_failure_modes_and_retry_limits_fold_without_partial_state() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let route = plan(&runtime, &f, review_signature(), "failure-policies");
    for policy in [
        p::RouteFailurePolicy::Abort,
        p::RouteFailurePolicy::Continue,
        p::RouteFailurePolicy::Replan,
    ] {
        let mut state = runtime.orchestration_state(&f.owner()).unwrap();
        state
            .routes
            .get_mut(&route.route)
            .unwrap()
            .declaration
            .dependencies[0]
            .on_failure = policy;
        let mut apply = |operation| {
            let fact = p::OrchestrationFact::seal(
                f.config.workspace.clone(),
                p::IdempotencyKey(format!("op:{}", state.revision)),
                state.revision,
                operation,
            )
            .unwrap();
            state.apply(&fact).unwrap();
        };
        apply(p::OrchestrationOperation::NodeStarted {
            route: route.route.clone(),
            node: route.nodes[0].id.clone(),
            intent: route.nodes[0].intent.clone(),
        });
        apply(p::OrchestrationOperation::NodeFinished {
            route: route.route.clone(),
            node: route.nodes[0].id.clone(),
            status: p::WorkNodeStatus::Failed,
            consumed: 1,
            result: None,
            summary: "verified pre-effect failure".into(),
            verification: p::VerificationOutcome::Fail,
            evidence: vec![],
            effects_absent: true,
        });
        let record = &state.routes[&route.route];
        assert_eq!(record.remaining, 7);
        match policy {
            p::RouteFailurePolicy::Abort => assert_eq!(record.status, p::WorkRouteStatus::Failed),
            p::RouteFailurePolicy::Replan => {
                assert_eq!(record.status, p::WorkRouteStatus::ReplanRequested)
            }
            p::RouteFailurePolicy::Continue => {
                assert_eq!(record.runnable(), vec![route.nodes[1].id.clone()])
            }
        }
        let before = state.clone();
        let fact = p::OrchestrationFact::seal(
            f.config.workspace.clone(),
            p::IdempotencyKey("bad-repeat".into()),
            state.revision,
            p::OrchestrationOperation::NodeRetried {
                route: route.route.clone(),
                node: route.nodes[0].id.clone(),
                intent: route.nodes[0].intent.clone(),
            },
        )
        .unwrap();
        assert!(state.apply(&fact).is_err());
        assert_eq!(state, before);
    }
}

#[test]
fn c16_crashed_child_action_is_unknown_even_when_startup_restores_a_waiting_handle() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let route = plan(&runtime, &f, review_signature(), "crashed-effect");
    let spec = &route.nodes[0];
    let revision = runtime.orchestration_state(&f.owner()).unwrap().revision;
    raw_fact(
        &runtime,
        "crash-node",
        revision,
        p::OrchestrationOperation::NodeStarted {
            route: route.route.clone(),
            node: spec.id.clone(),
            intent: spec.intent.clone(),
        },
    )
    .unwrap();
    let run = route.node_run_id(&spec.id, 1);
    for (id, payload) in [
        (
            "accepted",
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::Subagent,
                session_ref: p::SessionId("session:crash-child".into()),
                input_ref: p::InputRef("self-authored recovery fixture".into()),
                idempotency_key: None,
            }),
        ),
        (
            "started",
            p::EventPayload::ActionStarted(p::ActionStartedPayload {
                intent_id: p::ActionId("crashed-child-file".into()),
                backend: p::BackendKind::File,
                scope: spec.scope.clone(),
                remote_lease: None,
            }),
        ),
    ] {
        runtime
            .services()
            .store
            .append(p::Event::new(
                p::EventId(format!("crash:{id}")),
                run.clone(),
                None,
                payload,
                p::SchemaVersion(1),
                now(),
                p::Provenance {
                    source: p::Source::Subagent,
                    actor: p::Actor::System,
                    trust_tier: p::TrustTier::VerifiedProcess,
                    caused_by: None,
                },
            ))
            .unwrap();
    }
    drop(runtime);
    for _ in 0..2 {
        let runtime = f.build();
        let out = runtime
            .execute_project_route(&f.owner(), &route.route)
            .unwrap();
        assert_eq!(out.status, p::WorkRouteStatus::OutcomeUnknown);
        assert_eq!(out.nodes[&spec.id].reserved, spec.budget_units);
        assert!(!out.checkpointed);
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            events(&runtime)
                .iter()
                .filter(|e| e.kind == p::EventKind::ActionOutcomeUnknown)
                .count(),
            1
        );
        assert!(runtime
            .retry_project_node(
                &f.owner(),
                route.route.clone(),
                spec.id.clone(),
                p::ActionId("unsafe-retry".into()),
                p::IdempotencyKey("unsafe-retry".into())
            )
            .is_err());
        export_artifact(
            &runtime,
            forme_eval::CoreBrainCCase::Recovery,
            "recovery.json",
            vec![],
        );
    }
}

#[test]
fn c16_checkpoint_is_required_and_cancellation_cannot_resurrect_failed_nodes() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let route = plan(&runtime, &f, signature(), "checkpoint-boundary");
    let mut ledger = runtime.orchestration_state(&f.owner()).unwrap();
    let apply = |state: &mut p::OrchestrationLedger, operation| {
        let fact = p::OrchestrationFact::seal(
            f.config.workspace.clone(),
            p::IdempotencyKey(format!("fold:{}", state.revision)),
            state.revision,
            operation,
        )
        .unwrap();
        state.apply(&fact)
    };
    apply(
        &mut ledger,
        p::OrchestrationOperation::NodeStarted {
            route: route.route.clone(),
            node: "main".into(),
            intent: route.main.intent.clone(),
        },
    )
    .unwrap();
    apply(
        &mut ledger,
        p::OrchestrationOperation::NodeFinished {
            route: route.route.clone(),
            node: "main".into(),
            status: p::WorkNodeStatus::Failed,
            consumed: 0,
            result: None,
            summary: "pre-effect failure".into(),
            verification: p::VerificationOutcome::Fail,
            evidence: vec![],
            effects_absent: true,
        },
    )
    .unwrap();
    apply(
        &mut ledger,
        p::OrchestrationOperation::RouteCancelled {
            route: route.route.clone(),
        },
    )
    .unwrap();
    let before = ledger.clone();
    assert!(apply(
        &mut ledger,
        p::OrchestrationOperation::NodeRetried {
            route: route.route.clone(),
            node: "main".into(),
            intent: p::ActionId("new-but-cancelled".into())
        }
    )
    .is_err());
    assert_eq!(ledger, before);
    apply(
        &mut ledger,
        p::OrchestrationOperation::RouteClosed {
            route: route.route.clone(),
            status: p::WorkRouteStatus::Cancelled,
        },
    )
    .unwrap();
    let mut next = route.clone();
    next.route = p::ExecutionRouteRef("route:next".into());
    next.parent = p::RunId("run:next".into());
    assert!(apply(
        &mut ledger,
        p::OrchestrationOperation::RouteDeclared(Box::new(next.clone()))
    )
    .is_err());
    apply(
        &mut ledger,
        p::OrchestrationOperation::Checkpoint {
            project: route.project.clone(),
            route: route.route.clone(),
            input_digest: route.input_digest.clone(),
            at: now(),
        },
    )
    .unwrap();
    apply(
        &mut ledger,
        p::OrchestrationOperation::RouteDeclared(Box::new(next)),
    )
    .unwrap();
}

#[test]
fn c13_direct_project_cannot_borrow_the_owner_brains_unassigned_file_capability() {
    let f = file_fixture();
    let runtime = setup(&f); // File exists in composition, but this project has no file grant.
    let route = plan(&runtime, &f, signature(), "direct-unassigned");
    let path = f.config.data.artifact_root.join("unauthorized.txt");
    f.responses
        .lock()
        .unwrap()
        .push_back(file_call(path.to_str().unwrap()));
    let out = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_ne!(out.status, p::WorkRouteStatus::Completed);
    assert!(!path.exists());
    assert!(!events(&runtime).iter().any(|e| matches!(
        e.kind,
        p::EventKind::ActionStarted | p::EventKind::ApprovalRequested
    )));
}

#[test]
fn c15_unmatched_signature_executes_a_low_confidence_custom_review_route_without_activation() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let runtime = setup(&f);
    let mut sig = signature();
    sig.partition = p::TaskPartition::Parallel;
    sig.dependency = p::TaskDependency::Interdependent;
    sig.verification = p::TaskVerification::Environment;
    sig.extent = p::TaskExtent::LongRunning;
    sig.reversibility = p::TaskReversibility::HighRiskIrreversible;
    let route = plan(&runtime, &f, sig, "custom-route");
    assert!(route.pattern.is_none() && route.confidence_millis <= 250 && route.checkpoint_required);
    assert!(route.nodes.iter().any(|n| n.role.0 == "evaluator"));
    let out = runtime
        .execute_project_route(&f.owner(), &route.route)
        .unwrap();
    assert_eq!(out.status, p::WorkRouteStatus::Completed);
    assert!(out.checkpointed);
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
    assert!(!events(&runtime).iter().any(|e| matches!(
        e.kind,
        p::EventKind::CandidatePromoted | p::EventKind::StrategyActivated
    )));
    assert!(events(&runtime).iter().any(|e|matches!(&e.payload,p::EventPayload::CandidateCreated(c) if c.candidate_id.0==format!("route-strategy:{}",route.route.0))));
}

#[test]
fn c17_late_evidence_stays_out_of_the_current_review_baseline_and_is_seen_next_time() {
    let f = Fixture::new();
    *f.answer.lock().unwrap() = "ground-truth".into();
    let runtime = setup(&f);
    let first = plan(&runtime, &f, signature(), "snapshot-first");
    runtime
        .execute_project_route(&f.owner(), &first.route)
        .unwrap();
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    let mut updated = state.projects["project:review"].clone();
    updated.signature = review_signature();
    runtime
        .revise_project(
            &f.owner(),
            p::IdempotencyKey("snapshot-revise".into()),
            updated.clone(),
            p::canonical_digest(&project(&f)).unwrap(),
        )
        .unwrap();
    let charter = runtime
        .workspace_charter(&f.owner(), &f.config.workspace)
        .unwrap()
        .unwrap();
    let at = now() + 1000;
    let expected = forme_coordination::WorkspaceSupervisor::input_digest(
        &updated,
        &charter,
        &events(&runtime),
        at,
    )
    .unwrap();
    *f.answer.lock().unwrap() = "[]".into();
    f.delay_ms.store(250, Ordering::SeqCst);
    let before = f.calls.load(Ordering::SeqCst);
    std::thread::scope(|scope| {
        let handle = scope.spawn(|| runtime.cognitive_tick_at(at));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while f.calls.load(Ordering::SeqCst) == before && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            f.calls.load(Ordering::SeqCst) > before,
            "tick must have fixed its snapshot before injection"
        );
        runtime
            .services()
            .store
            .append(p::Event::new(
                p::EventId("late:evidence".into()),
                p::RunId("owner:late-evidence".into()),
                None,
                p::EventPayload::ObservationRecorded(p::ObservationRecordedPayload {
                    source: p::Source::UserTurn,
                    scope: updated.scope.clone(),
                    grant_ref: None,
                    learning_reply: None,
                }),
                p::SchemaVersion(1),
                now(),
                p::Provenance {
                    source: p::Source::UserTurn,
                    actor: p::Actor::Owner,
                    trust_tier: p::TrustTier::OwnerInput,
                    caused_by: None,
                },
            ))
            .unwrap();
        let report = handle.join().unwrap().unwrap();
        assert!(matches!(report,CognitiveTickOutcome::Ran(r) if !r.failed()));
    });
    f.delay_ms.store(0, Ordering::SeqCst);
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    let next = state.routes.values().find(|r| !r.closed).unwrap();
    assert_eq!(next.declaration.review_digest, expected);
    assert_ne!(
        next.declaration.review_digest,
        forme_coordination::WorkspaceSupervisor::input_digest(
            &updated,
            &charter,
            &events(&runtime),
            at
        )
        .unwrap()
    );
    *f.answer.lock().unwrap() = "ground-truth".into();
    runtime
        .execute_project_route(&f.owner(), &next.declaration.route)
        .unwrap();
    let reviews = forme_coordination::WorkspaceSupervisor::reviews(
        &runtime.orchestration_state(&f.owner()).unwrap(),
        &charter,
        &events(&runtime),
        at + 1,
    )
    .unwrap();
    assert!(reviews.iter().any(|r| r
        .reasons
        .contains(&"project-charter-or-evidence-diff".into())));
}
