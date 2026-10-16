//! B1 partial-write recovery uses the same durable facts online and on replay.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex,
};

use forme_cognition::{
    ChangeDirection, CognitiveMapStore, CognitiveMapUpdateProposal, CognitiveRuntime,
    GovernanceCandidate, GovernanceEvidence, MapScope, MapUpdateKind, StableCognition,
    StableCognitionKind, Tick,
};
use forme_memory::{CandidateSpec, EventSourcedMemory};
use forme_protocol as p;
use forme_store::{
    EventCursor, EventStore, Projection, ProjectionScope, ReplayReport, SchemaSnapshot,
    SqliteEventStore, StoreOptions,
};

struct InterruptCandidate {
    store: SqliteEventStore,
    armed: AtomicBool,
    pause: Mutex<Option<Arc<AuditPause>>>,
}

#[derive(Default)]
struct AuditPause {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl AuditPause {
    fn hold(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).unwrap();
        }
    }
    fn wait(&self) {
        let (state, timeout) = self
            .changed
            .wait_timeout_while(
                self.state.lock().unwrap(),
                std::time::Duration::from_secs(5),
                |state| !state.0,
            )
            .unwrap();
        assert!(
            state.0 && !timeout.timed_out(),
            "maintenance did not reach its controlled pause"
        );
    }
    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

impl EventStore for InterruptCandidate {
    fn append(&self, event: p::Event) -> p::Result<p::EventId> {
        if event.kind == p::EventKind::ReevaluationTaskCreated {
            let pause = self.pause.lock().unwrap().take();
            if let Some(pause) = pause {
                pause.hold();
            }
        }
        if event.kind == p::EventKind::CandidateCreated && self.armed.swap(false, Ordering::SeqCst)
        {
            return Err(p::Error("candidate fixture stopped before append".into()));
        }
        self.store.append(event)
    }
    fn read_run(&self, run: p::RunId) -> EventCursor {
        self.store.read_run(run)
    }
    fn project<P: Projection>(&self, scope: ProjectionScope) -> p::Result<P::State> {
        self.store.project::<P>(scope)
    }
    fn replay(&self, run: p::RunId, at: SchemaSnapshot) -> p::Result<ReplayReport> {
        self.store.replay(run, at)
    }
}

