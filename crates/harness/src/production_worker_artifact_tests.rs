use super::*;
use forme_execution::{
    ActionBackend, ActionStatus, CancelToken, EventSink, ExecutionPlanner, OutputBudget,
};
use std::sync::{atomic::Ordering, Arc};
#[path = "production_worker_artifact_fixture.rs"]
mod fixture;
#[path = "production_worker_artifact_host.rs"]
mod host;
use fixture::*;
use host::HostFixture;

fn id(binding: &p::WorkerArtifactBinding) -> p::ContentRef {
    p::ContentRef(format!(
        "content:{}",
        p::sha256_content_digest(binding.object_id().unwrap().as_bytes()).0
    ))
}
fn read(f: &Fixture, id: &p::ContentRef) -> Vec<u8> {
    let record = f.body(id).unwrap();
    f.protection
        .content_service()
        .decrypt(&record.envelope.binding(), &record.envelope)
        .unwrap()
}

#[test]
fn worker_original_has_one_encrypted_identity_across_duplicate_writes_and_restart() {
    let f = Fixture::new();
    let binding = material();
    let original = png();
    let first = f.sink.write_worker_output(&binding, &original).unwrap();
    assert_eq!(first.content_ref, id(&binding));
    assert_eq!(first.digest, p::sha256_content_digest(&original));
    let body = f.body(&first.content_ref).unwrap();
    assert!(!body
        .envelope
        .ciphertext
        .windows(original.len())
        .any(|w| w == original));
    let before = f.keys.allocations.load(Ordering::SeqCst);
    assert_eq!(
        f.sink.write_worker_output(&binding, &original).unwrap(),
        first
    );
    assert_eq!(f.body(&first.content_ref).unwrap(), body);
    assert_eq!(f.keys.allocations.load(Ordering::SeqCst), before);
    assert_eq!(read(&f, &first.content_ref), original);
    assert_eq!(
        std::fs::read_dir(f.root.0.join("artifacts"))
            .unwrap()
            .count(),
        0
    );
    let root = f.root.clone();
    drop(f);
    let f = Fixture::open(root);
    assert_eq!(
        f.sink.write_worker_output(&binding, &original).unwrap(),
        first
    );
    assert_eq!(f.body(&first.content_ref).unwrap(), body);
    assert_eq!(f.keys.allocations.load(Ordering::SeqCst), 0);
}

#[test]
fn worker_partial_body_metadata_and_receipt_writes_recover_only_original_material() {
    for cut in 1..=3 {
        let f = Fixture::new();
        let binding = material();
        let target = id(&binding);
        f.keys.fail_at.store(cut, Ordering::SeqCst);
        assert!(f.sink.write_worker_output(&binding, &png()).is_err());
        let original = f.body(&target);
        assert_eq!(original.is_some(), cut > 1);
        assert_eq!(f.body(&worker_metadata_ref(&target)).is_some(), cut > 2);
        assert!(f.body(&receipt_ref(&target)).is_none());
        let root = f.root.clone();
        drop(f);
        let f = Fixture::open(root);
        let record = f.sink.write_worker_output(&binding, &png()).unwrap();
        if let Some(original) = original {
            assert_eq!(f.body(&target).unwrap(), original);
        }
        assert_eq!(record.content_ref, target);
        assert_eq!(read(&f, &target), png());
        assert!(read_worker_metadata(&f.store, &f.protection, &target)
            .unwrap()
            .is_some());
        assert!(registered_material(&f.store, &target, &scope())
            .unwrap()
            .is_some());
    }
}

#[test]
fn worker_conflicting_body_metadata_or_receipt_cannot_be_replaced() {
    for component in 0..3 {
        let f = Fixture::new();
        let binding = material();
        let target = id(&binding);
        let object = match component {
            0 => binding.object_id().unwrap(),
            1 => metadata_object(&target),
            _ => receipt_object(&target),
        };
        let conflict = f.put(&object, b"self-authored conflicting material");
        assert!(f.sink.write_worker_output(&binding, &png()).is_err());
        assert_eq!(f.body(&conflict.content_ref).unwrap(), conflict);
        assert_eq!(
            read(&f, &conflict.content_ref),
            b"self-authored conflicting material"
        );
    }
}

#[test]
fn worker_foreign_owner_changed_digest_and_invalid_png_never_register() {
    let f = Fixture::new();
    let original = material();
    let mut foreign = original.clone();
    foreign.manifest.binding.owner_ref = p::VerifiedPrincipal("owner:other".into());
    foreign.descriptor.binding.owner_ref = foreign.manifest.binding.owner_ref.clone();
    foreign.manifest.manifest_digest = foreign.manifest.expected_digest().unwrap();
    foreign.descriptor.manifest_digest = foreign.manifest.manifest_digest.clone();
    foreign.validate().unwrap();
    assert!(f.sink.write_worker_output(&foreign, &png()).is_err());
    assert!(f.body(&id(&foreign)).is_none());
    let mut changed = png();
    changed[15] ^= 1;
    assert!(f.sink.write_worker_output(&original, &changed).is_err());
    let mut corrupt = original.clone();
    corrupt.manifest.content_digest = Some(p::sha256_content_digest(&changed));
    corrupt.manifest.manifest_digest = corrupt.manifest.expected_digest().unwrap();
    corrupt.descriptor.content_digest = corrupt.manifest.content_digest.clone().unwrap();
    corrupt.descriptor.manifest_digest = corrupt.manifest.manifest_digest.clone();
    corrupt.validate().unwrap();
    assert!(f.sink.write_worker_output(&corrupt, &changed).is_err());
    assert!(f.body(&id(&corrupt)).is_none());
    assert!(f.body(&id(&original)).is_none());
    assert_eq!(f.keys.allocations.load(Ordering::SeqCst), 0);
}

