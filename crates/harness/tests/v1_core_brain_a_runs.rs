//! V1 closure A run-level scenarios (C1/C2/C3/C5/C6) driven through the
//! formal production composition and a real HTTP model endpoint fixture.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use forme_cognition::{CognitiveMapStore, CognitiveMapUpdateProposal, MapScope, MapUpdateKind};
use forme_context::ContextHydrator;
use forme_harness::{
    AgentHarness, CapabilityRuntimeConfig, CognitiveRuntimeConfig, DataProtectionConfig,
    McpRuntimeConfig, ProductionRuntime, ProductionRuntimeBuilder, ProductionRuntimeConfig,
    RuntimeServiceStatus, SchedulerGatewayControl,
};
use forme_models::{ModelRuntimeConfig, ModelStrength};
use forme_protocol as p;
use forme_store::{EventStore, SqliteEventStore, StoreOptions};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static NEXT_PATH: AtomicU64 = AtomicU64::new(1);
const MODEL_KEY_ENV: &str = "FORME_V1A_RUNS_MODEL_KEY";

struct TestEnvironment {
    _lock: MutexGuard<'static, ()>,
}

impl TestEnvironment {
    fn new() -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::set_var(MODEL_KEY_ENV, "v1a-runs-model-secret");
        Self { _lock: lock }
    }
}

struct ModelServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    handle: Option<JoinHandle<()>>,
}

impl ModelServer {
    fn start(answer: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (stop_flag, captured) = (stop.clone(), requests.clone());
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
            requests,
            handle: Some(handle),
        }
    }

    fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn request(&self, index: usize) -> String {
        self.requests.lock().unwrap()[index].clone()
    }
}

