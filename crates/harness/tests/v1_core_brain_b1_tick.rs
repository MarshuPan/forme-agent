//! V1 closure B1 (C7, R-TICK): the owner-brain three-phase cognitive tick,
//! driven through the formal production composition and a real HTTP model
//! endpoint fixture. Assertions read the event log and the durable tick
//! ledger, never a final answer.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use forme_cognition::{
    ChangeDirection, CognitiveMapStore, CognitiveMapUpdateProposal, CognitiveTickPhase,
    CognitiveTickPhaseStatus, GovernanceCandidate, GovernanceDecision, GovernanceEvidence,
    IntentionState, IntentionTrigger, MapScope, MapUpdateKind, ProspectiveIntention,
    StableCognition, StableCognitionKind,
};
use forme_harness::{
    AgentHarness, CapabilityRuntimeConfig, CognitiveRuntimeConfig, CognitiveTickOutcome,
    CognitiveTickReport, DataProtectionConfig, ProductionRuntime, ProductionRuntimeBuilder,
    ProductionRuntimeConfig,
};
use forme_models::{ModelRuntimeConfig, ModelStrength};
use forme_protocol as p;
use forme_store::{CognitiveTickLedger, CognitiveTickWindowState, EventStore, SqliteEventStore};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static NEXT_PATH: AtomicU64 = AtomicU64::new(1);
const MODEL_KEY_ENV: &str = "FORME_V1B1_TICK_MODEL_KEY";
const TICK_INTERVAL_MS: i64 = 60_000;
/// The production tick lease; a crashed attempt is retried after it lapses.
const TICK_LEASE_MS: i64 = 600_000;

struct TestEnvironment {
    _lock: MutexGuard<'static, ()>,
}

impl TestEnvironment {
    fn new() -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::set_var(MODEL_KEY_ENV, "v1b1-tick-model-secret");
        Self { _lock: lock }
    }
}

/// An HTTP model endpoint whose answers can be held back, so a test can keep
/// an owner turn in its foreground model call while the tick runs.
struct ModelServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    open: Arc<(Mutex<bool>, Condvar)>,
    requests: Arc<Mutex<Vec<String>>>,
    handle: Option<JoinHandle<()>>,
}

impl ModelServer {
    fn start(answer: &'static str) -> Self {
        Self::with_gate(answer, true)
    }

    fn held(answer: &'static str) -> Self {
        Self::with_gate(answer, false)
    }

