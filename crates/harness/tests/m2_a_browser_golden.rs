#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forme_approval::{ApprovalGrant, GrantScope};
use forme_capabilities::{InMemorySelectionPolicyRegistry, SelectionEvidenceSet};
use forme_coordination::InMemoryCoordinationRegistry;
use forme_eval::{
    portable_replay_from_events, M3ArtifactStore, M3CArtifactStore, M3CControlEventTrace,
    M3CGoldenArtifactBody, M3CGovernedActionTrace, M3CGovernedRunTrace, TraceManifest,
};
use forme_execution::{
    BrowserBackend, ExecutionBackendRegistry, FileArtifactStore, HeadlessChromeDriver,
    OutputBudget, RejectingContentResolver, RejectingSecretResolver,
};
use forme_harness::{
    AgentHarness, GovernanceConfig, HarnessConfig, M3DomainRuntime, M3EvolutionHarness,
    M3RuntimeCompatibility, ReactiveHarness, ResumeInput,
};
use forme_loop::InMemoryLoopRegistry;
use forme_models::{
    Cost, InMemoryModelAdaptationRegistry, ModelCapability, ModelOutput, ModelProfile,
    ModelResponse, ModelStrength, ModelToolCall, RateLimit, ScriptedModelProvider, Url,
};
use forme_policy::{
    ActionMatcher, ArgMatcher, DelegationGrant, DelegationSubject, PolicyLayer, PolicyLayerSource,
    PolicyRule,
};
use forme_protocol as p;
use forme_store::{
    EventStore, EvolutionEventStore, EvolutionProjection, SqliteEventStore, StoreOptions,
};

const CASE_REF: &str = "case:m2-a-real-browser-mutation";
const EVAL_REF: &str = "eval:m2-a-real-browser-mutation";
const SNAPSHOT_REF: &str = "snapshot:m2-a-real-browser-mutation:v1";

#[test]
#[ignore = "requires an installed Chrome or Edge executable and launches a real browser process"]
fn s38_real_browser_golden_runs_harness_approval_and_observes_server_mutation() {
    let fixture = LoopbackFixture::start();
    let root = TestRoot::new("real-browser-golden");
    let executable = find_browser_executable().expect("Chrome or Edge is required for M2-A");
    let timeout = p::DurationMs(20_000);
    let driver = Arc::new(
        HeadlessChromeDriver::new(
            executable,
            Arc::new(RejectingContentResolver),
            Arc::new(FileArtifactStore::new(root.path.join("artifacts")).unwrap()),
            timeout,
        )
        .unwrap(),
    );
    let backend = BrowserBackend::new(
        p::ProviderId("driver:headless-chrome".into()),
        driver,
        Arc::new(RejectingSecretResolver),
        OutputBudget::truncate_at(4_096),
        timeout,
    )
    .unwrap();
    let registry = Arc::new(ExecutionBackendRegistry::default());
    registry.register(Arc::new(backend)).unwrap();
    let intent = browser_intent(&fixture);
    let governance = browser_governance(&intent, fixture.origin());
    let profile = model_profile();
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();
    let artifact_store = store.clone();
    let harness = ReactiveHarness::new(
        store,
        Arc::new(
            ScriptedModelProvider::new(
                profile.clone(),
                vec![
                    tool_response(intent),
                    final_response("browser mutation verified"),
                ],
            )
            .unwrap(),
        ),
        registry,
        governance,
        HarnessConfig::for_model(&profile),
    )
    .unwrap();

    let run = harness.submit_run(run_request()).unwrap();
    assert_eq!(fixture.mutations(), 0);
    let pending = harness
        .pending_approvals(p::SessionId("session:m2-a-browser-golden".into()))
        .unwrap();
    assert_eq!(pending.len(), 1);
    harness
        .resume(
            run.clone(),
            ResumeInput::Approval(one_shot_grant(&pending[0])),
        )
        .unwrap();
    assert_eq!(
        harness.wait(run.clone()).unwrap().status,
        p::RunStatus::Complete
    );
    wait_for_mutation(&fixture);
    assert_eq!(fixture.mutations(), 1);

    let events = harness.stream_events(run.clone()).events();
    assert_event_order(
        &events,
        &[
            p::EventKind::ToolCallProposed,
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::ApprovalRequested,
            p::EventKind::RunWaiting,
            p::EventKind::ApprovalResolved,
            p::EventKind::RunResumed,
            p::EventKind::ToolPolicyEvaluated,
            p::EventKind::CompetenceGateEvaluated,
            p::EventKind::ActionPlanned,
            p::EventKind::ActionStarted,
            p::EventKind::ActionOutputDelta,
            p::EventKind::ActionCompleted,
            p::EventKind::CapabilityEvidenceRecorded,
            p::EventKind::VerificationStarted,
            p::EventKind::VerificationFinished,
            p::EventKind::RunComplete,
        ],
    );
    let planned = events
        .iter()
        .find_map(|event| match &event.payload {
            p::EventPayload::ActionPlanned(payload) => Some(payload),
            _ => None,
        })
        .unwrap();
    assert_eq!(planned.backend, p::BackendKind::Browser);
    assert!(planned.approval_ref.is_some());
    assert!(planned.plan_digest.0.starts_with("sha256:"));
    let output = events
        .iter()
        .find(|event| event.kind == p::EventKind::ActionOutputDelta)
        .unwrap();
    assert_eq!(output.provenance.trust_tier, p::TrustTier::Untrusted);
    let p::EventPayload::ActionOutputDelta(output_payload) = &output.payload else {
        unreachable!()
    };
    assert_eq!(output_payload.trust, p::TrustTier::Untrusted);
    let completion = events
        .iter()
        .find_map(|event| match &event.payload {
            p::EventPayload::ActionCompleted(payload) => Some((event, payload)),
            _ => None,
        })
        .unwrap();
    assert_eq!(completion.0.provenance.trust_tier, p::TrustTier::Untrusted);
    let receipt = completion.1.receipt.as_ref().unwrap();
    assert_eq!(receipt.effect, p::EffectStatus::Committed);
    assert_eq!(receipt.trust, p::TrustTier::Untrusted);
    for forbidden in [
        p::EventKind::ActionOutcomeUnknown,
        p::EventKind::CandidateCreated,
        p::EventKind::CandidatePromoted,
        p::EventKind::MemoryNodeAppended,
        p::EventKind::MemoryEdgeAppended,
    ] {
        assert!(!events.iter().any(|event| event.kind == forbidden));
    }

    if let Some(output_dir) = std::env::var_os("FORME_M2_GOLDEN_ARTIFACT_DIR") {
        write_offline_artifacts(Path::new(&output_dir), &run, &events, planned, receipt);
    }
    if let Some(output_dir) = std::env::var_os("FORME_M3_A_ARTIFACT_DIR") {
        write_m3_a_artifacts(Path::new(&output_dir), artifact_store, &run, &events).unwrap();
    }
}