impl Drop for ModelServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn database_path(label: &str) -> PathBuf {
    let tag = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "forme-v1a-runs-{label}-{}-{tag}-{}.db",
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
        agent_profile: p::AgentProfileRef("agent:v1a-runs".into()),
        models: vec![ModelRuntimeConfig {
            id: "model:v1a-runs".into(),
            provider: "v1a-runs-provider".into(),
            model: "v1a-runs-model".into(),
            base_url: format!("http://{model}/v1"),
            credential_env: MODEL_KEY_ENV.into(),
            context_window: 32_768,
            tool_use: false,
            strength: ModelStrength::Standard,
            input_microunits_per_million: 0,
            output_microunits_per_million: 0,
            requests_per_minute: 600,
            tokens_per_minute: 1_000_000,
            timeout_ms: 5_000,
            fallback_group: Some("v1a-runs".into()),
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
        cognition: CognitiveRuntimeConfig::default(),
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
        agent_profile: p::AgentProfileRef("agent:v1a-runs".into()),
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

fn events(store: &SqliteEventStore, run: &p::RunId) -> Vec<p::Event> {
    store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
}

fn total_events(store: &SqliteEventStore) -> usize {
    store
        .run_ids()
        .unwrap()
        .iter()
        .map(|run| events(store, run).len())
        .sum()
}

fn session_bound(store: &SqliteEventStore, run: &p::RunId) -> p::SessionBoundPayload {
    events(store, run)
        .into_iter()
        .find_map(|event| match event.payload {
            p::EventPayload::SessionBound(payload) => Some(payload),
            _ => None,
        })
        .expect("run is bound")
}

fn now_ms() -> p::Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn c1_startup_recovery_turns_a_crashed_action_into_one_unknown_without_driver_calls() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("unused");
    let path = database_path("crash");
    let config = config("owner:crash", &path, model.address);
    let run = p::RunId("run:crashed-before-terminal".into());
    let intent = p::ActionId("action:crashed-shell".into());

    let first = build(&config);
    let store = first.services().store.clone();
    for (id, payload) in [
        (
            "crash:accepted",
            p::EventPayload::RunAccepted(p::RunAcceptedPayload {
                source: p::Source::UserTurn,
                session_ref: p::SessionId("session:crash".into()),
                input_ref: p::InputRef("run the crashed command".into()),
                idempotency_key: None,
            }),
        ),
        (
            "crash:started",
            p::EventPayload::ActionStarted(p::ActionStartedPayload {
                intent_id: intent.clone(),
                backend: p::BackendKind::Shell,
                scope: p::Scope("workspace:default".into()),
                remote_lease: None,
            }),
        ),
    ] {
        store
            .append(p::Event::new(
                p::EventId(id.into()),
                run.clone(),
                None,
                payload,
                p::SchemaVersion(1),
                now_ms(),
                internal_provenance(),
            ))
            .unwrap();
    }
    // The process dies after the side effect may have begun.
    drop(store);
    drop(first);

    for restart in 0..2 {
        let runtime = build(&config);
        let recovered = events(&runtime.services().store, &run);
        let unknown = recovered
            .iter()
            .filter(|event| {
                matches!(&event.payload,
                    p::EventPayload::ActionOutcomeUnknown(payload) if payload.intent_id == intent)
            })
            .count();
        assert_eq!(
            unknown, 1,
            "restart {restart} must hold exactly one unknown"
        );
        assert!(recovered.iter().all(|event| !matches!(
            event.kind,
            p::EventKind::ActionCompleted | p::EventKind::ActionFailed | p::EventKind::RunComplete
        )));
        assert!(
            runtime.harness().wait(run.clone()).is_err(),
            "an unknown outcome is never reported as a terminal result"
        );
        drop(runtime);
    }
    assert_eq!(model.calls(), 0, "recovery never re-drives the model");
    remove_database(&path);
}

fn map_scope() -> MapScope {
    MapScope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:default".into()),
    }
}

fn map_proposal(statement: &str) -> CognitiveMapUpdateProposal {
    CognitiveMapUpdateProposal {
        schema_version: p::SchemaVersion(1),
        candidate_id: None,
        scope: map_scope(),
        kind: MapUpdateKind::Frame,
        confidence: p::Confidence(0.3),
        evidence: vec![p::EvidenceRef("trace:continuity".into())],
        frame: Some(p::JudgmentFrameRef("frame:continuity".into())),
        quality: None,
        blindspot: None,
        resource: None,
        reflection_inputs: vec![p::EvidenceRef("feedback:continuity".into())],
        statement: statement.into(),
        provenance: internal_provenance(),
    }
}

/// The canonical fact the memory service persists for a long-term node in
/// the owner workspace. Appended through the single-writer store, it is the
/// same event `add_node` produces.
fn memory_node_event(id: &str, content: &str) -> p::EventPayload {
    p::EventPayload::MemoryNodeAppended(p::MemoryNodeAppendedPayload {
        node_id: p::NodeId(id.into()),
        kind: p::MemoryNodeType("fact".into()),
        content_ref: p::ContentRef(content.into()),
        tier: p::StabilityTier::Working,
        confidence: p::Confidence(0.7),
        scope: p::Scope("memory|long-term|workspace:default|workspace:default".into()),
        resting_activation: p::RestingActivation(0.2),
        recency: p::Recency(10),
    })
}

fn context_request(label: &str) -> forme_context::ContextHydrationRequest {
    forme_context::ContextHydrationRequest {
        schema_version: p::SchemaVersion(1),
        run: p::RunId(format!("run:{label}")),
        session: p::SessionId(format!("session:{label}")),
        workspace: p::WorkspaceRef("workspace:default".into()),
        scope: p::Scope("workspace:default".into()),
        brain_call: true,
        selected_skills: Vec::new(),
        model: p::ModelProfileRef("model:v1a-runs".into()),
        budget: forme_context::ContextBudget {
            schema_version: p::SchemaVersion(1),
            max_tokens: 32_768,
            reserve: 1_024,
        },
    }
}

fn future_reminder(id: &str) -> p::ScheduleCommand {
    let due = now_ms() + 3_600_000;
    p::ScheduleCommand {
        schema_version: p::SchemaVersion(1),
        intention: p::ProspectiveIntention {
            schema_version: p::SchemaVersion(1),
            id: p::IntentionId(id.into()),
            source: p::IntentionSource::Commitment,
            trigger: p::IntentionTrigger::At(due),
            state: p::IntentionState::Pending,
            seed: p::SeedRef(format!("scheduled reminder {id}")),
            provenance: owner_provenance(),
            expires_at: Some(due + 30_000),
        },
        session: p::SessionId(format!("session:{id}")),
        envelope: reminder_envelope(due),
        budget: p::Budget("units:1".into()),
    }
}

fn reminder_envelope(starts_at: p::Timestamp) -> p::AutonomyEnvelope {
    p::AutonomyEnvelope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:default".into()),
        capability: p::CapabilitySet {
            schema_version: p::SchemaVersion(1),
            capabilities: vec![p::CapabilityRef("capability:local-notification".into())],
            permissions: vec![p::PermissionRef("permission:local-notification".into())],
        },
        action_type: vec![p::ActionType::Deliver],
        risk_limit: p::Risk::Low,
        approval_rule: p::ApprovalRule::Allow,
        budget: p::Budget("units:1".into()),
        timebox: p::Timebox {
            schema_version: p::SchemaVersion(1),
            starts_at,
            expires_at: starts_at + 3_600_000,
            max_turns: 2,
        },
        rollback: p::RollbackReq {
            schema_version: p::SchemaVersion(1),
            required: false,
            boundary: None,
        },
    }
}