    fn with_gate(answer: &'static str, open: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let gate = Arc::new((Mutex::new(open), Condvar::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (stop_flag, held, captured) = (stop.clone(), gate.clone(), requests.clone());
        let handle = std::thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let request = read_message(&mut stream);
                        captured.lock().unwrap().push(request);
                        let (lock, released) = &*held;
                        let mut open = lock.lock().unwrap();
                        while !*open && !stop_flag.load(Ordering::SeqCst) {
                            open = released
                                .wait_timeout(open, Duration::from_millis(20))
                                .unwrap()
                                .0;
                        }
                        drop(open);
                        let body = format!(
                            r#"{{"choices":[{{"message":{{"content":"{answer}"}},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":11,"completion_tokens":3}}}}"#
                        );
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.flush();
                        let _ = stream.shutdown(Shutdown::Both);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("model fixture accept failed: {error}"),
                }
            }
        });
        Self {
            address,
            stop,
            open: gate,
            requests,
            handle: Some(handle),
        }
    }

    fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn wait_for_calls(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.calls() < count {
            assert!(Instant::now() < deadline, "the model call never arrived");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn release(&self) {
        let (lock, released) = &*self.open;
        *lock.lock().unwrap() = true;
        released.notify_all();
    }
}

impl Drop for ModelServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.release();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn read_message(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8_192];
    let mut expected = None;
    loop {
        let read = stream.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if expected.is_none() {
            if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::trim)
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                expected = Some(header_end + 4 + content_length);
            }
        }
        if expected.is_some_and(|length| bytes.len() >= length) {
            break;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn database_path(label: &str) -> PathBuf {
    let tag = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "forme-v1b1-tick-{label}-{}-{tag}-{}.db",
        std::process::id(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}

fn remove_database(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    let _ = std::fs::remove_dir_all(path.with_extension("artifacts"));
}

fn config(owner: &str, path: &Path, model: SocketAddr) -> ProductionRuntimeConfig {
    ProductionRuntimeConfig {
        schema_version: p::SchemaVersion(1),
        owner: p::OwnerPrincipalRef(owner.into()),
        store_path: path.to_path_buf(),
        workspace: p::WorkspaceRef("workspace:default".into()),
        agent_profile: p::AgentProfileRef("agent:v1b1-tick".into()),
        models: vec![ModelRuntimeConfig {
            id: "model:v1b1-tick".into(),
            provider: "v1b1-tick-provider".into(),
            model: "v1b1-tick-model".into(),
            base_url: format!("http://{model}/v1"),
            credential_env: MODEL_KEY_ENV.into(),
            context_window: 32_768,
            tool_use: false,
            strength: ModelStrength::Standard,
            input_microunits_per_million: 0,
            output_microunits_per_million: 0,
            requests_per_minute: 600,
            tokens_per_minute: 1_000_000,
            timeout_ms: 20_000,
            fallback_group: Some("v1b1-tick".into()),
            default: true,
            tool_schema_versions: Vec::new(),
        }],
        capabilities: CapabilityRuntimeConfig::default(),
        scheduler_enabled: true,
        scheduler: p::SchedulerConfig {
            schema_version: p::SchemaVersion(1),
            tick: p::DurationMs(1_000),
            lease: p::DurationMs(30_000),
            max_claims_per_tick: 1,
        },
        cognition: CognitiveRuntimeConfig {
            tick_interval: p::DurationMs(TICK_INTERVAL_MS as u64),
            ..CognitiveRuntimeConfig::default()
        },
        data: DataProtectionConfig {
            schema_version: p::SchemaVersion(1),
            artifact_root: path.with_extension("artifacts"),
        },
        authority_lease: p::DurationMs(30_000),
        context_limits: forme_context::ContextHydrationLimits::default(),
    }
}

fn build(config: &ProductionRuntimeConfig) -> ProductionRuntime {
    ProductionRuntimeBuilder::from_config(config.clone())
        .unwrap()
        .build()
        .unwrap()
}

fn request(session: &str, input: &str, key: &str) -> p::RunRequest {
    p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef(session.into()),
        agent_profile: p::AgentProfileRef("agent:v1b1-tick".into()),
        input: p::RunInput(input.into()),
        budget: None,
        idempotency_key: Some(p::IdempotencyKey(key.into())),
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

fn internal_provenance() -> p::Provenance {
    p::Provenance {
        source: p::Source::Internal,
        actor: p::Actor::System,
        trust_tier: p::TrustTier::VerifiedProcess,
        caused_by: None,
    }
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn append(store: &SqliteEventStore, run: &str, id: &str, payload: p::EventPayload) {
    store
        .append(p::Event::new(
            p::EventId(id.into()),
            p::RunId(run.into()),
            None,
            payload,
            p::SchemaVersion(1),
            now_ms(),
            internal_provenance(),
        ))
        .unwrap();
}

/// A run that failed with recorded failure evidence in `scope`.
fn record_failure(store: &SqliteEventStore, run: &str, scope: &str, impact: p::Impact) {
    append(
        store,
        run,
        &format!("{run}:accepted"),
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId(format!("session:{run}")),
            input_ref: p::InputRef("deploy the preview build".into()),
            idempotency_key: None,
        }),
    );
    append(
        store,
        run,
        &format!("{run}:failure"),
        p::EventPayload::FailureEvidenceRecorded(p::FailureEvidenceRecordedPayload {
            failure_ref: p::FailureEvidenceRef(format!("failure:{run}")),
            class: p::FailureClass::ExecutionFailure,
            impact,
            scope: p::Scope(scope.into()),
            related_refs: vec![p::EvidenceRef(format!("trace:{run}"))],
            suggested_fix: None,
        }),
    );
    append(
        store,
        run,
        &format!("{run}:failed"),
        p::EventPayload::RunFailed(p::RunFailedPayload {
            stop_reason: p::StopReason("the preview deploy failed".into()),
            result_ref: None,
        }),
    );
}

fn events(store: &SqliteEventStore, run: &p::RunId) -> Vec<p::Event> {
    store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
}

fn all_events(store: &SqliteEventStore) -> Vec<p::Event> {
    store
        .run_ids()
        .unwrap()
        .iter()
        .flat_map(|run| events(store, run))
        .collect()
}

fn count(store: &SqliteEventStore, kind: p::EventKind) -> usize {
    all_events(store)
        .iter()
        .filter(|event| event.kind == kind)
        .count()
}

fn ran(outcome: CognitiveTickOutcome) -> CognitiveTickReport {
    match outcome {
        CognitiveTickOutcome::Ran(report) => *report,
        CognitiveTickOutcome::NotClaimed { reason, .. } => {
            panic!("the tick claimed no window: {reason}")
        }
    }
}

fn statuses(report: &CognitiveTickReport) -> Vec<(CognitiveTickPhase, CognitiveTickPhaseStatus)> {
    report
        .phases
        .iter()
        .map(|phase| (phase.phase, phase.status))
        .collect()
}

fn owner_digest(runtime: &ProductionRuntime) -> p::SchemaDigest {
    runtime.services().authority.owner_digest.clone()
}

/// Promotes a frame whose evidence was last reproduced long ago, so the
/// maintenance phase finds it due for re-evaluation.
fn promote_stale_frame(runtime: &ProductionRuntime, frame: &str) {
    let cognition = runtime.services().cognition.clone();
    let scope = MapScope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:b1".into()),
    };
    let candidate = cognition
        .propose_update(CognitiveMapUpdateProposal {
            schema_version: p::SchemaVersion(1),
            candidate_id: None,
            scope: scope.clone(),
            kind: MapUpdateKind::Frame,
            confidence: p::Confidence(0.3),
            evidence: vec![p::EvidenceRef("evidence:b1:frame".into())],
            frame: Some(p::JudgmentFrameRef(frame.into())),
            quality: None,
            blindspot: None,
            resource: None,
            reflection_inputs: Vec::new(),
            statement: "verify before shipping".into(),
            provenance: owner_provenance(),
        })
        .unwrap();
    let evidence = vec![
        GovernanceEvidence {
            schema_version: p::SchemaVersion(1),
            reference: p::EvidenceRef("evidence:b1:first".into()),
            observed_at: 10,
            verified_process: true,
        },
        GovernanceEvidence {
            schema_version: p::SchemaVersion(1),
            reference: p::EvidenceRef("evidence:b1:second".into()),
            observed_at: 20,
            verified_process: true,
        },
    ];
    let decision = cognition
        .govern_and_apply(GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: candidate,
            direction: ChangeDirection::CautionIncreasing,
            confidence: p::Confidence(0.9),
            evidence: evidence.clone(),
            impact: p::Impact::Low,
            provenance: owner_provenance(),
            conflicts: Vec::new(),
            owner_confirmed: true,
            target: StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef(frame.into()),
                scope,
                kind: StableCognitionKind::Frame,
                statement: "verify before shipping".into(),
                tier: p::StabilityTier::Stable,
                confidence: p::Confidence(0.9),
                evidence,
                last_reproduced_at: 30,
                active: false,
            },
        })
        .unwrap();
    assert_eq!(decision, GovernanceDecision::Promote);
}