#[test]
fn worker_pending_and_completed_deletion_fences_prevent_resurrection_after_restart() {
    for partial in [false, true] {
        let f = Fixture::new();
        let binding = material();
        let target = id(&binding);
        f.sink.write_worker_output(&binding, &png()).unwrap();
        let service = f.protection.lifecycle_service(f.store.clone());
        let request = f.deletion();
        let plan = service.plan(&request).unwrap();
        assert_eq!(plan.objects.len(), 3);
        if partial {
            f.keys.fail_destroy.store(true, Ordering::SeqCst);
        }
        let result = service.apply_plan(request.clone(), &plan.digest);
        assert_eq!(result.is_err(), partial);
        if let Ok(receipt) = result {
            assert_eq!(
                receipt.remote_disposition,
                p::RemoteDeletionDisposition::Unknown
            );
        }
        assert!(f.sink.write_worker_output(&binding, &png()).is_err());
        assert!(f.body(&target).is_none());
        let root = f.root.clone();
        drop(service);
        drop(f);
        let f = Fixture::open(root);
        assert!(f.sink.write_worker_output(&binding, &png()).is_err());
        if partial {
            f.protection
                .lifecycle_service(f.store.clone())
                .apply_plan(request, &plan.digest)
                .unwrap();
            assert!(f.sink.write_worker_output(&binding, &png()).is_err());
        }
        assert!(read_worker_metadata(&f.store, &f.protection, &target)
            .unwrap()
            .is_none());
        assert!(registered_material(&f.store, &target, &scope())
            .unwrap()
            .is_none());
    }
}

#[test]
fn worker_host_original_png_reaches_protected_core_before_completion_with_no_second_capture() {
    let f = Fixture::new();
    let h = HostFixture::new(&f, Some(f.sink.clone()));
    let plan = h.prepare();
    let store = f.store.clone();
    let events = EventSink::with_observer(move |event| {
        if let p::EventPayload::ActionCompleted(value) = event {
            let receipt = value.receipt.as_ref().unwrap();
            let target = receipt.content_ref.as_ref().unwrap();
            let (material, body) = registered_material(&store, target, &scope())?.unwrap();
            assert!(body.is_some());
            assert_eq!(material.content_digest, p::sha256_content_digest(&png()));
        }
        Ok(())
    });
    let result = h.execute(&plan, &events);
    assert_eq!(result.status, ActionStatus::Completed);
    let receipt = result.external_receipt.unwrap();
    assert_eq!(receipt.effect, p::EffectStatus::Observed);
    assert_eq!(receipt.trust, p::TrustTier::Untrusted);
    let binding = h.transport.original();
    let target = id(&binding);
    assert_eq!(receipt.content_ref, Some(target.clone()));
    assert_eq!(read(&f, &target), png());
    assert!(
        read_worker_metadata(&f.store, &f.protection, &target)
            .unwrap()
            .unwrap()
            .binding
            == binding
    );
    assert_eq!(
        h.execute(&plan, &EventSink::default()).status,
        ActionStatus::Completed
    );
    assert_eq!(h.transport.captures.load(Ordering::SeqCst), 1);
    assert_eq!(h.transport.dispatches.load(Ordering::SeqCst), 1);
    let sealed = std::fs::read(f.root.0.join("worker.results").join(format!(
        "data-{}.bin",
        &binding.manifest.manifest_digest.0[7..]
    )))
    .unwrap();
    assert!(!sealed.windows(png().len()).any(|part| part == png()));
    let payloads = events.events();
    assert_eq!(
        payloads
            .iter()
            .filter(|e| e.kind() == p::EventKind::ActionStarted)
            .count(),
        1
    );
    assert_eq!(
        payloads
            .iter()
            .filter(|e| e.kind() == p::EventKind::ActionCompleted)
            .count(),
        1
    );
    assert!(!serde_json::to_string(&payloads)
        .unwrap()
        .contains("worker-artifact-metadata"));
}