#[test]
#[ignore = "requires an installed Chrome or Edge executable and launches a real browser process"]
fn s68_m3_candidate_live_v2_regression_rollback_and_live_v1_are_governed() {
    let fixture = LoopbackFixture::start();
    let root = TestRoot::new("m3-c-real-browser-golden");
    let executable = find_browser_executable().expect("Chrome or Edge is required for M3-C");
    let timeout = p::DurationMs(20_000);
    let driver = Arc::new(
        HeadlessChromeDriver::new(
            executable,
            Arc::new(RejectingContentResolver),
            Arc::new(FileArtifactStore::new(root.path.join("artifacts")).unwrap()),
            timeout,
        )
        .unwrap(),
    );
    let backend = BrowserBackend::new(
        p::ProviderId("driver:headless-chrome".into()),
        driver,
        Arc::new(RejectingSecretResolver),
        OutputBudget::truncate_at(4_096),
        timeout,
    )
    .unwrap();
    let registry = Arc::new(ExecutionBackendRegistry::default());
    registry.register(Arc::new(backend)).unwrap();
    let baseline_intent = m3_c_browser_intent(&fixture, "baseline");
    let v2_intent = m3_c_browser_intent(&fixture, "active-v2");
    let restored_intent = m3_c_browser_intent(&fixture, "restored-v1");
    let governance = browser_governance(&baseline_intent, fixture.origin());
    let profile = model_profile();
    let mut config = HarnessConfig::for_model(&profile);
    config.workspace = p::WorkspaceRef("workspace:m2-a-browser-golden".into());
    let store = SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap();

    let baseline_harness = ReactiveHarness::new(
        store.clone(),
        Arc::new(
            ScriptedModelProvider::new(
                profile.clone(),
                vec![
                    m3_c_tool_response(baseline_intent, "baseline"),
                    final_response("M3-C baseline browser mutation verified"),
                ],
            )
            .unwrap(),
        ),
        registry.clone(),
        governance.clone(),
        config.clone(),
    )
    .unwrap();
    let (baseline_live, baseline_events) = run_m3_c_browser(
        &baseline_harness,
        &fixture,
        m3_c_run_request("baseline"),
        None,
        1,
    );

    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace".into());
    let seed_snapshot = seed_m3_browser_v1(&store, &baseline_live.run, &aggregate).unwrap();
    let baseline_replay = portable_replay_from_events(
        m3_replay_request(
            p::ReplayBundleRef("bundle:m3-c:real-browser:baseline".into()),
            vec![baseline_live.run.clone()],
            baseline_live.event_refs.clone(),
            seed_snapshot,
        ),
        &baseline_events,
    )
    .unwrap();
    let replay_report =
        forme_eval::DeterministicReplayEngine::<SqliteEventStore>::exact_portable(&baseline_replay)
            .unwrap();
    assert!(replay_report.projections_match);
    assert_eq!(replay_report.effect_calls, 0);

    let candidate = m3_browser_candidate(2, "loop:m3-a-browser:v1", p::EvolutionImpact::Bounded);
    let control = M3EvolutionHarness::new(store.clone());
    let candidate_run = p::RunId("run:m3-c:candidate-v2".into());
    let evaluation_run = p::RunId("run:m3-c:evaluation-v2".into());
    let promotion_run = p::RunId("run:m3-c:promotion-v2".into());
    let activation_run = p::RunId("run:m3-c:activation-v2".into());
    let regression_run = p::RunId("run:m3-c:regression-v2".into());
    control
        .record_candidate(candidate_run.clone(), candidate.clone())
        .unwrap();
    let evaluated = control
        .evaluate(
            evaluation_run.clone(),
            &candidate,
            m3_browser_comparison(&candidate, baseline_replay.manifest.bundle.clone()),
        )
        .unwrap();
    let promotion = control
        .promote(promotion_run.clone(), &candidate, &evaluated.evaluation)
        .unwrap();
    let activated = control
        .activate(
            activation_run.clone(),
            aggregate.clone(),
            &candidate,
            &evaluated.evaluation,
            promotion,
            Some(p::OwnerControlRef("owner-control:m3-c-browser:v2".into())),
        )
        .unwrap();

    let governed_harness = ReactiveHarness::new(
        store.clone(),
        Arc::new(
            ScriptedModelProvider::new(
                profile.clone(),
                vec![
                    m3_c_tool_response(v2_intent, "active-v2"),
                    final_response("M3-C active v2 browser mutation verified"),
                    m3_c_tool_response(restored_intent, "restored-v1"),
                    final_response("M3-C restored v1 browser mutation verified"),
                ],
            )
            .unwrap(),
        ),
        registry,
        governance,
        config,
    )
    .unwrap()
    .with_m3_domain_runtime(m3_c_domain_runtime(&profile));
    let (live_v2, live_v2_events) = run_m3_c_browser(
        &governed_harness,
        &fixture,
        m3_c_run_request("active-v2"),
        Some(candidate.proposed_version.clone()),
        2,
    );
    assert_eq!(
        live_v2.session_bound.evolution_snapshot,
        Some(activated.snapshot.snapshot.clone())
    );

    let regression_evidence = p::EvidenceRef("evidence:m3-a-browser-regression-injected".into());
    store
        .append(m3_control_event(
            p::EventId("event:m3-c:regression-failure-v2".into()),
            regression_run.clone(),
            p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
                failure_ref: p::FailureEvidenceRef(regression_evidence.0.clone()),
                class: p::FailureClass::VerificationFailure,
                impact: p::Impact::High,
                scope: candidate.scope.clone(),
                related_refs: vec![p::EvidenceRef(candidate.proposed_version.0.clone())],
                suggested_fix: Some(p::SuggestedFixRef(
                    "restore the repository-owned known-good strategy".into(),
                )),
            }),
        ))
        .unwrap();
    let regression = control
        .evaluate(
            regression_run.clone(),
            &candidate,
            m3_browser_regression_comparison(&candidate, baseline_replay.manifest.bundle.clone()),
        )
        .unwrap();
    assert_eq!(
        regression.decision,
        forme_cognition::EvolutionDecision::Rollback
    );
    let rolled_back = control
        .rollback(
            regression_run.clone(),
            aggregate,
            p::StrategyDomain::Loop,
            candidate.scope.clone(),
            candidate.baseline.clone(),
            vec![regression_evidence],
            p::InFlightDisposition::KeepPinned,
            None,
        )
        .unwrap();
    let (restored_v1, restored_v1_events) = run_m3_c_browser(
        &governed_harness,
        &fixture,
        m3_c_run_request("restored-v1"),
        Some(candidate.baseline.clone()),
        3,
    );
    assert_eq!(
        restored_v1.session_bound.evolution_snapshot,
        Some(rolled_back.snapshot.snapshot.clone())
    );

    let candidate_events = read_run_events(&store, &candidate_run);
    let evaluation_events = read_run_events(&store, &evaluation_run);
    let promotion_events = read_run_events(&store, &promotion_run);
    let activation_events = read_run_events(&store, &activation_run);
    let regression_events = read_run_events(&store, &regression_run);
    let control_events = vec![
        control_trace(&candidate_events, p::EventKind::CandidateCreated),
        control_trace(
            &evaluation_events,
            p::EventKind::EvolutionEvaluationRecorded,
        ),
        control_trace(&promotion_events, p::EventKind::CandidatePromoted),
        control_trace(&activation_events, p::EventKind::StrategyActivated),
        control_trace(&regression_events, p::EventKind::FailureEvidenceRecorded),
        control_trace(
            &regression_events,
            p::EventKind::EvolutionEvaluationRecorded,
        ),
        control_trace(&regression_events, p::EventKind::StrategyRolledBack),
    ];
    let mut all_events = baseline_events;
    all_events.extend(candidate_events);
    all_events.extend(evaluation_events);
    all_events.extend(promotion_events);
    all_events.extend(activation_events);
    all_events.extend(live_v2_events);
    all_events.extend(regression_events);
    all_events.extend(restored_v1_events);
    let mut source_runs = all_events
        .iter()
        .map(|event| event.run_id.clone())
        .collect::<Vec<_>>();
    source_runs.sort();
    source_runs.dedup();
    let lineage = portable_replay_from_events(
        m3_replay_request(
            p::ReplayBundleRef("bundle:m3-c:real-browser:lineage".into()),
            source_runs,
            all_events
                .iter()
                .map(|event| event.event_id.clone())
                .collect(),
            rolled_back.snapshot.clone(),
        ),
        &all_events,
    )
    .unwrap();
    let final_active = store
        .active(p::StrategyDomain::Loop, candidate.scope.clone())
        .unwrap()
        .unwrap();
    let body = M3CGoldenArtifactBody {
        schema_version: p::SchemaVersion(1),
        case_ref: p::EvaluationCaseRef("case:m3-c-real-browser-evolution".into()),
        scope: candidate.scope.clone(),
        baseline_replay,
        baseline_live,
        candidate,
        evaluation: evaluated.evaluation,
        regression_evaluation: regression.evaluation,
        control_events,
        activated_snapshot: activated.snapshot,
        live_v2,
        rolled_back_snapshot: rolled_back.snapshot,
        restored_v1,
        final_active,
        lineage,
        event_order: all_events
            .iter()
            .map(|event| event.event_id.clone())
            .collect(),
        event_kinds: all_events.iter().map(|event| event.kind).collect(),
        external_mutation_count: fixture.mutations() as u32,
    };
    let output = std::env::var_os("FORME_M3_C_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.path.join("m3-c-artifact"));
    let receipt = M3CArtifactStore::new(output)
        .and_then(|writer| writer.write(&body))
        .unwrap();
    println!(
        "M3-C artifact {} {}",
        receipt.digest.0,
        receipt.path.display()
    );
}