#[test]
fn c7_one_window_observes_then_reflects_then_maintains_over_a_fixed_range() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("tick-order answer");
    let path = database_path("order");
    let config = config("owner:b1-order", &path, model.address);
    let runtime = build(&config);
    let store = runtime.services().store.clone();
    let harness = runtime.harness();

    // A new run outcome, a failure, and memory due for maintenance.
    let turn = harness
        .submit_run(request(
            "session:b1-order",
            "summarise the week",
            "b1:order:turn",
        ))
        .unwrap();
    harness.wait(turn).unwrap();
    record_failure(
        &store,
        "run:b1-order-failed",
        "workspace:b1",
        p::Impact::High,
    );
    promote_stale_frame(&runtime, "frame:b1-stale");
    let calls = model.calls();
    let actions = count(&store, p::EventKind::ActionStarted);
    let runs = count(&store, p::EventKind::RunAccepted);
    let promotions = count(&store, p::EventKind::CandidatePromoted);

    let report = ran(runtime.cognitive_tick_at(now_ms()).unwrap());
    assert_eq!(
        statuses(&report),
        vec![
            (
                CognitiveTickPhase::ObserveAndPropose,
                CognitiveTickPhaseStatus::Ran
            ),
            (
                CognitiveTickPhase::ReflectAndCandidate,
                CognitiveTickPhaseStatus::Ran
            ),
            (
                CognitiveTickPhase::MaintainMemory,
                CognitiveTickPhaseStatus::Ran
            ),
        ]
    );
    assert_eq!(report.model_calls(), 0);
    assert_eq!(model.calls(), calls, "the tick never calls a model");

    // The tick stream is ordered: observation and opportunity first, then
    // the reflection record, then the maintenance record last.
    let tick = events(&store, &report.run);
    let kinds = tick.iter().map(|event| event.kind).collect::<Vec<_>>();
    let position = |kind| kinds.iter().position(|seen| *seen == kind).unwrap();
    assert_eq!(kinds[0], p::EventKind::ObservationRecorded);
    let reflection = position(p::EventKind::ReflectionProduced);
    let maintenance = position(p::EventKind::MemoryMaintenanceApplied);
    assert!(position(p::EventKind::OpportunityDetected) < reflection);
    assert!(position(p::EventKind::ProactiveProposalEmitted) < reflection);
    assert!(reflection < maintenance);
    assert_eq!(maintenance, kinds.len() - 1);
    assert!(kinds[..reflection].iter().all(|kind| matches!(
        kind,
        p::EventKind::ObservationRecorded
            | p::EventKind::OpportunityDetected
            | p::EventKind::ValueGateEvaluated
            | p::EventKind::ImpulseRaised
            | p::EventKind::CompetenceGateEvaluated
            | p::EventKind::DecisionTraceRecorded
            | p::EventKind::ProactiveProposalEmitted
            | p::EventKind::CandidateCreated
    )));
    assert!(tick.iter().all(|event| match &event.payload {
        p::EventPayload::ProactiveProposalEmitted(payload) =>
            payload.delivery == p::DeliveryMode::Hitchhike && payload.attention_cost == 0,
        _ => true,
    }));

    // Each phase reports exactly what it appended, in order.
    let ids = |range: std::ops::Range<usize>| {
        tick[range]
            .iter()
            .map(|event| event.event_id.clone())
            .collect::<Vec<_>>()
    };
    let observe = report.phase(CognitiveTickPhase::ObserveAndPropose).unwrap();
    assert_eq!(observe.event_refs, ids(0..reflection));
    let reflect = report
        .phase(CognitiveTickPhase::ReflectAndCandidate)
        .unwrap();
    assert_eq!(reflect.event_refs.last(), Some(&tick[reflection].event_id));
    let maintain = report.phase(CognitiveTickPhase::MaintainMemory).unwrap();
    assert_eq!(
        maintain.event_refs.last(),
        Some(&tick[maintenance].event_id)
    );

    // Reflection only proposes: low-confidence blind-spot candidates.
    let cognition_stream = runtime.services().aggregates.memory_cognition.clone();
    let reflections = events(&store, &cognition_stream)
        .into_iter()
        .filter_map(|event| match event.payload {
            p::EventPayload::CandidateCreated(payload)
                if payload
                    .candidate_id
                    .0
                    .starts_with("candidate:tick-reflection:") =>
            {
                Some((event.event_id, payload))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!reflections.is_empty());
    assert!(reflections.iter().any(|(_, payload)| payload
        .target
        .0
        .starts_with("cognitive-map|workspace:b1|blindspot|")));
    for (event, payload) in &reflections {
        assert!(payload.confidence.0 <= 0.3 + f32::EPSILON);
        assert!(reflect.event_refs.contains(event));
    }

    // Maintenance marked the stale frame for re-evaluation with a downgrade
    // candidate; nothing was deleted, promoted, or acted on.
    let cognition_events = events(&store, &cognition_stream);
    assert!(cognition_events.iter().any(|event| matches!(&event.payload,
        p::EventPayload::ReevaluationTaskCreated(payload)
            if payload.derived_refs == vec![p::ObjectRef("frame:b1-stale".into())])));
    assert!(cognition_events.iter().any(|event| matches!(&event.payload,
        p::EventPayload::CandidateCreated(payload)
            if payload.target.0 == "downgrade:frame:b1-stale")));
    assert_eq!(count(&store, p::EventKind::ActionStarted), actions);
    assert_eq!(count(&store, p::EventKind::RunAccepted), runs);
    assert_eq!(count(&store, p::EventKind::CandidatePromoted), promotions);
    let stable = runtime.services().cognition.stable_objects();
    assert_eq!(stable.len(), 1);
    assert!(!stable[0].active, "the stale frame awaits re-evaluation");
    assert_eq!(
        stable[0].confidence,
        p::Confidence(0.9),
        "confidence never rises"
    );

    // The ledger holds the finished window, and a restart folds the same
    // stable cognition from the log.
    assert_eq!(
        store
            .tick_window(&owner_digest(&runtime), report.claim.window)
            .unwrap(),
        Some(CognitiveTickWindowState::Finished {
            attempt: 1,
            report_digest: report.digest.clone(),
            consumed: report.event_range.to.clone(),
        })
    );
    if let Some(directory) = std::env::var_os("FORME_CORE_B_ARTIFACT_DIR") {
        let source_commit = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert!(source_commit.status.success());
        let stream_heads: std::collections::BTreeMap<_, _> = store
            .run_ids()
            .unwrap()
            .into_iter()
            .map(|run| {
                let last = events(&store, &run).last().unwrap().stream_seq;
                (run.0, last)
            })
            .collect();
        let positions: Vec<_> = tick
            .iter()
            .map(|event| {
                serde_json::json!({"seq": event.stream_seq, "kind": event.kind,
                    "digest": p::canonical_digest(event).unwrap()})
            })
            .collect();
        let snapshot: Vec<_> = stable
            .iter()
            .map(|object| {
                serde_json::json!({"object": object.object, "active": object.active,
                    "confidence": object.confidence})
            })
            .collect();
        let artifact = serde_json::json!({
            "schema_version": 1, "kind": "forme-core-brain-tick", "grade": "ControlledRuntime",
            "source_commit": String::from_utf8(source_commit.stdout).unwrap().trim(),
            "composition_digest": runtime.manifest().digest,
            "cognition_snapshot": p::canonical_digest(&snapshot).unwrap(),
            "report_digest": report.digest, "run": report.run, "stream_heads": stream_heads,
            "phases": ["ObserveAndPropose", "ReflectAndCandidate", "MaintainMemory"],
            "positions": positions, "tick_model_calls": report.model_calls(),
            "endpoint_calls_before": calls, "endpoint_calls_after": model.calls()
        });
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            PathBuf::from(directory).join("tick.json"),
            serde_json::to_vec_pretty(&artifact).unwrap(),
        )
        .unwrap();
    }
    drop(store);
    drop(harness);
    drop(runtime);
    let restarted = build(&config);
    assert_eq!(restarted.services().cognition.stable_objects(), stable);
    drop(restarted);
    remove_database(&path);
}