fn run() -> p::RunId {
    p::RunId("brain:b1-recovery".into())
}
fn scope() -> MapScope {
    MapScope {
        schema_version: p::SchemaVersion(1),
        scope: p::Scope("workspace:recovery".into()),
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
fn store() -> Arc<InterruptCandidate> {
    Arc::new(InterruptCandidate {
        store: SqliteEventStore::open_in_memory(StoreOptions::default()).unwrap(),
        armed: AtomicBool::new(false),
        pause: Mutex::new(None),
    })
}
type Memory = EventSourcedMemory<InterruptCandidate>;
type Cognition = CognitiveRuntime<InterruptCandidate>;
fn open(store: &Arc<InterruptCandidate>) -> (Arc<Memory>, Cognition) {
    let memory = Arc::new(Memory::with_clock(store.clone(), run(), || 100).unwrap());
    let cognition = Cognition::open(store.clone(), memory.clone(), run()).unwrap();
    (memory, cognition)
}
fn events(store: &InterruptCandidate) -> Vec<p::Event> {
    store
        .read_run(run())
        .collect::<p::Result<Vec<_>>>()
        .unwrap()
}
fn promote(memory: &Memory, cognition: &Cognition, id: &str, reproduced: i64) {
    let evidence = vec![GovernanceEvidence {
        schema_version: p::SchemaVersion(1),
        reference: p::EvidenceRef("evidence:checked".into()),
        observed_at: 10,
        verified_process: true,
    }];
    memory
        .create_candidate_spec(CandidateSpec {
            schema_version: p::SchemaVersion(1),
            id: p::CandidateId(id.into()),
            target: p::CandidateTargetRef("stable:frame:recovery".into()),
            evidence_refs: vec![evidence[0].reference.clone()],
            confidence: p::Confidence(0.9),
            provenance: provenance(),
            target_tier: p::StabilityTier::Stable,
        })
        .unwrap();
    cognition
        .govern_and_apply(GovernanceCandidate {
            schema_version: p::SchemaVersion(1),
            candidate_id: p::CandidateId(id.into()),
            direction: ChangeDirection::CautionIncreasing,
            confidence: p::Confidence(0.9),
            evidence: evidence.clone(),
            impact: p::Impact::Low,
            provenance: provenance(),
            conflicts: Vec::new(),
            owner_confirmed: true,
            target: StableCognition {
                schema_version: p::SchemaVersion(1),
                object: p::ObjectRef("frame:recovery".into()),
                scope: scope(),
                kind: StableCognitionKind::Frame,
                statement: "verify the build output".into(),
                tier: p::StabilityTier::Stable,
                confidence: p::Confidence(0.9),
                evidence,
                last_reproduced_at: reproduced,
                active: false,
            },
        })
        .unwrap();
}
fn tick() -> Tick {
    Tick {
        schema_version: p::SchemaVersion(1),
        now: 1_000,
        stale_after_ms: 100,
    }
}

#[test]
fn maintenance_recovers_after_invalidation_before_candidate_without_duplicate_facts() {
    let store = store();
    let (memory, cognition) = open(&store);
    promote(&memory, &cognition, "candidate:original", 10);
    let head = events(&store).last().unwrap().stream_seq;
    let snapshot = cognition.stable_snapshot_at(head).unwrap();
    store.armed.store(true, Ordering::SeqCst);
    assert!(cognition.audit_stale_snapshot(tick(), &snapshot).is_err());
    assert!(!cognition.stable_objects()[0].active);
    drop(cognition);
    drop(memory);

    let (_memory, cognition) = open(&store);
    assert_eq!(cognition.stable_snapshot_at(head).unwrap(), snapshot);
    let candidates = cognition.audit_stale_snapshot(tick(), &snapshot).unwrap();
    assert_eq!(candidates.len(), 1);
    let recorded = events(&store);
    assert_eq!(
        recorded
            .iter()
            .filter(|event| event.kind == p::EventKind::FailureEvidenceRecorded)
            .count(),
        1
    );
    assert_eq!(
        recorded
            .iter()
            .filter(|event| event.kind == p::EventKind::ReevaluationTaskCreated)
            .count(),
        1
    );
    assert_eq!(
        recorded
            .iter()
            .filter(|event| matches!(&event.payload,
        p::EventPayload::CandidateCreated(payload) if payload.candidate_id == candidates[0]))
            .count(),
        1
    );
    assert_eq!(
        cognition.audit_stale_snapshot(tick(), &snapshot).unwrap(),
        candidates
    );
    assert_eq!(events(&store), recorded, "retry appends no duplicate facts");
    let (_memory, replayed) = open(&store);
    assert_eq!(replayed.stable_objects(), cognition.stable_objects());
}

#[test]
fn an_old_snapshot_never_invalidates_a_newly_reproduced_object() {
    let store = store();
    let (memory, cognition) = open(&store);
    promote(&memory, &cognition, "candidate:old", 10);
    let head = events(&store).last().unwrap().stream_seq;
    let snapshot = cognition.stable_snapshot_at(head).unwrap();
    promote(&memory, &cognition, "candidate:new", 999);
    let before = events(&store);
    assert!(cognition
        .audit_stale_snapshot(tick(), &snapshot)
        .unwrap()
        .is_empty());
    assert_eq!(events(&store), before);
    assert!(cognition.stable_objects()[0].active);
    assert_eq!(cognition.stable_objects()[0].last_reproduced_at, 999);
    assert_eq!(cognition.stable_snapshot_at(head).unwrap(), snapshot);
    assert!(cognition.stable_snapshot_at(100_000).is_err());
}

#[test]
fn maintenance_and_owner_promotion_commit_in_one_semantic_order() {
    let store = store();
    let (memory, cognition) = open(&store);
    let cognition = Arc::new(cognition);
    promote(&memory, &cognition, "candidate:before-race", 10);
    let snapshot = cognition.stable_objects();
    let pause = Arc::new(AuditPause::default());
    *store.pause.lock().unwrap() = Some(pause.clone());
    let auditing = {
        let cognition = cognition.clone();
        std::thread::spawn(move || cognition.audit_stale_snapshot(tick(), &snapshot).unwrap())
    };
    pause.wait();
    let (entered, waiting) = std::sync::mpsc::channel();
    let (finished, completion) = std::sync::mpsc::channel();
    let promoting = {
        let memory = memory.clone();
        let cognition = cognition.clone();
        std::thread::spawn(move || {
            entered.send(()).unwrap();
            promote(&memory, &cognition, "candidate:after-race", 999);
            finished.send(()).unwrap();
        })
    };
    waiting
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let premature = completion.recv_timeout(std::time::Duration::from_millis(100));
    pause.release();
    assert!(
        matches!(premature, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
        "a new promotion must wait for the pending maintenance command"
    );
    assert_eq!(auditing.join().unwrap().len(), 1);
    promoting.join().unwrap();
    let recorded = events(&store);
    let invalidation = recorded
        .iter()
        .find(|event| event.kind == p::EventKind::ReevaluationTaskCreated)
        .unwrap();
    let promotion = recorded
        .iter()
        .rev()
        .find(|event| event.kind == p::EventKind::CandidatePromoted)
        .unwrap();
    assert!(invalidation.stream_seq < promotion.stream_seq);
    assert!(cognition.stable_objects()[0].active);
    assert!(cognition.is_object_usable(&p::ObjectRef("frame:recovery".into())));
    assert_eq!(cognition.stable_objects()[0].last_reproduced_at, 999);
    let (_memory, replayed) = open(&store);
    assert_eq!(cognition.stable_objects(), replayed.stable_objects());
}

#[test]
fn reflection_recovers_its_candidate_and_rejects_changed_recorded_facts() {
    let store = store();
    let (memory, cognition) = open(&store);
    let proposal = CognitiveMapUpdateProposal {
        schema_version: p::SchemaVersion(1),
        candidate_id: Some(p::CandidateId("candidate:reflection".into())),
        scope: scope(),
        kind: MapUpdateKind::BlindSpot,
        confidence: p::Confidence(0.3),
        evidence: vec![p::EvidenceRef("failure:checked".into())],
        frame: None,
        quality: None,
        blindspot: Some(p::BlindSpotModelRef("blindspot:recovery".into())),
        resource: None,
        reflection_inputs: vec![p::EvidenceRef("failure:checked".into())],
        statement: "a failure needs a stricter verification step".into(),
        provenance: provenance(),
    };
    store.armed.store(true, Ordering::SeqCst);
    assert!(cognition.propose_update(proposal.clone()).is_err());
    drop(cognition);
    drop(memory);
    let (memory, cognition) = open(&store);
    let before = events(&store);
    let mut changed = proposal.clone();
    changed.confidence = p::Confidence(0.8);
    assert!(cognition.propose_update(changed).is_err());
    assert_eq!(events(&store), before);
    let id = cognition.propose_update(proposal.clone()).unwrap();
    assert_eq!(
        memory.candidate_record(&id).unwrap().spec.confidence,
        p::Confidence(0.3)
    );
    cognition.propose_update(proposal).unwrap();
    let recorded = events(&store);
    assert_eq!(
        recorded
            .iter()
            .filter(|event| event.kind == p::EventKind::ReflectionProduced)
            .count(),
        1
    );
    assert_eq!(
        recorded
            .iter()
            .filter(|event| event.kind == p::EventKind::CognitiveMapUpdateProposed)
            .count(),
        1
    );
    assert_eq!(
        recorded
            .iter()
            .filter(|event| event.kind == p::EventKind::CandidateCreated)
            .count(),
        1
    );
}