#[test]
fn worker_lost_response_and_partial_registration_recover_without_redispatch() {
    for cut in 0..=3 {
        let f = Fixture::new();
        let h = HostFixture::new(&f, Some(f.sink.clone()));
        let plan = h.prepare();
        if cut == 0 {
            h.transport.mode.store(1, Ordering::SeqCst);
        } else {
            f.keys.fail_at.store(cut, Ordering::SeqCst);
        }
        let events = EventSink::default();
        let result = h.execute(&plan, &events);
        assert_eq!(result.status, ActionStatus::Unknown);
        assert!(!events
            .events()
            .iter()
            .any(|e| e.kind() == p::EventKind::ActionCompleted));
        let original = h.transport.original();
        let body = f.body(&id(&original));
        h.transport.mode.store(0, Ordering::SeqCst);
        f.keys.fail_at.store(0, Ordering::SeqCst);
        assert_eq!(
            h.execute(&plan, &EventSink::default()).status,
            ActionStatus::Completed
        );
        if let Some(body) = body {
            assert_eq!(f.body(&id(&original)).unwrap(), body);
        }
        assert_eq!(h.transport.captures.load(Ordering::SeqCst), 1);
        assert_eq!(h.transport.dispatches.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn worker_changed_grant_dimensions_deadline_or_bytes_cannot_complete_core() {
    for mode in 2..=5 {
        let f = Fixture::new();
        let h = HostFixture::new(&f, Some(f.sink.clone()));
        let plan = h.prepare();
        h.transport.mode.store(mode, Ordering::SeqCst);
        let events = EventSink::default();
        for _ in 0..2 {
            assert_eq!(h.execute(&plan, &events).status, ActionStatus::Unknown);
        }
        assert!(!events
            .events()
            .iter()
            .any(|e| e.kind() == p::EventKind::ActionCompleted));
        assert!(f.body(&id(&h.transport.original())).is_none());
        assert_eq!(h.transport.captures.load(Ordering::SeqCst), 1);
        assert_eq!(h.transport.dispatches.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn worker_requires_protected_consumer_at_planning_and_dispatch() {
    for plain in [false, true] {
        let f = Fixture::new();
        let artifact = plain.then(|| {
            Arc::new(FileArtifactStore::new(f.root.0.join("plain")).unwrap())
                as Arc<dyn ArtifactStore>
        });
        let h = HostFixture::new(&f, artifact);
        assert!(h.backend.plan(&intent()).is_err());
        let independent = forme_execution::DefaultExecutionPlanner::new(
            OutputBudget::strict(4096),
            p::DurationMs(30_000),
        );
        let plan = independent.plan(&intent()).unwrap();
        let events = EventSink::default();
        assert!(h
            .backend
            .execute(plan, &events, CancelToken::default())
            .is_err());
        assert!(events.events().is_empty());
        assert_eq!(h.transport.dispatches.load(Ordering::SeqCst), 0);
    }
}

struct WrongRef(Arc<ProtectedRuntimeArtifacts>);
impl ArtifactStore for WrongRef {
    fn supports_worker_output(&self) -> bool {
        true
    }
    fn write_worker_output(
        &self,
        binding: &p::WorkerArtifactBinding,
        bytes: &[u8],
    ) -> p::Result<ArtifactRecord> {
        let mut record = self.0.write_worker_output(binding, bytes)?;
        record.content_ref = p::ContentRef("artifact:unregistered-reference".into());
        Ok(record)
    }
    fn write(&self, _: &p::Scope, _: &str, _: &[u8]) -> p::Result<ArtifactRecord> {
        panic!("worker output cannot use generic write")
    }
}
#[test]
fn worker_rejects_artifact_writer_substituting_the_registered_content_reference() {
    let f = Fixture::new();
    let h = HostFixture::new(&f, Some(Arc::new(WrongRef(f.sink.clone()))));
    let plan = h.prepare();
    let events = EventSink::default();
    assert_eq!(h.execute(&plan, &events).status, ActionStatus::Unknown);
    assert!(!events
        .events()
        .iter()
        .any(|e| e.kind() == p::EventKind::ActionCompleted));
    assert!(f.body(&id(&h.transport.original())).is_some());
    assert_eq!(h.transport.captures.load(Ordering::SeqCst), 1);
}

#[test]
fn worker_event_persistence_failure_does_not_append_unknown_after_completion() {
    let f = Fixture::new();
    let h = HostFixture::new(&f, Some(f.sink.clone()));
    let plan = h.prepare();
    let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = completed.clone();
    let events = EventSink::with_observer(move |event| {
        if seen.load(Ordering::SeqCst) {
            return Err(p::Error(
                "fixture interrupted post-completion evidence".into(),
            ));
        }
        if event.kind() == p::EventKind::ActionCompleted {
            seen.store(true, Ordering::SeqCst);
        }
        Ok(())
    });
    assert!(h
        .backend
        .execute(plan, &events, CancelToken::default())
        .is_err());
    assert!(completed.load(Ordering::SeqCst));
    assert_eq!(
        events
            .events()
            .iter()
            .filter(|e| e.kind() == p::EventKind::ActionCompleted)
            .count(),
        1
    );
    assert!(!events
        .events()
        .iter()
        .any(|e| e.kind() == p::EventKind::ActionOutcomeUnknown));
}