#[test]
fn c7_duplicate_signals_and_restarts_never_rerun_a_window() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("unused");
    let path = database_path("duplicate");
    let config = config("owner:b1-duplicate", &path, model.address);
    let runtime = build(&config);
    let store = runtime.services().store.clone();
    record_failure(
        &store,
        "run:b1-duplicate",
        "workspace:b1-dup",
        p::Impact::High,
    );

    let now = now_ms();
    let first = ran(runtime.cognitive_tick_at(now).unwrap());
    assert!(!first.failed());
    let proposals = count(&store, p::EventKind::ProactiveProposalEmitted);
    let candidates = count(&store, p::EventKind::CandidateCreated);
    assert!(proposals > 0);
    let total = all_events(&store).len();
    for signal in [now, now + 1, now + TICK_INTERVAL_MS - 1] {
        assert!(
            matches!(
                runtime.cognitive_tick_at(signal).unwrap(),
                CognitiveTickOutcome::NotClaimed { .. }
            ),
            "a duplicate signal inside the cadence never opens a window"
        );
    }
    assert_eq!(all_events(&store).len(), total);
    assert!(runtime.scheduler_pass(now + 30_000).cognitive.is_none());

    // The fulfilled window and the cadence survive a restart.
    drop(store);
    drop(runtime);
    let runtime = build(&config);
    let store = runtime.services().store.clone();
    assert!(matches!(
        runtime.cognitive_tick_at(now + 1_000).unwrap(),
        CognitiveTickOutcome::NotClaimed { .. }
    ));
    assert_eq!(
        count(&store, p::EventKind::ProactiveProposalEmitted),
        proposals
    );
    assert_eq!(count(&store, p::EventKind::CandidateCreated), candidates);

    // The next window has nothing new: every phase is unchanged, the tick
    // appends nothing, and no model is called.
    let calls = model.calls();
    let quiet = ran(runtime.cognitive_tick_at(now + TICK_INTERVAL_MS).unwrap());
    assert!(quiet
        .phases
        .iter()
        .all(|phase| phase.status == CognitiveTickPhaseStatus::NoChange));
    assert_eq!(quiet.event_refs().count(), 0);
    assert!(events(&store, &quiet.run).is_empty());
    assert_eq!(quiet.model_calls(), 0);
    assert_eq!(model.calls(), calls);
    assert_eq!(
        count(&store, p::EventKind::ProactiveProposalEmitted),
        proposals
    );
    drop(store);
    drop(runtime);
    remove_database(&path);
}