#[test]
fn c2_owner_brain_state_continues_across_restart_without_synthetic_facts() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("unused");
    let path = database_path("continuity");
    let config = config("owner:continuity", &path, model.address);

    let first = build(&config);
    let services = first.services();
    let refs = first.manifest().aggregate_refs.clone();
    services
        .store
        .append(p::Event::new(
            p::EventId("c2:memory-node".into()),
            refs.memory_cognition.clone(),
            None,
            memory_node_event(
                "node:owner-prefers-evidence",
                "content:owner prefers evidence-first answers",
            ),
            p::SchemaVersion(1),
            now_ms(),
            owner_provenance(),
        ))
        .unwrap();
    let candidate = services
        .cognition
        .propose_update(map_proposal("continuity candidate stays a candidate"))
        .unwrap();
    // Owner steering available to the A-wave production brain: the owner
    // schedules commitments and withdraws one of them.
    let kept = SchedulerGatewayControl::schedule(
        first.harness().as_ref(),
        future_reminder("intention:continuity-kept"),
    )
    .unwrap();
    let withdrawn = SchedulerGatewayControl::schedule(
        first.harness().as_ref(),
        future_reminder("intention:continuity-withdrawn"),
    )
    .unwrap();
    forme_harness::SchedulerService::cancel(
        first.harness().as_ref(),
        withdrawn.clone(),
        p::Actor::Owner,
    )
    .unwrap();
    let jobs_before = SchedulerGatewayControl::list_jobs(first.harness().as_ref()).unwrap();
    let digest = first.manifest().digest.clone();
    let before = events(&services.store, &refs.memory_cognition);
    let candidate_before = services.memory.candidate_record(&candidate).unwrap();
    drop(services);
    drop(first);

    let second = build(&config);
    let services = second.services();
    assert_eq!(second.manifest().aggregate_refs, refs);
    assert_eq!(second.manifest().digest, digest);
    let after = events(&services.store, &refs.memory_cognition);
    assert_eq!(
        after
            .iter()
            .map(|event| &event.event_id)
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|event| &event.event_id)
            .collect::<Vec<_>>(),
        "a restart appends no synthetic brain facts"
    );
    let hydrated = services
        .context_hydrator
        .hydrate(&context_request("continuity"))
        .unwrap();
    assert!(
        hydrated.sources.memory_summary.raw_event_count >= 1,
        "the replayed memory fact reaches the formal context after restart"
    );
    // Ground truth: the next brain call recalls the fact written before the
    // restart and records the recall slice it actually assembled.
    let recalled = second
        .harness()
        .submit_run(request(
            "session:continuity",
            "what do I prefer?",
            "continuity:recall",
        ))
        .unwrap();
    second.harness().wait(recalled.clone()).unwrap();
    let finished = events(&services.store, &recalled)
        .into_iter()
        .find_map(|event| match event.payload {
            p::EventPayload::ContextBuildFinished(payload) => Some(payload),
            _ => None,
        })
        .unwrap();
    assert!(finished.slice_refs.contains(&p::ContextSliceRef(
        "memory:recall:node:owner-prefers-evidence".into()
    )));
    let request_text = model.request(model.calls() - 1);
    let (_, prompt) = request_text.split_once("\r\n\r\n").unwrap();
    assert!(prompt.contains("owner prefers evidence-first answers"));
    assert_eq!(
        services.memory.candidate_record(&candidate).unwrap(),
        candidate_before
    );
    let jobs_after = SchedulerGatewayControl::list_jobs(second.harness().as_ref()).unwrap();
    let state = |jobs: &[p::ScheduledJob], id: &p::IntentionId| {
        jobs.iter()
            .find(|job| &job.intention.id == id)
            .map(|job| job.intention.state)
    };
    assert_eq!(state(&jobs_after, &kept), state(&jobs_before, &kept));
    assert_eq!(
        state(&jobs_after, &withdrawn),
        state(&jobs_before, &withdrawn)
    );
    assert_ne!(
        state(&jobs_after, &kept),
        state(&jobs_after, &withdrawn),
        "the owner's withdrawal survives the restart"
    );

    // New facts continue the same authority-ordered stream.
    let last_seq = after.iter().map(|event| event.stream_seq).max().unwrap();
    services
        .cognition
        .propose_update(map_proposal("a fact proposed after restart"))
        .unwrap();
    let continued = events(&services.store, &refs.memory_cognition);
    let appended = continued
        .iter()
        .filter(|event| event.stream_seq > last_seq)
        .map(|event| event.stream_seq)
        .collect::<Vec<_>>();
    assert!(!appended.is_empty());
    assert_eq!(
        appended,
        (last_seq + 1..=last_seq + appended.len() as u64).collect::<Vec<_>>(),
        "stream_seq continues without gaps after restart"
    );
    drop(services);
    drop(second);
    remove_database(&path);
}