fn run_m3_c_browser(
    harness: &ReactiveHarness,
    fixture: &LoopbackFixture,
    request: p::RunRequest,
    expected_strategy: Option<p::StrategyVersionRef>,
    mutation_ordinal: u32,
) -> (M3CGovernedRunTrace, Vec<p::Event>) {
    let run = harness.submit_run(request).unwrap();
    let pending = harness
        .pending_approvals(p::SessionId("session:m2-a-browser-golden".into()))
        .unwrap();
    assert_eq!(pending.len(), 1);
    let grant = one_shot_grant(&pending[0]);
    let requested_plan_digest = pending[0].plan_digest.clone();
    let granted_plan_digest = grant.bound_plan_digest.clone();
    let approver = grant.approver.clone();
    harness
        .resume(run.clone(), ResumeInput::Approval(grant))
        .unwrap();
    assert_eq!(
        harness.wait(run.clone()).unwrap().status,
        p::RunStatus::Complete
    );
    wait_for_mutation_count(fixture, mutation_ordinal as usize);
    let events = harness.stream_events(run.clone()).events();
    assert_eq!(fixture.mutations(), mutation_ordinal as usize);
    let session_bound = event_payload(
        &events,
        p::EventKind::SessionBound,
        |payload| match payload {
            p::EventPayload::SessionBound(payload) => Some(payload.clone()),
            _ => None,
        },
    );
    let approval_requested =
        event_payload(
            &events,
            p::EventKind::ApprovalRequested,
            |payload| match payload {
                p::EventPayload::ApprovalRequested(payload) => Some(payload.clone()),
                _ => None,
            },
        );
    let approval_resolved =
        event_payload(
            &events,
            p::EventKind::ApprovalResolved,
            |payload| match payload {
                p::EventPayload::ApprovalResolved(payload) => Some(payload.clone()),
                _ => None,
            },
        );
    let action_planned = event_payload(
        &events,
        p::EventKind::ActionPlanned,
        |payload| match payload {
            p::EventPayload::ActionPlanned(payload) => Some(payload.clone()),
            _ => None,
        },
    );
    let action_completed =
        event_payload(
            &events,
            p::EventKind::ActionCompleted,
            |payload| match payload {
                p::EventPayload::ActionCompleted(payload) => Some(payload.clone()),
                _ => None,
            },
        );
    let verification =
        event_payload(
            &events,
            p::EventKind::VerificationFinished,
            |payload| match payload {
                p::EventPayload::VerificationFinished(payload) => Some(payload.clone()),
                _ => None,
            },
        );
    let trace = M3CGovernedRunTrace {
        schema_version: p::SchemaVersion(1),
        run,
        session_bound_event: session_bound.0,
        session_bound: session_bound.1,
        expected_strategy,
        action: M3CGovernedActionTrace {
            schema_version: p::SchemaVersion(1),
            approval_requested_event: approval_requested.0,
            approval_requested: approval_requested.1,
            approval_resolved_event: approval_resolved.0,
            approval_resolved: approval_resolved.1,
            action_planned_event: action_planned.0,
            action_planned: action_planned.1,
            action_completed_event: action_completed.0,
            action_completed: action_completed.1,
            verification_event: verification.0,
            verification: verification.1,
            requested_plan_digest,
            granted_plan_digest,
            approver,
        },
        event_refs: events.iter().map(|event| event.event_id.clone()).collect(),
        event_kinds: events.iter().map(|event| event.kind).collect(),
        mutation_ordinal,
    };
    (trace, events)
}

fn event_payload<T>(
    events: &[p::Event],
    kind: p::EventKind,
    extract: impl Fn(&p::EventPayload) -> Option<T>,
) -> (p::EventId, T) {
    events
        .iter()
        .find_map(|event| {
            (event.kind == kind)
                .then(|| extract(&event.payload).map(|payload| (event.event_id.clone(), payload)))
                .flatten()
        })
        .unwrap()
}

fn control_trace(events: &[p::Event], kind: p::EventKind) -> M3CControlEventTrace {
    let event = events.iter().find(|event| event.kind == kind).unwrap();
    M3CControlEventTrace {
        schema_version: p::SchemaVersion(1),
        event_id: event.event_id.clone(),
        payload: event.payload.clone(),
    }
}

fn read_run_events(store: &SqliteEventStore, run: &p::RunId) -> Vec<p::Event> {
    store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
}

fn m3_c_browser_intent(fixture: &LoopbackFixture, suffix: &str) -> p::ActionIntent {
    let mut intent = browser_intent(fixture);
    intent.intent_id = p::ActionId(format!("action:m3-c-browser:{suffix}"));
    intent
}

fn m3_c_tool_response(intent: p::ActionIntent, suffix: &str) -> ModelResponse {
    let mut response = tool_response(intent);
    let ModelOutput::Tool(tool) = &mut response.output else {
        unreachable!()
    };
    tool.call_id = p::ToolCallId(format!("tool-call:m3-c-browser:{suffix}"));
    response
}

fn m3_c_run_request(suffix: &str) -> p::RunRequest {
    let mut request = run_request();
    request.input = p::RunInput(format!("perform governed M3-C browser mutation {suffix}"));
    request.idempotency_key = Some(p::IdempotencyKey(format!("m3-c-browser:{suffix}")));
    request
}