#[test]
fn c7_a_foreground_session_defers_heavy_phases_and_keeps_their_inputs() {
    let _env = TestEnvironment::new();
    let model = ModelServer::held("foreground answer");
    let path = database_path("foreground");
    let config = config("owner:b1-foreground", &path, model.address);
    let runtime = build(&config);
    let store = runtime.services().store.clone();
    let harness = runtime.harness();
    record_failure(
        &store,
        "run:b1-foreground",
        "workspace:b1-fg",
        p::Impact::High,
    );
    let now = now_ms();
    runtime
        .services()
        .intentions
        .proactive
        .create(ProspectiveIntention {
            schema_version: p::SchemaVersion(1),
            id: p::IntentionId("intention:b1-due".into()),
            source: p::IntentionSource::Commitment,
            trigger: IntentionTrigger::At(now - 1),
            state: IntentionState::Pending,
            seed: p::SeedRef("seed:b1-due-review".into()),
            provenance: owner_provenance(),
            expires_at: Some(now + 3_600_000),
        })
        .unwrap();

    // An owner turn is in flight on its own thread: its model call is held
    // open, so its session is in the foreground while the tick runs.
    let foreground = {
        let harness = harness.clone();
        std::thread::spawn(move || {
            harness.submit_run(request(
                "session:b1-foreground",
                "keep working",
                "b1:fg:turn",
            ))
        })
    };
    model.wait_for_calls(1);

    let deferred = ran(runtime.cognitive_tick_at(now).unwrap());
    assert_eq!(
        statuses(&deferred),
        vec![
            (
                CognitiveTickPhase::ObserveAndPropose,
                CognitiveTickPhaseStatus::Ran
            ),
            (
                CognitiveTickPhase::ReflectAndCandidate,
                CognitiveTickPhaseStatus::Deferred
            ),
            (
                CognitiveTickPhase::MaintainMemory,
                CognitiveTickPhaseStatus::Deferred
            ),
        ]
    );
    for phase in &deferred.phases[1..] {
        assert!(phase.deferred_reason.is_some());
        assert!(phase.event_refs.is_empty());
    }
    // Only the due commitment ran; failure observations waited.
    let tick = events(&store, &deferred.run);
    assert_eq!(
        tick.iter()
            .filter(|event| event.kind == p::EventKind::ProactiveProposalEmitted)
            .count(),
        1
    );
    assert!(tick.iter().all(|event| !matches!(
        event.kind,
        p::EventKind::ObservationRecorded
            | p::EventKind::ReflectionProduced
            | p::EventKind::MemoryMaintenanceApplied
    )));
    assert_eq!(deferred.consumed, deferred.event_range.from);
    assert_eq!(deferred.model_calls(), 0);
    assert_eq!(model.calls(), 1, "the tick added no model call");

    // The foreground turn was never blocked by the tick.
    model.release();
    let turn = foreground.join().unwrap().unwrap();
    harness.wait(turn).unwrap();

    // The next window, without a foreground session, reads the kept inputs.
    let resumed = ran(runtime.cognitive_tick_at(now + TICK_INTERVAL_MS).unwrap());
    assert_eq!(resumed.event_range.from, deferred.event_range.from);
    assert_eq!(
        resumed
            .phase(CognitiveTickPhase::ReflectAndCandidate)
            .unwrap()
            .status,
        CognitiveTickPhaseStatus::Ran
    );
    let cognition_stream = runtime.services().aggregates.memory_cognition.clone();
    assert!(events(&store, &cognition_stream)
        .iter()
        .any(|event| matches!(&event.payload,
            p::EventPayload::CandidateCreated(payload)
                if payload.target.0.starts_with("cognitive-map|workspace:b1-fg|blindspot|"))));
    assert!(events(&store, &resumed.run)
        .iter()
        .any(|event| event.kind == p::EventKind::ObservationRecorded));
    drop(store);
    drop(harness);
    drop(runtime);
    remove_database(&path);
}

