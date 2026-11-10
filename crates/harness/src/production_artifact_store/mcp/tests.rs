use super::*;
use crate::production_artifact_store::worker_fixture::{now, owner, Fixture};
use forme_execution::{ActionBackend, ActionStatus, CancelToken, EventSink};
use std::sync::{atomic::Ordering, Arc};
#[path = "fixture.rs"]
mod fixture;
use fixture::*;

#[test]
fn discovery_material_is_encrypted_idempotent_and_reopens_without_new_keys() {
    let f = Fixture::new();
    let original = original();
    let saved = save(&f, &original).unwrap();
    assert_eq!(saved.content_ref, id(&original));
    let body = f.body(&saved.content_ref).unwrap();
    assert!(!body
        .envelope
        .ciphertext
        .windows(original.bytes.len())
        .any(|bytes| bytes == original.bytes));
    let allocations = f.keys.allocations.load(Ordering::SeqCst);
    let mut later = original.clone();
    later.observation.observed_at += 100;
    assert_eq!(save(&f, &later).unwrap(), saved);
    assert_eq!(f.body(&saved.content_ref).unwrap(), body);
    assert_eq!(f.keys.allocations.load(Ordering::SeqCst), allocations);
    let root = f.root.clone();
    drop(f);
    let f = Fixture::open(root);
    assert_eq!(save(&f, &later).unwrap(), saved);
    assert_eq!(f.keys.allocations.load(Ordering::SeqCst), 0);
    assert_eq!(
        read_material_original(&f.store, &saved.content_ref, &owner())
            .unwrap()
            .unwrap()
            .1,
        original.bytes
    );
    assert_eq!(
        std::fs::read_dir(f.root.0.join("artifacts"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn discovery_material_partial_writes_resume_only_the_same_original() {
    for cut in 1..=3 {
        let f = Fixture::new();
        let original = original();
        f.keys.fail_at.store(cut, Ordering::SeqCst);
        assert!(save(&f, &original).is_err());
        let old = f.body(&id(&original));
        assert_eq!(old.is_some(), cut > 1);
        assert!(read_material_original(&f.store, &id(&original), &owner())
            .unwrap()
            .is_none());
        let root = f.root.clone();
        drop(f);
        let f = Fixture::open(root);
        save(&f, &original).unwrap();
        if let Some(old) = old {
            assert_eq!(f.body(&id(&original)).unwrap(), old);
        }
        assert_eq!(
            read_material_original(&f.store, &id(&original), &owner())
                .unwrap()
                .unwrap()
                .1,
            original.bytes
        );
    }
}

#[test]
fn discovery_material_conflicts_cannot_replace_body_metadata_or_registration() {
    for component in 0..3 {
        let f = Fixture::new();
        let original = original();
        let target = id(&original);
        let object = match component {
            0 => managed_mcp_material_object(&original.request, &original.manifest).unwrap(),
            1 => metadata_object(&target),
            _ => receipt_object(&target),
        };
        let conflict = f.put(&object, b"conflicting fixture material");
        assert!(save(&f, &original).is_err());
        assert_eq!(f.body(&conflict.content_ref).unwrap(), conflict);
    }
}

#[test]
fn discovery_material_rejects_changed_terminal_source_and_owner_before_persistence() {
    let f = Fixture::new();
    let original = original();
    for change in 0..9 {
        let mut changed = original.clone();
        match change {
            0 => changed.observation.nonce = p::Nonce("nonce:foreign".into()),
            1 => changed.observation.receipt_ref = Some("receipt:foreign".into()),
            2 => changed.manifest.binding.permit_digest = p::canonical_digest(&"other").unwrap(),
            3 => changed.bytes[0] ^= 1,
            4 => {
                changed.observation.state = p::WorkerState::Unknown;
                changed.observation.outcome = Some(p::WorkerOutcome::Unknown);
            }
            5 => changed.manifest.binding.owner_ref = p::VerifiedPrincipal("owner:foreign".into()),
            6 => changed.manifest.evidence_refs = vec!["evidence:other".into()],
            7 => {
                changed.manifest.binding.normalized_action_digest =
                    Some(p::canonical_digest(&"other").unwrap())
            }
            _ => changed.request.execution.as_mut().unwrap().intent.source = p::Source::UserTurn,
        }
        changed.manifest.manifest_digest = changed.manifest.expected_digest().unwrap();
        assert!(save(&f, &changed).is_err(), "mutation {change}");
    }
    let mut foreign = original.clone();
    let other = p::VerifiedPrincipal("owner:other".into());
    foreign.request.job.owner_ref = other.clone();
    foreign.request.permit.owner_ref = other.clone();
    foreign.manifest.binding.owner_ref = other;
    foreign.manifest.binding.permit_digest = p::canonical_digest(&foreign.request.permit).unwrap();
    foreign.manifest.manifest_digest = foreign.manifest.expected_digest().unwrap();
    validate_managed_mcp_material(
        &foreign.request,
        &foreign.observation,
        &foreign.manifest,
        &foreign.bytes,
    )
    .unwrap();
    assert!(save(&f, &foreign).is_err());
    assert_eq!(f.keys.allocations.load(Ordering::SeqCst), 0);
}

#[test]
fn discovery_material_pending_and_completed_deletion_fences_survive_restart() {
    for partial in [false, true] {
        let f = Fixture::new();
        let original = original();
        save(&f, &original).unwrap();
        let lifecycle = f.protection.lifecycle_service(f.store.clone());
        let request = f.deletion();
        let plan = lifecycle.plan(&request).unwrap();
        assert_eq!(plan.objects.len(), 3);
        f.keys.fail_destroy.store(partial, Ordering::SeqCst);
        assert_eq!(
            lifecycle.apply_plan(request.clone(), &plan.digest).is_err(),
            partial
        );
        assert!(save(&f, &original).is_err());
        assert!(read_material_original(&f.store, &id(&original), &owner())
            .unwrap()
            .is_none());
        let root = f.root.clone();
        drop(lifecycle);
        drop(f);
        let f = Fixture::open(root);
        if partial {
            f.protection
                .lifecycle_service(f.store.clone())
                .apply_plan(request, &plan.digest)
                .unwrap();
        }
        assert!(save(&f, &original).is_err());
        assert!(read_material_original(&f.store, &id(&original), &owner())
            .unwrap()
            .is_none());
    }
}

#[test]
fn discovery_backend_persists_original_before_completion_and_replay_never_dispatches() {
    let f = Fixture::new();
    let (worker, transport) = backend(f.sink.clone());
    let plan = worker.plan(&intent()).unwrap();
    let store = f.store.clone();
    let sink = EventSink::with_observer(move |event| {
        if let p::EventPayload::ActionCompleted(value) = event {
            let receipt = value.receipt.as_ref().unwrap();
            let id = receipt.content_ref.as_ref().unwrap();
            let (_, bytes) = read_material_original(&store, id, &owner())?.unwrap();
            assert_eq!(
                receipt.content_digest,
                Some(p::sha256_content_digest(&bytes))
            );
            assert_eq!(receipt.trust, p::TrustTier::Untrusted);
        }
        Ok(())
    });
    let result = worker
        .execute(plan.clone(), &sink, CancelToken::default())
        .unwrap();
    assert_eq!(result.status, ActionStatus::Completed);
    assert_eq!(
        worker
            .execute(plan, &EventSink::default(), CancelToken::default())
            .unwrap()
            .external_receipt,
        result.external_receipt
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert!(!sink.events().iter().any(|e| matches!(
        e,
        p::EventPayload::McpDiscovered(_) | p::EventPayload::McpCallEvent(_)
    )));
    assert!(!serde_json::to_string(&sink.events())
        .unwrap()
        .contains("untrusted fixture description"));
}

struct WrongReference(Arc<ProtectedRuntimeArtifacts>);
impl ManagedMcpDiscoveryConsumer for WrongReference {
    fn persist_discovery(
        &self,
        request: &p::WorkerDispatchRequest,
        observation: &p::WorkerObservation,
        manifest: &p::WorkerResultManifest,
        bytes: &[u8],
    ) -> p::Result<ArtifactRecord> {
        let mut record = self
            .0
            .persist_discovery(request, observation, manifest, bytes)?;
        record.content_ref = p::ContentRef("content:substituted".into());
        Ok(record)
    }
}
#[test]
fn discovery_backend_rejects_consumer_failure_and_substitution_without_second_dispatch() {
    for wrong in [false, true] {
        let f = Fixture::new();
        let consumer: Arc<dyn ManagedMcpDiscoveryConsumer> = if wrong {
            Arc::new(WrongReference(f.sink.clone()))
        } else {
            f.keys.fail_at.store(1, Ordering::SeqCst);
            f.sink.clone()
        };
        let (worker, transport) = backend(consumer);
        let plan = worker.plan(&intent()).unwrap();
        let sink = EventSink::default();
        assert_eq!(
            worker
                .execute(plan, &sink, CancelToken::default())
                .unwrap()
                .status,
            ActionStatus::Unknown
        );
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        assert!(!sink
            .events()
            .iter()
            .any(|event| matches!(event, p::EventPayload::ActionCompleted(_))));
    }
}
#[test]
fn discovery_completion_append_failure_never_adds_a_contradictory_unknown() {
    let f = Fixture::new();
    let (worker, transport) = backend(f.sink.clone());
    let plan = worker.plan(&intent()).unwrap();
    let sink = EventSink::with_observer(|event| {
        if matches!(event, p::EventPayload::ActionCompleted(_)) {
            return Err(p::Error("fixture rejects completion append".into()));
        }
        Ok(())
    });
    assert!(worker.execute(plan, &sink, CancelToken::default()).is_err());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert!(!sink
        .events()
        .iter()
        .any(|event| matches!(event, p::EventPayload::ActionOutcomeUnknown(_))));
    assert!(f.body(&id(&transport.original())).is_some());
}

#[path = "source_tests.rs"]
mod source_tests;