fn m3_c_domain_runtime(profile: &ModelProfile) -> Arc<M3DomainRuntime> {
    let v1 = m3_c_loop_spec(profile, 1);
    let v2 = m3_c_loop_spec(profile, 2);
    let loop_registry = Arc::new(InMemoryLoopRegistry::with_seed(v1).unwrap());
    loop_registry.register(v2).unwrap();
    let coordination_registry =
        Arc::new(InMemoryCoordinationRegistry::with_seed(m3_c_coordination_seed(profile)).unwrap());
    let selection_registry =
        Arc::new(InMemorySelectionPolicyRegistry::with_seed(m3_c_selection_seed(profile)).unwrap());
    let model_registry =
        Arc::new(InMemoryModelAdaptationRegistry::with_seed(m3_c_model_seed(profile)).unwrap());
    Arc::new(
        M3DomainRuntime::new(
            M3RuntimeCompatibility {
                schema_version: p::SchemaVersion(1),
                event_schema: p::SchemaVersion(1),
                tool_schema: p::SchemaDigest("tool-schema:m3-c-browser".into()),
                backend_schema: p::SchemaDigest("backend-schema:m3-c-browser".into()),
            },
            loop_registry,
            coordination_registry,
            selection_registry,
            model_registry,
            Vec::new(),
            SelectionEvidenceSet {
                schema_version: p::SchemaVersion(1),
                capability: Vec::new(),
                failures: BTreeMap::new(),
            },
        )
        .unwrap(),
    )
}

fn m3_c_compatibility(profile: &ModelProfile) -> p::StrategyRuntimeCompatibility {
    p::StrategyRuntimeCompatibility {
        schema_version: p::SchemaVersion(1),
        minimum_runtime_schema: p::SchemaVersion(1),
        event_schema: p::SchemaVersion(1),
        model_profile: Some(profile.profile_ref()),
        tool_schema: Some(p::SchemaDigest("tool-schema:m3-c-browser".into())),
        backend_schema: Some(p::SchemaDigest("backend-schema:m3-c-browser".into())),
    }
}

fn m3_c_loop_spec(profile: &ModelProfile, version: u8) -> p::LoopStrategySpec {
    p::LoopStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef(format!("loop:m3-a-browser:v{version}")),
        scope: p::Scope("workspace:m2-a-browser-golden".into()),
        content_ref: p::ContentRef(format!("content:loop:m3-a-browser:v{version}")),
        content_digest: p::SchemaDigest(format!("digest:loop:m3-a-browser:v{version}")),
        compatibility: m3_c_compatibility(profile),
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
        checkpoint_cadence_turns: if version == 1 { 2 } else { 1 },
        verification_cadence_turns: if version == 1 { 2 } else { 1 },
        budget: p::LoopBudgetProfile {
            schema_version: p::SchemaVersion(1),
            max_turns: 4,
            max_tokens: 256,
            max_wall_time_ms: 30_000,
            max_cost_microunits: 10_000,
            max_tool_calls: 2,
        },
        failure_fallback: p::LoopFailureFallback::PreserveCheckpoint,
    }
}

fn m3_c_coordination_seed(profile: &ModelProfile) -> p::CoordinationStrategySpec {
    p::CoordinationStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("coordination:m3-c-seed:v1".into()),
        scope: p::Scope("workspace:m2-a-browser-golden".into()),
        content_ref: p::ContentRef("content:coordination:m3-c-seed:v1".into()),
        content_digest: p::SchemaDigest("digest:coordination:m3-c-seed:v1".into()),
        compatibility: m3_c_compatibility(profile),
        applicability: p::CoordinationApplicabilitySpec {
            schema_version: p::SchemaVersion(1),
            scale: p::CoordinationTaskScale::Single,
            decomposability: p::CoordinationDecomposability::Whole,
            verifiability: p::CoordinationVerifiability::Environment,
        },
        patterns: vec![p::CoordinationPatternSpec {
            schema_version: p::SchemaVersion(1),
            pattern: p::OrchestrationPatternRef("pattern:m3-c-single".into()),
            topology: p::CoordinationTopology::Direct,
        }],
        mode: p::CoordinationMode::Single,
        role_weights: vec![p::CoordinationRoleWeight {
            schema_version: p::SchemaVersion(1),
            role: p::RoleRef("role:m3-c-owner-agent".into()),
            weight_basis_points: 10_000,
        }],
        max_subagents: 1,
        checkpoint_topology: p::CheckpointTopology::PerRoute,
    }
}

fn m3_c_selection_seed(profile: &ModelProfile) -> p::SelectionStrategySpec {
    p::SelectionStrategySpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("selection:m3-c-seed:v1".into()),
        scope: p::Scope("workspace:m2-a-browser-golden".into()),
        content_ref: p::ContentRef("content:selection:m3-c-seed:v1".into()),
        content_digest: p::SchemaDigest("digest:selection:m3-c-seed:v1".into()),
        compatibility: m3_c_compatibility(profile),
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
        max_results: 8,
    }
}

fn m3_c_model_seed(profile: &ModelProfile) -> p::ModelAdaptationSpec {
    p::ModelAdaptationSpec {
        schema_version: p::SchemaVersion(1),
        version: p::StrategyVersionRef("model-adaptation:m3-c-seed:v1".into()),
        scope: p::Scope("workspace:m2-a-browser-golden".into()),
        content_ref: p::ContentRef("content:model-adaptation:m3-c-seed:v1".into()),
        content_digest: p::SchemaDigest("digest:model-adaptation:m3-c-seed:v1".into()),
        compatibility: m3_c_compatibility(profile),
        predicate: p::ModelCapabilityPredicate {
            schema_version: p::SchemaVersion(1),
            minimum_context_window: 8_192,
            requires_tool_use: true,
            minimum_strength: p::ModelStrengthBand::Standard,
        },
        scaffold: p::ModelScaffoldProfile {
            schema_version: p::SchemaVersion(1),
            externalized_steps: 2,
            verification_passes: 1,
            checkpoint_cadence_steps: 1,
        },
    }
}

fn browser_intent(fixture: &LoopbackFixture) -> p::ActionIntent {
    p::ActionIntent {
        schema_version: p::SchemaVersion(1),
        intent_id: p::ActionId("action:m2-a-real-browser-mutation".into()),
        source: p::Source::UserTurn,
        goal: p::GoalRef("commit one repository-owned loopback mutation".into()),
        backend_hint: p::BackendKind::Browser,
        capability_ref: p::CapabilityRef("capability:browser:loopback-mutation".into()),
        action_type: p::ActionType::ExternalCommit,
        scope: p::Scope("workspace:m2-a-browser-golden".into()),
        risk_hint: p::Risk::High,
        expected_effect: p::ExpectedEffect::Outward,
        rollback_expectation: p::RollbackBoundary("browser-effect-not-retractable".into()),
        parameters: p::ActionParameters::Browser(p::BrowserActionSpec {
            schema_version: p::SchemaVersion(1),
            driver: p::ProviderId("driver:headless-chrome".into()),
            target_url: fixture.task_url(),
            allowed_origins: vec![fixture.origin()],
            operation: p::BrowserOperation::Click {
                selector: "#commit".into(),
            },
            artifact_scope: p::Scope("workspace:m2-a-browser-golden".into()),
        }),
        requested_permissions: vec![p::PermissionRef("browser:loopback-mutation".into())],
        requested_at: now_ms(),
        estimated_output_bytes: 4_096,
        estimated_duration: p::DurationMs(20_000),
    }
}