#[test]
fn c7_a_crashed_window_retries_its_recorded_range_and_never_repeats_a_proposal() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("unused");
    let path = database_path("retry");
    let config = config("owner:b1-retry", &path, model.address);
    let runtime = build(&config);
    let store = runtime.services().store.clone();
    let owner = owner_digest(&runtime);

    // A pressing unknown outcome is governed first; a failure with a blank
    // scope then poisons the window, so every attempt fails part-way.
    append(
        &store,
        "run:b1-unknown",
        "run:b1-unknown:accepted",
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId("session:b1-unknown".into()),
            input_ref: p::InputRef("rotate the preview certificate".into()),
            idempotency_key: None,
        }),
    );
    append(
        &store,
        "run:b1-unknown",
        "run:b1-unknown:unknown",
        p::EventPayload::ActionOutcomeUnknown(p::ActionOutcomeUnknownPayload {
            intent_id: p::ActionId("action:b1-unknown".into()),
            probe_hint: p::ProbeHintRef("probe:certificate-state".into()),
            remote_lease: None,
        }),
    );
    record_failure(&store, "run:b1-poisoned", "   ", p::Impact::High);

    let start = now_ms();
    let first = ran(runtime.cognitive_tick_at(start).unwrap());
    assert!(first.failed(), "the poisoned window fails: {first:?}");
    let failed_at = first
        .phases
        .iter()
        .position(|phase| phase.status == CognitiveTickPhaseStatus::Failed)
        .unwrap();
    assert!(first.phases[failed_at + 1..]
        .iter()
        .all(|phase| phase.status == CognitiveTickPhaseStatus::Skipped));
    assert!(matches!(
        store.tick_window(&owner, start).unwrap(),
        Some(CognitiveTickWindowState::Claimed { attempt: 1, .. })
    ));
    let proposals = count(&store, p::EventKind::ProactiveProposalEmitted);
    assert!(proposals > 0, "the unknown outcome was surfaced first");

    // Input that arrives later belongs to a later window, not to a retry.
    record_failure(
        &store,
        "run:b1-late",
        "workspace:b1-late",
        p::Impact::Medium,
    );
    assert!(matches!(
        runtime.cognitive_tick_at(start + 1_000).unwrap(),
        CognitiveTickOutcome::NotClaimed { .. }
    ));
    for attempt in 2..=3_u32 {
        let now = start + i64::from(attempt - 1) * (TICK_LEASE_MS + 1);
        let retry = ran(runtime.cognitive_tick_at(now).unwrap());
        assert_eq!(retry.claim.window, start);
        assert_eq!(retry.claim.attempt, attempt);
        assert_eq!(retry.event_range, first.event_range);
        assert!(retry.failed());
        assert_eq!(
            count(&store, p::EventKind::ProactiveProposalEmitted),
            proposals,
            "a retried window never repeats a proposal"
        );
    }

    // After its attempts are exhausted the window closes without running,
    // so it can no longer stall the windows after it.
    let closed_at = start + 3 * (TICK_LEASE_MS + 1);
    let closed = ran(runtime.cognitive_tick_at(closed_at).unwrap());
    assert_eq!(closed.claim.attempt, 4);
    assert!(closed
        .phases
        .iter()
        .all(|phase| phase.status == CognitiveTickPhaseStatus::Skipped));
    assert!(matches!(
        store.tick_window(&owner, start).unwrap(),
        Some(CognitiveTickWindowState::Finished { attempt: 4, .. })
    ));
    let late = ran(runtime
        .cognitive_tick_at(closed_at + TICK_INTERVAL_MS)
        .unwrap());
    assert!(!late.failed());
    assert!(late
        .event_range
        .to
        .0
        .contains_key(&p::RunId("run:b1-late".into())));
    assert!(!late
        .event_range
        .to
        .0
        .contains_key(&p::RunId("run:b1-poisoned".into())));
    assert_eq!(
        late.phase(CognitiveTickPhase::ReflectAndCandidate)
            .unwrap()
            .status,
        CognitiveTickPhaseStatus::Ran
    );
    drop(store);
    drop(runtime);
    remove_database(&path);
}

