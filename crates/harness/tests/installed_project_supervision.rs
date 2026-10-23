#[path = "support/core_brain.rs"]
mod support;
use forme_harness::*;
use forme_protocol as p;
use forme_store::EventStore;
use std::sync::atomic::Ordering;
use support::*;

fn project(f: &Fixture, id: &str) -> p::WorkspaceProject {
    p::WorkspaceProject {
        schema_version: p::SchemaVersion(1),
        reference: id.into(),
        workspace: f.config.workspace.clone(),
        goal: p::GoalRef(format!("goal:{id}")),
        done: p::DoneContractRef(format!("done:{id}")),
        summary: "review project ground-truth".into(),
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
    let runtime = f.build();
    let charter = p::WorkspaceCharterRecord {
        schema_version: p::SchemaVersion(1),
        workspace: f.config.workspace.clone(),
        version: 0,
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
        .apply_workspace_charter(&f.owner(), p::IdempotencyKey("charter".into()), change)
        .unwrap();
    runtime
        .register_project(
            &f.owner(),
            p::IdempotencyKey("register".into()),
            project(f, "project:one"),
        )
        .unwrap();
    runtime
}
fn control(f: &Fixture, key: &str, version: u64, enabled: bool) -> p::ProjectSupervisionCommand {
    p::ProjectSupervisionCommand {
        schema_version: p::SchemaVersion(1),
        command: p::IdempotencyKey(key.into()),
        project: "project:one".into(),
        expected_project_digest: p::canonical_digest(&project(f, "project:one")).unwrap(),
        expected_version: version,
        enabled,
    }
}
fn head(runtime: &ProductionRuntime) -> u64 {
    runtime
        .services()
        .store
        .run_head(&runtime.services().aggregates.charter)
        .unwrap()
}

#[test]
fn supervision_survives_restart_skips_background_and_resume_is_not_execution() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let before = head(&runtime);
    let snapshot = runtime.workspace_control_snapshot(&f.owner()).unwrap();
    assert_eq!(head(&runtime), before);
    assert_eq!(snapshot.projects[0].review, p::ProjectReviewStatus::Due);
    assert_eq!(
        snapshot.projects[0].supervision,
        p::ProjectSupervisionState::default()
    );
    let pause = control(&f, "pause", 0, false);
    let receipt = runtime
        .control_project_supervision(&f.owner(), pause.clone())
        .unwrap();
    assert_eq!(head(&runtime), before + 1);
    assert_eq!(
        receipt,
        runtime
            .control_project_supervision(&f.owner(), pause.clone())
            .unwrap()
    );
    assert_eq!(head(&runtime), before + 1);
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    let mut replay = p::OrchestrationLedger::default();
    for event in events(&runtime) {
        replay.fold(&event).unwrap();
    }
    assert_eq!(replay, state);
    drop(runtime);
    let runtime = f.build();
    assert_eq!(runtime.orchestration_state(&f.owner()).unwrap(), state);
    assert_eq!(
        runtime
            .workspace_control_snapshot(&f.owner())
            .unwrap()
            .projects[0]
            .review,
        p::ProjectReviewStatus::Paused
    );
    runtime.cognitive_tick_at(now() + 1000).unwrap();
    assert!(runtime
        .orchestration_state(&f.owner())
        .unwrap()
        .routes
        .is_empty());
    let calls = f.calls.load(Ordering::SeqCst);
    let resumed = runtime
        .control_project_supervision(&f.owner(), control(&f, "resume", 1, true))
        .unwrap();
    assert_eq!(resumed.status, p::WorkbenchCommandStatus::Confirmed);
    assert_eq!(f.calls.load(Ordering::SeqCst), calls);
    assert!(runtime
        .orchestration_state(&f.owner())
        .unwrap()
        .routes
        .is_empty());
    runtime.cognitive_tick_at(now() + 65_000).unwrap();
    assert_eq!(
        runtime
            .orchestration_state(&f.owner())
            .unwrap()
            .routes
            .len(),
        1
    );
    assert!(!events(&runtime)
        .iter()
        .any(|e| e.kind == p::EventKind::ActionStarted));
}

#[test]
fn supervision_owner_digest_version_and_command_binding_reject_without_events() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let before = head(&runtime);
    let command = control(&f, "bound", 0, false);
    assert!(runtime
        .control_project_supervision(&p::VerifiedPrincipal("other".into()), command.clone())
        .is_err());
    let mut invalid = command.clone();
    invalid.expected_version = 9;
    assert!(runtime
        .control_project_supervision(&f.owner(), invalid)
        .is_err());
    let mut invalid = command.clone();
    invalid.expected_project_digest = p::SchemaDigest("stale".into());
    assert!(runtime
        .control_project_supervision(&f.owner(), invalid)
        .is_err());
    assert_eq!(head(&runtime), before);
    let original = runtime
        .control_project_supervision(&f.owner(), command.clone())
        .unwrap();
    let after = head(&runtime);
    let mut changed = command.clone();
    changed.enabled = true;
    assert!(runtime
        .control_project_supervision(&f.owner(), changed)
        .is_err());
    assert!(runtime
        .control_project_supervision(&f.owner(), control(&f, "stale", 0, true))
        .is_err());
    let mut revised = project(&f, "project:one");
    revised.summary = "updated future segment".into();
    runtime
        .revise_project(
            &f.owner(),
            p::IdempotencyKey("revision".into()),
            revised,
            command.expected_project_digest.clone(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .control_project_supervision(&f.owner(), command)
            .unwrap(),
        original
    );
    assert_eq!(head(&runtime), after + 1);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn supervision_concurrent_cas_has_one_winner_and_does_not_cancel_existing_route() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let spec = project(&f, "project:one");
    let route = runtime
        .plan_project_route(
            &f.owner(),
            p::IdempotencyKey("explicit".into()),
            &spec.reference,
            spec.signature.clone(),
            p::canonical_digest(&"input").unwrap(),
            now(),
        )
        .unwrap();
    let before = runtime.orchestration_state(&f.owner()).unwrap().routes;
    let at = head(&runtime);
    let owner = f.owner();
    let a = control(&f, "concurrent-a", 0, false);
    let b = control(&f, "concurrent-b", 0, true);
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| runtime.control_project_supervision(&owner, a));
        let second = scope.spawn(|| runtime.control_project_supervision(&owner, b));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(head(&runtime), at + 1);
    let state = runtime.orchestration_state(&f.owner()).unwrap();
    assert_eq!(state.routes, before);
    assert!(!state.routes[&route.route].closed);
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn paused_project_allows_explicit_owner_plan_and_large_review_is_honest() {
    let f = Fixture::new();
    let runtime = setup(&f);
    runtime
        .control_project_supervision(&f.owner(), control(&f, "pause", 0, false))
        .unwrap();
    let spec = project(&f, "project:one");
    runtime
        .plan_project_route(
            &f.owner(),
            p::IdempotencyKey("owner-after-pause".into()),
            &spec.reference,
            spec.signature,
            p::canonical_digest(&"explicit input").unwrap(),
            now(),
        )
        .unwrap();
    for index in 0..18 {
        let id = format!("project:extra-{index:02}");
        runtime
            .register_project(
                &f.owner(),
                p::IdempotencyKey(format!("register-{index}")),
                project(&f, &id),
            )
            .unwrap();
    }
    let before = head(&runtime);
    let view = runtime.workspace_control_snapshot(&f.owner()).unwrap();
    assert_eq!(head(&runtime), before);
    assert_eq!(
        view.projects
            .iter()
            .filter(|v| v.review == p::ProjectReviewStatus::Due)
            .count(),
        16
    );
    assert_eq!(
        view.projects
            .iter()
            .filter(|v| v.review == p::ProjectReviewStatus::Unverifiable)
            .count(),
        2
    );
    assert!(!view
        .projects
        .iter()
        .any(|v| v.review == p::ProjectReviewStatus::NotDue));
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn old_ledger_encoding_digest_and_version_zero_are_preserved() {
    let old = r#"{"schema_version":1,"revision":0,"projects":{},"routes":{},"checkpoints":{},"commands":{}}"#;
    let decoded: p::OrchestrationLedger = serde_json::from_str(old).unwrap();
    assert_eq!(serde_json::to_string(&decoded).unwrap(), old);
    assert_eq!(
        p::canonical_digest(&decoded).unwrap().0,
        "sha256:78b515d62be17d122b74c051328200b9bd1ca3c7964d34db220b674a9cd8b6fd"
    );
    assert_eq!(
        decoded.project_supervision("historical"),
        p::ProjectSupervisionState::default()
    );
}

#[test]
fn store_rejects_untrusted_supervision_provenance_without_a_projection_change() {
    let f = Fixture::new();
    let runtime = setup(&f);
    let before = runtime.orchestration_state(&f.owner()).unwrap();
    let fact = p::OrchestrationFact::seal(
        f.config.workspace.clone(),
        p::IdempotencyKey("forged".into()),
        before.revision,
        p::OrchestrationOperation::ProjectSupervisionChanged {
            project: "project:one".into(),
            expected_project_digest: p::canonical_digest(&project(&f, "project:one")).unwrap(),
            expected_version: 0,
            enabled: false,
        },
    )
    .unwrap();
    let after = head(&runtime);
    for (index, trust) in [p::TrustTier::Untrusted, p::TrustTier::VerifiedProcess]
        .into_iter()
        .enumerate()
    {
        let forged = p::Event::new(
            p::EventId(format!("forged:{index}")),
            runtime.services().aggregates.charter.clone(),
            None,
            p::EventPayload::DecisionTraceRecorded(p::DecisionTraceRecordedPayload {
                trace_ref: p::DecisionTraceRef("trace:forged".into()),
                refs: p::DecisionRefs {
                    map: None,
                    user: None,
                    agent_self: None,
                    trust: None,
                    failure: vec![],
                },
                rationale: p::Rationale("forged control".into()),
                workspace_snapshot: p::AgentWorkspaceSnapshotRef("snapshot:forged".into()),
                resource_graph_snapshot: None,
                evolution_snapshot: None,
                federation_snapshot: None,
                orchestration: Some(fact.clone()),
            }),
            p::SchemaVersion(1),
            now(),
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: trust,
                caused_by: None,
            },
        );
        assert!(runtime.services().store.append(forged).is_err());
    }
    assert_eq!(head(&runtime), after);
    assert_eq!(runtime.orchestration_state(&f.owner()).unwrap(), before);
}

#[test]
fn concurrent_pause_and_background_tick_never_append_an_automatic_route_after_pause() {
    let f = Fixture::new();
    let runtime = setup(&f);
    for i in 0..7 {
        runtime
            .register_project(
                &f.owner(),
                p::IdempotencyKey(format!("race-register-{i}")),
                project(&f, &format!("project:race-{i}")),
            )
            .unwrap();
    }
    let projects = runtime.orchestration_state(&f.owner()).unwrap().projects;
    let barrier = std::sync::Barrier::new(2);
    let at = now() + 1000;
    std::thread::scope(|scope| {
        let tick = scope.spawn(|| {
            barrier.wait();
            runtime.cognitive_tick_at(at)
        });
        barrier.wait();
        for spec in projects.values() {
            runtime
                .control_project_supervision(
                    &f.owner(),
                    p::ProjectSupervisionCommand {
                        schema_version: p::SchemaVersion(1),
                        command: p::IdempotencyKey(format!("race-pause:{}", spec.reference)),
                        project: spec.reference.clone(),
                        expected_project_digest: p::canonical_digest(spec).unwrap(),
                        expected_version: 0,
                        enabled: false,
                    },
                )
                .unwrap();
        }
        tick.join().unwrap().unwrap();
    });
    let mut paused = std::collections::BTreeSet::new();
    for event in runtime
        .services()
        .store
        .read_run(runtime.services().aggregates.charter.clone())
    {
        if let p::EventPayload::DecisionTraceRecorded(payload) = event.unwrap().payload {
            if let Some(fact) = payload.orchestration {
                match fact.operation {
                    p::OrchestrationOperation::ProjectSupervisionChanged {
                        project,
                        enabled: false,
                        ..
                    } => {
                        paused.insert(project);
                    }
                    p::OrchestrationOperation::RouteDeclared(route) => {
                        assert!(!paused.contains(&route.project))
                    }
                    _ => {}
                }
            }
        }
    }
    assert_eq!(paused.len(), 8);
    let before = runtime.orchestration_state(&f.owner()).unwrap().routes;
    runtime.cognitive_tick_at(at + 65_000).unwrap();
    assert_eq!(
        runtime.orchestration_state(&f.owner()).unwrap().routes,
        before
    );
    assert!(!events(&runtime)
        .iter()
        .any(|event| event.kind == p::EventKind::ActionStarted));
}
