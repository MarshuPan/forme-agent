//! Real SQLite restart and durable owner-control fault windows.
use forme_execution::ExecutionBackendRegistry;
use forme_harness::{
    AgentHarness, GatewayControl, GovernanceConfig, HarnessConfig, ReactiveHarness, ResumeInput,
};
use forme_loop::{HandoffResolution, LoopState, ResolvedOutcome};
use forme_models::{
    Cost, ModelCapability, ModelHandoff, ModelOutput, ModelProfile, ModelProvider, ModelRequest,
    ModelResponse, ModelStrength, RateLimit, ScriptedModelProvider, Url,
};
use forme_protocol as p;
use forme_store::{
    ContentKeyResolver, EventStore, FileContentKeyResolver, SqliteEventStore,
    StoreContentProtection, StoreOptions,
};
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct Database(PathBuf);
static DATABASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
impl Database {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forme-control-recovery-{}-{}",
            std::process::id(),
            DATABASE_SEQUENCE.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn open(&self) -> SqliteEventStore {
        let owner = p::VerifiedPrincipal("owner:control-test".into());
        let protection = StoreContentProtection::new(
            &owner,
            p::Scope("workspace:control-test".into()),
            Arc::new(FileContentKeyResolver::new(self.0.join("keys")).unwrap()),
        )
        .unwrap();
        SqliteEventStore::open_with_content_protection(
            self.0.join("events.sqlite"),
            StoreOptions::default(),
            protection,
        )
        .unwrap()
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn profile() -> ModelProfile {
    ModelProfile {
        schema_version: p::SchemaVersion(1),
        provider: p::ProviderId("control-test".into()),
        model: "bounded".into(),
        base_url: Url::parse("https://control.invalid/v1").unwrap(),
        capability: ModelCapability {
            schema_version: p::SchemaVersion(1),
            context_window: 16384,
            tool_use: true,
            strength: ModelStrength::Standard,
        },
        cost: Cost {
            schema_version: p::SchemaVersion(1),
            input_microunits_per_million: 1,
            output_microunits_per_million: 1,
        },
        rate_limit: RateLimit {
            schema_version: p::SchemaVersion(1),
            requests_per_minute: 60,
            tokens_per_minute: 100000,
        },
        credential_ref: p::CredentialRef("secret:control-test".into()),
    }
}
struct CountModel {
    inner: ScriptedModelProvider,
    calls: Arc<AtomicUsize>,
}
impl p::ExternalProvider for CountModel {
    fn kind(&self) -> p::ProviderKind {
        p::ExternalProvider::kind(&self.inner)
    }
    fn id(&self) -> p::ProviderId {
        p::ExternalProvider::id(&self.inner)
    }
    fn declared_capabilities(&self) -> p::CapabilitySet {
        p::ExternalProvider::declared_capabilities(&self.inner)
    }
    fn trust_default(&self) -> p::TrustTier {
        p::ExternalProvider::trust_default(&self.inner)
    }
}
impl p::ModelProvider for CountModel {}
impl ModelProvider for CountModel {
    fn call(&self, request: ModelRequest) -> p::Result<ModelResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.call(request)
    }
    fn profile(&self) -> ModelProfile {
        self.inner.profile()
    }
}
fn harness(
    store: SqliteEventStore,
    outputs: Vec<ModelOutput>,
) -> (Arc<ReactiveHarness>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let model = CountModel {
        calls: calls.clone(),
        inner: ScriptedModelProvider::new(
            profile(),
            outputs
                .into_iter()
                .map(|output| ModelResponse {
                    schema_version: p::SchemaVersion(1),
                    output,
                    usage: p::ModelUsage {
                        input_tokens: 4,
                        output_tokens: 1,
                    },
                    finish_reason: p::FinishReason("stop".into()),
                })
                .collect(),
        )
        .unwrap(),
    };
    (
        Arc::new(
            ReactiveHarness::new(
                store,
                Arc::new(model),
                Arc::new(ExecutionBackendRegistry::default()),
                GovernanceConfig::default(),
                HarnessConfig::for_model(&profile()),
            )
            .unwrap(),
        ),
        calls,
    )
}
fn append(store: &SqliteEventStore, run: &p::RunId, name: &str, payload: p::EventPayload) {
    store
        .append(p::Event::new(
            p::EventId(format!("{}:{name}", run.0)),
            run.clone(),
            None,
            payload,
            p::SchemaVersion(1),
            1,
            p::Provenance {
                source: p::Source::Internal,
                actor: p::Actor::System,
                trust_tier: p::TrustTier::VerifiedProcess,
                caused_by: None,
            },
        ))
        .unwrap();
}
fn seed(store: &SqliteEventStore, run: &str) -> p::RunId {
    let run = p::RunId(run.into());
    append(
        store,
        &run,
        "accepted",
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId(format!("session:{}", run.0)),
            input_ref: p::InputRef("recover a bounded action".into()),
            idempotency_key: None,
        }),
    );
    append(
        store,
        &run,
        "started",
        p::EventPayload::ActionStarted(p::ActionStartedPayload {
            intent_id: action(&run),
            backend: p::BackendKind::Shell,
            scope: p::Scope("workspace:test".into()),
            remote_lease: None,
        }),
    );
    run
}
fn action(run: &p::RunId) -> p::ActionId {
    p::ActionId(format!("action:{}", run.0))
}
fn command(run: &p::RunId, key: &str) -> p::RunControl {
    p::RunControl::ResolveToolOutcome {
        command: p::IdempotencyKey(key.into()),
        resolution: p::ToolOutcomeResolution {
            schema_version: p::SchemaVersion(1),
            action: action(run),
            outcome: p::ToolOutcomeKind::Completed,
            result_ref: Some(p::ActionResultRef("owner-result".into())),
            failure_ref: None,
            reason: None,
            summary: "owner confirmed completion".into(),
        },
    }
}
fn claim(store: &SqliteEventStore, run: &p::RunId, command: &p::RunControl) {
    let events = store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap();
    let checkpoint = events
        .iter()
        .rev()
        .find(|e| {
            matches!(
                e.kind,
                p::EventKind::ActionOutcomeUnknown | p::EventKind::HandoffRequested
            )
        })
        .unwrap();
    let key = match command {
        p::RunControl::ResolveToolOutcome { command, .. }
        | p::RunControl::ResolveHandoff { command, .. } => command,
        _ => panic!(),
    };
    let owner_digest = test_owner_digest();
    let binding = p::ControlResolutionBinding {
        schema_version: p::SchemaVersion(1),
        command_digest: p::canonical_digest(&("control-resolution-v1", key)).unwrap(),
        resolution_digest: p::canonical_digest(&(&owner_digest, run, command)).unwrap(),
        target_digest: p::canonical_digest(&(run, &checkpoint.event_id)).unwrap(),
    };
    let event = p::Event::new(
        p::EventId(format!("{}:claim", run.0)),
        run.clone(),
        None,
        p::EventPayload::RunResumed(p::RunResumedPayload {
            wait_reason: p::WaitReason("owner_resolution_claim".into()),
            resume_ref: binding.as_ref(),
        }),
        p::SchemaVersion(1),
        2,
        p::Provenance {
            source: p::Source::OwnerControl,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    );
    assert!(store.append(event.clone()).is_err());
    store
        .append_control_resolution_claim(event, command.clone())
        .unwrap();
}
fn test_owner_digest() -> p::SchemaDigest {
    p::sha256_content_digest(b"owner:control-test")
}
fn control_resolution_ref(command: &p::RunControl) -> p::ContentRef {
    let key = match command {
        p::RunControl::ResolveToolOutcome { command, .. }
        | p::RunControl::ResolveHandoff { command, .. } => command,
        _ => panic!(),
    };
    let command_digest = p::canonical_digest(&("control-resolution-v1", key)).unwrap();
    let object = format!(
        "run-control-resolution:{}:{}",
        test_owner_digest().0,
        command_digest.0
    );
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(object.as_bytes()).0
    ))
}
fn events(store: &SqliteEventStore, run: &p::RunId) -> Vec<p::Event> {
    store
        .read_run(run.clone())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
}
fn handoff() -> ModelOutput {
    ModelOutput::Handoff(ModelHandoff {
        schema_version: p::SchemaVersion(1),
        target: p::HandoffTargetRef("target:reviewer".into()),
        reason: p::ReasonRef("bounded review".into()),
    })
}
fn handoff_command(key: &str) -> p::RunControl {
    p::RunControl::ResolveHandoff {
        command: p::IdempotencyKey(key.into()),
        resolution: p::HandoffResolutionRequest {
            schema_version: p::SchemaVersion(1),
            target: p::HandoffTargetRef("target:reviewer".into()),
            accepted: true,
            result: Some("review complete".into()),
        },
    }
}
fn submit(h: &ReactiveHarness) -> p::RunId {
    h.submit_run(p::RunRequest {
        schema_version: p::SchemaVersion(1),
        source: p::Source::UserTurn,
        session: p::SessionRef("session:handoff".into()),
        agent_profile: p::AgentProfileRef("agent:test".into()),
        input: p::RunInput("review one step".into()),
        budget: None,
        idempotency_key: None,
    })
    .unwrap()
}