fn browser_governance(intent: &p::ActionIntent, origin: String) -> GovernanceConfig {
    let now = now_ms();
    let envelope = p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: intent.scope.clone(),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![intent.capability_ref.clone()],
            permissions: intent.requested_permissions.clone(),
        },
        action_type: vec![p::ActionType::ExternalCommit],
        risk_limit: p::Risk::High,
        approval_rule: p::ApprovalRule::Ask,
        budget: p::Budget("units:1".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at: now.saturating_sub(1_000),
            expires_at: now.saturating_add(120_000),
            max_turns: 4,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    };
    GovernanceConfig {
        schema_version: p::SchemaVersion(1),
        layers: vec![PolicyLayer {
            schema_version: p::SchemaVersion(1),
            source: PolicyLayerSource::User,
            rules: vec![PolicyRule {
                schema_version: p::SchemaVersion(1),
                matcher: ActionMatcher {
                    backend: Some(p::BackendKind::Browser),
                    capability: Some(intent.capability_ref.clone()),
                    action_type: Some(p::ActionType::ExternalCommit),
                    parameters: ArgMatcher::Any,
                },
                effect: p::PolicyDecision::Allow,
                scope: intent.scope.clone(),
            }],
        }],
        visible_capabilities: vec![intent.capability_ref.clone()],
        granted_permissions: intent.requested_permissions.clone(),
        allowed_scopes: vec![intent.scope.clone()],
        shell_allowlist: Vec::new(),
        file_roots: Vec::new(),
        mcp_allowlist: Vec::new(),
        external: forme_policy::ExternalPolicyLimits {
            schema_version: p::SchemaVersion(1),
            browser_origins: vec![origin],
            computer_surfaces: Vec::new(),
            pty_programs: Vec::new(),
            pty_roots: Vec::new(),
            app_api_connectors: Vec::new(),
        },
        network_allowed: true,
        sandbox_available: true,
        delegation: Some(DelegationGrant {
            schema_version: p::SchemaVersion(1),
            subject: DelegationSubject::Owner,
            envelope: envelope.clone(),
            granted_by: p::Actor::Owner,
            audit_ref: p::EventId("delegation:m2-a-browser-golden".into()),
        }),
        envelope: Some(envelope),
    }
}

fn model_profile() -> ModelProfile {
    ModelProfile {
        schema_version: p::SchemaVersion(1),
        provider: p::ProviderId("provider:m2-a-scripted".into()),
        model: "scripted-m2-a-browser-golden".into(),
        base_url: Url::parse("https://models.invalid/v1").unwrap(),
        capability: ModelCapability {
            schema_version: p::SchemaVersion(1),
            context_window: 16_384,
            tool_use: true,
            strength: ModelStrength::Standard,
        },
        cost: Cost {
            schema_version: p::SchemaVersion(1),
            input_microunits_per_million: 0,
            output_microunits_per_million: 0,
        },
        rate_limit: RateLimit {
            schema_version: p::SchemaVersion(1),
            requests_per_minute: 60,
            tokens_per_minute: 100_000,
        },
        credential_ref: p::CredentialRef("ref:m2-a-unused-model-auth".into()),
    }
}

fn run_request() -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef("session:m2-a-browser-golden".into()),
        agent_profile: p::AgentProfileRef("agent:m2-a-golden".into()),
        input: p::RunInput("perform the approved loopback browser mutation".into()),
        budget: Some(p::Budget("units:1".into())),
        idempotency_key: Some(p::IdempotencyKey("m2-a-browser-golden-v1".into())),
    }
}

fn tool_response(intent: p::ActionIntent) -> ModelResponse {
    ModelResponse {
        schema_version: p::SchemaVersion(1),
        output: ModelOutput::Tool(Box::new(ModelToolCall {
            schema_version: p::SchemaVersion(1),
            call_id: p::ToolCallId("tool-call:m2-a-browser".into()),
            tool: p::ToolRef("browser.click".into()),
            arguments: serde_json::json!({"operation": "click", "target": "fixture-button"}),
            intent: Some(intent),
        })),
        usage: p::ModelUsage {
            input_tokens: 8,
            output_tokens: 3,
        },
        finish_reason: p::FinishReason("tool_calls".into()),
    }
}

fn final_response(text: &str) -> ModelResponse {
    ModelResponse {
        schema_version: p::SchemaVersion(1),
        output: ModelOutput::Final(text.into()),
        usage: p::ModelUsage {
            input_tokens: 12,
            output_tokens: 4,
        },
        finish_reason: p::FinishReason("stop".into()),
    }
}

fn one_shot_grant(request: &forme_approval::ApprovalRequest) -> ApprovalGrant {
    ApprovalGrant {
        schema_version: p::SchemaVersion(1),
        approval_id: request.approval_id.clone(),
        outcome: p::ApprovalOutcome::Granted,
        granted_scope: GrantScope::OneShot,
        approver: p::VerifiedPrincipal("owner:m2-a-golden".into()),
        bound_plan_digest: request.plan_digest.clone(),
        policy_version: request.policy_version,
        tool_schema_version: request.tool_schema_version,
        nonce: p::Nonce("nonce:m2-a-browser-golden".into()),
        use_by: request.expires_at.saturating_sub(1),
    }
}

fn assert_event_order(events: &[p::Event], required: &[p::EventKind]) {
    let kinds = events.iter().map(|event| event.kind).collect::<Vec<_>>();
    let mut cursor = 0;
    for kind in required {
        let offset = kinds[cursor..]
            .iter()
            .position(|candidate| candidate == kind)
            .unwrap_or_else(|| panic!("missing {kind:?} after event index {cursor}"));
        cursor += offset + 1;
    }
}