#[test]
fn c7_a_recovered_window_keeps_its_cognition_snapshot_and_judgment_time() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("unused");
    let path = database_path("snapshot");
    let config = config("owner:b1-snapshot", &path, model.address);
    let runtime = build(&config);
    let store = runtime.services().store.clone();
    let owner = owner_digest(&runtime);
    let start = now_ms();
    // Claim before any stable cognition exists, then simulate loss of the
    // process before it runs the phases. An object arrives after this head.
    let range = forme_store::TickEventRange {
        from: forme_store::TickEventCursor::default(),
        to: store.run_heads().unwrap(),
    };
    store
        .claim_tick(forme_store::CognitiveTickClaimRequest {
            owner_digest: owner,
            window: start,
            lease_nonce: p::Nonce("nonce:before-snapshot".into()),
            now_ms: start,
            lease_ttl: p::DurationMs(1),
            range: range.clone(),
            min_spacing: p::DurationMs(TICK_INTERVAL_MS as u64),
        })
        .unwrap()
        .unwrap();
    promote_stale_frame(&runtime, "frame:late-snapshot");
    let before = count(&store, p::EventKind::ReevaluationTaskCreated);
    let retried = ran(runtime.cognitive_tick_at(start + 1_000).unwrap());
    assert_eq!(retried.event_range, range);
    assert_eq!(retried.claim.window, start);
    assert_eq!(
        retried
            .phase(CognitiveTickPhase::MaintainMemory)
            .unwrap()
            .status,
        CognitiveTickPhaseStatus::NoChange
    );
    assert_eq!(count(&store, p::EventKind::ReevaluationTaskCreated), before);
    assert!(runtime.services().cognition.stable_objects()[0].active);

    let next = ran(runtime.cognitive_tick_at(start + TICK_INTERVAL_MS).unwrap());
    assert_eq!(
        next.phase(CognitiveTickPhase::MaintainMemory)
            .unwrap()
            .status,
        CognitiveTickPhaseStatus::Ran
    );
    assert!(!runtime.services().cognition.stable_objects()[0].active);
    assert_eq!(model.calls(), 0);
    // The frozen API does not change shape or fabricate a report on a
    // duplicate signal; the HTTP companion reports NotClaimed instead.
    assert!(runtime.tick_once().is_err());
    assert!(matches!(
        runtime.signal_cognitive_tick().unwrap(),
        CognitiveTickOutcome::NotClaimed { .. }
    ));
    drop(store);
    drop(runtime);
    remove_database(&path);
}