#[test]
fn c2_run_command_idempotency_is_durable_concurrent_and_conflict_safe() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("idempotent answer");
    let path = database_path("idempotency");
    let config = config("owner:idempotency", &path, model.address);

    let first = build(&config);
    let harness = first.harness();
    let original = harness
        .submit_run(request("session:idem", "first command", "command:one"))
        .unwrap();
    assert_eq!(
        harness.wait(original.clone()).unwrap().status,
        p::RunStatus::Complete
    );
    assert_eq!(model.calls(), 1);
    assert_eq!(
        harness
            .submit_run(request("session:idem", "first command", "command:one"))
            .unwrap(),
        original,
        "a replayed command returns its original receipt"
    );
    let store = first.services().store.clone();
    let written = total_events(&store);
    let conflict = harness
        .submit_run(request(
            "session:idem",
            "a different command",
            "command:one",
        ))
        .unwrap_err();
    assert!(conflict.0.contains("different request"), "{conflict:?}");
    assert_eq!(
        total_events(&store),
        written,
        "a conflicting reuse writes nothing"
    );

    let racers = (0..4)
        .map(|_| {
            let harness = harness.clone();
            std::thread::spawn(move || {
                harness
                    .submit_run(request("session:race", "raced command", "command:race"))
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let raced = racers
        .into_iter()
        .map(|racer| racer.join().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(raced.len(), 1, "concurrent submissions share one receipt");
    let raced = raced.into_iter().next().unwrap();
    harness.wait(raced).unwrap();
    let accepted_race = store
        .run_ids()
        .unwrap()
        .iter()
        .flat_map(|run| events(&store, run))
        .filter(|event| {
            matches!(&event.payload, p::EventPayload::RunAccepted(payload)
                if payload.idempotency_key == Some(p::IdempotencyKey("command:race".into())))
        })
        .count();
    assert_eq!(accepted_race, 1);
    assert_eq!(model.calls(), 2);
    drop(harness);
    drop(store);
    drop(first);

    let second = build(&config);
    let harness = second.harness();
    let store = second.services().store.clone();
    let written = total_events(&store);
    assert_eq!(
        harness
            .submit_run(request("session:idem", "first command", "command:one"))
            .unwrap(),
        original,
        "the durable ledger answers a replay after restart"
    );
    assert_eq!(
        harness.wait(original).unwrap().status,
        p::RunStatus::Complete,
        "the original terminal fact answers the replay"
    );
    assert!(harness
        .submit_run(request(
            "session:idem",
            "a different command",
            "command:one"
        ))
        .is_err());
    assert_eq!(
        total_events(&store),
        written,
        "replay and conflict write nothing"
    );
    assert_eq!(model.calls(), 2, "a replay never executes again");
    drop(harness);
    drop(store);
    drop(second);
    remove_database(&path);
}

#[test]
fn c3_formal_context_carries_history_rules_and_brain_slices_to_the_model() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("context answer");
    let path = database_path("context");
    let config = config("owner:context", &path, model.address);
    let runtime = build(&config);
    let harness = runtime.harness();
    let store = runtime.services().store.clone();

    let first = harness
        .submit_run(request(
            "session:context",
            "remember marker-alpha",
            "context:1",
        ))
        .unwrap();
    harness.wait(first.clone()).unwrap();
    let second = harness
        .submit_run(request("session:context", "what did I say?", "context:2"))
        .unwrap();
    harness.wait(second.clone()).unwrap();

    let finished = |run: &p::RunId| {
        events(&store, run)
            .into_iter()
            .find_map(|event| match event.payload {
                p::EventPayload::ContextBuildFinished(payload) => Some(payload),
                _ => None,
            })
            .expect("context build is recorded")
    };
    let cold = finished(&first);
    assert!(cold.sources.contains(&p::ContextSource::Rules));
    assert!(!cold.sources.contains(&p::ContextSource::History));
    assert!(
        !cold.slice_refs.is_empty(),
        "a formal brain call is never empty"
    );
    let warm = finished(&second);
    assert!(warm.sources.contains(&p::ContextSource::Rules));
    assert!(warm.sources.contains(&p::ContextSource::History));
    let slices = warm
        .slice_refs
        .iter()
        .map(|slice| slice.0.as_str())
        .collect::<BTreeSet<_>>();
    for expected in [
        "brain:identity",
        "brain:cognition-summary",
        "brain:workspace-state",
    ] {
        assert!(slices.contains(expected), "missing {expected}: {slices:?}");
    }

    // Ground truth: what the model endpoint actually received.
    assert_eq!(model.calls(), 2);
    let request = model.request(1);
    let prompt = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("the model request has a body");
    assert!(prompt.contains("marker-alpha"), "history reached the model");
    assert!(prompt.contains("Serve only the configured owner"));
    assert!(
        !prompt.contains("v1a-runs-model-secret"),
        "the credential travels only in the authorization header"
    );
    drop(harness);
    drop(store);
    drop(runtime);
    remove_database(&path);
}

#[test]
fn c5_scheduler_pass_runs_through_the_same_runtime_as_owner_turns() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("scheduled answer");
    let path = database_path("scheduler");
    let config = config("owner:scheduler", &path, model.address);
    let runtime = build(&config);
    let harness = runtime.harness();
    let store = runtime.services().store.clone();

    let turn = harness
        .submit_run(request("session:turn", "owner turn", "scheduler:turn"))
        .unwrap();
    harness.wait(turn.clone()).unwrap();

    let due = now_ms() - 1;
    let mut reminder = future_reminder("intention:due-now");
    reminder.intention.trigger = p::IntentionTrigger::At(due);
    reminder.intention.expires_at = Some(due + 60_000);
    reminder.envelope = reminder_envelope(due);
    let intention = SchedulerGatewayControl::schedule(harness.as_ref(), reminder).unwrap();
    let pass = runtime.scheduler_pass(now_ms());
    let report = pass.scheduled.expect("scheduler is enabled").unwrap();
    assert!(
        !report.started_runs.is_empty(),
        "the due intention started a run: {report:?}"
    );
    let scheduled = report.started_runs[0].clone();
    let _ = harness.wait(scheduled.clone());
    assert!(pass
        .cognitive
        .expect("the first pass claims the brain tick")
        .is_ok());
    assert!(
        runtime.scheduler_pass(now_ms()).cognitive.is_none(),
        "the brain tick honours its cadence"
    );

    let owner_bound = session_bound(&store, &turn);
    let scheduled_bound = session_bound(&store, &scheduled);
    assert_eq!(
        owner_bound.composition_manifest,
        Some(runtime.manifest().digest.clone())
    );
    assert_eq!(
        scheduled_bound.composition_manifest,
        owner_bound.composition_manifest
    );
    assert_eq!(scheduled_bound.policy_profile, owner_bound.policy_profile);
    assert_eq!(scheduled_bound.model_profile, owner_bound.model_profile);
    assert_eq!(scheduled_bound.toolset_ref, owner_bound.toolset_ref);
    assert_eq!(scheduled_bound.workspace, owner_bound.workspace);
    let source = |run: &p::RunId| {
        events(&store, run)
            .into_iter()
            .find_map(|event| match event.payload {
                p::EventPayload::RunAccepted(payload) => Some(payload.source),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(source(&turn), p::Source::UserTurn);
    assert_ne!(source(&scheduled), p::Source::UserTurn);
    assert!(SchedulerGatewayControl::list_jobs(harness.as_ref())
        .unwrap()
        .iter()
        .any(|job| job.intention.id == intention && job.run == Some(scheduled.clone())));
    drop(harness);
    drop(store);
    drop(runtime);
    remove_database(&path);
}

#[test]
fn c6_required_configuration_fails_closed_and_optional_providers_degrade() {
    let _env = TestEnvironment::new();
    let model = ModelServer::start("unused");

    let path = database_path("missing-credential");
    let mut missing_credential = config("owner:config", &path, model.address);
    missing_credential.models[0].credential_env = "FORME_V1A_RUNS_UNSET_KEY".into();
    std::env::remove_var("FORME_V1A_RUNS_UNSET_KEY");
    let error = match ProductionRuntimeBuilder::from_config(missing_credential)
        .and_then(|builder| builder.build())
    {
        Ok(_) => panic!("a default model without its credential must fail closed"),
        Err(error) => error,
    };
    assert!(!error.0.contains("v1a-runs-model-secret"));
    remove_database(&path);

    // An optional fallback profile without its credential degrades to a
    // Disabled doctor row and never becomes selectable.
    let mut optional_fallback = config("owner:config", &path, model.address);
    let mut fallback = optional_fallback.models[0].clone();
    fallback.id = "model:v1a-fallback".into();
    fallback.default = false;
    fallback.credential_env = "FORME_V1A_RUNS_UNSET_KEY".into();
    optional_fallback.models.push(fallback);
    let degraded = build(&optional_fallback);
    assert_eq!(
        degraded.manifest().model_profiles,
        vec![p::ModelProfileRef("model:v1a-runs".into())]
    );
    let row = degraded
        .doctor()
        .checks
        .into_iter()
        .find(|check| check.name == "model:model:v1a-fallback")
        .expect("the unavailable fallback is explained");
    assert_eq!(row.status, RuntimeServiceStatus::Disabled);
    assert!(!format!("{:?}", degraded.doctor()).contains("FORME_V1A_RUNS_UNSET_KEY"));
    drop(degraded);
    remove_database(&path);

    let mut no_owner = config("owner:config", &path, model.address);
    no_owner.owner = p::OwnerPrincipalRef(" ".into());
    assert!(ProductionRuntimeBuilder::from_config(no_owner).is_err());

    // A file where the store's parent directory should be makes the store
    // unwritable; composition must fail before anything is half-built.
    let blocker = database_path("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let unwritable = config("owner:config", &blocker.join("brain.db"), model.address);
    assert!(ProductionRuntimeBuilder::from_config(unwritable)
        .and_then(|builder| builder.build())
        .is_err());
    let _ = std::fs::remove_file(&blocker);

    let in_memory =
        ProductionRuntimeBuilder::from_config(config("owner:config", &path, model.address))
            .unwrap()
            .build_with_store(SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap());
    assert!(
        matches!(&in_memory, Err(error) if error.0.contains("durable")),
        "a temporary in-memory store can never host the production brain"
    );

    let path = database_path("missing-mcp");
    let mut missing_mcp = config("owner:config", &path, model.address);
    missing_mcp.capabilities.mcp = vec![McpRuntimeConfig {
        server: forme_capabilities::StdioMcpServer {
            schema_version: p::SchemaVersion(1),
            provider_id: p::ProviderId("mcp:missing".into()),
            command: path
                .with_extension("no-such-mcp-server.exe")
                .display()
                .to_string(),
            args: Vec::new(),
            env: Vec::new(),
            allowlist: forme_capabilities::McpAllowlist {
                schema_version: p::SchemaVersion(1),
                tools: vec!["read".into()],
                resources: Vec::new(),
            },
            timeout: p::DurationMs(1_000),
            declared_capabilities: p::CapabilitySet {
                schema_version: p::SchemaVersion(1),
                capabilities: vec![p::CapabilityRef("mcp:mcp:missing:read".into())],
                permissions: vec![p::PermissionRef("permission:mcp-read".into())],
            },
        },
        inherit_env: Vec::new(),
        enabled: true,
        trust: Some(p::TrustTier::ApprovedSource),
        grant: Some(reminder_envelope(0)),
        discover_on_start: true,
    }];
    missing_mcp.capabilities.mcp[0]
        .grant
        .as_mut()
        .unwrap()
        .budget = missing_mcp.capabilities.envelope_budget.clone();
    let runtime = build(&missing_mcp);
    let row = runtime
        .doctor()
        .checks
        .into_iter()
        .find(|check| check.name == "mcp:mcp:missing")
        .expect("the missing executable is explained");
    assert_eq!(row.status, RuntimeServiceStatus::Unsupported);
    assert!(runtime
        .services()
        .backends
        .backend(p::BackendKind::Mcp)
        .is_err());
    let report = events(
        &runtime.services().store,
        &runtime.manifest().aggregate_refs.memory_cognition,
    )
    .into_iter()
    .filter(|event| event.kind == p::EventKind::ConfigDoctorReport)
    .map(|event| serde_json::to_string(&event.payload).unwrap())
    .collect::<Vec<_>>();
    assert!(!report.is_empty(), "the doctor report enters the event log");
    for payload in &report {
        assert!(!payload.contains("v1a-runs-model-secret"));
        assert!(!payload.contains(MODEL_KEY_ENV));
    }
    drop(runtime);
    remove_database(&path);
    assert_eq!(model.calls(), 0);
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