#[test]
fn concurrent_duplicate_resolution_commits_once_and_replays_after_sqlite_restart() {
    let db = Database::new();
    let store = db.open();
    let run = seed(&store, "run:duplicate");
    let (h, calls) = harness(store.clone(), vec![]);
    let c = command(&run, "command:one");
    std::thread::scope(|s| {
        let a = s.spawn(|| h.control(run.clone(), c.clone()));
        let b = s.spawn(|| h.control(run.clone(), c.clone()));
        a.join().unwrap().unwrap();
        b.join().unwrap().unwrap();
    });
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let before = events(&store, &run);
    assert_eq!(
        before
            .iter()
            .filter(|e| e.kind == p::EventKind::ActionCompleted)
            .count(),
        1
    );
    assert_eq!(
        before
            .iter()
            .filter(|e| p::ControlResolutionBinding::from_event(e)
                .unwrap()
                .is_some())
            .count(),
        1
    );
    drop(h);
    drop(store);
    let store = db.open();
    let (h, calls) = harness(store.clone(), vec![]);
    h.control(run.clone(), c.clone()).unwrap();
    assert_eq!(events(&store, &run), before);
    assert_eq!(h.wait(run.clone()).unwrap().status, p::RunStatus::Complete);
    let mut changed = c;
    if let p::RunControl::ResolveToolOutcome { resolution, .. } = &mut changed {
        resolution.summary = "different claim".into();
    }
    assert!(h.control(run.clone(), changed).is_err());
    let other = seed(&store, "run:cross-run");
    h.recover_unknown_outcomes().unwrap();
    assert!(h
        .control(other.clone(), command(&other, "command:one"))
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn different_commands_cannot_adjudicate_one_checkpoint_twice() {
    let db = Database::new();
    let store = db.open();
    let run = seed(&store, "run:race");
    let (h, calls) = harness(store.clone(), vec![]);
    let results = std::thread::scope(|s| {
        let a = s.spawn(|| h.control(run.clone(), command(&run, "command:a")));
        let b = s.spawn(|| h.control(run.clone(), command(&run, "command:b")));
        [a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        events(&store, &run)
            .iter()
            .filter(|e| e.kind == p::EventKind::ActionCompleted)
            .count(),
        1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn claim_without_result_survives_restart_and_only_exact_command_can_finish_adjudication() {
    let db = Database::new();
    let store = db.open();
    let run = seed(&store, "run:claimed");
    let (h, _) = harness(store.clone(), vec![]);
    drop(h);
    let c = command(&run, "command:uncertain");
    claim(&store, &run, &c);
    drop(store);
    let store = db.open();
    let (h, calls) = harness(store.clone(), vec![]);
    let before = events(&store, &run);
    let claim_event = before
        .iter()
        .find(|event| {
            p::ControlResolutionBinding::from_event(event)
                .unwrap()
                .is_some()
        })
        .unwrap();
    assert_eq!(
        store
            .read_control_resolution_claim(&run, claim_event)
            .unwrap(),
        Some(c.clone())
    );
    h.control(run.clone(), c).unwrap();
    assert!(h
        .control(run.clone(), command(&run, "command:replacement"))
        .is_err());
    assert!(h
        .resume(
            run.clone(),
            ResumeInput::ToolOutcome(
                action(&run),
                ResolvedOutcome::Completed(p::ActionResultRef("replacement".into()), "new".into())
            )
        )
        .is_err());
    assert_eq!(
        events(&store, &run)
            .iter()
            .filter(|e| e.kind == p::EventKind::ActionCompleted)
            .count(),
        1
    );
    assert_eq!(
        h.run_summary(run.clone()).unwrap().status,
        p::RunStatus::Complete
    );
    assert!(matches!(
        p::project_run_pending(run, &before).unwrap().pending,
        Some(p::RunPendingKind::ResolutionUnknown {
            reason: p::ResolutionUncertainty::ClaimWithoutResult,
            ..
        })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn companion_write_rolls_back_when_claim_event_cannot_be_appended() {
    let db = Database::new();
    let store = db.open();
    let run = seed(&store, "run:claim-rollback");
    let (h, _) = harness(store.clone(), vec![]);
    drop(h);
    let existing_id = events(&store, &run)
        .iter()
        .find(|event| event.kind == p::EventKind::ActionStarted)
        .unwrap()
        .event_id
        .clone();
    let c = command(&run, "command:rollback");
    let checkpoint = events(&store, &run)
        .iter()
        .find(|event| event.kind == p::EventKind::ActionOutcomeUnknown)
        .unwrap()
        .event_id
        .clone();
    let key = match &c {
        p::RunControl::ResolveToolOutcome { command, .. } => command,
        _ => unreachable!(),
    };
    let binding = p::ControlResolutionBinding {
        schema_version: p::SchemaVersion(1),
        command_digest: p::canonical_digest(&("control-resolution-v1", key)).unwrap(),
        resolution_digest: p::canonical_digest(&(&test_owner_digest(), &run, &c)).unwrap(),
        target_digest: p::canonical_digest(&(&run, &checkpoint)).unwrap(),
    };
    let event = p::Event::new(
        existing_id,
        run.clone(),
        None,
        p::EventPayload::RunResumed(p::RunResumedPayload {
            wait_reason: p::WaitReason("owner_resolution_claim".into()),
            resume_ref: binding.as_ref(),
        }),
        p::SchemaVersion(1),
        3,
        p::Provenance {
            source: p::Source::OwnerControl,
            actor: p::Actor::Owner,
            trust_tier: p::TrustTier::OwnerInput,
            caused_by: None,
        },
    );
    assert!(store
        .append_control_resolution_claim(event, c.clone())
        .is_err());
    assert!(store
        .protected_content_for_current_owner(&control_resolution_ref(&c))
        .unwrap()
        .is_none());
    assert!(!events(&store, &run)
        .iter()
        .any(|event| p::ControlResolutionBinding::from_event(event)
            .unwrap()
            .is_some()));
}

#[test]
fn unreadable_resolution_companion_stays_unknown_and_rejects_replacement() {
    let db = Database::new();
    let store = db.open();
    let run = seed(&store, "run:claim-unreadable");
    let (h, _) = harness(store.clone(), vec![]);
    drop(h);
    let original = command(&run, "command:unreadable");
    claim(&store, &run, &original);
    let claim_event = events(&store, &run)
        .into_iter()
        .find(|event| {
            p::ControlResolutionBinding::from_event(event)
                .unwrap()
                .is_some()
        })
        .unwrap();
    let record = store
        .protected_content_for_current_owner(&control_resolution_ref(&original))
        .unwrap()
        .unwrap();
    drop(store);

    let keys = FileContentKeyResolver::new(db.0.join("keys")).unwrap();
    keys.destroy(&record.envelope.key_digest).unwrap();

    let store = db.open();
    assert!(store
        .read_control_resolution_claim(&run, &claim_event)
        .is_err());
    let (h, _) = harness(store.clone(), vec![]);
    let before = events(&store, &run);
    assert!(h
        .control(run.clone(), command(&run, "command:replacement-unreadable"))
        .is_err());
    assert_eq!(events(&store, &run), before);
}

#[test]
fn adjudicated_action_without_run_terminal_finishes_without_calling_model_or_driver() {
    for outcome in [
        p::ToolOutcomeKind::Completed,
        p::ToolOutcomeKind::Failed,
        p::ToolOutcomeKind::Cancelled,
    ] {
        let db = Database::new();
        let store = db.open();
        let run = seed(&store, "run:terminal-window");
        let (h, _) = harness(store.clone(), vec![]);
        drop(h);
        let mut c = command(&run, "command:window");
        if let p::RunControl::ResolveToolOutcome { resolution: r, .. } = &mut c {
            r.outcome = outcome;
            r.failure_ref = Some(p::FailureEvidenceRef("failure:owner".into()));
            r.reason = Some(p::ReasonRef("cancel:owner".into()));
        }
        claim(&store, &run, &c);
        let terminal = match outcome {
            p::ToolOutcomeKind::Completed => {
                p::EventPayload::ActionCompleted(p::ActionCompletedPayload {
                    intent_id: action(&run),
                    result_ref: p::ActionResultRef("owner-result".into()),
                    receipt: None,
                    remote_receipt: None,
                })
            }
            p::ToolOutcomeKind::Failed => p::EventPayload::ActionFailed(p::ActionFailedPayload {
                intent_id: action(&run),
                failure_ref: p::FailureEvidenceRef("failure:owner".into()),
                remote_lease: None,
            }),
            p::ToolOutcomeKind::Cancelled => {
                p::EventPayload::ActionCancelled(p::ActionCancelledPayload {
                    intent_id: action(&run),
                    reason: p::ReasonRef("cancel:owner".into()),
                })
            }
        };
        append(&store, &run, "action-terminal", terminal);
        drop(store);
        let store = db.open();
        let (h, calls) = harness(store.clone(), vec![]);
        let expected = match outcome {
            p::ToolOutcomeKind::Completed => p::RunStatus::Complete,
            p::ToolOutcomeKind::Failed => p::RunStatus::Failed,
            p::ToolOutcomeKind::Cancelled => p::RunStatus::Aborted,
        };
        assert_eq!(h.wait(run.clone()).unwrap().status, expected);
        h.control(run.clone(), c).unwrap();
        let before = events(&store, &run);
        assert!(h.recover_unknown_outcomes().unwrap().is_empty());
        assert_eq!(events(&store, &run), before);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn handoff_restores_only_original_binding_and_duplicate_never_repeats_continuation() {
    let db = Database::new();
    let store = db.open();
    let (h, _) = harness(store.clone(), vec![handoff()]);
    let run = submit(&h);
    drop(h);
    drop(store);
    let store = db.open();
    let (h, calls) = harness(store.clone(), vec![ModelOutput::Final("integrated".into())]);
    assert!(matches!(
        h.state(run.clone()).unwrap(),
        LoopState::Suspended(_)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let c = handoff_command("command:handoff");
    h.control(run.clone(), c.clone()).unwrap();
    h.control(run.clone(), c).unwrap();
    assert_eq!(h.wait(run.clone()).unwrap().status, p::RunStatus::Complete);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        events(&store, &run)
            .iter()
            .filter(|e| e.kind == p::EventKind::HandoffResolved)
            .count(),
        1
    );
}

#[test]
fn resolved_handoff_crash_is_inspect_or_cancel_without_continuation_replay() {
    let db = Database::new();
    let store = db.open();
    let (h, _) = harness(store.clone(), vec![handoff()]);
    let run = submit(&h);
    drop(h);
    let c = handoff_command("command:handoff-crash");
    claim(&store, &run, &c);
    append(
        &store,
        &run,
        "resolved",
        p::EventPayload::HandoffResolved(p::HandoffResolvedPayload {
            target: p::HandoffTargetRef("target:reviewer".into()),
            reason: p::ReasonRef("handoff accepted".into()),
        }),
    );
    drop(store);
    let store = db.open();
    let (h, calls) = harness(store.clone(), vec![]);
    h.control(run.clone(), c).unwrap();
    assert!(h
        .resume(
            run.clone(),
            ResumeInput::Handoff(HandoffResolution {
                schema_version: p::SchemaVersion(1),
                target: p::HandoffTargetRef("target:reviewer".into()),
                accepted: true,
                result: None
            })
        )
        .is_err());
    assert_eq!(
        h.run_summary(run.clone()).unwrap().status,
        p::RunStatus::Waiting
    );
    assert!(matches!(
        p::project_run_pending(run.clone(), &events(&store, &run))
            .unwrap()
            .pending,
        Some(p::RunPendingKind::ResolutionUnknown {
            reason: p::ResolutionUncertainty::HandoffContinuation,
            ..
        })
    ));
    h.cancel(run.clone()).unwrap();
    assert_eq!(h.wait(run).unwrap().status, p::RunStatus::Aborted);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn waiting_does_not_become_running_when_unrelated_observations_arrive() {
    let db = Database::new();
    let store = db.open();
    let run = seed(&store, "run:waiting");
    let (h, _) = harness(store.clone(), vec![]);
    append(
        &store,
        &run,
        "observation",
        p::EventPayload::ObservationRecorded(p::ObservationRecordedPayload {
            source: p::Source::Internal,
            scope: p::Scope("workspace:test".into()),
            grant_ref: None,
            learning_reply: None,
        }),
    );
    assert_eq!(h.run_summary(run).unwrap().status, p::RunStatus::Waiting);
}

#[test]
fn stale_handoff_reply_cannot_consume_a_later_reused_target() {
    let db = Database::new();
    let store = db.open();
    let (h, calls) = harness(store.clone(), vec![handoff(), handoff()]);
    let run = submit(&h);
    let c = handoff_command("command:first");
    h.control(run.clone(), c.clone()).unwrap();
    h.control(run.clone(), c).unwrap();
    assert!(h
        .control(run.clone(), handoff_command("command:stale"))
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(matches!(
        p::project_run_pending(run.clone(), &events(&store, &run))
            .unwrap()
            .pending,
        Some(p::RunPendingKind::ResolutionUnknown {
            reason: p::ResolutionUncertainty::ReusedHandoffTarget,
            ..
        })
    ));
}

#[test]
fn interrupted_model_call_becomes_an_explicit_failed_run_without_retry_or_fake_usage() {
    let db = Database::new();
    let store = db.open();
    let run = p::RunId("run:model-outcome-unknown".into());
    append(
        &store,
        &run,
        "accepted",
        p::EventPayload::RunAccepted(p::RunAcceptedPayload {
            source: p::Source::UserTurn,
            session_ref: p::SessionId("session:model-outcome-unknown".into()),
            input_ref: p::InputRef("continue the interrupted request".into()),
            idempotency_key: None,
        }),
    );
    append(
        &store,
        &run,
        "model-started",
        p::EventPayload::ModelCallStarted(p::ModelCallStartedPayload {
            call_id: p::ModelCallId("call:model-outcome-unknown".into()),
            model_profile: p::ModelProfileRef("model:primary".into()),
        }),
    );
    drop(store);

    let store = db.open();
    let (harness, calls) = harness(
        store.clone(),
        vec![ModelOutput::Final("must not be used".into())],
    );
    let observed = events(&store, &run);
    assert_eq!(observed.len(), 3);
    assert_eq!(
        observed
            .iter()
            .filter(|event| event.kind == p::EventKind::ModelCallStarted)
            .count(),
        1
    );
    assert!(!observed
        .iter()
        .any(|event| event.kind == p::EventKind::ModelCallFinished));
    assert!(matches!(
        observed.last().map(|event| &event.payload),
        Some(p::EventPayload::RunFailed(payload))
            if payload.stop_reason.0 == "model_call_outcome_unknown_after_restart"
                && payload.result_ref.is_none()
    ));
    assert_eq!(
        harness.run_summary(run.clone()).unwrap().status,
        p::RunStatus::Failed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let before = observed;
    assert!(harness.recover_unknown_outcomes().unwrap().is_empty());
    assert_eq!(events(&store, &run), before);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