#[test]
fn c7_an_unknown_action_outcome_is_surfaced_and_never_retried() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("unused");
    let path = database_path("unknown");
    let config = config("owner:b1-unknown", &path, model.address);
    let run = p::RunId("run:b1-crashed-action".into());
    let intent = p::ActionId("action:b1-crashed-shell".into());

    let first = build(&config);
    let store = first.services().store.clone();
    append(
        &store,
        &run.0,
        "b1-crash:accepted",
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId("session:b1-crash".into()),
            input_ref: p::InputRef("run the migration".into()),
            idempotency_key: None,
        }),
    );
    append(
        &store,
        &run.0,
        "b1-crash:started",
        p::EventPayload::ActionStarted(p::ActionStartedPayload {
            intent_id: intent.clone(),
            backend: p::BackendKind::Shell,
            scope: p::Scope("workspace:default".into()),
            remote_lease: None,
        }),
    );
    // The process dies after the side effect may have begun.
    drop(store);
    drop(first);

    let runtime = build(&config);
    let store = runtime.services().store.clone();
    let action_events = |store: &SqliteEventStore| {
        events(store, &run)
            .into_iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    p::EventKind::ActionPlanned
                        | p::EventKind::ActionStarted
                        | p::EventKind::ActionCompleted
                        | p::EventKind::ActionFailed
                )
            })
            .count()
    };
    let before = action_events(&store);
    let started = count(&store, p::EventKind::ActionStarted);

    let report = ran(runtime.cognitive_tick_at(now_ms()).unwrap());
    assert!(!report.failed());
    let tick = events(&store, &report.run);
    assert!(tick
        .iter()
        .any(|event| event.kind == p::EventKind::ProactiveProposalEmitted));
    let cognition_stream = runtime.services().aggregates.memory_cognition.clone();
    assert!(events(&store, &cognition_stream)
        .iter()
        .any(|event| matches!(&event.payload,
            p::EventPayload::CandidateCreated(payload)
                if payload.target.0.contains("|blindspot|outcome-unknown failures"))));
    assert_eq!(action_events(&store), before, "the tick never retries");
    assert_eq!(count(&store, p::EventKind::ActionStarted), started);
    assert!(
        runtime.harness().wait(run.clone()).is_err(),
        "the unknown outcome still awaits an owner ruling"
    );
    drop(store);
    drop(runtime);
    remove_database(&path);
}