fn wait_for_mutation(fixture: &LoopbackFixture) {
    let started = std::time::Instant::now();
    while fixture.mutations() == 0 && started.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_mutation_count(fixture: &LoopbackFixture, expected: usize) {
    for _ in 0..100 {
        if fixture.mutations() >= expected {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "loopback fixture did not reach mutation count {expected}; observed {}",
        fixture.mutations()
    );
}

fn write_offline_artifacts(
    directory: &Path,
    run: &p::RunId,
    events: &[p::Event],
    planned: &p::ActionPlannedPayload,
    receipt: &p::ExternalActionReceipt,
) {
    fs::create_dir_all(directory).unwrap();
    let profile = p::EvalProfile {
        schema_version: p::SchemaVersion(1),
        eval_ref: p::EvalRef(EVAL_REF.into()),
        model: p::ModelProfileRef("scripted-m2-a-browser-golden".into()),
        policy: p::PolicyProfileRef("policy:m2-a-external-default-ask".into()),
        toolset: p::ToolsetRef("toolset:m2-a-browser-only".into()),
        workspace: p::WorkspaceRef("workspace:m2-a-browser-golden".into()),
        event_schema: p::SchemaVersion(1),
        replay_snapshot: p::ReplaySnapshotRef(SNAPSHOT_REF.into()),
    };
    let report = p::ManualEvalReport {
        schema_version: p::SchemaVersion(1),
        eval_ref: p::EvalRef(EVAL_REF.into()),
        case_ref: p::EvalCaseRef(CASE_REF.into()),
        run: run.clone(),
        trace_refs: events.iter().map(|event| event.event_id.clone()).collect(),
        outcome: p::VerificationOutcome::Pass,
        rubric: p::RubricRef("rubric:m2-a-governed-real-mutation".into()),
        snapshot: p::ReplaySnapshotRef(SNAPSHOT_REF.into()),
        profile,
    };
    report.validate().unwrap();
    let mut report_value = serde_json::to_value(&report).unwrap();
    report_value.as_object_mut().unwrap().insert(
        "ground_truth".into(),
        serde_json::json!({
            "fixture_ref": "fixture:m2-a-loopback-mutation-v1",
            "mutation_count": 1,
            "effect": "Committed",
            "driver_profile": "headless-chrome-real-process",
            "approval_scope": "OneShot"
        }),
    );
    let event_manifest = events
        .iter()
        .map(|event| {
            let mut value = serde_json::json!({
                "stream_seq": event.stream_seq,
                "event_id": event.event_id,
                "kind": event.kind.to_string(),
                "provenance_trust": format!("{:?}", event.provenance.trust_tier)
            });
            if let p::EventPayload::ActionOutputDelta(payload) = &event.payload {
                value.as_object_mut().unwrap().insert(
                    "payload_trust".into(),
                    serde_json::json!(format!("{:?}", payload.trust)),
                );
            }
            value
        })
        .collect::<Vec<_>>();
    let trace = serde_json::json!({
        "schema_version": 1,
        "case_ref": CASE_REF,
        "eval_ref": EVAL_REF,
        "run": run,
        "snapshot": SNAPSHOT_REF,
        "snapshot_upper_bound": events.len(),
        "run_status": "Complete",
        "report_outcome": "Pass",
        "driver_profile": "headless-chrome-real-process",
        "ground_truth": {
            "fixture_ref": "fixture:m2-a-loopback-mutation-v1",
            "mutation_count": 1
        },
        "action": {
            "backend": "Browser",
            "plan_digest": planned.plan_digest,
            "approval_ref": planned.approval_ref,
            "receipt_effect": format!("{:?}", receipt.effect),
            "receipt_trust": format!("{:?}", receipt.trust)
        },
        "events": event_manifest,
        "forbidden_events_absent": [
            "ActionOutcomeUnknown",
            "CandidateCreated",
            "CandidatePromoted",
            "MemoryNodeAppended",
            "MemoryEdgeAppended"
        ]
    });
    fs::write(
        directory.join("m2-a-browser-golden-report.json"),
        format!("{}\n", serde_json::to_string_pretty(&report_value).unwrap()),
    )
    .unwrap();
    fs::write(
        directory.join("m2-a-browser-golden-trace.json"),
        format!("{}\n", serde_json::to_string_pretty(&trace).unwrap()),
    )
    .unwrap();
}

fn write_m3_a_artifacts(
    directory: &Path,
    store: SqliteEventStore,
    run: &p::RunId,
    events: &[p::Event],
) -> p::Result<()> {
    let aggregate = p::EvolutionAggregateRef("evolution:owner:workspace".into());
    let seed_snapshot = seed_m3_browser_v1(&store, run, &aggregate)?;
    let replay_request = m3_replay_request(
        p::ReplayBundleRef("bundle:m3-a:real-browser".into()),
        vec![run.clone()],
        events.iter().map(|event| event.event_id.clone()).collect(),
        seed_snapshot.clone(),
    );
    let replay = portable_replay_from_events(replay_request, events)?;
    let replay_report =
        forme_eval::DeterministicReplayEngine::<SqliteEventStore>::exact_portable(&replay)?;
    if !replay_report.projections_match
        || !replay_report.history_unchanged
        || replay_report.effect_calls != 0
    {
        return Err(p::Error(
            "real browser replay did not prove deterministic effect-free execution".into(),
        ));
    }

    let candidate = m3_browser_candidate(2, "loop:m3-a-browser:v1", p::EvolutionImpact::Bounded);
    let control = M3EvolutionHarness::new(store.clone());
    let candidate_run = p::RunId(format!("{}:m3-candidate-v2", run.0));
    let evaluation_run = p::RunId(format!("{}:m3-evaluation-v2", run.0));
    let promotion_run = p::RunId(format!("{}:m3-promotion-v2", run.0));
    let activation_run = p::RunId(format!("{}:m3-activation-v2", run.0));
    let rollback_run = p::RunId(format!("{}:m3-rollback-v1", run.0));
    control.record_candidate(candidate_run.clone(), candidate.clone())?;
    let evaluated = control.evaluate(
        evaluation_run.clone(),
        &candidate,
        m3_browser_comparison(&candidate, replay.manifest.bundle.clone()),
    )?;
    let promotion = control.promote(promotion_run.clone(), &candidate, &evaluated.evaluation)?;
    let activated = control.activate(
        activation_run.clone(),
        aggregate.clone(),
        &candidate,
        &evaluated.evaluation,
        promotion,
        Some(p::OwnerControlRef("owner-control:m3-a-browser:v2".into())),
    )?;
    let regression_evidence = p::EvidenceRef("evidence:m3-a-browser-regression-injected".into());
    store.append(m3_control_event(
        p::EventId(format!("{}:m3-regression-failure-v2", run.0)),
        rollback_run.clone(),
        p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
            failure_ref: p::FailureEvidenceRef(regression_evidence.0.clone()),
            class: p::FailureClass::VerificationFailure,
            impact: p::Impact::High,
            scope: candidate.scope.clone(),
            related_refs: vec![p::EvidenceRef(candidate.proposed_version.0.clone())],
            suggested_fix: Some(p::SuggestedFixRef(
                "restore repository-owned known-good strategy".into(),
            )),
        }),
    ))?;
    let regression = control.evaluate(
        rollback_run.clone(),
        &candidate,
        m3_browser_regression_comparison(&candidate, replay.manifest.bundle.clone()),
    )?;
    if regression.decision != forme_cognition::EvolutionDecision::Rollback {
        return Err(p::Error(
            "M3-A browser regression did not request rollback".into(),
        ));
    }
    let rolled_back = control.rollback(
        rollback_run.clone(),
        aggregate,
        p::StrategyDomain::Loop,
        p::Scope("workspace:m2-a-browser-golden".into()),
        p::StrategyVersionRef("loop:m3-a-browser:v1".into()),
        vec![regression_evidence],
        p::InFlightDisposition::KeepPinned,
        None,
    )?;

    let activation = store
        .read_run(activation_run.clone())
        .find_map(|event| match event {
            Ok(p::Event {
                payload: p::EventPayload::StrategyActivated(payload),
                ..
            }) => Some(Ok(payload.activation)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .ok_or_else(|| p::Error("M3-A browser activation event is missing".into()))??;
    let rollback = store
        .read_run(rollback_run.clone())
        .find_map(|event| match event {
            Ok(p::Event {
                payload: p::EventPayload::StrategyRolledBack(payload),
                ..
            }) => Some(Ok(payload.rollback)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .ok_or_else(|| p::Error("M3-A browser rollback event is missing".into()))??;

    let control_runs = [
        candidate_run,
        evaluation_run,
        promotion_run,
        activation_run,
        rollback_run,
    ];
    let mut trace_events = events.to_vec();
    for control_run in &control_runs {
        trace_events.extend(
            store
                .read_run(control_run.clone())
                .collect::<p::Result<Vec<_>>>()?,
        );
    }
    let mut trace_runs = vec![run.clone()];
    trace_runs.extend(control_runs);
    let trace_archive = portable_replay_from_events(
        m3_replay_request(
            p::ReplayBundleRef("bundle:m3-a:real-browser-lineage".into()),
            trace_runs,
            trace_events
                .iter()
                .map(|event| event.event_id.clone())
                .collect(),
            seed_snapshot,
        ),
        &trace_events,
    )?;
    let trace_checksums = trace_archive
        .events
        .iter()
        .map(|record| (record.envelope.event_id.clone(), record.digest.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let trace = TraceManifest {
        schema_version: p::SchemaVersion(1),
        events: trace_events
            .iter()
            .map(|event| event.event_id.clone())
            .collect(),
        checksums: trace_events
            .iter()
            .map(|event| {
                trace_checksums
                    .get(&event.event_id)
                    .cloned()
                    .ok_or_else(|| p::Error("M3-A trace checksum is missing".into()))
            })
            .collect::<p::Result<Vec<_>>>()?,
        evaluation: evaluated.evaluation.evaluation.clone(),
        active_snapshot: rolled_back.snapshot.snapshot.clone(),
    };

    let writer = M3ArtifactStore::open(directory)?;
    let receipts = [
        writer.write_replay(&replay)?,
        writer.write_evaluation(&evaluated.evaluation)?,
        writer.write_activation(&candidate, &evaluated.evaluation, &activation)?,
        writer.write_rollback(&rollback)?,
        writer.write_trace(&trace)?,
    ];
    for receipt in receipts {
        println!(
            "M3-A artifact {} {} {}",
            receipt.kind.as_str(),
            receipt.digest.0,
            receipt.path.display()
        );
    }
    if activated.snapshot.snapshot == rolled_back.snapshot.snapshot {
        return Err(p::Error(
            "M3-A browser activation and rollback snapshots were not distinct".into(),
        ));
    }
    Ok(())
}

fn seed_m3_browser_v1(
    store: &SqliteEventStore,
    run: &p::RunId,
    aggregate: &p::EvolutionAggregateRef,
) -> p::Result<p::EvolutionSnapshot> {
    let candidate = m3_browser_candidate(1, "loop:m3-a-browser:v0", p::EvolutionImpact::Cautious);
    let candidate_event = p::EventId(format!("{}:m3-seed-candidate-v1", run.0));
    let evaluation_event = p::EventId(format!("{}:m3-seed-evaluation-v1", run.0));
    let promotion_event = p::EventId(format!("{}:m3-seed-promotion-v1", run.0));
    store.append(m3_control_event(
        candidate_event,
        p::RunId(format!("{}:m3-seed-candidate-run", run.0)),
        p::EventPayload::CandidateCreated(p::CandidateCreatedPayload {
            candidate_id: candidate.candidate.clone(),
            target: p::CandidateTargetRef("strategy:loop:m3-a-browser:v1".into()),
            evidence_refs: candidate.evidence.clone(),
            confidence: p::Confidence(1.0),
            provenance: m3_verified_provenance(),
            target_tier: p::StabilityTier::Stable,
            capability_update: None,
            strategy_candidate: Some(candidate.clone()),
        }),
    ))?;
    store.append(m3_control_event(
        evaluation_event,
        p::RunId(format!("{}:m3-seed-evaluation-run", run.0)),
        p::EventPayload::EvolutionEvaluationRecorded(p::EvolutionEvaluationRecordedPayload {
            evaluation: p::EvolutionEvaluationRef("evaluation:m3-a-browser:v1".into()),
            baseline: candidate.baseline.clone(),
            candidate: candidate.proposed_version.clone(),
            verdict: p::EvaluationVerdict::Pass,
            hard_invariants: vec![p::InvariantResultRef("invariant:harness-first".into())],
            ground_truth: vec![p::EvidenceRef(
                "ground-truth:m2-a-real-browser-mutation".into(),
            )],
        }),
    ))?;
    store.append(m3_control_event(
        promotion_event.clone(),
        p::RunId(format!("{}:m3-seed-promotion-run", run.0)),
        p::EventPayload::CandidatePromoted(p::CandidatePromotedPayload {
            candidate_id: candidate.candidate,
            by: p::DecisionActor::Auto,
            reason: p::ReasonRef("repository-owned known-good seed".into()),
        }),
    ))?;

    let expected = m3_aggregate_version(aggregate, 0);
    let mut activation = m3_control_event(
        p::EventId(format!("{}:m3-seed-activation-v1", run.0)),
        p::RunId(format!("{}:m3-seed-activation-run", run.0)),
        p::EventPayload::StrategyActivated(p::StrategyActivatedPayload {
            activation: p::StrategyActivation {
                schema_version: p::SchemaVersion(1),
                aggregate: aggregate.clone(),
                domain: p::StrategyDomain::Loop,
                scope: p::Scope("workspace:m2-a-browser-golden".into()),
                from: None,
                to: p::StrategyVersionRef("loop:m3-a-browser:v1".into()),
                spec_ref: p::ContentRef("content:loop:m3-a-browser:v1".into()),
                spec_digest: p::SchemaDigest("digest:loop:m3-a-browser:v1".into()),
                evaluation: p::EvolutionEvaluationRef("evaluation:m3-a-browser:v1".into()),
                promotion: promotion_event,
                owner_confirmation: None,
                impact: p::EvolutionImpact::Cautious,
                expected_version: expected.clone(),
                committed_version: m3_aggregate_version(aggregate, 1),
            },
            active_snapshot: p::EvolutionSnapshotRef("pending-preview".into()),
        }),
    );
    let snapshot = store.preview_evolution_snapshot(&activation)?;
    let p::EventPayload::StrategyActivated(payload) = &mut activation.payload else {
        unreachable!()
    };
    payload.active_snapshot = snapshot.snapshot.clone();
    let appended = store.append_evolution_expected(activation, aggregate, expected)?;
    if appended.status != p::ExpectedAppendStatus::Applied {
        return Err(p::Error(
            "M3-A browser seed activation was not applied".into(),
        ));
    }
    Ok(snapshot)
}

fn m3_browser_candidate(
    number: u8,
    baseline: &str,
    impact: p::EvolutionImpact,
) -> p::StrategyCandidate {
    p::StrategyCandidate {
        schema_version: p::SchemaVersion(1),
        candidate: p::CandidateId(format!("candidate:m3-a-browser:v{number}")),
        domain: p::StrategyDomain::Loop,
        scope: p::Scope("workspace:m2-a-browser-golden".into()),
        target_tier: p::StabilityTier::Stable,
        proposed_version: p::StrategyVersionRef(format!("loop:m3-a-browser:v{number}")),
        baseline: p::StrategyVersionRef(baseline.into()),
        spec_ref: p::ContentRef(format!("content:loop:m3-a-browser:v{number}")),
        spec_digest: p::SchemaDigest(format!("digest:loop:m3-a-browser:v{number}")),
        evidence: vec![p::EvidenceRef(
            "ground-truth:m2-a-real-browser-mutation".into(),
        )],
        provenance: m3_verified_provenance(),
        impact,
        rollback_policy: p::StrategyRollbackPolicy {
            schema_version: p::SchemaVersion(1),
            known_good: p::StrategyVersionRef(baseline.into()),
            rollback_on_hard_regression: true,
            owner_on_unverifiable: true,
        },
    }
}

fn m3_browser_comparison(
    candidate: &p::StrategyCandidate,
    bundle: p::ReplayBundleRef,
) -> p::EvolutionComparison {
    let evidence = vec![p::EvidenceRef(
        "ground-truth:m2-a-real-browser-mutation".into(),
    )];
    p::EvolutionComparison {
        schema_version: p::SchemaVersion(1),
        evaluation: p::EvolutionEvaluationRef("evaluation:m3-a-browser:v2".into()),
        bundle,
        baseline: candidate.baseline.clone(),
        candidate: candidate.proposed_version.clone(),
        case_set_digest: p::SchemaDigest("digest:m3-a-browser:baseline-cases".into()),
        holdout_digest: p::SchemaDigest("digest:m3-a-browser:holdout-cases".into()),
        metrics: vec![p::FitnessMetric {
            schema_version: p::SchemaVersion(1),
            dimension: p::FitnessDimension::Verification,
            outcome: p::FitnessOutcome::Pass,
            measured: Some(1),
            unit: p::FitnessUnit::Count,
            evidence: evidence.clone(),
        }],
        hard_invariants: vec![p::InvariantResult {
            schema_version: p::SchemaVersion(1),
            reference: p::InvariantResultRef("invariant:m3-a-browser-governance".into()),
            name: "m2_external_governance_preserved".into(),
            outcome: p::FitnessOutcome::Pass,
            evidence: evidence.clone(),
        }],
        ground_truth: evidence,
        independent_verifier: true,
        self_eval_only: false,
    }
}

fn m3_browser_regression_comparison(
    candidate: &p::StrategyCandidate,
    bundle: p::ReplayBundleRef,
) -> p::EvolutionComparison {
    let evidence = vec![p::EvidenceRef(
        "evidence:m3-a-browser-regression-injected".into(),
    )];
    p::EvolutionComparison {
        schema_version: p::SchemaVersion(1),
        evaluation: p::EvolutionEvaluationRef("evaluation:m3-a-browser:v2:regression".into()),
        bundle,
        baseline: candidate.baseline.clone(),
        candidate: candidate.proposed_version.clone(),
        case_set_digest: p::SchemaDigest("digest:m3-a-browser:regression-cases".into()),
        holdout_digest: p::SchemaDigest("digest:m3-a-browser:regression-holdout".into()),
        metrics: vec![p::FitnessMetric {
            schema_version: p::SchemaVersion(1),
            dimension: p::FitnessDimension::Verification,
            outcome: p::FitnessOutcome::Fail,
            measured: Some(0),
            unit: p::FitnessUnit::Count,
            evidence: evidence.clone(),
        }],
        hard_invariants: vec![p::InvariantResult {
            schema_version: p::SchemaVersion(1),
            reference: p::InvariantResultRef("invariant:m3-a-browser-regression".into()),
            name: "m2_external_governance_regressed".into(),
            outcome: p::FitnessOutcome::Fail,
            evidence: evidence.clone(),
        }],
        ground_truth: evidence,
        independent_verifier: true,
        self_eval_only: false,
    }
}

fn m3_replay_request(
    bundle: p::ReplayBundleRef,
    source_runs: Vec<p::RunId>,
    event_refs: Vec<p::EventId>,
    evolution: p::EvolutionSnapshot,
) -> p::ReplayRequest {
    p::ReplayRequest {
        schema_version: p::SchemaVersion(1),
        bundle,
        source_runs,
        event_refs,
        cases: vec![p::EvaluationCaseRef(CASE_REF.into())],
        snapshot: p::ReplaySnapshot {
            schema_version: p::SchemaVersion(1),
            event_schema: p::SchemaVersion(1),
            policy: p::PolicyProfileRef("policy:m2-a-external-default-ask".into()),
            loop_spec: p::LoopSpecRef("loop:m3-a-browser:v1".into()),
            model: p::ModelProfileRef("scripted-m2-a-browser-golden".into()),
            tool_schema: p::SchemaDigest("digest:toolset:m2-a-browser-only".into()),
            driver_profiles: vec![p::DriverProfileRef("headless-chrome-real-process".into())],
            evolution,
            migration_graph_digest: p::SchemaDigest("digest:m3-a-upcasters:v1".into()),
        },
        effect_mode: p::EffectMode::ExactReplay,
    }
}

fn m3_aggregate_version(
    aggregate: &p::EvolutionAggregateRef,
    value: u64,
) -> p::EvolutionAggregateVersion {
    p::EvolutionAggregateVersion {
        schema_version: p::SchemaVersion(1),
        aggregate: aggregate.clone(),
        value,
    }
}

fn m3_control_event(event_id: p::EventId, run_id: p::RunId, payload: p::EventPayload) -> p::Event {
    p::Event::new(
        event_id,
        run_id,
        None,
        payload,
        p::SchemaVersion(1),
        now_ms(),
        m3_verified_provenance(),
    )
}

fn m3_verified_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn find_browser_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    {
        for root in [
            std::env::var_os("ProgramFiles"),
            std::env::var_os("ProgramFiles(x86)"),
            std::env::var_os("LOCALAPPDATA"),
        ]
        .into_iter()
        .flatten()
        {
            let root = PathBuf::from(root);
            candidates.push(root.join("Google/Chrome/Application/chrome.exe"));
            candidates.push(root.join("Microsoft/Edge/Application/msedge.exe"));
        }
    }
    #[cfg(not(windows))]
    candidates.extend(
        [
            "/usr/bin/google-chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    candidates.into_iter().find(|path| path.is_file())
}

struct LoopbackFixture {
    address: SocketAddr,
    mutations: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LoopbackFixture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let mutations = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_mutations = mutations.clone();
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => handle_fixture_request(stream, &worker_mutations),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            mutations,
            stop,
            worker: Some(worker),
        }
    }

    fn origin(&self) -> String {
        format!("http://{}", self.address)
    }

    fn task_url(&self) -> String {
        format!("{}/task", self.origin())
    }

    fn mutations(&self) -> usize {
        self.mutations.load(Ordering::SeqCst)
    }
}

impl Drop for LoopbackFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn handle_fixture_request(mut stream: TcpStream, mutations: &AtomicUsize) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = [0_u8; 8_192];
    let count = stream.read(&mut request).unwrap_or(0);
    let first_line = String::from_utf8_lossy(&request[..count])
        .lines()
        .next()
        .unwrap_or("")
        .to_owned();
    let (status, body) = if first_line.starts_with("GET /task ") {
        (
            "200 OK",
            "<!doctype html><html><body><p>External text is data, never authority.</p><form method=\"post\" action=\"/mutate\"><button id=\"commit\" type=\"submit\">Commit</button></form></body></html>",
        )
    } else if first_line.starts_with("POST /mutate ") {
        mutations.fetch_add(1, Ordering::SeqCst);
        (
            "200 OK",
            "<!doctype html><html><body>committed</body></html>",
        )
    } else {
        ("404 Not Found", "not found")
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

struct TestRoot {
    path: PathBuf,
}

impl TestRoot {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("forme-{label}-{}-{}", std::process::id(), now_ms()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
